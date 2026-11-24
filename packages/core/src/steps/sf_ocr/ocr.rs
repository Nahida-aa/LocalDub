//! sf_ocr: 关键帧 OCR, 消费 sf_ocr_pre 落盘的关键帧, 经 subtitle-ocr CLI 批量识别。
//!
//! 镜像 TS `packages/core/steps/sf_ocr/ocr.ts` (stepSfOcr)。
//! 只写出 `<video_dir>/sf_ocr/frames.json` (OcrFramesResult 原始逐帧结果);
//! 段合并 / 时间调整 / LLM 修正由下游 sf_ocr_fix 负责。

use crate::cmd::env::input::EnvKey;
use crate::cmd::env::ensure_bin;
use crate::steps::sf_ocr::args::{SfOcrDeps, SfOcrOutput};
use crate::steps::utils::paths::sf_ocr;
use crate::steps::utils::{now_iso, set_step_anyhow, StepPatch, StepStatus};
use crate::workflow::step_ctx::{CtxView, StepCtxView};
use std::process::Command;

/// 入口 (镜像 TS `stepSfOcr`)。
///
/// 意图面经 `sc.ctx().input()` 取（args / video_dir），事实面由装配层算好注入
/// [`SfOcrDeps`]（sf_ocr_pre 产出的关键帧目录）。
pub async fn step_sf_ocr(sc: impl StepCtxView, deps: SfOcrDeps) -> anyhow::Result<SfOcrOutput> {
    let input = sc.ctx().input();
    let video_dir = input.video_dir().to_string();
    tracing::info!(target: "sf_ocr", "start");

    set_step_anyhow(
        &video_dir,
        "sf_ocr",
        StepPatch {
            last_message: Some("OCR'ing keyframes...".into()),
            progress: Some(0.0),
            ..Default::default()
        },
    )?;

    let args = &input.steps.sf_ocr;
    let frame_dir = deps.keyframes_dir.as_path();
    if !frame_dir.exists() {
        return Err(anyhow::anyhow!(
            "Keyframe dir not found: {} — run sf_ocr_pre first",
            frame_dir.display()
        ));
    }

    let bin = ensure_bin(EnvKey::SubtitleOcrBin).await.map_err(|e| {
        anyhow::anyhow!(
            "{e}\n若下载失败, 请手动执行: cargo run -p cli -- env --action ensure --targets subtitle_ocr_bin"
        )
    })?;

    let frames_file = sf_ocr::FramesPath::new(&video_dir);
    std::fs::create_dir_all(sf_ocr::FramesPath::dir(&video_dir)).map_err(|e| {
        anyhow::anyhow!(
            "创建 {} 失败: {}",
            sf_ocr::FramesPath::dir(&video_dir).display(),
            e
        )
    })?;

    let mut cmd = Command::new(&bin);
    cmd.arg("--dir").arg(&frame_dir);
    cmd.arg("--out").arg(frames_file.as_path());
    cmd.arg("--text-confidence-threshold");
    cmd.arg(args.text_confidence_threshold.to_string());
    if args.subtitle_only {
        cmd.arg("--subtitle-only");
    }

    tracing::info!(target: "sf_ocr",
        "subtitle-ocr --dir {} --out {} --text-confidence-threshold {} {}",
        frame_dir.display(),
        frames_file.as_path().display(),
        args.text_confidence_threshold,
        if args.subtitle_only {
            "--subtitle-only"
        } else {
            ""
        }
    );
    let status = cmd
        .status()
        .map_err(|e| anyhow::anyhow!("spawn subtitle-ocr 失败: {e}"))?;
    if !status.success() {
        return Err(anyhow::anyhow!(
            "subtitle-ocr failed with exit code {:?}",
            status.code()
        ));
    }

    let data: serde_json::Value = {
        let raw = std::fs::read_to_string(frames_file.as_path())
            .map_err(|e| anyhow::anyhow!("读取 {} 失败: {}", frames_file.as_path().display(), e))?;
        serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("解析 {} 失败: {}", frames_file.as_path().display(), e))?
    };
    let frames = data
        .get("frames")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if frames.is_empty() {
        return Err(anyhow::anyhow!("sf_ocr: no OCR results from keyframes"));
    }
    tracing::info!(target: "sf_ocr",
        "{} frame results -> {}",
        frames.len(),
        frames_file.as_path().display()
    );

    if args.cleanup_frames {
        let _ = std::fs::remove_dir_all(&frame_dir);
        tracing::info!(target: "sf_ocr", "Keyframes cleaned up");
    }

    set_step_anyhow(
        &video_dir,
        "sf_ocr",
        StepPatch {
            status: Some(StepStatus::Success),
            completed_at: Some(now_iso()),
            progress: Some(100.0),
            last_message: Some(format!("OCR'd {} frame results", frames.len())),
            ..Default::default()
        },
    )?;
    tracing::info!(target: "sf_ocr", "done");
    Ok(SfOcrOutput {
        frames_path: frames_file.as_path().to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::sf_ocr::args::SfOcrArgs;
    use serde_json::json;

    fn input_at(input: serde_json::Value) -> crate::input::Input {
        serde_json::from_value(input).unwrap()
    }

    #[tokio::test]
    async fn args_defaults() {
        let input = input_at(json!({"steps": {"sf_ocr": {}}}));
        let cfg = SfOcrArgs::from_input(&input);
        assert_eq!(cfg.text_confidence_threshold, 0.45);
        assert!(cfg.subtitle_only);
        assert!(!cfg.cleanup_frames);
    }

    #[tokio::test]
    async fn args_snake_case() {
        let input = input_at(json!({"steps": {"sf_ocr": {
            "text_confidence_threshold": 0.7, "subtitle_only": false, "cleanup_frames": true
        }}}));
        let cfg = SfOcrArgs::from_input(&input);
        assert_eq!(cfg.text_confidence_threshold, 0.7);
        assert!(!cfg.subtitle_only);
        assert!(cfg.cleanup_frames);
    }

    #[tokio::test]
    async fn missing_frame_dir_errors() {
        let dir = std::env::temp_dir()
            .join(format!("ld_sfocr_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let input: crate::input::Input = serde_json::from_value(json!({
            "workflow": {"video_dir": dir}
        }))
        .unwrap();
        let wfi = crate::input::WorkflowInput::try_from(input).unwrap();
        let deps = SfOcrDeps::from_input(&wfi);
        let res = step_sf_ocr(crate::workflow::step_ctx::MyStepCtx::new("sf_ocr", wfi), deps).await;
        assert!(res.is_err());
        assert!(
            res.unwrap_err()
                .to_string()
                .contains("run sf_ocr_pre first"),
            "应提示先跑 sf_ocr_pre"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
