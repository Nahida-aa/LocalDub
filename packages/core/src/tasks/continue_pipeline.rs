//! continue 派发器 (镜像 TS `packages/core/tasks/continue.ts` 的 `continuePipeline`)。
//!
//! 与 [`crate::stages::pipeline::run_pipeline`] 的区别: 不是从序列头跑全部, 而是:
//! - `task.continueFrom` 存在 → 从该 stage 起把后续全部重置为 `pending` 再续跑;
//! - 否则 → 跳过已 `success` 的前缀 stage, 从第一个未完成 stage 续跑;
//! - `task.targetStage` 命中即停 (truncate 序列)。
//!
//! 不调用 `import_video` (TS `continuePipeline` 也不调), caller 应已保证 task 目录存在。

use crate::context::read_ctx;
use crate::stages::pipeline::{has_handler, run_stage};
use crate::stages::utils::{
    StagePatch, StageStatus, emit_log, now_iso, set_stage_anyhow, set_task_anyhow,
};

/// 续跑 pipeline (镜像 TS `continuePipeline`)。
pub fn continue_pipeline(task_dir: &str) -> anyhow::Result<()> {
    emit_log(Some(task_dir), "continue_pipeline: start");

    let ctx = read_ctx(task_dir).map_err(anyhow::Error::msg)?;
    let pipeline = ctx.pipeline.clone();
    let task_id = ctx.task.id.clone();
    let stages = crate::stages::get_stages(&ctx);

    // continueFrom / targetStage 从 ctx.input.task 读取 (镜像 TS ctx.input?.task)
    let continue_from = ctx
        .input
        .get("task")
        .and_then(|v| v.get("continueFrom"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let target_stage = ctx
        .input
        .get("task")
        .and_then(|v| v.get("targetStage"))
        .and_then(|v| v.as_str())
        .map(String::from);

    // targetStage 不在序列中则告警忽略 (镜像 TS)
    if let Some(ts) = &target_stage {
        if !stages.iter().any(|s| s == ts) {
            emit_log(
                Some(task_dir),
                &format!("[WARN] targetStage \"{ts}\" 不在 {pipeline} pipeline 中, 忽略"),
            );
        }
    }

    let mut start_idx = 0usize;

    if let Some(cf) = &continue_from {
        start_idx = stages
            .iter()
            .position(|s| s == cf)
            .ok_or_else(|| anyhow::anyhow!("Unknown stage \"{cf}\""))?;
        // 从 continueFrom 起把后续全部重置为 pending (镜像 TS for i=startIdx.. reset)。
        // 注: StagePatch 仅支持"设置"不支持"清空"可选字段, 故只改 status; 实际运行时
        // run_stage 会重新写入 started_at / completed_at, 残留的旧时间戳无害。
        for i in start_idx..stages.len() {
            set_stage_anyhow(
                task_dir,
                &stages[i],
                StagePatch {
                    status: Some(StageStatus::Pending),
                    ..Default::default()
                },
            )?;
        }
        emit_log(
            Some(task_dir),
            &format!(
                "[Pipeline] Resetting from \"{cf}\" ({} stage(s)), resuming...",
                stages.len() - start_idx
            ),
        );
    } else {
        // 无 continueFrom → 跳过已完成前缀, 从第一个未完成 stage 续跑
        let existing: std::collections::HashMap<String, StageStatus> = ctx
            .stages
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|s| (s.name, s.status))
            .collect();
        for (i, s) in stages.iter().enumerate() {
            if existing.get(s) != Some(&StageStatus::Success) {
                start_idx = i;
                break;
            }
        }
        if start_idx == 0 {
            emit_log(Some(task_dir), "[Pipeline] continue from beginning");
        } else {
            emit_log(
                Some(task_dir),
                &format!(
                    "[Pipeline] Skipping {start_idx} completed stage(s), resuming from \"{}\"",
                    stages[start_idx]
                ),
            );
        }
    }

    set_task_anyhow(
        task_dir,
        crate::stages::utils::TaskPatch {
            status: Some("running".to_string()),
            started_at: Some(now_iso()),
            current_stage: Some(Some(stages[start_idx].clone())),
            ..Default::default()
        },
    )?;

    emit_log(
        Some(task_dir),
        &format!("[Pipeline] Running runStages: {:?}", &stages[start_idx..]),
    );

    for i in start_idx..stages.len() {
        let stage = &stages[i];

        if !has_handler(stage) {
            emit_log(
                Some(task_dir),
                &format!("[WARN] [Pipeline] No handler for stage {stage}, skipping"),
            );
            continue;
        }

        set_stage_anyhow(
            task_dir,
            stage,
            StagePatch {
                status: Some(StageStatus::Running),
                started_at: Some(now_iso()),
                last_message: Some(format!("Starting {stage}...")),
                ..Default::default()
            },
        )?;
        set_task_anyhow(
            task_dir,
            crate::stages::utils::TaskPatch {
                status: Some("running".to_string()),
                current_stage: Some(Some(stage.clone())),
                ..Default::default()
            },
        )?;
        emit_log(Some(task_dir), &format!("[Pipeline] Running {stage}"));

        match run_stage(stage, task_dir) {
            Ok(()) => {
                if let Some(ts) = &target_stage {
                    if stage == ts {
                        emit_log(
                            Some(task_dir),
                            &format!("[Pipeline] 达到目标步骤 \"{ts}\", 停止"),
                        );
                        break;
                    }
                }
            }
            Err(e) => {
                let msg = e.to_string();
                emit_log(
                    Some(task_dir),
                    &format!("[ERROR] [Pipeline] Stage {stage} failed: {msg}"),
                );
                set_stage_anyhow(
                    task_dir,
                    stage,
                    StagePatch {
                        status: Some(StageStatus::Failed),
                        error_message: Some(msg.clone()),
                        completed_at: Some(now_iso()),
                        ..Default::default()
                    },
                )?;
                set_task_anyhow(
                    task_dir,
                    crate::stages::utils::TaskPatch {
                        status: Some("failed".to_string()),
                        error_message: Some(msg),
                        ..Default::default()
                    },
                )?;
                return Err(e);
            }
        }
    }

    set_task_anyhow(
        task_dir,
        crate::stages::utils::TaskPatch {
            status: Some("success".to_string()),
            completed_at: Some(now_iso()),
            current_stage: Some(None),
            ..Default::default()
        },
    )?;
    emit_log(
        Some(task_dir),
        &format!("[Pipeline] Task {task_id} completed"),
    );
    Ok(())
}
