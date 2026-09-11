use std::sync::{Arc, OnceLock};
use std::time::Instant;

use axum::Router;
use fnrpc::router::RpcRouter;
use fnrpc_axum::{FnrpcState, handle};
use tower_http::cors::CorsLayer;

use crate::ctx::{AppState, Ctx};
use config_rs::servers::ServerType;
use ld_core::servers::status::{ModelServerStatus, ServerRunState};

/// 进程启动时刻 (供 `GET /status` 计算 uptime)。
static START: OnceLock<Instant> = OnceLock::new();

/// `GET /status` 处理: 统一状态协议端点 (镜像 vox-lab `/status`, 供
/// `probe_server_status` / `cli servers status` 探测)。主服务器本身无模型,
/// `models` 为空; uptime 用进程启动时刻。
async fn status_handler() -> axum::Json<ModelServerStatus> {
    let uptime_s = START.get().map(|t| t.elapsed().as_secs()).unwrap_or(0) as u32;
    axum::Json(ModelServerStatus {
        host: Some("127.0.0.1".to_string()),
        status: ServerRunState::Running,
        port: Some(ServerType::Main.default_port()),
        uptime_s,
        models: std::collections::HashMap::new(),
        message: None,
    })
}

pub fn build_axum_router(router: Arc<RpcRouter<Ctx>>, app_state: AppState) -> Router {
    let cors = CorsLayer::permissive();

    let state = Arc::new(FnrpcState::from_arc(router, move |headers| Ctx {
        state: app_state.clone(),
        headers: headers.clone(),
    }));

    START.get_or_init(Instant::now);

    Router::new()
        .route(
            "/fnrpc/{*path}",
            axum::routing::get(handle::<Ctx>).post(handle::<Ctx>),
        )
        // 统一状态协议: `GET /status` → ModelServerStatus (镜像 vox-lab `/status`)。
        .route("/status", axum::routing::get(status_handler))
        .with_state(state)
        .layer(cors)
}
