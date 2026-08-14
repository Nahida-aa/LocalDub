use std::time::Duration;

use config_rs::servers::ServerType;
use mdns_sd::{ServiceDaemon, ServiceEvent};

const MDNS_TIMEOUT: Duration = Duration::from_millis(3000);

const DEFAULT_HOST: &str = "127.0.0.1";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum FoundVia {
    Mdns,
    Default,
    PortFile,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ServerInfo {
    pub host: String,
    pub port: u16,
    pub found_via: FoundVia,
}

/// Discover all matching servers via mDNS, falling back to default host/port.
///
/// Returns base URLs like `http://127.0.0.1:19109`.
pub async fn find_servers(type_: ServerType) -> Vec<String> {
    let list = find_server_via_mdns_all(type_, None).await;
    if !list.is_empty() {
        return list
            .into_iter()
            .map(|(host, port)| format!("http://{}:{}", host, port))
            .collect();
    }
    vec![format!("http://{}:{}", DEFAULT_HOST, type_.default_port())]
}

/// Discover a single server by mDNS, falling back to defaults.
pub async fn find_server(type_: ServerType) -> ServerInfo {
    let list = find_server_via_mdns_all(type_, None).await;
    if let Some((host, port)) = list.into_iter().next() {
        return ServerInfo {
            host,
            port,
            found_via: FoundVia::Mdns,
        };
    }
    ServerInfo {
        host: DEFAULT_HOST.to_string(),
        port: type_.default_port(),
        found_via: FoundVia::Default,
    }
}

/// Browse mDNS for the given server type.
///
/// 在时限内扫描 mDNS，收集某类服务的所有实例（IP + port）
///
/// Returns `(ip, port)` pairs for all resolved services of the matching type.
///
/// 是一个时间受限的采集（time-bounded collection），不是轮询、不是流式处理。核心行为：
/// 1. 启动 mDNS 浏览器
/// 2. 在 timeout 内尽可能多地接收 ServiceResolved 事件
/// 3. 超时后停止并返回结果
pub async fn find_server_via_mdns_all(
    type_: ServerType,
    timeout: Option<Duration>,
) -> Vec<(String, u16)> {
    let timeout = timeout.unwrap_or(MDNS_TIMEOUT);

    let Ok(daemon) = ServiceDaemon::new() else {
        return vec![];
    };
    let Ok(receiver) = daemon.browse(type_.service_name()) else {
        return vec![];
    };

    let deadline = std::time::Instant::now() + timeout;
    let mut results: Vec<(String, u16)> = vec![];

    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match receiver.recv_timeout(remaining) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                let port = info.get_port();
                for addr in info.get_addresses() {
                    let entry = (addr.to_string(), port);
                    if !results.contains(&entry) {
                        results.push(entry);
                    }
                }
            }
            _ => {}
        }
    }

    drop(daemon);
    results
}

/// Read the first `PORT=XXXX` line from process stdout.
pub fn read_port_from_output(output: &str) -> Option<u16> {
    output
        .lines()
        .find_map(|line| line.strip_prefix("PORT="))
        .and_then(|s| s.trim().parse::<u16>().ok())
}
