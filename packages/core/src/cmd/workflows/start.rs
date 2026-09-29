//! start 派发器 (镜像 TS `packages/core/cmd/workflows/startTask.ts` 的 `cmdStartTask`)。
//!
//! 语义: 无论是否带 `videoDir`, 都按 `input.workflow.url` 重新 `import_video` 生成/复用
//! 任务目录, 再从头跑完整 pipeline。`videoDir` 在此路径被忽略
//! (对齐 TS: start 分支忽略 videoDir, 不会续跑已有任务)。
//!
//! `input.workflow.engine = true` 时走引擎路径 [`crate::integration::workflow::run_workflow_engine`]
//! (真源 events.jsonl), 否则走 [`crate::workflows::pipeline::run_pipeline`] (真源 state.json)。
//!
//! 与 [`crate::workflows::continue_pipeline`] 的区别: start 会 import, continue 不 import。

use crate::input::Input;
use anyhow::Context;

/// 启动新任务: 导入视频 → 跑完整 pipeline (镜像 TS `cmdStartTask`)。
///
/// 返回新任务的绝对 video_dir (供 RPC/CLI 层用于跳转/提示)。
pub fn start_workflow(input: &Input) -> anyhow::Result<String> {
    let state =
        crate::workflows::import::download::import_video(input).context("import_video 失败")?;
    println!("[cli] 导入完成, video_dir = {}", state.workflow.video_dir);
    let engine = input.workflow.as_ref().and_then(|w| w.engine).unwrap_or(false);
    if engine {
        println!("[cli] 引擎模式 (aa-workflow, 真源 events.jsonl)");
        crate::integration::workflow::run_workflow_engine(
            &state.workflow.video_dir,
            &crate::integration::workflow::EngineOptions::from_input(input),
        )
        .context("run_workflow_engine 失败")?;
    } else {
        crate::workflows::pipeline::run_pipeline(&state.workflow.video_dir)
            .context("run_pipeline 失败")?;
    }
    println!("[cli] 完成: {}", state.workflow.video_dir);
    Ok(state.workflow.video_dir)
}
