// use std::fs;

// use config_rs::{root::base_dir, servers::ServerType};
// use ld_core::{
//     cmd::tasks::get_task::GroupInfo,
//     context::{self, Context, Task},
//     servers::discovery::ServerInfo,
//     utils::file_ops::{ensure_parent_dir, sanitize_relative_path},
// };
use device_rs::DeviceInfo;
// use serde::{Deserialize, Serialize};
// use specta::Type;

use config_rs::root::base_dir;

use crate::{commands, ctx::Ctx};

#[fnrpc::rpc_query]
pub async fn device_info(ctx: &Ctx) -> Result<DeviceInfo, String> {
    commands::device_info(&ctx.state)
}

/// 返回 media root (`base_dir()`) 绝对路径。
///
/// 供 Tauri 前端用 asset protocol 直读本地 media 文件
/// (与 axum `/media` ServeDir 的根一致), 摆脱桌面 UI 对 HTTP 静态目录的依赖。
#[fnrpc::rpc_query]
pub async fn get_workfolder() -> String {
    base_dir().to_string_lossy().into_owned()
}
