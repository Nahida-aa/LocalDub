use std::time::Duration;

use config_rs::servers::ServerType;

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
///
/// 底层用 [`mdns-sd-discovery`](https://docs.rs/mdns-sd-discovery)（走 OS 原生
/// DNS-SD：Linux 用 avahi D-Bus，macOS/Windows 用系统 API），与 zeroconf/bonjour
/// 互通。相比 mdns_sd 自实现的广播，原生栈在沙箱/跨库场景更可靠。
pub async fn find_server_via_mdns_all(
    type_: ServerType,
    timeout: Option<Duration>,
) -> Vec<(String, u16)> {
    let timeout = timeout.unwrap_or(MDNS_TIMEOUT);

    // mdns-sd-discovery 是 tokio async (avahi D-Bus)。若当前不在 tokio runtime
    // (如同步 cli 调用), 自建一个临时 runtime 跑 browse。
    match tokio::runtime::Handle::try_current() {
        Ok(_) => browse_mdns(type_, timeout).await,
        Err(_) => {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(_) => return vec![],
            };
            rt.block_on(browse_mdns(type_, timeout))
        }
    }
}

/// 用 mdns-sd-discovery 浏览指定 service type, 在 timeout 内收集 (host, port)。
async fn browse_mdns(type_: ServerType, timeout: Duration) -> Vec<(String, u16)> {
    use mdns_sd_discovery::{BrowseEvent, ServiceBrowserBuilder};

    // avahi service type 形如 `_ld-server._tcp` (无 `.local` 后缀)
    let service_type = type_.service_name().trim_end_matches(".local");
    let mut browser = match ServiceBrowserBuilder::new()
        .service_type(service_type)
        .browse()
        .await
    {
        Ok(b) => b,
        Err(_) => return vec![],
    };

    let deadline = std::time::Instant::now() + timeout;
    let mut results: Vec<(String, u16)> = vec![];

    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, browser.recv()).await {
            Ok(Some(Ok(BrowseEvent::Found(svc)))) => {
                for addr in svc.addresses {
                    let entry = (addr.to_string(), svc.port);
                    if !results.contains(&entry) {
                        results.push(entry);
                    }
                }
            }
            Ok(Some(Ok(BrowseEvent::Removed(_)))) => {}
            Ok(Some(Err(_))) => break, // 浏览失败
            Ok(None) | Err(_) => break,
        }
    }

    results
}

/// Read the first `PORT=XXXX` line from process stdout.
pub fn read_port_from_output(output: &str) -> Option<u16> {
    output
        .lines()
        .find_map(|line| line.strip_prefix("PORT="))
        .and_then(|s| s.trim().parse::<u16>().ok())
}
