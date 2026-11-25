//! workflow 顶层派发器 (镜像 TS `packages/core/cmd/workflows/workflow.ts` 的 `cmdTask`)。
//!
//! 根据 `input.workflow.action` 分派工作流动作（start / resume / import_and_start /
//! enqueue_* 等）。纯查询动作（get_group_list / list_queue）已拆到
//! [`crate::cmd::workflows::query`]（`command=query`）；针对单任务的非工作流动作
//! （status / get_workflow_ctx / generate_meta）归到
//! [`crate::cmd::workflows::video_action`]。
//!
//! 这是 `cmd/workflows` 的执行总入口, 对应 TS `run-workflow.ts` 里对 `cmdTask` 的调用。

use crate::cmd::workflows::{import_and_start_workflow, resume_workflow, start_workflow};
use crate::input::Input;
use crate::workflow::args::WorkflowAction;
use anyhow::Context;

/// 任务命令总派发 (镜像 TS `cmdTask`)。
pub async fn cmd_workflow(input: &Input) -> anyhow::Result<()> {
    let workflow = input.workflow.clone().unwrap_or_default();

    match workflow.action {
        Some(WorkflowAction::ImportAndStart) => {
            import_and_start_workflow(input)
                .await
                .context("import_and_start_workflow 失败")?;
        }
        // 对已导入目录的执行动作：start=从头全量（不 import、不 setCtx），
        // resume=setCtx + DagPlan 续跑。video_dir 运行时检查（WorkflowArgs 里是 Option）。
        Some(WorkflowAction::Start) => {
            start_workflow(input).await.context("start_workflow 失败")?;
        }
        Some(WorkflowAction::Resume) => {
            resume_workflow(input).await.context("resume_workflow 失败")?;
        }
        Some(WorkflowAction::Import) => {
            crate::workflow::import::download::import_video(input)
                .map(|_| ())
                .context("import_video 失败")?;
        }
        Some(WorkflowAction::EnqueueStart) => {
            crate::cmd::workflows::enqueue::enqueue_workflow(input, true)
                .context("enqueue_start 失败")?;
        }
        Some(WorkflowAction::EnqueueResume) => {
            crate::cmd::workflows::enqueue::enqueue_workflow(input, false)
                .context("enqueue_resume 失败")?;
        }
        Some(WorkflowAction::EnqueueImport) => {
            crate::cmd::workflows::enqueue::enqueue_import(input).context("enqueue_import 失败")?;
        }
        Some(WorkflowAction::EnqueueDir) => {
            crate::cmd::workflows::enqueue::enqueue_dir(input).context("enqueue_dir 失败")?;
        }
        Some(WorkflowAction::CancelQueue) => {
            let id = workflow.queue_id.ok_or_else(|| {
                anyhow::anyhow!("cancel_queue 需要 input.workflow.queueId 指定队列任务 ID")
            })?;
            crate::cmd::workflows::enqueue::cancel_queue(id).context("cancel_queue 失败")?;
        }
        None => {
            // 缺省保留旧行为：导入并跑（原 start 分支的语义）。新 start 需显式指定。
            import_and_start_workflow(input)
                .await
                .context("import_and_start_workflow 失败 (默认)")?;
        }
    }
    Ok(())
}
