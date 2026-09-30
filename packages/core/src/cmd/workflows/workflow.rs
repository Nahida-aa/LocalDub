//! workflow 顶层派发器 (镜像 TS `packages/core/cmd/workflows/workflow.ts` 的 `cmdTask`)。
//!
//! 根据 `input.workflow.action` 分派:
//! - `start`          → [`crate::cmd::workflows::start_workflow`] (重新 import + 跑全 pipeline)
//! - `continue`       → [`crate::cmd::workflows::continue_workflow`] (合并 input 续跑)
//! - `get_group_list`→ [`crate::cmd::workflows::get_workflow::get_group_list`]
//! - `get_workflow_ctx`   → 读 state.json 并打印
//! - `status`         → 读 state.json 并打印各 step 状态
//!
//! 这是 `cmd/workflows` 的总入口, 对应 TS `run-workflow.ts` 里对 `cmdTask` 的调用。

use crate::cmd::workflows::get_workflow::get_group_list;
use crate::cmd::workflows::{continue_workflow, start_workflow};
use crate::input::Input;
use crate::state::{read_input, read_state, read_steps_state};
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
            crate::cmd::workflows::import::import_workflow(input)
                .context("import_workflow 失败")?;
        }
        Some(WorkflowAction::EnqueueStart) => {
            crate::cmd::workflows::enqueue::enqueue_workflow(input, true)
                .context("enqueue_start 失败")?;
        }
        Some(WorkflowAction::EnqueueContinue) => {
            crate::cmd::workflows::enqueue::enqueue_workflow(input, false)
                .context("enqueue_continue 失败")?;
        }
        Some(WorkflowAction::EnqueueImport) => {
            crate::cmd::workflows::enqueue::enqueue_import(input).context("enqueue_import 失败")?;
        }
        Some(WorkflowAction::EnqueueDir) => {
            crate::cmd::workflows::enqueue::enqueue_dir(input).context("enqueue_dir 失败")?;
        }
        Some(WorkflowAction::ListQueue) => {
            crate::cmd::workflows::enqueue::list_queue().context("list_queue 失败")?;
        }
        Some(WorkflowAction::CancelQueue) => {
            let id = workflow.queue_id.ok_or_else(|| {
                anyhow::anyhow!("cancel_queue 需要 input.workflow.queueId 指定队列任务 ID")
            })?;
            crate::cmd::workflows::enqueue::cancel_queue(id).context("cancel_queue 失败")?;
        }
        Some(WorkflowAction::GetGroupList) => {
            let groups = get_group_list().map_err(|e| anyhow::anyhow!("{e}"))?;
            let json = serde_json::to_string_pretty(&groups)
                .map_err(|e| anyhow::anyhow!("序列化 group_list 失败: {e}"))?;
            println!("{json}");
        }
        Some(WorkflowAction::GetWorkflowCtx) => {
            let video_dir = workflow
                .video_dir
                .clone()
                .ok_or_else(|| anyhow::anyhow!("get_workflow_ctx 需要 input.workflow.videoDir"))?;
            // 三个状态文件各有各的写者，打印时就照它们的本来面目各来一份，
            // 不拼成一个"视图形状"。
            let st = read_state(&video_dir)
                .map_err(|e| anyhow::anyhow!("读取 {video_dir}/state.json 失败: {e}"))?;
            let input = read_input(&video_dir)
                .map_err(|e| anyhow::anyhow!("读取 {video_dir}/input.json 失败: {e}"))?;
            let steps = read_steps_state(&video_dir)
                .map_err(|e| anyhow::anyhow!("读取 {video_dir}/<step>/state.json 失败: {e}"))?;
            let json = serde_json::to_string_pretty(&serde_json::json!({
                "state": st, "input": input, "steps": steps.0,
            }))
            .map_err(|e| anyhow::anyhow!("序列化 ctx 失败: {e}"))?;
            println!("{json}");
        }
        Some(WorkflowAction::Status) => {
            let video_dir = workflow
                .video_dir
                .clone()
                .ok_or_else(|| anyhow::anyhow!("status 需要 input.workflow.videoDir"))?;
            let wf = read_state(&video_dir)
                .map_err(|e| anyhow::anyhow!("读取 {video_dir}/state.json 失败: {e}"))?
                .workflow;
            println!("workflow.status = {}", wf.status);
            for (name, st) in read_steps_state(&video_dir)
                .map_err(|e| anyhow::anyhow!("读取 {video_dir}/<step>/state.json 失败: {e}"))?
                .0
            {
                println!("  {}: {:?}", name.as_str(), st.status);
            }
        }
        Some(WorkflowAction::GenerateMeta) => {
            let video_dir = workflow
                .video_dir
                .clone()
                .ok_or_else(|| anyhow::anyhow!("generate_meta 需要 input.workflow.videoDir"))?;
            crate::cmd::workflows::meta::generate_meta(&video_dir).context("generate_meta 失败")?;
        }
        None => {
            // TS: 缺省走 start 分支
            start_workflow(input).context("start_workflow 失败 (默认)")?;
        }
    }
    Ok(())
}
