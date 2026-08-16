//! task 顶层派发器 (镜像 TS `packages/core/cmd/tasks/task.ts` 的 `cmdTask`)。
//!
//! 根据 `input.task.action` 分派:
//! - `start`          → [`crate::cmd::tasks::start_task`] (重新 import + 跑全 pipeline)
//! - `continue`       → [`crate::cmd::tasks::continue_task`] (合并 input 续跑)
//! - `get_group_list`→ [`crate::cmd::tasks::get_task::get_group_list`]
//! - `get_task_ctx`   → 读 ctx.json 并打印
//! - `status`         → 读 ctx.json 并打印各 stage 状态
//!
//! 这是 `cmd/tasks` 的总入口, 对应 TS `run-task.ts` 里对 `cmdTask` 的调用。

use crate::cmd::tasks::get_task::get_group_list;
use crate::cmd::tasks::{continue_task, start_task};
use crate::context::read_ctx;
use crate::input::Input;
use crate::tasks::args::TaskAction;
use anyhow::Context;

/// 任务命令总派发 (镜像 TS `cmdTask`)。
pub fn cmd_task(input: &Input) -> anyhow::Result<()> {
    let task = input.task.clone().unwrap_or_default();

    match task.action {
        Some(TaskAction::Continue) => {
            continue_task(input).context("continue_task 失败")?;
        }
        Some(TaskAction::Start) => {
            start_task(input).context("start_task 失败")?;
        }
        Some(TaskAction::EnqueueStart) => {
            crate::cmd::tasks::enqueue::enqueue_task(input, true).context("enqueue_start 失败")?;
        }
        Some(TaskAction::EnqueueContinue) => {
            crate::cmd::tasks::enqueue::enqueue_task(input, false).context("enqueue_continue 失败")?;
        }
        Some(TaskAction::GetGroupList) => {
            let groups = get_group_list().map_err(|e| anyhow::anyhow!("{e}"))?;
            let json = serde_json::to_string_pretty(&groups)
                .map_err(|e| anyhow::anyhow!("序列化 group_list 失败: {e}"))?;
            println!("{json}");
        }
        Some(TaskAction::GetTaskCtx) => {
            let task_dir = task
                .task_dir
                .clone()
                .ok_or_else(|| anyhow::anyhow!("get_task_ctx 需要 input.task.taskDir"))?;
            let ctx = read_ctx(&task_dir)
                .map_err(|e| anyhow::anyhow!("读取 {}/ctx.json 失败: {e}", task_dir))?;
            let json = serde_json::to_string_pretty(&ctx)
                .map_err(|e| anyhow::anyhow!("序列化 ctx 失败: {e}"))?;
            println!("{json}");
        }
        Some(TaskAction::Status) => {
            let task_dir = task
                .task_dir
                .clone()
                .ok_or_else(|| anyhow::anyhow!("status 需要 input.task.taskDir"))?;
            let ctx = read_ctx(&task_dir)
                .map_err(|e| anyhow::anyhow!("读取 {}/ctx.json 失败: {e}", task_dir))?;
            println!("task.status = {}", ctx.task.status);
            if let Some(stages) = &ctx.stages {
                for s in stages {
                    println!("  {}: {:?}", s.name, s.status);
                }
            } else {
                println!("  (无 stage 状态)");
            }
        }
        None => {
            // TS: 缺省走 start 分支
            start_task(input).context("start_task 失败 (默认)")?;
        }
    }
    Ok(())
}
