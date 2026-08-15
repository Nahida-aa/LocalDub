//! start 派发器 (镜像 TS `packages/core/cmd/tasks/startTask.ts` 的 `cmdStartTask`)。
//!
//! 语义: 无论是否带 `taskDir`, 都按 `input.task.url` 重新 `import_video` 生成/复用
//! 任务目录, 再从头跑完整 pipeline (`run_pipeline`)。`taskDir` 在此路径被忽略
//! (对齐 TS: start 分支忽略 taskDir, 不会续跑已有任务)。
//!
//! 与 [`crate::tasks::continue_pipeline`] 的区别: start 会 import, continue 不 import。

use crate::input::Input;
use anyhow::Context;

/// 启动新任务: 导入视频 → 跑完整 pipeline (镜像 TS `cmdStartTask`)。
pub fn start_task(input: &Input) -> anyhow::Result<()> {
    let ctx = crate::tasks::import::download::import_video(input).context("import_video 失败")?;
    println!("[cli] 导入完成, task_dir = {}", ctx.task.task_dir);
    crate::tasks::pipeline::run_pipeline(&ctx.task.task_dir).context("run_pipeline 失败")?;
    println!("[cli] 完成: {}", ctx.task.task_dir);
    Ok(())
}
