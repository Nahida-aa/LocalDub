//! continue 命令派发器 (镜像 TS `packages/core/cmd/workflows/continueTask.ts` 的 `cmdContinueTask`)。
//!
//! 语义 (对齐 TS):
//! 1. 要求 `input.workflow.video_dir` 指定已有任务目录, 否则报错;
//! 2. `setCtx` — 把当前 `input` 写回 `input.json`
//!    (TS `setCtx(video_dir, { input })`), 这样 `continue_from` / `regenIndices` 等
//!    以当前 `input.jsonc` 为准, 而非沿用陈旧的落盘配置;
//! 3. 读出合并后的 ctx, 交给 [`crate::workflow::continue_pipeline`] 续跑。
//!
//! `input.workflow.engine = true` 时走引擎路径——**开发中，暂不可用**
//! （Engine 臂现打印「开发中」并报错）。将来接入 DAG 中间件方案时复用
//! FsRunStore，`continue_from`/`target_step` 由 DAG 中间件承载；input.json
//! 的 setCtx 合并照常执行。
//!
//! 与 [`crate::cmd::workflows::start`] 的区别: continue 不重新 import, 直接沿用既有任务目录。

use crate::input::Input;
use crate::state::write_input;
use crate::workflow::args::ExecPath;
use crate::workflow::continue_pipeline;
use anyhow::Context;

/// 续跑已有任务 (镜像 TS `cmdContinueTask`): 合并 input → 跑 continue_pipeline。
pub async fn continue_workflow(input: &Input) -> anyhow::Result<()> {
    let video_dir = input
        .workflow
        .as_ref()
        .and_then(|t| t.video_dir.clone())
        .ok_or_else(|| {
            anyhow::anyhow!("continue 模式需要 input.workflow.video_dir 指定已有任务目录")
        })?;

    if !crate::state::state_path(&video_dir).exists() {
        return Err(anyhow::anyhow!(
            "continue 模式找不到 {}, 确认 video_dir 正确",
            crate::state::state_path(&video_dir).display()
        ));
    }

    // setCtx: 当前 input 落盘到 input.json (镜像 TS setCtx(video_dir, { input }))。
    // 落盘后不再读回——本次续跑用的就是这份 input。
    write_input(&video_dir, input)
        .map_err(|e| anyhow::anyhow!("写回 {}/input.json 失败: {e}", video_dir))?;

    let continue_from = input
        .workflow
        .as_ref()
        .and_then(|t| t.continue_from.clone());
    let label = continue_from
        .as_ref()
        .map(|cf| format!(" from \"{cf:?}\""))
        .unwrap_or_default();
    println!("[cli] 续跑模式, video_dir = {}{}", video_dir, label);

    let path = match input.workflow.as_ref() {
        Some(w) => w.exec_path()?,
        None => ExecPath::Serial,
    };
    match path {
        ExecPath::Engine => {
            // 引擎路径开发中：将来接入 DAG 中间件方案时直接复用 FsRunStore。
            println!("[cli] 引擎模式开发中, 暂不可用");
            anyhow::bail!("workflow.engine 引擎路径开发中");
        }
        ExecPath::Parallel => {
            println!("[cli] 实验性并行 (DAG 调度, 按步跳过已成功步骤)");
            crate::workflow::parallel::run_pipeline_parallel(input)
                .await
                .context("run_pipeline_parallel 失败")?;
        }
        ExecPath::Serial => {
            continue_pipeline(input).await.context("continue_pipeline 失败")?;
        }
    }
    println!("[cli] 完成: {}", video_dir);
    Ok(())
}

