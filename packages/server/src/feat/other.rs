// use std::fs;

// use config_rs::{root::base_dir, servers::ServerType};
// use core_rs::{
//     cmd::tasks::get_task::GroupInfo,
//     context::{self, Context, Task},
//     servers::discovery::ServerInfo,
//     utils::file_ops::{ensure_parent_dir, sanitize_relative_path},
// };
use device_rs::DeviceInfo;
// use serde::{Deserialize, Serialize};
// use specta::Type;

use crate::{commands, ctx::Ctx};

#[fnrpc::rpc_query]
pub async fn device_info(ctx: &Ctx) -> Result<DeviceInfo, String> {
    commands::device_info(&ctx.state)
}
