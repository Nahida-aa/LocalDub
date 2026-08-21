use std::sync::Arc;

use server::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 统一初始化 tracing (fmt + 任务文件落盘 + EnvFilter)。
    // 重复 init 会失败, 故忽略返回值 (server 内部若已初始化则跳过)。
    let _ = ld_core::logging::init();

    // fnRPC router (independent from rspc)
    let fnrpc_router = Arc::new(server::build_fn_rpc_router());

    let app_state = AppState::new();

    // Axum HTTP server for mobile web browser access
    let axum_state = app_state.clone();
    let axum_fnrpc = fnrpc_router.clone();
    let dist_dir = app_state
        .repo_root
        .join("packages")
        .join("app")
        .join("dist");
    tauri::async_runtime::spawn(async move {
        server::start(axum_state, axum_fnrpc, dist_dir, 19110).await;
    });

    let tauri_state = fnrpc_tauri::FnrpcTauriState::from_arc(fnrpc_router, move || server::Ctx {
        state: app_state.clone(),
        headers: axum::http::HeaderMap::new(),
    });
    // Tauri desktop
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(tauri_state)
        .invoke_handler(fnrpc_tauri::generate_handler!(server::Ctx))
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
