//! `/status` 端点冒烟测试: 统一状态协议返回 `ModelServerStatus` (running + 真实 uptime)。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn status_endpoint_returns_running_model_server_status() {
    let app = server::fnrpc_axum::build_axum_router(
        Arc::new(server::build_fn_rpc_router()),
        server::AppState::new(),
    );

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 8 * 1024).await.unwrap();
    let s: ld_core::servers::status::ModelServerStatus = serde_json::from_slice(&body).unwrap();
    assert_eq!(s.status, ld_core::servers::status::ServerRunState::Running);
    assert_eq!(s.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(s.port, Some(19110));
    assert!(s.models.is_empty());
}