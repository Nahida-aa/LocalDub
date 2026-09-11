//! workflow 顶层派发器 (镜像 TS `packages/core/cmd/workflows/workflow.ts` 的 `cmdTask`)。
//!
//! 根据 `input.workflow.action` 分派:
//! - `start`          → [`crate::cmd::workflows::start_workflow`] (重新 import + 跑全 pipeline)
//! - `continue`       → [`crate::cmd::workflows::continue_workflow`] (合并 input 续跑)
//! - `get_group_list`→ [`crate::cmd::workflows::get_workflow::get_group_list`]
//! - `get_workflow_ctx`   → 读 ctx.json 并打印
//! - `status`         → 读 ctx.json 并打印各 stage 状态
//!
//! 这是 `cmd/workflows` 的总入口, 对应 TS `run-workflow.ts` 里对 `cmdTask` 的调用。

use crate::cmd::workflows::get_workflow::get_group_list;
use crate::cmd::workflows::{continue_workflow, start_workflow};
use crate::context::read_ctx;
use crate::input::Input;
use crate::workflows::args::WorkflowAction;
use anyhow::Context;

/// 任务命令总派发 (镜像 TS `cmdTask`)。
pub fn cmd_workflow(input: &Input) -> anyhow::Result<()> {
    let workflow = input.workflow.clone().unwrap_or_default();

    match workflow.action {
        Some(WorkflowAction::Continue) => {
            continue_workflow(input).context("continue_workflow 失败")?;
        }
        Some(WorkflowAction::Start) => {
            start_workflow(input).context("start_workflow 失败")?;
        }
        Some(WorkflowAction::Import) => {
            crate::cmd::workflows::import::import_workflow(input).context("import_workflow 失败")?;
        }
        Some(WorkflowAction::EnqueueStart) => {
            crate::cmd::workflows::enqueue::enqueue_workflow(input, true).context("enqueue_start 失败")?;
        }
        Some(WorkflowAction::EnqueueContinue) => {
            crate::cmd::workflows::enqueue::enqueue_workflow(input, false).context("enqueue_continue 失败")?;
        }
        Some(WorkflowAction::EnqueueImport) => {
            crate::cmd::workflows::enqueue::enqueue_import(input).context("enqueue_import 失败")?;
        }
        Some(WorkflowAction::ListQueue) => {
            crate::cmd::workflows::enqueue::list_queue().context("list_queue 失败")?;
        }
        Some(WorkflowAction::CancelQueue) => {
            let id = workflow
                .queue_id
                .ok_or_else(|| anyhow::anyhow!("cancel_queue 需要 input.workflow.queueId 指定队列任务 ID"))?;
            crate::cmd::workflows::enqueue::cancel_queue(id).context("cancel_queue 失败")?;
        }
        Some(WorkflowAction::GetGroupList) => {
            let groups = get_group_list().map_err(|e| anyhow::anyhow!("{e}"))?;
            let json = serde_json::to_string_pretty(&groups)
                .map_err(|e| anyhow::anyhow!("序列化 group_list 失败: {e}"))?;
            println!("{json}");
        }
        Some(WorkflowAction::GetWorkflowCtx) => {
            let workflow_dir = workflow
                .workflow_dir
                .clone()
                .ok_or_else(|| anyhow::anyhow!("get_workflow_ctx 需要 input.workflow.workflowDir"))?;
            let ctx = read_ctx(&workflow_dir)
                .map_err(|e| anyhow::anyhow!("读取 {}/ctx.json 失败: {e}", workflow_dir))?;
            let json = serde_json::to_string_pretty(&ctx)
                .map_err(|e| anyhow::anyhow!("序列化 ctx 失败: {e}"))?;
            println!("{json}");
        }
        Some(WorkflowAction::Status) => {
            let workflow_dir = workflow
                .workflow_dir
                .clone()
                .ok_or_else(|| anyhow::anyhow!("status 需要 input.workflow.workflowDir"))?;
            let ctx = read_ctx(&workflow_dir)
                .map_err(|e| anyhow::anyhow!("读取 {}/ctx.json 失败: {e}", workflow_dir))?;
            println!("workflow.status = {}", ctx.workflow.status);
            if let Some(stages) = &ctx.stages {
                for s in stages {
                    println!("  {}: {:?}", s.name, s.status);
                }
            } else {
                println!("  (无 stage 状态)");
            }
        }
        Some(WorkflowAction::GenerateMeta) => {
            let workflow_dir = workflow
                .workflow_dir
                .clone()
                .ok_or_else(|| anyhow::anyhow!("generate_meta 需要 input.workflow.workflowDir"))?;
            crate::cmd::workflows::meta::generate_meta(&workflow_dir).context("generate_meta 失败")?;
        }
        None => {
            // TS: 缺省走 start 分支
            start_workflow(input).context("start_workflow 失败 (默认)")?;
        }
    }
    Ok(())
}
