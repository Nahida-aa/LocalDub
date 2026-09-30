//! continue 派发器 (镜像 TS `packages/core/workflows/continue.ts` 的 `continuePipeline`)。
//!
//! 与 [`crate::workflows::pipeline::run_pipeline`] 的区别: 不是从序列头跑全部, 而是:
//! - `workflow.continueFrom` 存在 → 从该 step 起把后续全部重置为 `pending` 再续跑;
//! - 否则 → 跳过已 `success` 的前缀 step, 从第一个未完成 step 续跑;
//! - `workflow.targetStep` 命中即停 (truncate 序列)。
//!
//! `input` 由调用者传入 (而非内部从磁盘 `read_input`), 以保证续跑使用的是调用者
//! 选定的那份配置 (例如 cli 从 `input.jsonc` 解析后写入的 input.json), 而不是可能
//! 陈旧的持久化配置。不调用 `import_video` (TS `continuePipeline` 也不调), caller
//! 应已保证 workflow 目录存在。

use crate::input::Input;
use crate::steps::utils::{now_iso, set_step_anyhow, set_workflow_anyhow, StepPatch, StepStatus};
use crate::workflows::pipeline::run_step;

/// 续跑 output_mode (镜像 TS `continuePipeline`)。
pub fn continue_pipeline(video_dir: &str, input: &Input) -> anyhow::Result<()> {
    // 进入 workflow span: 携带 video_dir 供 WorkflowFileLayer 落盘。
    let _workflow_guard = tracing::info_span!("workflow", video_dir = video_dir).entered();
    tracing::info!(target: "output_mode", "continue_pipeline: start");

    let video_id = crate::steps::utils::video_id(video_dir).unwrap_or_default();
    let steps = crate::steps::get_steps(input)?;

    // continueFrom / targetStep 从 input.workflow 读取 (镜像 TS ctx.input?.workflow)
    let continue_from = input
        .workflow
        .as_ref()
        .and_then(|w| w.continue_from.as_ref())
        .map(|s| s.as_str().to_string());
    let target_step = input
        .workflow
        .as_ref()
        .and_then(|w| w.target_step.as_ref())
        .map(|s| s.as_str().to_string());

    // targetStep 不在序列中则告警忽略 (镜像 TS)
    if let Some(ts) = &target_step {
        if !steps.iter().any(|s| s.as_str() == ts) {
            tracing::info!(target: "output_mode", "[WARN] targetStep \"{ts}\" 不在序列中, 忽略");
        }
    }

    let mut start_idx = 0usize;

    if let Some(cf) = &continue_from {
        start_idx = steps
            .iter()
            .position(|s| s.as_str() == cf)
            .ok_or_else(|| anyhow::anyhow!("Unknown step \"{cf}\""))?;
        // 从 continueFrom 起把后续全部重置为 pending (镜像 TS for i=startIdx.. reset)。
        // 注: StepPatch 仅支持"设置"不支持"清空"可选字段, 故只改 status; 实际运行时
        // run_step 会重新写入 started_at / completed_at, 残留的旧时间戳无害。
        for i in start_idx..steps.len() {
            set_step_anyhow(
                video_dir,
                steps[i].as_str(),
                StepPatch {
                    status: Some(StepStatus::Pending),
                    ..Default::default()
                },
            )?;
        }
        tracing::info!(target: "output_mode",
            "Resetting from \"{cf}\" ({} step(s)), resuming...",
            steps.len() - start_idx
        );
    } else {
        // 无 continueFrom → 跳过已完成前缀, 从第一个未完成 step 续跑
        let existing = crate::state::read_steps_state(video_dir).map_err(anyhow::Error::msg)?;
        for (i, s) in steps.iter().enumerate() {
            let done = existing
                .0
                .get(s)
                .is_some_and(|st| st.status == StepStatus::Success);
            if !done {
                start_idx = i;
                break;
            }
        }
        if start_idx == 0 {
            tracing::info!(target: "output_mode", "continue from beginning");
        } else {
            tracing::info!(target: "output_mode",
                "Skipping {start_idx} completed step(s), resuming from \"{}\"",
                steps[start_idx]
            );
        }
    }

    set_workflow_anyhow(
        video_dir,
        crate::steps::utils::WorkflowPatch {
            status: Some("running".to_string()),
            started_at: Some(now_iso()),
            current_step: Some(Some(steps[start_idx].as_str().to_string())),
            ..Default::default()
        },
    )?;

    tracing::info!(target: "output_mode",
        "Running runSteps: {:?}",
        &steps[start_idx..]
    );

    for i in start_idx..steps.len() {
        let step = steps[i];
        let step_str = step.as_str();

        set_step_anyhow(
            video_dir,
            step_str,
            StepPatch {
                status: Some(StepStatus::Running),
                started_at: Some(now_iso()),
                last_message: Some(format!("Starting {step_str}...")),
                ..Default::default()
            },
        )?;
        set_workflow_anyhow(
            video_dir,
            crate::steps::utils::WorkflowPatch {
                status: Some("running".to_string()),
                current_step: Some(Some(step_str.to_string())),
                ..Default::default()
            },
        )?;
        tracing::info!(target: "output_mode", "Running {step_str}");

        // 每步重读 input.json：前序步骤可能改配置（continue 也可能中途改过）。
        let input = crate::state::read_input(video_dir).map_err(anyhow::Error::msg)?;
        match run_step(step, video_dir, &input) {
            Ok(()) => {
                if let Some(ts) = &target_step {
                    if step_str == ts {
                        tracing::info!(target: "output_mode", "达到目标步骤 \"{ts}\", 停止");
                        break;
                    }
                }
            }
            Err(e) => {
                let msg = e.to_string();
                tracing::error!(target: "output_mode", "Step {step_str} failed: {msg}");
                set_step_anyhow(
                    video_dir,
                    step_str,
                    StepPatch {
                        status: Some(StepStatus::Failed),
                        error_message: Some(msg.clone()),
                        completed_at: Some(now_iso()),
                        ..Default::default()
                    },
                )?;
                set_workflow_anyhow(
                    video_dir,
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
        video_dir,
        crate::steps::utils::WorkflowPatch {
            status: Some("success".to_string()),
            completed_at: Some(now_iso()),
            current_step: Some(None),
            ..Default::default()
        },
    )?;
    tracing::info!(target: "output_mode", "Workflow {video_id} completed");
    Ok(())
}
