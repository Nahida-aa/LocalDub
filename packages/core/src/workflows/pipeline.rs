//! output_mode 串行派发器 (镜像 TS `packages/core/workflows/start.ts` 的 `runPipeline`)。
//!
//! 流程: 读 ctx → `get_steps` 得到阶段序列 → 逐个调用 `run_step` (handler 读 state.json
//! 跑完写回) → 每阶段前后用 `set_step` / `set_workflow` 标状态, 失败即中断。
//!
//! 本文件原位于 `steps/output_mode.rs`, 后迁移到 `workflows/` (与 start / continue 派发器同层),
//! 因为 `run_pipeline` 属于任务级编排而非某个具体 step 的实现。
//!
//! 目前 `run_step` 已注册 separate / separate_after / sf_ocr* / translate / split_audio /
//! asr / asr_fix / tts / mix_audio / mix_video; 后续阶段 (asr_ocr*) 移植后在此登记即可。

use crate::steps::asr::args::AsrInput;
use crate::steps::asr::fix::step_asr_fix;
use crate::steps::asr::fix_args::AsrFixInput;
use crate::steps::asr::step_asr;
use crate::steps::asr_ocr::args::AsrOcrInput;
use crate::steps::asr_ocr::fix_args::AsrOcrFixInput;
use crate::steps::asr_ocr::pre_args::AsrOcrPreInput;
use crate::steps::asr_ocr::{fix::step_asr_ocr_fix, ocr::step_asr_ocr, pre::step_asr_ocr_pre};
use crate::steps::get_steps;
use crate::steps::mix_audio::args::MixAudioInput;
use crate::steps::mix_audio::step_mix_audio;
use crate::steps::mix_video::args::MixVideoInput;
use crate::steps::mix_video::step_mix_video;
use crate::steps::separate::{
    after_args::SeparateAfterInput, args::SeparateInput, step_separate, step_separate_after,
};
use crate::steps::sf_ocr::args::SfOcrInput;
use crate::steps::sf_ocr::fix_args::SfOcrFixInput;
use crate::steps::sf_ocr::pre_args::SfOcrPreInput;
use crate::steps::sf_ocr::{step_sf_ocr, step_sf_ocr_fix, step_sf_ocr_pre};
use crate::steps::split_audio::args::SplitAudioInput;
use crate::steps::split_audio::step_split_audio;
use crate::steps::translate::args::TranslateInput;
use crate::steps::translate::step_translate;
use crate::steps::tts::args::TtsInput;
use crate::steps::tts::step_tts;
use crate::steps::utils::{now_iso, set_step_anyhow, set_workflow_anyhow, StepPatch, StepStatus};
use crate::workflows::args::StepName;

/// 运行完整 output_mode (镜像 TS `runPipeline`)。
///
/// 任务目录取自 `input.workflow.videoDir`（[`crate::input::Input::video_dir`]）。
///
/// 每步的入参是**该步骤自己的 `<Step>Input`**（依赖清单），统一由
/// `<Step>Input::new(video_dir, input)` 构造——不再有「什么都装」的组合态在
/// 步骤之间传来传去。
pub fn run_pipeline(input: &crate::input::Input) -> anyhow::Result<()> {
    let video_dir = input.video_dir()?;
    // 进入 workflow span: 携带 video_dir 供 WorkflowFileLayer 落盘到 <video_dir>/<tid>.log。
    let _workflow_guard = tracing::info_span!("workflow", video_dir = video_dir).entered();
    tracing::info!(target: "output_mode", "run_pipeline: start");

    let video_id = crate::steps::utils::video_id(video_dir).unwrap_or_default();
    let steps = get_steps(input)?;

    // targetStep 不在序列中则告警忽略 (镜像 TS)
    if let Some(ts) = input
        .workflow
        .as_ref()
        .and_then(|w| w.target_step.as_ref())
        .map(|s| s.as_str())
    {
        if !steps.iter().any(|s| s.as_str() == ts) {
            tracing::info!(target: "output_mode",
                "[WARN] targetStep \"{ts}\" 不在 {} 序列中, 忽略",
                crate::workflows::args::OutputMode::from_input(input).as_str()
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

        // 每步重读 input.json：continue / regen 会改写配置，handler 吃「此刻的配置」。
        let input = crate::state::read_input(video_dir).map_err(anyhow::Error::msg)?;
        match run_one_step(video_dir, *step, &input) {
            Ok(()) => {
                // 达到 targetStep 即停止 (镜像 TS)
                if let Some(ts) = input
                    .workflow
                    .as_ref()
                    .and_then(|w| w.target_step.as_ref())
                    .map(|s| s.as_str())
                {
                    if step_str == ts {
                        tracing::info!(target: "output_mode", "达到目标步骤 \"{ts}\", 停止");
                        break;
                    }
                }
            }
            Err(e) => {
                let msg = e.to_string();
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

/// 按 step 名分派到具体 handler (镜像 TS `STEP_HANDLERS`)。
///
/// **入参是 [`StepName`] + `(video_dir, input)`**（镜像 TS `runStep` 的
/// `readCtx(sp)` 结果）：每个分支自己用 `<Step>Input::new` 取依赖清单——步骤之间
/// 不再传一个共享的大对象，谁读什么写在各自的 Input 里。
///
/// **入参是 [`StepName`] 而非 `&str`**：`match` 因此是穷尽的——将来给枚举加
/// 变体却不在这里登记 handler，会**编译失败**，而不是静默跳过。
pub fn run_step(
    step: StepName,
    video_dir: &str,
    input: &crate::input::Input,
) -> anyhow::Result<()> {
    // 进入 step span: 携带 step 名供 WorkflowFileLayer 作为 [step] 前缀。
    let _step_guard = tracing::info_span!("step", step = step.as_str()).entered();
    match step {
        StepName::Separate => step_separate(&SeparateInput::new(video_dir, input)?),
        StepName::SeparateAfter => step_separate_after(&SeparateAfterInput::new(video_dir, input)),
        StepName::SfOcrPre => step_sf_ocr_pre(&SfOcrPreInput::new(video_dir, input)),
        StepName::SfOcr => step_sf_ocr(&SfOcrInput::new(video_dir, input)),
        StepName::SfOcrFix => step_sf_ocr_fix(&SfOcrFixInput::new(video_dir, input)),
        StepName::Translate => step_translate(&TranslateInput::new(video_dir, input)?),
        StepName::SplitAudio => step_split_audio(&SplitAudioInput::new(video_dir, input)?),
        StepName::Asr => step_asr(&AsrInput::new(video_dir, input)),
        StepName::AsrFix => step_asr_fix(&AsrFixInput::new(video_dir, input)),
        StepName::AsrOcrPre => step_asr_ocr_pre(&AsrOcrPreInput::new(video_dir, input)),
        StepName::AsrOcr => step_asr_ocr(&AsrOcrInput::new(video_dir, input)),
        StepName::AsrOcrFix => step_asr_ocr_fix(&AsrOcrFixInput::new(video_dir, input)),
        StepName::Tts => step_tts(&TtsInput::new(video_dir, input)),
        StepName::MixAudio => step_mix_audio(&MixAudioInput::new(video_dir, input)?),
        StepName::MixVideo => step_mix_video(&MixVideoInput::new(video_dir, input)?),
    }
}

/// 单步执行的状态包装：置 Running → 跑 → 失败置 Failed。
///
/// 串行 pipeline 与并行执行器（[`crate::workflows::parallel`]）共用。
/// 并行时两条线程各自 RMW **自己的** `<step>/state.json`（单写者原则），
/// 互不踩——这也是并行在这里可行的结构性前提。
pub(crate) fn run_one_step(
    video_dir: &str,
    step: StepName,
    input: &crate::input::Input,
) -> anyhow::Result<()> {
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
    // 并行下 current_step 无意义：当前活跃步骤由各 <step>/state.json 表达，
    // 不再维护 workflow.current_step 的每步投影。
    tracing::info!(target: "output_mode", "Running {step_str}");

    match run_step(step, video_dir, input) {
        Ok(()) => Ok(()),
        Err(e) => {
            let msg = e.to_string();
            tracing::error!(target: "output_mode", "Step {step_str} failed: {msg}");
            set_step_anyhow(
                video_dir,
                step_str,
                StepPatch {
                    status: Some(StepStatus::Failed),
                    error_message: Some(msg),
                    completed_at: Some(now_iso()),
                    ..Default::default()
                },
            )?;
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 铺一个任务目录：`state.json` + `input.json` + 直通用的假 srt，返回 input。
    fn setup(dir: &str, input: serde_json::Value) -> crate::input::Input {
        let input = crate::state::write_fixture(dir, input);
        // split_audio 会读 translate 产物；本测试禁用了 asr/asr_fix，translate 直通
        // 无内容可搬会写出空产物，split_audio 随即 fail-fast。故给 translate 的
        // **输入**（subtitle_source=asr → asr_fix/srt.json）造假数据，直通原样带出。
        seed_asr_fix_srt(dir);
        input
    }

    /// 落一份最小 `asr_fix/srt.json`，让 translate 直通有内容可搬。
    fn seed_asr_fix_srt(dir: &str) {
        let p = std::path::Path::new(dir).join("asr_fix").join("srt.json");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        crate::steps::utils::write_srt_json(
            &p,
            &[
                subtitle::SubtitleSegment {
                    text: "一".into(),
                    start_ms: 0,
                    end_ms: 1000,
                },
                subtitle::SubtitleSegment {
                    text: "二".into(),
                    start_ms: 1000,
                    end_ms: 2000,
                },
            ],
        )
        .unwrap();
    }

    #[test]
    fn run_pipeline_runs_separate_skip_in_subtitle() {
        let dir = std::env::temp_dir()
            .join(format!("ld_pipe_skip_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        let input = setup(
            &dir,
            json!({
                "workflow": {"id":"t","video_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"workflow": {"output_mode": "subtitle", "videoDir": dir},
                          "steps": {"separate": {"always": false}, "asr": {"enabled": false}, "asr_fix": {"enabled": false}, "translate": {"enabled": false}, "mix_video": {"enabled": false}}},
                "output_mode": "subtitle"
            }),
        );
        // subtitle + !always → separate 走 skip 分支 (无需 demucs 二进制)
        let res = run_pipeline(&input);
        assert!(res.is_ok(), "run_pipeline 不应失败: {:?}", res.err());

        let wf = crate::state::read_workflow(&dir).unwrap();
        assert_eq!(wf.status, "success");
        // subtitle 默认序列里 separate / separate_after 已注册 handler, 其余跳过
        assert_eq!(
            crate::state::read_step_state(&dir, "separate")
                .unwrap()
                .status,
            StepStatus::Success
        );
        assert_eq!(
            crate::state::read_step_state(&dir, "separate_after")
                .unwrap()
                .status,
            StepStatus::Success
        );
        assert_eq!(wf.current_step, None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_pipeline_target_step_stops_early() {
        let dir = std::env::temp_dir()
            .join(format!("ld_pipe_target_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        let input = setup(
            &dir,
            json!({
                "workflow": {"id":"t","video_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"workflow": {"output_mode": "subtitle", "targetStep": "separate", "videoDir": dir},
                          "steps": {"separate": {"always": false}, "asr": {"enabled": false}, "asr_fix": {"enabled": false}, "translate": {"enabled": false}, "mix_video": {"enabled": false}}},
                "output_mode": "subtitle"
            }),
        );
        let res = run_pipeline(&input);
        assert!(res.is_ok(), "run_pipeline 不应失败: {:?}", res.err());
        // targetStep=separate 命中即停
        let wf = crate::state::read_workflow(&dir).unwrap();
        assert_eq!(wf.status, "success");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
