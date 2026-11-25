//! tracing 中间件：在引擎执行（spawn 的 task）内打开带 video_dir 的 workflow span。
//!
//! 为什么需要 middleware 而不是在 LocalDub 调用侧包 span：
//! `run_workflow` 内部 `tokio::spawn` 一个 task 跑引擎，tokio 新 task **不继承**
//! 调用方的 span。LocalDub 在 `drive()` 里 `.instrument(span)` 只覆盖「等待流」的
//! 一侧，真正产生 step 日志的引擎 task 不在 span 内，`WorkflowFileLayer` 拿不到
//! video_dir → 不落盘 `<video_dir>/<id>.log`。
//!
//! middleware 由引擎在 spawn 的 task 内调用，从 `ctx.input` 取 video_dir 开 span
//! 包住 handler —— 于是 handler 内所有 step 的 tracing 事件都带 video_dir。

use aa_workflow_core::middleware::{Middleware, MiddlewareServerFnArgs};
use tracing::Instrument;

/// 注册到 workflow 定义：包住 handler，打开 `workflow{video_dir=...}` span。
pub fn view_span_middleware() -> Middleware {
    Middleware::new().server(|mut args: MiddlewareServerFnArgs| async move {
        let video_dir = args
            .ctx
            .input
            .get("workflow")
            .and_then(|w| w.get("video_dir"))
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string();
        // 诊断：确认 middleware 在引擎 task 内执行且拿到 video_dir。
        tracing::info!(target: "env", "view_span middleware: video_dir={video_dir}");
        let span = tracing::info_span!("workflow", video_dir = %video_dir);
        args.next(None).instrument(span).await
    })
}
