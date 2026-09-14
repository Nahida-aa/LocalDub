//! pipeline 串行派发器 (镜像 TS `packages/core/workflows/start.ts` 的 `runPipeline`)。
//!
//! 流程: 读 ctx → `get_steps` 得到阶段序列 → 逐个调用 `run_step` (handler 读 ctx.json
//! 跑完写回) → 每阶段前后用 `set_step` / `set_workflow` 标状态, 失败即中断。
//!
//! 本文件原位于 `steps/pipeline.rs`, 后迁移到 `workflows/` (与 start / continue 派发器同层),
//! 因为 `run_pipeline` 属于任务级编排而非某个具体 step 的实现。
//!
//! 目前 `run_step` 已注册 separate / separate_after / sf_ocr* / translate / split_audio /
//! asr / asr_fix / tts / mix_audio / mix_video; 后续阶段 (asr_ocr*) 移植后在此登记即可。

use crate::context::read_ctx;
use crate::steps::asr::fix::step_asr_fix;
use crate::steps::asr::step_asr;
use crate::steps::asr_ocr::{fix::step_asr_ocr_fix, ocr::step_asr_ocr, pre::step_asr_ocr_pre};
use crate::steps::get_steps;
use crate::steps::mix_audio::step_mix_audio;
use crate::steps::mix_video::step_mix_video;
use crate::steps::separate::{step_separate, step_separate_after};
use crate::steps::sf_ocr::{step_sf_ocr, step_sf_ocr_fix, step_sf_ocr_pre};
use crate::steps::split_audio::step_split_audio;
use crate::steps::translate::step_translate;
use crate::steps::tts::step_tts;
use crate::steps::utils::{now_iso, set_step_anyhow, set_workflow_anyhow, StepPatch, StepStatus};
use crate::workflows::args::StepName;

/// 运行完整 pipeline (镜像 TS `runPipeline`)。
pub fn run_pipeline(video_dir: &str) -> anyhow::Result<()> {
    // 进入 workflow span: 携带 video_dir 供 WorkflowFileLayer 落盘到 <video_dir>/<tid>.log。
    let _workflow_guard = tracing::info_span!("workflow", video_dir = video_dir).entered();
    tracing::info!(target: "pipeline", "run_pipeline: start");

    let ctx = read_ctx(video_dir).map_err(anyhow::Error::msg)?;
    let pipeline = ctx.pipeline.clone();
    let video_id = ctx.workflow.id.clone();
    let steps = get_steps(&ctx);

    // targetStep 不在序列中则告警忽略 (镜像 TS)
    if let Some(ts) = ctx.input.get("targetStep").and_then(|v| v.as_str()) {
        if !steps.iter().any(|s| s.as_str() == ts) {
            tracing::info!(target: "pipeline",
                "[WARN] targetStep \"{ts}\" 不在 {pipeline} pipeline 中, 忽略"
            );
        }
    }

    set_workflow_anyhow(
        video_dir,
        crate::steps::utils::WorkflowPatch {
            status: Some("running".to_string()),
            started_at: Some(now_iso()),
            ..Default::default()
        },
    )?;

    for step in &steps {
        // 每个 step 名都对应一个 run_step 分支（枚举穷尽），无需再查 handler 表。
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
        tracing::info!(target: "pipeline", "Running {step_str}");

        match run_step(*step, video_dir) {
            Ok(()) => {
                // 达到 targetStep 即停止 (镜像 TS)
                if let Some(ts) = ctx.input.get("targetStep").and_then(|v| v.as_str()) {
                    if step_str == ts {
                        tracing::info!(target: "pipeline", "达到目标步骤 \"{ts}\", 停止");
                        break;
                    }
                }
            }
            Err(e) => {
                let msg = e.to_string();
                tracing::error!(target: "pipeline", "Step {step_str} failed: {msg}");
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
    tracing::info!(target: "pipeline", "Workflow {video_id} completed");
    Ok(())
}

/// 按 step 名分派到具体 handler (镜像 TS `STEP_HANDLERS`)。
///
/// 每个 handler 自行 `read_ctx` 获取最新 ctx (与 TS `readCtx(sp)` 一致)。
///
/// **入参是 [`StepName`] 而非 `&str`**：`match` 因此是穷尽的——将来给枚举加
/// 变体却不在这里登记 handler，会**编译失败**，而不是静默跳过。
pub fn run_step(step: StepName, video_dir: &str) -> anyhow::Result<()> {
    // 进入 step span: 携带 step 名供 WorkflowFileLayer 作为 [step] 前缀。
    let _step_guard = tracing::info_span!("step", step = step.as_str()).entered();
    let ctx = read_ctx(video_dir).map_err(anyhow::Error::msg)?;
    match step {
        StepName::Separate => step_separate(&ctx),
        StepName::SeparateAfter => step_separate_after(&ctx),
        StepName::SfOcrPre => step_sf_ocr_pre(&ctx),
        StepName::SfOcr => step_sf_ocr(&ctx),
        StepName::SfOcrFix => step_sf_ocr_fix(&ctx),
        StepName::Translate => step_translate(&ctx),
        StepName::SplitAudio => step_split_audio(&ctx),
        StepName::Asr => step_asr(&ctx),
        StepName::AsrFix => step_asr_fix(&ctx),
        StepName::AsrOcrPre => step_asr_ocr_pre(&ctx),
        StepName::AsrOcr => step_asr_ocr(&ctx),
        StepName::AsrOcrFix => step_asr_ocr_fix(&ctx),
        StepName::Tts => step_tts(&ctx),
        StepName::MixAudio => step_mix_audio(&ctx),
        StepName::MixVideo => step_mix_video(&ctx),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::read_ctx_from_value;
    use serde_json::json;

    /// 构造一个写好 ctx.json 的临时 workflow 目录
    fn setup_ctx(
        dir: &str,
        input: serde_json::Value,
        pipeline: &str,
    ) -> crate::context::WorkflowCtx {
        std::fs::create_dir_all(dir).unwrap();
        let mut ctx = read_ctx_from_value(input).unwrap();
        ctx.workflow.video_dir = dir.to_string();
        ctx.workflow.id = "t".to_string();
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
                "workflow": {"id":"t","video_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"steps": {"separate": {"always": false}, "asr": {"enabled": false}, "asr_fix": {"enabled": false}, "translate": {"enabled": false}, "mix_video": {"enabled": false}}},
                "pipeline": "subtitle"
            }),
            "subtitle",
        );
        // subtitle + !always → separate 走 skip 分支 (无需 demucs 二进制)
        let res = run_pipeline(&dir);
        assert!(res.is_ok(), "run_pipeline 不应失败: {:?}", res.err());

        let reread = crate::context::read_ctx(&dir).unwrap();
        assert_eq!(reread.workflow.status, "success");
        let st = reread.steps.unwrap();
        // subtitle 默认序列里 separate / separate_after 已注册 handler, 其余跳过
        let by_name: std::collections::HashMap<&str, &crate::context::WorkflowStep> =
            st.iter().map(|s| (s.name.as_str(), s)).collect();
        assert_eq!(by_name["separate"].status, StepStatus::Success);
        assert_eq!(by_name["separate_after"].status, StepStatus::Success);
        assert_eq!(reread.workflow.current_step, None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_pipeline_target_step_stops_early() {
        let dir = std::env::temp_dir()
            .join(format!("ld_pipe_target_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        setup_ctx(
            &dir,
            json!({
                "workflow": {"id":"t","video_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"targetStep": "separate", "steps": {"separate": {"always": false}, "asr": {"enabled": false}, "asr_fix": {"enabled": false}, "translate": {"enabled": false}, "mix_video": {"enabled": false}}},
                "pipeline": "subtitle"
            }),
            "subtitle",
        );
        let res = run_pipeline(&dir);
        assert!(res.is_ok(), "run_pipeline 不应失败: {:?}", res.err());
        let reread = crate::context::read_ctx(&dir).unwrap();
        // 仅一个 step (subtitle 默认 omit split_audio), targetStep=separate 命中即停
        assert_eq!(reread.workflow.status, "success");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
