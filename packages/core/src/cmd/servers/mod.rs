//! 服务器命令 (`cli servers`), 镜像 TS `packages/cli/src/feat/command/servers.ts` 的 `cmdServers`。
//!
//! 动作:
//! - `discovery`: 用 mDNS/DNS-SD 列出某类型的所有服务器实例 (核心诉求)
//! - `status`: 发现服务器并探测其 `/status` 健康端点
//! - `start` / `stop`: 发现服务器地址 (进程启动/停止由上层 CLI/服务层负责, 此处不做进程管理)
//!
//! 发现走 `crate::servers::discovery` (底层用 mdns-sd-discovery, 即 OS 原生 DNS-SD/avahi)。

use std::time::Duration;

use crate::input::Input;
use crate::servers::args::ServerAction;
use crate::servers::discovery::{ServerInfo, find_server, find_server_via_mdns_all};
use config_rs::servers::ServerType;

/// 服务器状态 (镜像 TS `torchStatus` 的简化, 探测 `/status` 是否可连)。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ServerStatus {
    pub host: String,
    pub port: u16,
    /// `running` = `/status` 可连; `stopped` = 不可连
    pub status: String,
}

/// 执行 `cli servers` 命令 (镜像 TS `cmdServers`)。返回打印用的结果字符串。
pub fn cmd_servers(input: &Input) -> anyhow::Result<String> {
    let args = input.servers.clone().unwrap_or_default();
    match args.action {
        ServerAction::Discovery => discovery(args.name),
        ServerAction::Status => status(args.name),
        ServerAction::Start => Ok(format!("[Servers] start: 由上层服务层负责, 已发现地址 {:?}", find_all(args.name))),
        ServerAction::Stop => Ok(format!("[Servers] stop: 由上层服务层负责, 已发现地址 {:?}", find_all(args.name))),
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
fn status(name: Option<ServerType>) -> anyhow::Result<String> {
    let types: Vec<ServerType> = match name {
        Some(n) => vec![n],
        None => ServerType::ALL.to_vec(),
    };
    let mut results: Vec<ServerStatus> = vec![];
    for t in types {
        let info = futures_block_on(find_server(t));
        results.push(ServerStatus {
            status: probe_status(&info).to_string(),
            ..server_status_from(info)
        });
    }
    Ok(serde_json::to_string_pretty(&results)?)
}

fn server_status_from(info: ServerInfo) -> ServerStatus {
    ServerStatus {
        host: info.host,
        port: info.port,
        status: String::new(),
    }
}

/// 探测 `http://{host}:{port}/status` 是否可连 (TS `fetchStatsRes` 简化)。
fn probe_status(info: &ServerInfo) -> &'static str {
    let url = format!("http://{}:{}/status", info.host, info.port);
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
