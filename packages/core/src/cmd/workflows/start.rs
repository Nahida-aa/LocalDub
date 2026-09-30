//! start 派发器 (镜像 TS `packages/core/cmd/workflows/startTask.ts` 的 `cmdStartTask`)。
//!
//! 语义: 无论是否带 `videoDir`, 都按 `input.workflow.url` 重新 `import_video` 生成/复用
//! 任务目录, 再从头跑完整 pipeline。`videoDir` 在此路径被忽略
//! (对齐 TS: start 分支忽略 videoDir, 不会续跑已有任务)。
//!
//! 执行路径三选一（[`crate::workflows::args::ExecPath`]）：
//! - `engine = true`   → 引擎路径——**开发中，暂不可用**（aa-workflow 集成暂停，
//!   见 `integration/workflow/mod.rs`；Engine 臂打印「开发中」并报错）；
//! - `parallel = true` → 实验性 DAG 并行 [`crate::workflows::parallel::run_pipeline_parallel`]
//!   (真源各 `<step>/state.json`)；
//! - 都不设 → [`crate::workflows::pipeline::run_pipeline`] (真源 state.json)。
//! 两者同设在 [`crate::workflows::args::WorkflowArgs::exec_path`] 里报错。
//!
//! 与 [`crate::workflows::continue_pipeline`] 的区别: start 会 import, continue 不 import。

use crate::input::Input;
use crate::workflows::args::ExecPath;
use anyhow::Context;

/// 启动新任务: 导入视频 → 跑完整 pipeline (镜像 TS `cmdStartTask`)。
///
/// 返回新任务的绝对 video_dir (供 RPC/CLI 层用于跳转/提示)。
pub fn start_workflow(input: &Input) -> anyhow::Result<String> {
    let st =
        crate::workflows::import::download::import_video(input).context("import_video 失败")?;
    println!("[cli] 导入完成, video_dir = {}", st.workflow.video_dir);
    let path = match input.workflow.as_ref() {
        Some(w) => w.exec_path()?,
        None => ExecPath::Serial,
    };
    match path {
        ExecPath::Engine => {
            // 引擎路径开发中（aa-workflow 集成暂停，见 integration/workflow/mod.rs）。
            // 原接线（恢复时还原）：
            // println!("[cli] 引擎模式 (aa-workflow, 真源 events.jsonl)");
            // crate::integration::workflow::run_workflow_engine(
            //     &st.workflow.video_dir,
            //     &crate::integration::workflow::EngineOptions::from_input(input),
            // )
            // .context("run_workflow_engine 失败")?;
            println!("[cli] 引擎模式开发中, 暂不可用");
            anyhow::bail!("workflow.engine 引擎路径开发中");
        }
        ExecPath::Parallel => {
            println!("[cli] 并行模式 (实验, DAG 调度, 真源各 <step>/state.json)");
            crate::workflows::parallel::run_pipeline_parallel(&st.workflow.video_dir, input)
                .context("run_pipeline_parallel 失败")?;
        }
        ExecPath::Serial => {
            crate::workflows::pipeline::run_pipeline(&st.workflow.video_dir, input)
                .context("run_pipeline 失败")?;
        }
    }
    println!("[cli] 完成: {}", st.workflow.video_dir);
    Ok(st.workflow.video_dir)
}
