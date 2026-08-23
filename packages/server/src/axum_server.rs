use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

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
    // 先保存 shutdown 信号 (build_axum_router 会 move state)。
    let shutdown_signal = state.shutdown.clone();
    // 启动任务队列 worker (串行执行入队的任务)。
    {
        let worker = state.queue.clone();
        tokio::spawn(async move {
            worker.run_worker().await;
        });
    }
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
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            // 端口被占: 探测是否已有主服务器实例在运行 (桌面端/独立 server 可能重复启动)。
            // 已有实例 -> 优雅返回不重复启动; 否则真错误 -> panic。
            if server_already_running(port).await {
                eprintln!("[Axum] 主服务器已在运行 (端口 {port}), 跳过本次启动");
                return;
            }
            panic!("Failed to bind HTTP server {addr}: {e}");
        }
    };

    // 通过 mDNS 注册主服务器, 使其它设备/客户端可发现 (镜像 Python mdns_server.py
    // 对 demucs/voxcpm 的注册; 本服务器用 Rust mdns_sd, service 名 _ld-server._tcp.local)。
    // daemon 在 serve 结束后 drop, mdns_sd 自动注销服务。
    let _mdns_daemon = register_mdns(port);

    eprintln!("[Axum] HTTP server listening on http://{}", addr);
    // 优雅关闭: 收到 `fnrpc shutdown` 通知 (AppState.shutdown) 时停止。
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal.notified().await;
        })
        .await
        .expect("HTTP server error");
}

/// 探测 `{host}:{port}/fnrpc/health_check` 是否已有主服务器在运行。
async fn server_already_running(port: u16) -> bool {
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpStream;
    use tokio::time::timeout;
    let Ok(stream) = timeout(
        Duration::from_millis(2000),
        TcpStream::connect(("127.0.0.1", port)),
    )
    .await
    else {
        return false;
    };
    let Ok(mut stream) = stream else {
        return false;
    };
    // 发一个最小 HTTP GET /fnrpc/health_check 请求; 有响应说明 server 在跑。
    let req = format!(
        "GET /fnrpc/health_check HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    );
    if stream.write_all(req.as_bytes()).await.is_err() {
        return false;
    }
    use tokio::io::AsyncReadExt;
    let mut buf = [0u8; 128];
    match timeout(Duration::from_millis(2000), stream.read(&mut buf)).await {
        Ok(Ok(n)) if n > 0 => true,
        _ => false,
    }
}

/// 用 mdns_sd 注册 `_ld-server._tcp.local` 服务 (主服务器, 端口 `port`)。
/// 返回 `ServiceDaemon` 供调用方持有 (drop 即注销)。注册失败仅告警, 不影响服务器启动。
fn register_mdns(port: u16) -> Option<mdns_sd::ServiceDaemon> {
    use mdns_sd::{ServiceDaemon, ServiceInfo};
    let mdns = match ServiceDaemon::new() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[mDNS] 创建 daemon 失败, 跳过注册: {e}");
            return None;
        }
    };
    // ip 用空 + enable_addr_auto 让库自动探测本机地址 (镜像 mdns_sd register example)。
    let service_info = ServiceInfo::new(
        "_ld-server._tcp.local.",
        "ld-server",
        "ld-server.local.",
        "",
        port,
        None::<std::collections::HashMap<String, String>>,
    );
    let service_info = match service_info {
        Ok(si) => si.enable_addr_auto(), // 让库自动探测本机 IP 并广播 (对齐 mdns_sd register example)
        Err(e) => {
            eprintln!("[mDNS] 构建 ServiceInfo 失败, 跳过注册: {e}");
            return None;
        }
    };
    match mdns.register(service_info) {
        Ok(receiver) => {
            eprintln!("[mDNS] 已注册 _ld-server._tcp.local 端口 {port}");
            // 消费 register 事件接收器, 避免未读
            let _ = receiver;
            Some(mdns)
        }
        Err(e) => {
            eprintln!("[mDNS] 注册失败: {e}");
            None
        }
    }
}
