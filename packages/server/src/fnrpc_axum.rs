use std::sync::Arc;

use axum::Router;
use fnrpc::router::RpcRouter;
use fnrpc_axum::{handle, FnrpcState};
use tower_http::cors::CorsLayer;

use crate::ctx::{AppState, Ctx};

pub fn build_axum_router(router: Arc<RpcRouter<Ctx>>, app_state: AppState) -> Router {
    let cors = CorsLayer::permissive();

    let state = Arc::new(FnrpcState::from_arc(router, move |headers| Ctx {
        state: app_state.clone(),
        headers: headers.clone(),
    }));

    Router::new()
        .route(
            "/fnrpc/{*path}",
            axum::routing::get(handle::<Ctx>).post(handle::<Ctx>),
        )
        .with_state(state)
        .layer(cors)
}
