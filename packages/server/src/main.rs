use std::path::PathBuf;
use std::sync::Arc;

use config_rs::root::repo_root;
use server::axum_server::start;
use server::build_fn_rpc_router;

fn main() {
    // 统一初始化 tracing (fmt + 任务文件落盘 + EnvFilter)。
    let _ = ld_core::logging::init();

    let repo_root = repo_root();
    let app_state = server::AppState::new();
    let fnrpc_router = Arc::new(build_fn_rpc_router());
    let dist_dir: PathBuf = repo_root.join("packages/app/dist");

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(start(app_state, fnrpc_router, dist_dir, 19110));
}
