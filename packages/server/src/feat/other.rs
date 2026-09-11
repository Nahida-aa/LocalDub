// use std::fs;

// use config_rs::{root::base_dir, servers::ServerType};
// use ld_core::{
//     cmd::workflows::get_workflow::GroupInfo,
//     context::{self, Context, Workflow},
//     servers::discovery::ServerInfo,
//     utils::file_ops::{ensure_parent_dir, sanitize_relative_path},
// };
use device_rs::DeviceInfo;
// use serde::{Deserialize, Serialize};
// use specta::Type;

use config_rs::root::repo_root;

use crate::ctx::Ctx;

#[fnrpc::rpc_query]
pub async fn device_info(_ctx: &Ctx) -> Result<DeviceInfo, String> {
    // 直接调用 device-rs 纯 Rust 采集 (此前 spawn bun 跑 TS cli.ts 只因语言不同)。
    Ok(device_rs::get_device_info())
}

/// 返回 media root (`repo_root()`) 绝对路径。
///
/// 供 Tauri 前端用 asset protocol 直读本地 media 文件
/// (与 axum `/media` ServeDir 的根一致), 摆脱桌面 UI 对 HTTP 静态目录的依赖。
#[fnrpc::rpc_query]
pub async fn get_workfolder() -> String {
    repo_root().to_string_lossy().into_owned()
}
