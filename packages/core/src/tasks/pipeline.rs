//! pipeline 串行派发器 (镜像 TS `packages/core/tasks/start.ts` 的 `runPipeline`)。
//!
//! 流程: 读 ctx → `get_stages` 得到阶段序列 → 逐个调用 `run_stage` (handler 读 ctx.json
//! 跑完写回) → 每阶段前后用 `set_stage` / `set_task` 标状态, 失败即中断。
//!
//! 本文件原位于 `stages/pipeline.rs`, 后迁移到 `tasks/` (与 start / continue 派发器同层),
//! 因为 `run_pipeline` 属于任务级编排而非某个具体 stage 的实现。
//!
//! 目前 `run_stage` 已注册 separate / separate_after / sf_ocr* / translate / split_audio /
//! asr / asr_fix / tts / mix_audio / mix_video; 后续阶段 (asr_ocr*) 移植后在此登记即可。

use crate::context::read_ctx;
use crate::stages::asr::fix::stage_asr_fix;
use crate::stages::asr::stage_asr;
use crate::stages::get_stages;
use crate::stages::mix_audio::stage_mix_audio;
use crate::stages::mix_video::stage_mix_video;
use crate::stages::separate::{stage_separate, stage_separate_after};
use crate::stages::sf_ocr::{stage_sf_ocr, stage_sf_ocr_fix, stage_sf_ocr_pre};
use crate::stages::split_audio::stage_split_audio;
use crate::stages::translate::stage_translate;
use crate::stages::tts::stage_tts;
use crate::stages::utils::{
    StagePatch, StageStatus, now_iso, set_stage_anyhow, set_task_anyhow,
};

/// 运行完整 pipeline (镜像 TS `runPipeline`)。
pub fn run_pipeline(task_dir: &str) -> anyhow::Result<()> {
    // 进入 task span: 携带 task_dir 供 TaskFileLayer 落盘到 <task_dir>/<tid>.log。
    let _task_guard = tracing::info_span!("task", task_dir = task_dir).entered();
    tracing::info!(target: "pipeline", "run_pipeline: start");

    let ctx = read_ctx(task_dir).map_err(anyhow::Error::msg)?;
    let pipeline = ctx.pipeline.clone();
    let task_id = ctx.task.id.clone();
    let stages = get_stages(&ctx);

    // targetStage 不在序列中则告警忽略 (镜像 TS)
    if let Some(ts) = ctx.input.get("targetStage").and_then(|v| v.as_str()) {
        if !stages.iter().any(|s| s == ts) {
            tracing::info!(target: "pipeline", 
                "[WARN] targetStage \"{ts}\" 不在 {pipeline} pipeline 中, 忽略"
            );
        }
    }

    set_task_anyhow(
        task_dir,
        crate::stages::utils::TaskPatch {
            status: Some("running".to_string()),
            started_at: Some(now_iso()),
            ..Default::default()
        },
    )?;

    for stage in &stages {
        // 先检查 handler 是否存在 (镜像 TS: 无 handler 则 warn + skip, 不标记 running)
        if !has_handler(stage) {
            tracing::info!(target: "pipeline", 
                "[WARN] [Pipeline] No handler for stage {stage}, skipping"
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
        tracing::info!(target: "pipeline", "Running {stage}");

        match run_stage(stage, task_dir) {
            Ok(()) => {
                // 达到 targetStage 即停止 (镜像 TS)
                if let Some(ts) = ctx.input.get("targetStage").and_then(|v| v.as_str()) {
                    if stage == ts {
                        tracing::info!(target: "pipeline", "达到目标步骤 \"{ts}\", 停止");
                        break;
                    }
                }
            }
            Err(e) => {
                let msg = e.to_string();
                tracing::error!(target: "pipeline", "[Pipeline] Stage {stage} failed: {msg}");
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
    tracing::info!(target: "pipeline", "Task {task_id} completed");
    Ok(())
}

/// 是否存在已注册的 handler (镜像 TS `STAGE_HANDLERS[stage]` 是否存在)。
pub fn has_handler(stage: &str) -> bool {
    matches!(
        stage,
        "separate"
            | "separate_after"
            | "sf_ocr_pre"
            | "sf_ocr"
            | "sf_ocr_fix"
            | "translate"
            | "split_audio"
            | "asr"
            | "asr_fix"
            | "tts"
            | "mix_audio"
            | "mix_video"
    )
}

/// 按 stage 名分派到具体 handler (镜像 TS `STAGE_HANDLERS`)。
///
/// 每个 handler 自行 `read_ctx` 获取最新 ctx (与 TS `readCtx(sp)` 一致)。
/// 调用方已通过 [`has_handler`] 过滤, 此处仅处理已知 stage。
pub fn run_stage(stage: &str, task_dir: &str) -> anyhow::Result<()> {
    // 进入 stage span: 携带 stage 名供 TaskFileLayer 作为 [stage] 前缀。
    let _stage_guard = tracing::info_span!("stage", stage = stage).entered();
    let ctx = read_ctx(task_dir).map_err(anyhow::Error::msg)?;
    match stage {
        "separate" => stage_separate(&ctx),
        "separate_after" => stage_separate_after(&ctx),
        "sf_ocr_pre" => stage_sf_ocr_pre(&ctx),
        "sf_ocr" => stage_sf_ocr(&ctx),
        "sf_ocr_fix" => stage_sf_ocr_fix(&ctx),
        "translate" => stage_translate(&ctx),
        "split_audio" => stage_split_audio(&ctx),
        "asr" => stage_asr(&ctx),
        "asr_fix" => stage_asr_fix(&ctx),
        "tts" => stage_tts(&ctx),
        "mix_video" => stage_mix_video(&ctx),
        "mix_audio" => stage_mix_audio(&ctx),
        // 后续阶段在此登记, 例如:
        // "asr" => stage_asr(&ctx),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::read_ctx_from_value;
    use serde_json::json;

    /// 构造一个写好 ctx.json 的临时 task 目录
    fn setup_ctx(dir: &str, input: serde_json::Value, pipeline: &str) -> crate::context::TaskCtx {
        std::fs::create_dir_all(dir).unwrap();
        let mut ctx = read_ctx_from_value(input).unwrap();
        ctx.task.task_dir = dir.to_string();
        ctx.task.id = "t".to_string();
        ctx.pipeline = pipeline.to_string();
        crate::context::write_ctx(dir, &ctx).unwrap();
        ctx
    }

    #[test]
    fn run_pipeline_runs_separate_skip_in_subtitle() {
        let dir = std::env::temp_dir()
            .join(format!("ld_pipe_skip_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        setup_ctx(
            &dir,
            json!({
                "task": {"id":"t","task_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"stages": {"separate": {"always": false}, "asr": {"enabled": false}, "asr_fix": {"enabled": false}, "translate": {"enabled": false}, "mix_video": {"enabled": false}}},
                "pipeline": "subtitle"
            }),
            "subtitle",
        );
        // subtitle + !always → separate 走 skip 分支 (无需 demucs 二进制)
        let res = run_pipeline(&dir);
        assert!(res.is_ok(), "run_pipeline 不应失败: {:?}", res.err());

        let reread = crate::context::read_ctx(&dir).unwrap();
        assert_eq!(reread.task.status, "success");
        let st = reread.stages.unwrap();
        // subtitle 默认序列里 separate / separate_after 已注册 handler, 其余跳过
        let by_name: std::collections::HashMap<&str, &crate::context::TaskStage> =
            st.iter().map(|s| (s.name.as_str(), s)).collect();
        assert_eq!(by_name["separate"].status, StageStatus::Success);
        assert_eq!(by_name["separate_after"].status, StageStatus::Success);
        assert_eq!(reread.task.current_stage, None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_pipeline_target_stage_stops_early() {
        let dir = std::env::temp_dir()
            .join(format!("ld_pipe_target_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        setup_ctx(
            &dir,
            json!({
                "task": {"id":"t","task_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"targetStage": "separate", "stages": {"separate": {"always": false}, "asr": {"enabled": false}, "asr_fix": {"enabled": false}, "translate": {"enabled": false}, "mix_video": {"enabled": false}}},
                "pipeline": "subtitle"
            }),
            "subtitle",
        );
        let res = run_pipeline(&dir);
        assert!(res.is_ok(), "run_pipeline 不应失败: {:?}", res.err());
        let reread = crate::context::read_ctx(&dir).unwrap();
        // 仅一个 stage (subtitle 默认 omit split_audio), targetStage=separate 命中即停
        assert_eq!(reread.task.status, "success");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
