//! start 派发器 (镜像 TS `packages/core/cmd/workflows/startTask.ts` 的 `cmdStartTask`)。
//!
//! 语义: 无论是否带 `videoDir`, 都按 `input.workflow.url` 重新 `import_video` 生成/复用
//! 任务目录, 再从头跑完整 pipeline (`run_pipeline`)。`videoDir` 在此路径被忽略
//! (对齐 TS: start 分支忽略 videoDir, 不会续跑已有任务)。
//!
//! 与 [`crate::workflows::continue_pipeline`] 的区别: start 会 import, continue 不 import。

use crate::input::Input;
use anyhow::Context;

/// 启动新任务: 导入视频 → 跑完整 pipeline (镜像 TS `cmdStartTask`)。
///
/// 返回新任务的绝对 workflow_dir (供 RPC/CLI 层用于跳转/提示)。
pub fn start_workflow(input: &Input) -> anyhow::Result<String> {
    let ctx =
        crate::workflows::import::download::import_video(input).context("import_video 失败")?;
    println!(
        "[cli] 导入完成, workflow_dir = {}",
        ctx.workflow.workflow_dir
    );
    crate::workflows::pipeline::run_pipeline(&ctx.workflow.workflow_dir)
        .context("run_pipeline 失败")?;
    println!("[cli] 完成: {}", ctx.workflow.workflow_dir);
    Ok(ctx.workflow.workflow_dir)
}
