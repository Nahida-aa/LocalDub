//! workflow 顶层派发器 (镜像 TS `packages/core/cmd/workflows/workflow.ts` 的 `cmdTask`)。
//!
//! 根据 `input.workflow.action` 决定做什么（import_and_start / start / resume / import），
//! 由 `input.workflow.is_enqueue` 决定本地执行还是交给主服务器队列——两者正交，
//! 不再用 enqueue_* 变体表达。纯查询动作（get_group_list / list_queue）已拆到
//! [`crate::cmd::workflows::query`]（`command=query`）；针对单任务的非工作流动作
//! （status / get_workflow_ctx / generate_meta）归到
//! [`crate::cmd::workflows::video_action`]。
//!
//! 这是 `cmd/workflows` 的执行总入口, 对应 TS `run-workflow.ts` 里对 `cmdTask` 的调用。

use crate::cmd::workflows::{import_and_start_workflow, resume_workflow, start_workflow};
use crate::input::Input;
use crate::workflow::args::WorkflowAction;
use anyhow::Context;

/// 任务命令总派发 (镜像 `cmdTask`)。
pub async fn cmd_workflow(input: &Input) -> anyhow::Result<()> {
    let workflow = input.workflow.clone().unwrap_or_default();
    let action = workflow.action.unwrap_or(WorkflowAction::ImportAndStart);

    if workflow.is_enqueue {
        // is_enqueue 与 action 正交；不可入队的 action 在这里明确报错。
        if !action.enqueueable() {
            anyhow::bail!("action {action:?} 不支持入队 (is_enqueue=true)");
        }
        return enqueue(input, action);
    }

    // 本地直接执行。
    match action {
        WorkflowAction::ImportAndStart => {
            import_and_start_workflow(input)
                .await
                .context("import_and_start_workflow 失败")?;
        }
        // 对已导入目录的执行动作：start=从头全量（不 import、不 setCtx），
        // resume=setCtx + DagPlan 续跑。video_dir 运行时检查（WorkflowArgs 里是 Option）。
        WorkflowAction::Start => {
            start_workflow(input).await.context("start_workflow 失败")?;
        }
        WorkflowAction::Resume => {
            resume_workflow(input).await.context("resume_workflow 失败")?;
        }
        WorkflowAction::Import => {
            crate::workflow::import::download::import_video(input)
                .map(|_| ())
                .context("import_video 失败")?;
        }
        WorkflowAction::ImportAndStartByDir => {
            let r = crate::cmd::workflows::enqueue_dir::run_dir_locally(input)
                .await
                .context("import_and_start_by_dir 本地批量执行失败")?;
            println!(
                "[cli] 批量完成: scanned={} done={} failed={} skipped={} errors={}",
                r.scanned, r.done, r.failed, r.skipped, r.errors
            );
            if !r.failed_videos.is_empty() {
                println!("[cli] 失败视频: {}", r.failed_videos.join(", "));
            }
        }
    }
    Ok(())
}

/// 把一个 action 交给主服务器队列。RPC 方法名按 action 选，body 保持真实 action。
fn enqueue(input: &Input, action: WorkflowAction) -> anyhow::Result<()> {
    match action {
        WorkflowAction::ImportAndStart => crate::cmd::workflows::enqueue::enqueue_workflow(input, true)
            .map(|_| ())
            .context("入队 import_and_start 失败"),
        WorkflowAction::Resume => crate::cmd::workflows::enqueue::enqueue_workflow(input, false)
            .map(|_| ())
            .context("入队 resume 失败"),
        WorkflowAction::Import => crate::cmd::workflows::enqueue::enqueue_import(input)
            .map(|_| ())
            .context("入队 import 失败"),
        WorkflowAction::ImportAndStartByDir => crate::cmd::workflows::enqueue::enqueue_dir(input)
            .map(|_| ())
            .context("入队 import_and_start_by_dir 失败"),
        _ => anyhow::bail!("action {action:?} 不支持入队"),
    }
}
