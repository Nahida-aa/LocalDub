//! 任务入队: CLI 通过主服务器的 fnrpc 把 start/continue 任务加入队列。
//!
//! 流程: 用服务器发现找主服务器 → reqwest POST `/fnrpc/enqueue_start` 或
//! `/fnrpc/enqueue_continue`, body 为序列化的 `Input`。主服务器 worker 串行执行。

use crate::input::Input;
use crate::servers::discovery::find_server_via_mdns_all;
use config_rs::servers::ServerType;
use std::time::Duration;

/// 把当前 input 入队到主服务器任务队列。
///
/// `is_start`: true → enqueue_start; false → enqueue_continue。
/// 返回队列 ID (字符串)。
pub fn enqueue_task(input: &Input, is_start: bool) -> anyhow::Result<String> {
    // 用服务器发现找主服务器地址 (优先 IPv4, 避免 link-local IPv6 无法连接)。
    let (host, port) = discover_server();
    let url = format!(
        "http://{host}:{port}/fnrpc/{}",
        if is_start { "enqueue_start" } else { "enqueue_continue" }
    );

    let mut body_value = serde_json::to_value(input)
        .map_err(|e| anyhow::anyhow!("序列化 input 失败: {e}"))?;
    // 入队的是「任务本身」: 把 CLI 的 enqueue_start/enqueue_continue 命令动作
    // 还原为 worker 实际执行的 start/continue (队列项按 action 分派执行)。
    let action = if is_start { "start" } else { "continue" };
    if let Some(task) = body_value.get_mut("task") {
        if let Some(t) = task.get_mut("action") {
            *t = serde_json::Value::String(action.to_string());
        }
    }
    let body = body_value;

    let client = http_client()?;

    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| anyhow::anyhow!("调用主服务器入队失败 ({url}): {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().unwrap_or_default();
        return Err(anyhow::anyhow!(
            "主服务器入队失败 ({url}): HTTP {} {text}",
            status.as_u16()
        ));
    }
    // fnrpc 返回 `{"json": <queue_id>}`; 提取 json 字段。
    let json: serde_json::Value = resp
        .json()
        .map_err(|e| anyhow::anyhow!("解析入队响应失败: {e}"))?;
    let queue_id = json
        .get("json")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| "?".to_string());
    println!(
        "[cli] 已加入队列 (id={queue_id}): {}",
        if is_start { "start" } else { "continue" }
    );
    Ok(queue_id)
}

/// 列出主服务器任务队列 (fnrpc list_queue)。
pub fn list_queue() -> anyhow::Result<()> {
    let (host, port) = discover_server();
    let url = format!("http://{host}:{port}/fnrpc/list_queue");

    let resp = http_client()?
        .get(&url)
        .send()
        .map_err(|e| anyhow::anyhow!("调用主服务器 list_queue 失败 ({url}): {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().unwrap_or_default();
        return Err(anyhow::anyhow!(
            "主服务器 list_queue 失败 ({url}): HTTP {} {text}",
            status.as_u16()
        ));
    }
    let json: serde_json::Value = resp
        .json()
        .map_err(|e| anyhow::anyhow!("解析 list_queue 响应失败: {e}"))?;
    let entries = json
        .get("json")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if entries.is_empty() {
        println!("[cli] 队列为空");
        return Ok(());
    }
    println!("[cli] 队列 ({} 项):", entries.len());
    for e in &entries {
        let id = e.get("id").and_then(|v| v.as_str()).unwrap_or("?");
        let st = e.get("status").and_then(|v| v.as_str()).unwrap_or("?");
        let task = e.get("input").and_then(|v| v.get("task"));
        let action = task
            .and_then(|t| t.get("action"))
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let target = task
            .and_then(|t| t.get("url"))
            .and_then(|v| v.as_str())
            .or_else(|| task.and_then(|t| t.get("taskDir")).and_then(|v| v.as_str()))
            .unwrap_or("-");
        let err = e
            .get("error")
            .and_then(|v| v.as_str())
            .map(|s| format!("  error={s}"))
            .unwrap_or_default();
        println!("  id={id:<4} {st:<8} {action:<9} {target}{err}");
    }
    Ok(())
}

/// 取消主服务器队列中的待执行任务 (fnrpc cancel_queue)。
pub fn cancel_queue(id: u64) -> anyhow::Result<()> {
    let (host, port) = discover_server();
    let url = format!("http://{host}:{port}/fnrpc/cancel_queue");

    let resp = http_client()?
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&id)
        .send()
        .map_err(|e| anyhow::anyhow!("调用主服务器 cancel_queue 失败 ({url}): {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().unwrap_or_default();
        return Err(anyhow::anyhow!(
            "主服务器 cancel_queue 失败 ({url}): HTTP {} {text}",
            status.as_u16()
        ));
    }
    let json: serde_json::Value = resp
        .json()
        .map_err(|e| anyhow::anyhow!("解析 cancel_queue 响应失败: {e}"))?;
    let ok = json.get("json").and_then(|v| v.as_bool()).unwrap_or(false);
    if ok {
        println!("[cli] 已取消队列任务 id={id}");
    } else {
        println!("[cli] 取消失败: 队列任务 id={id} 不存在或非 queued 状态");
    }
    Ok(())
}

fn http_client() -> anyhow::Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| anyhow::anyhow!("构建 HTTP client 失败: {e}"))
}

/// 用 mDNS 发现主服务器地址。优先本机回环 127.0.0.1 (主服务器与 CLI 同机时最可靠),
/// 再取其它可连 IPv4; 避免 link-local IPv6 (fe80::) 和 docker0 等虚拟网桥地址
/// (本机连它们通常不通)。无发现时 fallback 到本机默认端口。
fn discover_server() -> (String, u16) {
    let default_port = ServerType::Server.default_port();
    let list = futures_block_on(find_server_via_mdns_all(ServerType::Server, None));
    // 1) 优先回环 127.0.0.1 (本机场景必然可连)
    for (host, port) in &list {
        if host == "127.0.0.1" || host == "::1" {
            return (host.clone(), *port);
        }
    }
    // 2) 再取其它可连 IPv4 (排除 link-local IPv6 / docker0 / 未指定)
    for (host, port) in &list {
        if is_ipv4_usable(host) {
            return (host.clone(), *port);
        }
    }
    // 退化: 第一个地址
    if let Some((h, p)) = list.into_iter().next() {
        return (h, p);
    }
    ("127.0.0.1".to_string(), default_port)
}

/// IPv4 可连接判断: 回环或私有地址 (排除 link-local IPv6 / docker0 / 多播 / 未指定)。
fn is_ipv4_usable(host: &str) -> bool {
    if host == "0.0.0.0" {
        return true;
    }
    // 私有 IPv4: 10.x / 192.168.x / 172.16-31.x (排除 docker 网桥 172.17-31 通常不可连)
    if host.starts_with("10.") || host.starts_with("192.168.") {
        return true;
    }
    if host.starts_with("172.") {
        if let Some(rest) = host.strip_prefix("172.") {
            if let Some(first) = rest.split('.').next() {
                if let Ok(n) = first.parse::<u8>() {
                    // 172.16 保留私有, 但 docker 网桥 (172.17~172.31) 本机不可达, 排除
                    return n == 16;
                }
            }
        }
    }
    false
}

/// 在无 tokio runtime 上下文中跑 async mDNS 发现。
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
