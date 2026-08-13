use fnrpc::middlewares::tracing::TracingLayer;

use crate::{
    ctx::Ctx,
    feat::{
        demo::tick,
        file_op::{
            list_app_directory, read_app_file_bin, read_app_file_json, read_app_file_text,
            write_app_file_json, write_app_file_text,
        },
        other::device_info,
        servers::{check_torch, find_server, start_torch, start_voxcpm, stop_torch, stop_voxcpm},
        tasks::{
            continue_task, get_group_list, get_task_ctx, log::watch_task_log, tree::watch_task_tree,
        },
    },
};
use std::sync::atomic::{AtomicU64, Ordering};

#[fnrpc::rpc_query]
pub async fn health_check() -> &'static str {
    "ok"
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

#[fnrpc::rpc_query]
pub async fn get_count() -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("count: {n}")
}

#[fnrpc::rpc_mutate]
pub async fn reset_count() -> () {
    COUNTER.store(0, Ordering::Relaxed);
}

pub fn build_fn_rpc_router() -> fnrpc::router::RpcRouter<Ctx> {
    fnrpc::router::RpcRouterBuilder::<Ctx>::new()
        .route_fn(read_app_file_text)
        .route_fn(write_app_file_text)
        .route_fn(read_app_file_json)
        .route_fn(write_app_file_json)
        .route_fn(read_app_file_bin)
        .route_fn(list_app_directory)
        .subscribe(watch_task_log)
        .subscribe(watch_task_tree)
        .subscribe(tick)
        .route_fn(get_group_list)
        .route_fn(get_task_ctx)
        .route_fn(health_check)
        .route_fn(get_count)
        .route_fn(reset_count)
        .route_fn(find_server)
        .route_fn(start_torch)
        .route_fn(stop_torch)
        .route_fn(check_torch)
        .route_fn(start_voxcpm)
        .route_fn(stop_voxcpm)
        .route_fn(device_info)
        .route_fn(continue_task)
        .layer(TracingLayer)
        .build()

    // .query(crate::feat::demo::func::greet)
    // .query(crate::feat::demo::func::add)
    // .query(crate::feat::demo::func::get_user)
    // .query(crate::feat::demo::func::divide)
    // .mutate(crate::feat::demo::func::create_user)
    // .subscribe(crate::feat::demo::func::tick)
    // .subscribe(crate::feat::demo::func::echo_stream)
    // .subscribe(crate::feat::demo::func::watch_status)
}
