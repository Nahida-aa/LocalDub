//! continue 命令派发器 (镜像 TS `packages/core/cmd/workflows/continueTask.ts` 的 `cmdContinueTask`)。
//!
//! 语义 (对齐 TS):
//! 1. 要求 `input.workflow.videoDir` 指定已有任务目录, 否则报错;
//! 2. `setCtx` — 把当前 `input` 合并写回 `ctx.json` 的 `input` 字段
//!    (TS `setCtx(videoDir, { input })`), 这样 `continueFrom` / `regenIndices` 等
//!    以当前 `input.jsonc` 为准, 而非沿用陈旧的持久化 ctx;
//! 3. 读出合并后的 ctx, 交给 [`crate::workflows::continue_pipeline`] 续跑。
//!
//! 与 [`crate::cmd::workflows::start`] 的区别: continue 不重新 import, 直接沿用既有任务目录。

use crate::context::{read_ctx, write_ctx};
use crate::input::Input;
use crate::workflows::continue_pipeline;
use anyhow::Context;

/// 续跑已有任务 (镜像 TS `cmdContinueTask`): 合并 input → 跑 continue_pipeline。
pub fn continue_workflow(input: &Input) -> anyhow::Result<()> {
    let workflow_dir = input
        .workflow
        .as_ref()
        .and_then(|t| t.workflow_dir.clone())
        .ok_or_else(|| {
            anyhow::anyhow!("continue 模式需要 input.workflow.videoDir 指定已有任务目录")
        })?;

    if !std::path::Path::new(&workflow_dir)
        .join("ctx.json")
        .exists()
    {
        return Err(anyhow::anyhow!(
            "continue 模式找不到 {}/ctx.json, 确认 videoDir 正确",
            workflow_dir
        ));
    }

    // setCtx: 把当前 input 合并进 ctx.json 的 input 字段 (镜像 TS setCtx(videoDir, { input }))
    let mut ctx = read_ctx(&workflow_dir)
        .map_err(|e| anyhow::anyhow!("读取 {}/ctx.json 失败: {e}", workflow_dir))?;
    ctx.input =
        serde_json::to_value(input).map_err(|e| anyhow::anyhow!("序列化 input 失败: {e}"))?;
    write_ctx(&workflow_dir, &ctx)
        .map_err(|e| anyhow::anyhow!("写回 {}/ctx.json 失败: {e}", workflow_dir))?;

    let continue_from = input
        .workflow
        .as_ref()
        .and_then(|t| t.continue_from.clone());
    let label = continue_from
        .as_ref()
        .map(|cf| format!(" from \"{cf:?}\""))
        .unwrap_or_default();
    println!("[cli] 续跑模式, workflow_dir = {}{}", workflow_dir, label);

    continue_pipeline(&ctx).context("continue_pipeline 失败")?;
    println!("[cli] 完成: {}", workflow_dir);
    Ok(())
}
