use fnrpc::middlewares::tracing::TracingLayer;
use ld_core::stages::{
    asr::out::AsrResult,
    mix_audio::out::{Timing, TimingsFile},
    split_audio::out::{SplitAudioSegment, SplitAudioTiming},
    translate::out::{TranslateResult, TranslateSegment},
    tts::out::{TtsFile, TtsSegment},
};
use subtitle_ocr_post::OcrSegmentFilterResult;

use crate::{
    ctx::Ctx,
    feat::{
        env::{env_check, env_ensure},
        file_op::{
            list_app_directory, read_app_file_bin, read_app_file_json, read_app_file_text,
            write_app_file_json, write_app_file_text,
        },
        other::{device_info, get_workfolder},
        servers::{find_server, get_server_status, shutdown, start_main, start_voxcpm, stop_voxcpm},
        workflows::{
            cancel_queue, continue_workflow, enqueue_continue, enqueue_import, enqueue_start,
            get_group_list,
            get_workflow_ctx, list_queue, log::watch_workflow_log, regen_tts, start_workflow,
            tree::watch_workflow_tree,
        },
    },
};

#[fnrpc::rpc_query]
pub async fn health_check() -> &'static str {
    "ok"
}

pub fn build_fn_rpc_router() -> fnrpc::router::RpcRouter<Ctx> {
    fnrpc::router::RpcRouterBuilder::<Ctx>::new()
        .route_fn(read_app_file_text)
        .route_fn(write_app_file_text)
        .route_fn(read_app_file_json)
        .route_fn(write_app_file_json)
        .route_fn(read_app_file_bin)
        .route_fn(list_app_directory)
        .subscribe(watch_workflow_log)
        .subscribe(watch_workflow_tree)
        .route_fn(get_group_list)
        .route_fn(get_workflow_ctx)
        .route_fn(health_check)
        .route_fn(find_server)
        .route_fn(get_server_status)
        .route_fn(start_voxcpm)
        .route_fn(stop_voxcpm)
        .route_fn(shutdown)
        .route_fn(start_main)
        .route_fn(device_info)
        .route_fn(get_workfolder)
        .route_fn(env_check)
        .route_fn(env_ensure)
        .route_fn(continue_workflow)
        .route_fn(regen_tts)
        .route_fn(start_workflow)
        .route_fn(enqueue_start)
        .route_fn(enqueue_continue)
        .route_fn(enqueue_import)
        .route_fn(list_queue)
        .route_fn(cancel_queue)
        // 导出结果类型 (UI Timeline 渲染 workflow out.json 用, 无对应 RPC 返回)。
        .register_type::<AsrResult>()
        .register_type::<TranslateResult>()
        .register_type::<TranslateSegment>()
        .register_type::<SplitAudioSegment>()
        .register_type::<SplitAudioTiming>()
        .register_type::<TtsFile>()
        .register_type::<TtsSegment>()
        .register_type::<Timing>()
        .register_type::<TimingsFile>()
        .register_type::<OcrSegmentFilterResult>()
        .layer(TracingLayer)
        .build()
}
