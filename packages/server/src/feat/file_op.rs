use std::fs;

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
