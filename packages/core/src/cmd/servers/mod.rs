//! 服务器命令 (`cli servers`), 镜像 TS `packages/cli/src/feat/command/servers.ts` 的 `cmdServers`。
//!
//! 动作:
//! - `discovery`: 用 mDNS/DNS-SD 列出某类型的所有服务器实例 (核心诉求)
//! - `status`: 发现服务器并探测其 `/status`, 输出完整模型级状态 (见 [`crate::servers::status`])
//! - `start`: 启动主服务器 (packages/server, Rust 二进制)
//! - `stop`: 停止主服务器
//!
//! 发现走 `crate::servers::discovery` (底层用 mdns-sd-discovery, 即 OS 原生 DNS-SD/avahi)。

use std::process::Command;
use std::time::Duration;

use crate::input::Input;
use crate::servers::args::ServerAction;
use crate::servers::discovery::find_server_via_mdns_all;
use crate::servers::status::ModelServerStatus;
use crate::steps::utils::find_release_bin;
use anyhow::Context;
use config_rs::servers::ServerType;

/// 执行 `cli servers` 命令 (镜像 TS `cmdServers`)。返回打印用的结果字符串。
pub fn cmd_servers(input: &Input) -> anyhow::Result<String> {
    let args = input.servers.clone().unwrap_or_default();
    match args.action {
        ServerAction::Discovery => discovery(args.name),
        ServerAction::Status => status(args.name),
        ServerAction::Start => start(args.name, args.foreground),
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

/// `status` 动作: 发现服务器并探测其 `/status`, 输出完整模型级状态。
///
/// 只用 mDNS **真实发现**的实例 (不用默认端口 fallback), 避免「未运行却报默认端口」
/// 的误导。每类型一条 [`ModelServerStatus`](crate::servers::status::ModelServerStatus)
/// (取首个 running 实例的完整状态, 全未运行则首条)。
fn status(name: Option<ServerType>) -> anyhow::Result<String> {
    let types: Vec<ServerType> = match name {
        Some(n) => vec![n],
        None => ServerType::ALL.to_vec(),
    };
    let mut results: Vec<ModelServerStatus> = vec![];
    for t in types {
        results.push(futures_block_on(
            crate::servers::status::probe_server_status(t),
        ));
    }
    Ok(serde_json::to_string_pretty(&results)?)
}

/// 在无 tokio runtime 上下文中跑一个 async 发现/探测 (mdns-sd-discovery 需 tokio)。
fn futures_block_on<T>(fut: impl std::future::Future<Output = T>) -> T {
    crate::utils::runtime::block_on(fut)
}

/// `start` 动作: 启动主服务器 (packages/server, Rust 二进制)。
///
/// 只支持 `ServerType::Main` (主服务器)。已运行则直接返回。
///
/// - `foreground=false` (默认): spawn detached, stdout/stderr 追加重定向到
///   `<base_dir>/logs/server.log` (直接继承 cli 的终端管道会在 cli 退出后触发 EPIPE)。
/// - `foreground=true`: 继承终端 stdio 阻塞运行, 日志实时可见, Ctrl+C 直接终止
///   (不设独立进程组, 与 cli 同进程组共享终端信号)。server 退出后返回。
fn start(name: Option<ServerType>, foreground: bool) -> anyhow::Result<String> {
    let t = name.unwrap_or(ServerType::Main);
    if t != ServerType::Main {
        return Err(anyhow::anyhow!(
            "暂仅支持启动主服务器 (main 类型), 收到 {t:?}"
        ));
    }
    start_main_server(foreground)
}

/// 启动主服务器 (供 CLI 与桌面端共用; 幂等, 已在运行则直接返回)。
///
/// - `foreground=false` (默认): spawn detached, stdout/stderr 追加重定向到
///   `<base_dir>/logs/server.log` (直接继承调用方终端管道会在其退出后触发 EPIPE)。
/// - `foreground=true`: 继承当前终端 stdio 阻塞运行, 日志实时可见, Ctrl+C 直接终止
///   (不设独立进程组, 与调用方同进程组共享终端信号)。server 退出后返回。
///
/// 内部含阻塞操作 (健康轮询最多 15s), async 调用方需放 `spawn_blocking`。
pub fn start_main_server(foreground: bool) -> anyhow::Result<String> {
    // 已在运行?
    if let Some((h, p)) = running_server() {
        return Ok(format!("主服务器已在运行: http://{h}:{p}/"));
    }

    // 定位 server 二进制
    let bin = find_release_bin("server").ok_or_else(|| {
        anyhow::anyhow!("未找到 server 二进制 (target/release/server 或 target/debug/server)")
    })?;

    if foreground {
        // 前台模式: 继承终端 stdio, 阻塞至 server 退出。
        let status = Command::new(&bin)
            .status()
            .map_err(|e| anyhow::anyhow!("启动主服务器 {bin:?} 失败: {e}"))?;
        return Ok(format!("主服务器已退出: {status}"));
    }

    // detached: 独立进程组 + stdio 重定向到日志文件
    let log_path = config_rs::root::repo_root().join("logs").join("server.log");
    if let Some(dir) = log_path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("创建日志目录 {:?} 失败", dir))?;
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
        Some(c) => c
            .get(&url)
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false),
        None => false,
    }
}
