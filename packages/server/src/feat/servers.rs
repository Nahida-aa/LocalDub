use crate::{commands, ctx::Ctx};
use config_rs::servers::ServerType;
use ld_core::servers::discovery::ServerInfo;

#[fnrpc::rpc_query]
pub async fn find_server(input: ServerType) -> ServerInfo {
    ld_core::servers::discovery::find_server(input).await
}

#[fnrpc::rpc_mutate]
pub async fn start_torch(ctx: &Ctx) -> Result<u16, String> {
    commands::start_torch(&ctx.state)
}

#[fnrpc::rpc_mutate]
pub async fn stop_torch(ctx: &Ctx) -> Result<(), String> {
    commands::stop_torch(&ctx.state)
}

#[fnrpc::rpc_query]
pub async fn check_torch(ctx: &Ctx) -> bool {
    commands::check_torch(&ctx.state)
}

#[fnrpc::rpc_mutate]
pub async fn start_voxcpm(ctx: &Ctx) -> Result<u16, String> {
    commands::start_voxcpm(&ctx.state)
}

#[fnrpc::rpc_mutate]
pub async fn stop_voxcpm(ctx: &Ctx) -> Result<(), String> {
    commands::stop_voxcpm(&ctx.state)
}

/// 触发主服务器优雅关闭 (fnrpc 端点, 供 `cli servers stop` 调用)。
#[fnrpc::rpc_mutate]
pub async fn shutdown(ctx: &Ctx) -> &'static str {
    ctx.state.shutdown.notify_one();
    "shutting down"
}

/// 启动主服务器 (幂等, 已在运行则返回提示)。
///
/// 供桌面 UI 一键启动: UI 的 fnrpc 走 Tauri IPC 直连 app 进程内 router,
/// 在 app 进程内 spawn detached 主服务器 (与 `cli servers start` 同一份逻辑,
/// 见 `ld_core::cmd::servers::start_main_server`)。
/// 内部含阻塞健康轮询 (最多 15s), 放 spawn_blocking 避免卡 tokio worker。
#[fnrpc::rpc_mutate]
pub async fn start_main() -> Result<String, String> {
    tokio::task::spawn_blocking(|| {
        ld_core::cmd::servers::start_main_server(false).map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| format!("start_main 任务崩溃: {e}"))?
}

// #[fnrpc::rpc_query]
// pub async fn check_voxcpm(ctx: &Ctx) -> bool {
//     commands::check_voxcpm(&ctx.state)
// }
