//! start 派发器 (镜像 TS `packages/core/cmd/workflows/startTask.ts` 的 `cmdStartTask`)。
//!
//! 两个入口：
//! - [`import_and_start_workflow`]（wire `import_and_start`，原 start）：无论是否带
//!   `video_dir` 都按 `input.workflow.url` 重新 `import_video` 生成/复用任务目录，
//!   再从头跑完整 pipeline。`video_dir` 在此路径被忽略。
//! - [`start_workflow`]（wire `start`）：对**已导入**目录从头跑全量，不 import、
//!   不 setCtx、不跳过（与 continue 的区别）。
//!
//! 执行路径三选一（[`crate::workflow::args::ExecPath`]）：
//! - `engine = true`   → 引擎路径——core 直驱 aa-workflow（真源事件日志，
//!   [`crate::integration::workflow::engine::run_engine`]）；continue/resume 仍开发中；
//! - `parallel = true` → **实验性并行** [`crate::workflow::parallel::run_pipeline_parallel`]
//!   (真源各 `<step>/state.json`)；
//! - 都不设 → [`crate::workflow::pipeline::run_pipeline`] (真源 view.json)。
//! 两者同设在 [`crate::workflow::args::WorkflowArgs::exec_path`] 里报错。

use crate::input::{Input, WorkflowInput};
use crate::workflow::args::ExecPath;
use anyhow::Context;

/// 把已构造的 run input 按执行路径分派（两入口共用；不含 import / setCtx）。
async fn run_from_imported(input: &Input, wfi: WorkflowInput) -> anyhow::Result<()> {
    let path = match input.workflow.as_ref() {
        Some(w) => w.exec_path()?,
        None => ExecPath::Serial,
    };
    match path {
        ExecPath::Engine => {
            println!("[cli] 引擎路径 (core 直驱, 真源事件日志)");
            crate::integration::workflow::engine::run_engine(&wfi).await?;
        }
        ExecPath::Parallel => {
            println!("[cli] 实验性并行 (DAG 调度, 真源各 <step>/state.json)");
            crate::workflow::parallel::run_pipeline_parallel(input)
                .await
                .context("run_pipeline_parallel 失败")?;
        }
        ExecPath::Serial => {
            crate::workflow::pipeline::run_pipeline(input)
                .await
                .context("run_pipeline 失败")?;
        }
    }
    Ok(())
}

/// 导入视频 + 从头跑全量 pipeline（原 start 语义）。
///
/// 返回新任务的绝对 video_dir (供 RPC/CLI 层用于跳转/提示)。
pub async fn import_and_start_workflow(input: &Input) -> anyhow::Result<String> {
    let (st, input) =
        crate::workflow::import::download::import_video(input).context("import_video 失败")?;
    println!("[cli] 导入完成, video_dir = {}", st.workflow.video_dir);

    let wfi = WorkflowInput::try_from(input.clone()).map_err(anyhow::Error::msg)?;
    run_from_imported(&input, wfi).await?;

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
    let wfi = WorkflowInput::try_from(input.clone()).map_err(anyhow::Error::msg)?;
    run_from_imported(input, wfi).await?;

    println!("[cli] 完成: {video_dir}");
    Ok(video_dir)
}
