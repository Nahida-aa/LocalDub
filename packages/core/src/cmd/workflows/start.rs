//! start 派发器 (镜像 TS `packages/core/cmd/workflows/startTask.ts` 的 `cmdStartTask`)。
//!
//! 两个入口：
//! - [`import_and_start_workflow`]（wire `import_and_start`，原 start）：无论是否带
//!   `video_dir` 都按 `input.workflow.url` 重新 `import_video` 生成/复用任务目录，
//!   再从头跑完整 pipeline。`video_dir` 在此路径被忽略。
//! - [`start_workflow`]（wire `start`）：对**已导入**目录从头跑全量，不 import、
//!   不 setCtx、不跳过（与 continue 的区别）。
//!
//! 执行路径唯一：引擎路径（core 直驱 aa-workflow，真源事件日志
//! `<video_dir>/workflow-store/<run_id>/logs.jsonl`，见
//! [`crate::integration::workflow::engine::run_engine`]）。
//! 过渡的串行 pipeline / 实验性 parallel 已下线。

use crate::input::{Input, WorkflowInput};
use anyhow::Context;

/// 驱动已构造的 run input（两入口共用；不含 import / setCtx）。
async fn run_from_imported(wfi: WorkflowInput) -> anyhow::Result<()> {
    println!("[cli] 引擎路径 (core 直驱, 真源事件日志)");
    crate::integration::workflow::engine::run_engine(&wfi).await
}

/// 导入视频 + 从头跑全量 pipeline（原 start 语义）。
///
/// 返回新任务的绝对 video_dir (供 RPC/CLI 层用于跳转/提示)。
pub async fn import_and_start_workflow(input: &Input) -> anyhow::Result<String> {
    let (st, input) =
        crate::workflow::import::download::import_video(input).context("import_video 失败")?;
    println!("[cli] 导入完成, video_dir = {}", st.workflow.video_dir);

    let wfi = WorkflowInput::try_from(input.clone()).map_err(anyhow::Error::msg)?;
    run_from_imported(wfi).await?;

    println!("[cli] 完成: {}", st.workflow.video_dir);
    Ok(st.workflow.video_dir)
}

/// 对**已导入**目录从头跑全量 pipeline（不 import、不 setCtx、不跳过）。
///
/// 与 continue 的区别：continue 会写回 input.json（setCtx）并按 `<step>/state.json`
/// 跳过已 success 的步骤；这里全量重跑。
pub async fn start_workflow(input: &Input) -> anyhow::Result<String> {
    let video_dir = input
        .workflow
        .as_ref()
        .and_then(|w| w.video_dir.clone())
        .ok_or_else(|| anyhow::anyhow!("start 模式需要 input.workflow.video_dir 指定已导入目录"))?;

    if !crate::state::state_path(&video_dir).exists() {
        return Err(anyhow::anyhow!(
            "start 模式找不到 {}, 请先 import",
            crate::state::state_path(&video_dir).display()
        ));
    }

    println!("[cli] 从头跑全量（已导入）, video_dir = {video_dir}");

    // 读回 import 落定的 input.json：它的 video_dir 是绝对路径。
    // 调用方（input.jsonc）的视频目录可能是相对路径，直接用会让 concat 类步骤
    // 把相对路径写进 ffmpeg 列表、被相对列表文件目录二次拼接（双份路径）。
    let settled = crate::state::read_input(&video_dir).map_err(anyhow::Error::msg)?;
    let wfi = WorkflowInput::try_from(settled.clone()).map_err(anyhow::Error::msg)?;
    run_from_imported(wfi).await?;

    println!("[cli] 完成: {video_dir}");
    Ok(video_dir)
}
