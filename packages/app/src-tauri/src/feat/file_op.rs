use std::fs;
use std::time::Duration;

use config_rs::{
    root::base_dir,
    // servers::ServerType
};
use core_rs::{
    // cmd::tasks::get_task::GroupInfo,
    // context::{self, Context, Task},
    // servers::discovery::ServerInfo,
    utils::file_ops::{ensure_parent_dir, sanitize_relative_path},
};
// use device_rs::DeviceInfo;
use serde::{Deserialize, Serialize};
use specta::Type;

// use crate::{commands, ctx::Ctx};

#[fnrpc::rpc_query]
pub async fn read_app_file_text(relative_path: String) -> Result<String, String> {
    let path = base_dir().join(&relative_path);
    fs::read_to_string(&path).map_err(|e| format!("Failed to read {}: {}", path.display(), e))
}

#[fnrpc::rpc_query]
pub async fn read_app_file_json(relative_path: String) -> Result<serde_json::Value, String> {
    let path = base_dir().join(&relative_path);
    let text = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    serde_json::from_str(&text).map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
}

#[fnrpc::rpc_query]
pub async fn read_app_file_bin(relative_path: String) -> Result<Vec<u8>, String> {
    let path = base_dir().join(&relative_path);
    fs::read(&path).map_err(|e| format!("Failed to read {}: {}", path.display(), e))
}

#[fnrpc::rpc_mutate]
pub async fn write_app_file_text(relative_path: String, content: String) -> Result<(), String> {
    let path = sanitize_relative_path(&relative_path)?;
    ensure_parent_dir(&path)?;
    fs::write(&path, &content).map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}
#[fnrpc::rpc_mutate]
pub async fn write_app_file_json(
    relative_path: String,
    content: serde_json::Value,
) -> Result<(), String> {
    let path = sanitize_relative_path(&relative_path)?;
    ensure_parent_dir(&path)?;
    let text = serde_json::to_string_pretty(&content)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    fs::write(&path, &text).map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}
#[fnrpc::rpc_mutate]
pub async fn write_app_file_binary(relative_path: String, content: Vec<u8>) -> Result<(), String> {
    let path = sanitize_relative_path(&relative_path)?;
    ensure_parent_dir(&path)?;
    fs::write(&path, &content).map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}

#[derive(Debug, Serialize, Deserialize, Type)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
}

#[fnrpc::rpc_query]
pub async fn list_app_directory(relative_path: String) -> Result<Vec<DirEntry>, String> {
    let path = base_dir().join(&relative_path);
    let entries =
        fs::read_dir(&path).map_err(|e| format!("Failed to list {}: {}", path.display(), e))?;

    let mut result = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let metadata = entry
            .metadata()
            .map_err(|e| format!("Failed to read metadata: {}", e))?;
        result.push(DirEntry {
            name: entry.file_name().to_string_lossy().to_string(),
            is_dir: metadata.is_dir(),
            size: if metadata.is_file() {
                Some(metadata.len())
            } else {
                None
            },
        });
    }

    result.sort_by(|a, b| {
        if a.is_dir != b.is_dir {
            b.is_dir.cmp(&a.is_dir)
        } else {
            a.name.cmp(&b.name)
        }
    });

    Ok(result)
}

#[derive(Debug, Serialize, Deserialize, Type)]
pub struct ImportedConfig {
    pub path: String,
    pub size: usize,
    pub is_json: bool,
    pub decoded: bool,
    pub preview: String,
    /// 实际生效的请求 User-Agent
    pub ua: String,
}

/// 订阅站通常按 User-Agent 决定返回格式（clash-verge → Clash 明文配置，v2rayNG → base64 节点列表）。
/// 按此顺序探测，内容分级越低越优，拿到 rank 1 立即停止。
const PROBE_UAS: &[&str] = &["clash-verge/v2.2.3", "v2rayNG/1.8.5"];

fn filename_from_url(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next()?;
    let name = path.rsplit('/').next().filter(|s| !s.is_empty())?;
    if name.contains('.') {
        Some(name.to_string())
    } else {
        None
    }
}

/// 内容分级，越低越优：
/// - 1: JSON，或含 `proxies:` + `proxy-groups:`/`rules:` 的 Clash 明文配置（直接可用）
/// - 2: 明文节点列表（含 `://`）
/// - 3: base64 订阅（解码后含 `://`）
/// - 4: 无法识别（HTML 错误页等，视为探测失败）
fn rank_content(text: &str) -> u8 {
    if serde_json::from_str::<serde_json::Value>(text).is_ok() {
        return 1;
    }
    let lower = text.to_lowercase();
    if lower.contains("proxies:") && (lower.contains("proxy-groups:") || lower.contains("rules:")) {
        return 1;
    }
    if text.contains("://") {
        return 2;
    }
    if try_decode_base64_sub(text).is_some() {
        return 3;
    }
    4
}

async fn fetch_bytes(client: &reqwest::Client, url: &str, ua: &str) -> Result<Vec<u8>, String> {
    let resp = client
        .get(url)
        .header(reqwest::header::USER_AGENT, ua)
        .send()
        .await
        .map_err(|e| format!("[{ua}] {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("[{ua}] HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| format!("[{ua}] {e}"))?;
    if bytes.len() > 10 * 1024 * 1024 {
        return Err(format!("[{ua}] response too large (>10MB)"));
    }
    Ok(bytes.to_vec())
}

/// 尝试将内容当作 base64 订阅解码；解码结果"像订阅明文"（含 `://` 且无异常控制字符）才接受。
fn try_decode_base64_sub(text: &str) -> Option<String> {
    use base64::Engine;
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.is_empty() || compact.len() % 4 != 0 {
        return None;
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&compact)
        .ok()?;
    let plain = String::from_utf8(decoded).ok()?;
    if plain.contains("://")
        && !plain
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t')
    {
        Some(plain)
    } else {
        None
    }
}

fn save_config(filename: &str, bytes: &[u8], ua: &str) -> Result<ImportedConfig, String> {
    let path = sanitize_relative_path(filename)?;
    ensure_parent_dir(&path)?;
    let text = String::from_utf8_lossy(bytes);
    let is_json = serde_json::from_str::<serde_json::Value>(&text).is_ok();
    let (content, decoded) = if is_json {
        (
            serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| serde_json::to_string_pretty(&v).ok())
                .unwrap_or_else(|| text.to_string()),
            false,
        )
    } else if let Some(plain) = try_decode_base64_sub(&text) {
        (plain, true)
    } else {
        (text.to_string(), false)
    };
    fs::write(&path, &content).map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
    let preview: String = content.chars().take(200).collect();
    Ok(ImportedConfig {
        path: path.display().to_string(),
        size: content.len(),
        is_json,
        decoded,
        preview,
        ua: ua.to_string(),
    })
}

#[fnrpc::rpc_mutate]
pub async fn import_config_from_url(url: String) -> Result<ImportedConfig, String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("URL must start with http:// or https://".to_string());
    }
    let filename = filename_from_url(&url).unwrap_or_else(|| "proxy.json".to_string());

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    // 自动探测：按 PROBE_UAS 顺序请求，内容分级越低越优，拿到 rank 1 直接停止；
    // 否则保留当前最优候选，全部 UA 尝试完后取最优结果。
    let mut best: Option<(u8, Vec<u8>, &str)> = None;
    let mut errors: Vec<String> = Vec::new();
    for ua in PROBE_UAS {
        match fetch_bytes(&client, &url, ua).await {
            Ok(bytes) => {
                let rank = rank_content(&String::from_utf8_lossy(&bytes));
                if rank == 4 {
                    errors.push(format!("[{ua}] content not recognized"));
                    continue;
                }
                if best.as_ref().map_or(true, |(br, _, _)| rank < *br) {
                    best = Some((rank, bytes, ua));
                }
                if rank == 1 {
                    break;
                }
            }
            Err(e) => errors.push(e),
        }
    }

    let (_, bytes, ua) = best.ok_or_else(|| {
        if errors.is_empty() {
            "Failed to detect config format".to_string()
        } else {
            format!("All requests failed: {}", errors.join("; "))
        }
    })?;

    save_config(&filename, &bytes, ua)
}
