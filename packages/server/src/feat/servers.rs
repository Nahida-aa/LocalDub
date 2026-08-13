use crate::{commands, ctx::Ctx};
use config_rs::servers::ServerType;
use core_rs::servers::discovery::ServerInfo;

#[fnrpc::rpc_query]
pub async fn find_server(input: ServerType) -> ServerInfo {
    core_rs::servers::discovery::find_server(input).await
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

// #[fnrpc::rpc_query]
// pub async fn check_voxcpm(ctx: &Ctx) -> bool {
//     commands::check_voxcpm(&ctx.state)
// }
