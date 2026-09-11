//! import 命令派发器: 只导入任务, 不跑 pipeline。
//!
//! 语义:
//! 1. 按 `input.workflow.url` 下载/拷贝视频 → 探测 (帧率等) → 写 ctx.json;
//! 2. 返回 workflow_dir (绝对路径), 供后续 `continue` 续跑。
//!
//! 与 [`crate::cmd::workflows::start`] 的区别: import 不跑 pipeline, start 会跑。
//! 批量场景可先批量 import (快), 再逐个 enqueue_continue 串行跑重活。

use crate::input::Input;
use anyhow::Context;

/// 导入任务 (不跑 pipeline), 返回 workflow_dir 绝对路径。
pub fn import_workflow(input: &Input) -> anyhow::Result<String> {
    let ctx =
        crate::workflows::import::download::import_video(input).context("import_video 失败")?;
    let workflow_dir = ctx.workflow.workflow_dir;
    println!("[cli] 导入完成, workflow_dir = {workflow_dir}");
    Ok(workflow_dir)
}
