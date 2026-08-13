use std::sync::Arc;

use server::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Respect `RUST_LOG` (e.g. `RUST_LOG=fs=debug` to trace file-watcher events);
    // fall back to `info` when the env var is unset.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

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
        .manage(tauri_state)
        .invoke_handler(fnrpc_tauri::generate_handler!(server::Ctx))
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
