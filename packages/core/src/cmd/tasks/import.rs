//! import 命令派发器: 只导入任务, 不跑 pipeline。
//!
//! 语义:
//! 1. 按 `input.task.url` 下载/拷贝视频 → 探测 (帧率等) → 写 ctx.json;
//! 2. 返回 task_dir (绝对路径), 供后续 `continue` 续跑。
//!
//! 与 [`crate::cmd::tasks::start`] 的区别: import 不跑 pipeline, start 会跑。
//! 批量场景可先批量 import (快), 再逐个 enqueue_continue 串行跑重活。

use anyhow::Context;
use crate::input::Input;

/// 导入任务 (不跑 pipeline), 返回 task_dir 绝对路径。
pub fn import_task(input: &Input) -> anyhow::Result<String> {
    let ctx = crate::tasks::import::download::import_video(input).context("import_video 失败")?;
    let task_dir = ctx.task.task_dir;
    println!("[cli] 导入完成, task_dir = {task_dir}");
    Ok(task_dir)
}
