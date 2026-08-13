use std::path::PathBuf;
use std::sync::Arc;

use fnrpc::router::RpcRouter;
// use rspc::Procedures;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use crate::{
    ctx::{AppState, Ctx},
    fnrpc_axum::build_axum_router,
};
use config_rs::root::base_dir;

// async fn fnrpc_handle(
//     router: &RpcRouter<Ctx>,
//     state: &AppState,
//     path: &str,
//     input: Value,
// ) -> Response {
//     let ctx = Ctx {
//         state: state.clone(),
//         headers: HeaderMap::new(),
//     };

//     let kind = router.get_procedure_kind(path);

//     match kind {
//         Some("subscribe") => match router.dispatch_subscribe(&ctx, path, input) {
//             Ok(stream) => Sse::new(
//                 stream.map(|item| -> Result<Event, std::convert::Infallible> {
//                     match item {
//                         Ok(val) => Ok(Event::default().json_data(val).unwrap()),
//                         Err(e) => Ok(Event::default().data(format!("error: {e}"))),
//                     }
//                 }),
//             )
//             .keep_alive(axum::response::sse::KeepAlive::new())
//             .into_response(),
//             Err(_) => StatusCode::NOT_FOUND.into_response(),
//         },
//         Some(_) => {
//             // query or mutate → JSON
//             let result = router.dispatch(&ctx, path, input).await.unwrap_or_default();
//             Json(result).into_response()
//         }
//         None => StatusCode::NOT_FOUND.into_response(),
//     }
// }

// async fn fnrpc_get_handler(
//     Extension(router): Extension<RpcRouter<Ctx>>,
//     Extension(state): Extension<AppState>,
//     Path(path): Path<String>,
//     Query(params): Query<HashMap<String, String>>,
// ) -> Response {
//     let input: Value = params
//         .get("input")
//         .and_then(|s| serde_json::from_str(s).ok())
//         .unwrap_or(Value::Null);
//     fnrpc_handle(&router, &state, &path, input).await
// }

// async fn fnrpc_post_handler(
//     Extension(router): Extension<RpcRouter<Ctx>>,
//     Extension(state): Extension<AppState>,
//     Path(path): Path<String>,
//     Json(input): Json<Value>,
// ) -> Response {
//     fnrpc_handle(&router, &state, &path, input).await
// }

pub async fn start(
    // procedures: Procedures<AppState>,
    state: AppState,
    fnrpc_router: Arc<RpcRouter<Ctx>>,
    dist_dir: PathBuf,
    port: u16,
) {
    // let state_for_rspc = state.clone();
    // let rspc_router =
    //     rspc_axum::endpoint::<AppState, _, _, _>(procedures, move || state_for_rspc.clone());

    let media_root = base_dir();
    let app = build_axum_router(fnrpc_router, state)
        .nest_service("/media", ServeDir::new(&media_root))
        // dev 下前端在 vite(1420), 媒体在 axum(19110) 跨源.
        // <audio>/<video> 标签播放不受 CORS 限制, 但波形 fetch() 需要 CORS 头,
        // 否则波形永远加载不出来 (仅有占位进度条).
        .layer(CorsLayer::permissive())
        // .nest("/rspc", rspc_router)
        // .route(
        //     "/fnrpc/*path",
        //     axum::routing::get(fnrpc_get_handler).post(fnrpc_post_handler),
        // )
        // .layer(CorsLayer::permissive())
        // .layer(Extension(fnrpc_router))
        // .layer(Extension(state))
        .fallback_service(ServeDir::new(&dist_dir).append_index_html_on_directories(true));

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind HTTP server");

    eprintln!("[Axum] HTTP server listening on http://{}", addr);
    axum::serve(listener, app).await.expect("HTTP server error");
}
