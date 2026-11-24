//! continue 命令派发器 (镜像 TS `packages/core/cmd/workflows/continueTask.ts` 的 `cmdContinueTask`)。
//!
//! 语义 (对齐 TS):
//! 1. 要求 `input.workflow.video_dir` 指定已有任务目录, 否则报错;
//! 2. `setCtx` — 把当前 `input` 写回 `input.json`
//!    (TS `setCtx(video_dir, { input })`), 这样 `resume_from` / `regenIndices` 等
//!    以当前 `input.jsonc` 为准, 而非沿用陈旧的落盘配置;
//! 3. 读出合并后的 ctx，走引擎 resume 续跑。
//!
//! 引擎路径：新 run_id + DagPlan（source_run_id / from / goals）从源 run 复用
//! 未失效步骤，`resume_from` / `target_step` 由 DAG 中间件承载；input.json 的
//! setCtx 合并照常执行。过渡的串行 resume_pipeline / 实验性 parallel 已下线。
//!
//! 与 [`crate::cmd::workflows::start`] 的区别: continue 不重新 import, 直接沿用既有任务目录。

use crate::input::Input;
use crate::state::write_input;

/// 续跑已有任务 (镜像 TS `cmdContinueTask`): 合并 input → 引擎 resume。
pub async fn resume_workflow(input: &Input) -> anyhow::Result<()> {
    let video_dir = input
        .workflow
        .as_ref()
        .and_then(|t| t.video_dir.clone())
        .ok_or_else(|| {
            anyhow::anyhow!("resume 模式需要 input.workflow.video_dir 指定已有任务目录")
        })?;

    if !crate::state::state_path(&video_dir).exists() {
        return Err(anyhow::anyhow!(
            "resume 模式找不到 {}, 确认 video_dir 正确",
            crate::state::state_path(&video_dir).display()
        ));
    }

    // setCtx: 当前 input 落盘到 input.json (镜像 TS setCtx(video_dir, { input }))。
    // 写回前把 video_dir 规范化为绝对路径：调用方（input.jsonc）给的多是相对路径，
    // 原样落盘会让下游步骤（尤其 concat 类）把相对路径二次拼接成双份。
    // resolve_path 对已是绝对路径的值原样返回。
    let mut settled = input.clone();
    if let Some(w) = settled.workflow.as_mut() {
        w.video_dir = Some(
            config_rs::root::resolve_path(&video_dir)
                .to_string_lossy()
                .into_owned(),
        );
    }
    write_input(&video_dir, &settled)
        .map_err(|e| anyhow::anyhow!("写回 {}/input.json 失败: {e}", video_dir))?;

    let resume_from: Vec<crate::workflow::args::StepName> = input
        .workflow
        .as_ref()
        .map(|t| t.resume_from.clone())
        .unwrap_or_default();
    let label = if resume_from.is_empty() {
        String::new()
    } else {
        format!(" from {resume_from:?}")
    };
    println!("[cli] 续跑模式, video_dir = {}{}", video_dir, label);

    // 引擎 resume：读回落定的绝对 video_dir，构造 run input，
    // 用新 run_id + DagPlan(source_run_id) 从源 run 复用未失效步骤。
    let settled = crate::state::read_input(&video_dir).map_err(anyhow::Error::msg)?;
    let wfi = crate::input::WorkflowInput::try_from(settled).map_err(anyhow::Error::msg)?;
    println!("[cli] 引擎 resume (新 run + DagPlan, 真源事件日志)");
    crate::integration::workflow::engine::run_engine_resume(&wfi).await?;
    println!("[cli] 完成: {}", video_dir);
    Ok(())
}

