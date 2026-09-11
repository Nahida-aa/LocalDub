//! continue 派发器 (镜像 TS `packages/core/workflows/continue.ts` 的 `continuePipeline`)。
//!
//! 与 [`crate::workflows::pipeline::run_pipeline`] 的区别: 不是从序列头跑全部, 而是:
//! - `workflow.continueFrom` 存在 → 从该 step 起把后续全部重置为 `pending` 再续跑;
//! - 否则 → 跳过已 `success` 的前缀 step, 从第一个未完成 step 续跑;
//! - `workflow.targetStep` 命中即停 (truncate 序列)。
//!
//! ctx 由调用者传入 (而非内部从磁盘 `read_ctx`), 以保证续跑使用的是调用者选定的
//! 那份 ctx (例如 cli 从 `input.jsonc` 解析后写入的 ctx), 而不是可能陈旧的持久化 ctx。
//! 不调用 `import_video` (TS `continuePipeline` 也不调), caller 应已保证 workflow 目录存在。

use crate::context::WorkflowCtx;
use crate::steps::utils::{now_iso, set_step_anyhow, set_workflow_anyhow, StepPatch, StepStatus};
use crate::workflows::pipeline::{has_handler, run_step};

/// 续跑 pipeline (镜像 TS `continuePipeline`)。
///
/// `ctx` 必须由调用者先 `read_ctx(workflow_dir)` (或自行构造) 后传入; 内部不再读磁盘,
/// 避免续跑基于陈旧 ctx。
pub fn continue_pipeline(ctx: &WorkflowCtx) -> anyhow::Result<()> {
    let workflow_dir = ctx.workflow.workflow_dir.as_str();
    // 进入 workflow span: 携带 workflow_dir 供 WorkflowFileLayer 落盘。
    let _workflow_guard = tracing::info_span!("workflow", workflow_dir = workflow_dir).entered();
    tracing::info!(target: "pipeline", "continue_pipeline: start");

    let pipeline = ctx.pipeline.clone();
    let video_id = ctx.workflow.id.clone();
    let steps = crate::steps::get_steps(&ctx);

    // continueFrom / targetStep 从 ctx.input.workflow 读取 (镜像 TS ctx.input?.workflow)
    let continue_from = ctx
        .input
        .get("workflow")
        .and_then(|v| v.get("continueFrom"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let target_step = ctx
        .input
        .get("workflow")
        .and_then(|v| v.get("targetStep"))
        .and_then(|v| v.as_str())
        .map(String::from);

    // targetStep 不在序列中则告警忽略 (镜像 TS)
    if let Some(ts) = &target_step {
        if !steps.iter().any(|s| s == ts) {
            tracing::info!(target: "pipeline",
                "[WARN] targetStep \"{ts}\" 不在 {pipeline} pipeline 中, 忽略"
            );
        }
    }

    let mut start_idx = 0usize;

    if let Some(cf) = &continue_from {
        start_idx = steps
            .iter()
            .position(|s| s == cf)
            .ok_or_else(|| anyhow::anyhow!("Unknown step \"{cf}\""))?;
        // 从 continueFrom 起把后续全部重置为 pending (镜像 TS for i=startIdx.. reset)。
        // 注: StepPatch 仅支持"设置"不支持"清空"可选字段, 故只改 status; 实际运行时
        // run_step 会重新写入 started_at / completed_at, 残留的旧时间戳无害。
        for i in start_idx..steps.len() {
            set_step_anyhow(
                workflow_dir,
                &steps[i],
                StepPatch {
                    status: Some(StepStatus::Pending),
                    ..Default::default()
                },
            )?;
        }
        tracing::info!(target: "pipeline",
            "Resetting from \"{cf}\" ({} step(s)), resuming...",
            steps.len() - start_idx
        );
    } else {
        // 无 continueFrom → 跳过已完成前缀, 从第一个未完成 step 续跑
        let existing: std::collections::HashMap<String, StepStatus> = ctx
            .steps
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|s| (s.name, s.status))
            .collect();
        for (i, s) in steps.iter().enumerate() {
            if existing.get(s) != Some(&StepStatus::Success) {
                start_idx = i;
                break;
            }
        }
        if start_idx == 0 {
            tracing::info!(target: "pipeline", "continue from beginning");
        } else {
            tracing::info!(target: "pipeline",
                "Skipping {start_idx} completed step(s), resuming from \"{}\"",
                steps[start_idx]
            );
        }
    }

    set_workflow_anyhow(
        workflow_dir,
        crate::steps::utils::WorkflowPatch {
            status: Some("running".to_string()),
            started_at: Some(now_iso()),
            current_step: Some(Some(steps[start_idx].clone())),
            ..Default::default()
        },
    )?;

    tracing::info!(target: "pipeline",
        "Running runSteps: {:?}",
        &steps[start_idx..]
    );

    for i in start_idx..steps.len() {
        let step = &steps[i];

        if !has_handler(step) {
            tracing::info!(target: "pipeline",
                "[WARN] No handler for step {step}, skipping"
            );
            continue;
        }

        set_step_anyhow(
            workflow_dir,
            step,
            StepPatch {
                status: Some(StepStatus::Running),
                started_at: Some(now_iso()),
                last_message: Some(format!("Starting {step}...")),
                ..Default::default()
            },
        )?;
        set_workflow_anyhow(
            workflow_dir,
            crate::steps::utils::WorkflowPatch {
                status: Some("running".to_string()),
                current_step: Some(Some(step.clone())),
                ..Default::default()
            },
        )?;
        tracing::info!(target: "pipeline", "Running {step}");

        match run_step(step, workflow_dir) {
            Ok(()) => {
                if let Some(ts) = &target_step {
                    if step == ts {
                        tracing::info!(target: "pipeline", "达到目标步骤 \"{ts}\", 停止");
                        break;
                    }
                }
            }
            Err(e) => {
                let msg = e.to_string();
                tracing::error!(target: "pipeline", "Step {step} failed: {msg}");
                set_step_anyhow(
                    workflow_dir,
                    step,
                    StepPatch {
                        status: Some(StepStatus::Failed),
                        error_message: Some(msg.clone()),
                        completed_at: Some(now_iso()),
                        ..Default::default()
                    },
                )?;
                set_workflow_anyhow(
                    workflow_dir,
                    crate::steps::utils::WorkflowPatch {
                        status: Some("failed".to_string()),
                        error_message: Some(msg),
                        ..Default::default()
                    },
                )?;
                return Err(e);
            }
        }
    }

    set_workflow_anyhow(
        workflow_dir,
        crate::steps::utils::WorkflowPatch {
            status: Some("success".to_string()),
            completed_at: Some(now_iso()),
            current_step: Some(None),
            ..Default::default()
        },
    )?;
    tracing::info!(target: "pipeline", "Workflow {video_id} completed");
    Ok(())
}
