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

    // 启动时拉起主服务器 (幂等, 已在运行则跳过): 视频播放依赖 http /media
    // (webkitgtk 的 <video> 仅支持 http 流式), 默认保证桌面开箱即用。
    // 失败仅记日志, UI 的 Main Server 卡片可手动重试 (fnrpc start_main)。
    tauri::async_runtime::spawn_blocking(move || {
        match ld_core::cmd::servers::start_main_server(false) {
            Ok(msg) => tracing::info!("[main] {msg}"),
            Err(e) => tracing::warn!("[main] 主服务器自动启动失败: {e:#}"),
        }
    });

    // 桌面端不内嵌 HTTP server: UI 的 fnrpc 走 Tauri IPC (直连上面的 router),
    // media 走 asset protocol 直读文件 (图片/静态资源), 视频走独立主服务器的
    // http /media (上方自动拉起)。

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
