//! 服务器命令 (`cli servers`), 镜像 TS `packages/cli/src/feat/command/servers.ts` 的 `cmdServers`。
//!
//! 动作:
//! - `discovery`: 用 mDNS/DNS-SD 列出某类型的所有服务器实例 (核心诉求)
//! - `status`: 发现服务器并探测其 `/status` 健康端点
//! - `start`: 启动主服务器 (packages/server, Rust 二进制)
//! - `stop`: 停止主服务器
//!
//! 发现走 `crate::servers::discovery` (底层用 mdns-sd-discovery, 即 OS 原生 DNS-SD/avahi)。

use std::process::Command;
use std::time::Duration;

use anyhow::Context;
use crate::input::Input;
use crate::servers::args::ServerAction;
use crate::servers::discovery::find_server_via_mdns_all;
use crate::stages::utils::find_release_bin;
use config_rs::servers::ServerType;

/// 服务器状态 (探测 `/status` 是否可连)。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ServerStatus {
    /// 可连接地址; 仅 `running` 时非空, 否则 `null` (服务未跑, 地址无意义)
    pub host: Option<String>,
    /// 端口号; 仅 `running` 时非空, 否则 `null`
    pub port: Option<u16>,
    /// `running` = `/status` 可连; `stopped` = 可发现但探测失败; `not_found` = 未发现实例
    pub status: String,
}

/// 执行 `cli servers` 命令 (镜像 TS `cmdServers`)。返回打印用的结果字符串。
pub fn cmd_servers(input: &Input) -> anyhow::Result<String> {
    let args = input.servers.clone().unwrap_or_default();
    match args.action {
        ServerAction::Discovery => discovery(args.name),
        ServerAction::Status => status(args.name),
        ServerAction::Start => start(args.name),
        ServerAction::Stop => stop(args.name),
    }
}

/// 列出某类型 (或所有) 服务器的 mDNS 实例。
fn find_all(name: Option<ServerType>) -> Vec<(String, u16)> {
    match name {
        Some(n) => futures_block_on(find_server_via_mdns_all(n, None)),
        None => {
            // 不指定 name 时收集所有类型 (与 TS 逐类 findServer 对齐)
            let mut out = vec![];
            for t in ServerType::ALL {
                let list = futures_block_on(find_server_via_mdns_all(*t, None));
                for (h, p) in list {
                    let e = (h, p);
                    if !out.contains(&e) {
                        out.push(e);
                    }
                }
            }
            out
        }
    }
}

/// `discovery` 动作: 列出发现的服务器实例。
fn discovery(name: Option<ServerType>) -> anyhow::Result<String> {
    let list = find_all(name);
    let json = serde_json::to_string_pretty(&list)?;
    Ok(json)
}

/// `status` 动作: 发现服务器并探测 `/status`。
///
/// 只用 mDNS **真实发现**的实例 (不用 `find_server` 的默认端口 fallback),
/// 避免「未运行却报默认端口」的误导。未发现实例时 status = "not_found"。
fn status(name: Option<ServerType>) -> anyhow::Result<String> {
    let types: Vec<ServerType> = match name {
        Some(n) => vec![n],
        None => ServerType::ALL.to_vec(),
    };
    let mut results: Vec<ServerStatus> = vec![];
    for t in types {
        let list = futures_block_on(find_server_via_mdns_all(t, None));
        if list.is_empty() {
            // 没有 mDNS 发现的实例: 地址为 null
            results.push(ServerStatus {
                host: None,
                port: None,
                status: "not_found".to_string(),
            });
            continue;
        }
        // 主服务器/服务可能注册了多接口地址 (IPv4/IPv6/docker 等), 逐条探测会导致
        // 多条重复状态且大部分不可连。这里探测到第一个 running 即报告, 每类型一条:
        // 任一地址可连 -> running; 有实例但全不可连 -> stopped; 无实例 -> not_found。
        let mut found_any = false;
        for (host, port) in list {
            if probe_server_health(t, &host, port) == "running" {
                results.push(ServerStatus {
                    status: "running".to_string(),
                    host: Some(host),
                    port: Some(port),
                });
                found_any = true;
                break;
            }
        }
        if !found_any {
            results.push(ServerStatus {
                status: "stopped".to_string(),
                host: None,
                port: None,
            });
        }
    }
    Ok(serde_json::to_string_pretty(&results)?)
}

/// 探测 `http://{host}:{port}/status` 是否可连 (TS `fetchStatsRes` 简化)。
/// 探测服务器健康: 主服务器用 fnrpc health_check, 其它类型 (voxcpm) 用 `/status`。
fn probe_server_health(t: ServerType, host: &str, port: u16) -> &'static str {
    let url = if t == ServerType::Main {
        format!("http://{host}:{port}/fnrpc/health_check")
    } else {
        format!("http://{host}:{port}/status")
    };
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .ok();
    match client {
        Some(c) => match c.get(&url).send() {
            Ok(resp) if resp.status().is_success() => "running",
            _ => "stopped",
        },
        None => "stopped",
    }
}

/// 在无 tokio runtime 上下文中跑一个 async 发现 (mdns-sd-discovery 需 tokio)。
fn futures_block_on<T>(fut: impl std::future::Future<Output = T>) -> T {
    match tokio::runtime::Handle::try_current() {
        Ok(_) => tokio::runtime::Handle::current().block_on(fut),
        Err(_) => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(fut)
        }
    }
}

/// `start` 动作: 启动主服务器 (packages/server, Rust 二进制)。
///
/// 只支持 `ServerType::Main` (主服务器)。已运行则直接返回; 否则 spawn
/// `target/{release,debug}/server` 二进制并轮询健康端点直到就绪。
///
/// 日志: detached 进程不占终端, stdout/stderr 追加重定向到 `<base_dir>/logs/server.log`
/// (直接继承 cli 的终端管道会在 cli 退出后触发 EPIPE, 所以不能继承)。
fn start(name: Option<ServerType>) -> anyhow::Result<String> {
    let t = name.unwrap_or(ServerType::Main);
    if t != ServerType::Main {
        return Err(anyhow::anyhow!(
            "暂仅支持启动主服务器 (server 类型), 收到 {t:?}"
        ));
    }

    // 已在运行?
    if let Some((h, p)) = running_server() {
        return Ok(format!("主服务器已在运行: http://{h}:{p}/"));
    }

    // 定位 server 二进制
    let bin = find_release_bin("server").ok_or_else(|| {
        anyhow::anyhow!("未找到 server 二进制 (target/release/server 或 target/debug/server)")
    })?;

    // detached: 独立进程组 + stdio 重定向到日志文件
    let log_path = config_rs::root::base_dir().join("logs").join("server.log");
    if let Some(dir) = log_path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("创建日志目录 {:?} 失败", dir))?;
    }
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("打开日志文件 {log_path:?} 失败"))?;

    let mut cmd = Command::new(&bin);
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(
            log_file.try_clone().context("克隆日志文件句柄失败")?,
        ))
        .stderr(std::process::Stdio::from(log_file));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0); // 独立进程组, 不受 cli 终端信号影响
    }
    cmd.spawn()
        .map_err(|e| anyhow::anyhow!("启动主服务器 {bin:?} 失败: {e}"))?;

    // 健康轮询 fnrpc health_check
    for _ in 0..30 {
        if server_healthy() {
            return Ok(format!(
                "主服务器已启动: http://127.0.0.1:{}/ (日志: {})",
                ServerType::Main.default_port(),
                log_path.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Err(anyhow::anyhow!(
        "主服务器启动超时 ({}s), 查看日志: {}",
        30 * 500 / 1000,
        log_path.display()
    ))
}

/// `stop` 动作: 停止主服务器。
///
/// 通过 fnrpc `/fnrpc/shutdown` 优雅停止 (AppState.shutdown 通知 axum 退出)。
fn stop(_name: Option<ServerType>) -> anyhow::Result<String> {
    let port = ServerType::Main.default_port();
    // 尝试 fnrpc shutdown (当前主服务器未提供该端点, 预留)
    let url = format!("http://127.0.0.1:{port}/fnrpc/shutdown");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .ok();
    if let Some(c) = client {
        if let Ok(resp) = c.post(&url).send() {
            if resp.status().is_success() {
                return Ok("主服务器已停止".to_string());
            }
        }
    }
    Ok(format!(
        "主服务器 (端口 {port}) 未提供 shutdown 端点, 请手动停止对应进程"
    ))
}

/// 判断主服务器是否在运行 (直接探测默认端口 fnrpc health_check)。
///
/// 不依赖 mDNS (mdns_sd 注册在此环境可能不广播), 主服务器固定监听 19110,
/// 直接 HTTP 探测最可靠。
fn running_server() -> Option<(String, u16)> {
    let port = ServerType::Main.default_port();
    if server_healthy_at("127.0.0.1", port) {
        Some(("127.0.0.1".to_string(), port))
    } else {
        None
    }
}

/// 探测主服务器 fnrpc health_check (`GET /fnrpc/health_check`) 是否可连。
fn server_healthy() -> bool {
    server_healthy_at("127.0.0.1", ServerType::Main.default_port())
}

fn server_healthy_at(host: &str, port: u16) -> bool {
    let url = format!("http://{host}:{port}/fnrpc/health_check");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .ok();
    match client {
        Some(c) => c.get(&url).send().map(|r| r.status().is_success()).unwrap_or(false),
        None => false,
    }
}
