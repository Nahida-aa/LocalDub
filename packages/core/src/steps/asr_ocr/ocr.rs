//! asr_ocr: OCR frames extracted by asr_ocr_pre, consuming subtitle-ocr CLI.
//!
//! Mirrors TS `packages/core/steps/04_asr_ocr/ocr.ts` (stepAsrOcr).
//! Writes `<video_dir>/asr_ocr/frames.json` (OcrFramesResult).

use crate::cmd::env::ensure_bin;
use crate::steps::asr_ocr::args::{AsrOcrDeps, AsrOcrOutput};
use crate::steps::utils::paths::asr_ocr;
use crate::steps::utils::{asr_ocr_dir, now_iso, set_step_anyhow, StepPatch, StepStatus};
use crate::workflow::step_ctx::{CtxView, StepCtxView};
use anyhow::Result;
use std::fs;
use std::process::Command;

/// Entry point (mirrors TS `stepAsrOcr`).
///
/// 意图面经 `sc.ctx().input()` 取（args / video_dir），事实面由装配层算好注入
/// [`AsrOcrDeps`]（asr_ocr_pre 产出的帧目录）。
pub async fn step_asr_ocr(sc: impl StepCtxView, deps: AsrOcrDeps) -> Result<AsrOcrOutput> {
    let input = sc.ctx().input();
    let video_dir = input.video_dir().to_string();
    tracing::info!(target: "asr_ocr", "step_asr_ocr: start");

    set_step_anyhow(
        &video_dir,
        "asr_ocr",
        StepPatch {
            last_message: Some("OCR'ing frames...".into()),
            progress: Some(0.0),
            ..Default::default()
        },
    )?;

    let cfg = &input.steps.asr_ocr;
    let frame_dir = deps.frames_dir.as_path();
    if !frame_dir.exists() {
        return Err(anyhow::anyhow!(
            "Frame directory not found: {} — run asr_ocr_pre first",
            frame_dir.display()
        ));
    }

    let bin = ensure_bin("subtitle_ocr_bin").await.map_err(|e| {
        anyhow::anyhow!(
            "{e}\nIf download fails, run: cargo run -p cli -- env --action ensure --targets subtitle_ocr_bin"
        )
    })?;

    let out_dir = asr_ocr_dir(&video_dir);
    fs::create_dir_all(&out_dir)
        .map_err(|e| anyhow::anyhow!("创建 {} 失败: {}", out_dir.display(), e))?;
    let out_file = asr_ocr::FramesPath::new(&video_dir);

    let mut cmd = Command::new(&bin);
    cmd.arg("--dir").arg(&frame_dir);
    cmd.arg("--out").arg(out_file.as_path());
    cmd.arg("--text-confidence-threshold");
    cmd.arg(cfg.ocr.text_confidence_threshold.to_string());
    if cfg.ocr.subtitle_only {
        cmd.arg("--subtitle-only");
    }

    tracing::info!(target: "asr_ocr",
        "subtitle-ocr --dir {} --out {} --text-confidence-threshold {} {}",
        frame_dir.display(),
        out_file.as_path().display(),
        cfg.ocr.text_confidence_threshold,
        if cfg.ocr.subtitle_only { "--subtitle-only" } else { "" }
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
        let raw = fs::read_to_string(out_file.as_path())
            .map_err(|e| anyhow::anyhow!("读取 {} 失败: {}", out_file.as_path().display(), e))?;
        serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("解析 {} 失败: {}", out_file.as_path().display(), e))?
    };
    let frames = data
        .get("frames")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if frames.is_empty() {
        return Err(anyhow::anyhow!("asr_ocr: no OCR results from frames"));
    }
    tracing::info!(target: "asr_ocr",
        "{} frame results -> {}", frames.len(), out_file.as_path().display()
    );

    if cfg.ocr.cleanup_frames {
        let _ = fs::remove_dir_all(&frame_dir);
        tracing::info!(target: "asr_ocr", "Frames cleaned up");
    }

    set_step_anyhow(
        &video_dir,
        "asr_ocr",
        StepPatch {
            status: Some(StepStatus::Success),
            completed_at: Some(now_iso()),
            progress: Some(100.0),
            ..Default::default()
        },
    )?;
    tracing::info!(target: "asr_ocr", "done");
    Ok(AsrOcrOutput {
        frames_path: out_file.as_path().to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_frames_errors() {
        let dir = std::env::temp_dir()
            .join(format!("ld_asrocr_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // 帧目录缺失是最先被拦的条件（早于任何 OCR 二进制调用）
        let input: crate::input::Input = serde_json::from_value(serde_json::json!({
            "workflow": {"video_dir": dir}
        }))
        .unwrap();
        let wfi = crate::input::WorkflowInput::try_from(input).unwrap();
        let deps = AsrOcrDeps::from_input(&wfi);
        let res = step_asr_ocr(crate::workflow::step_ctx::MyStepCtx::new("asr_ocr", wfi), deps).await;
        assert!(res.is_err());
        assert!(
            res.unwrap_err()
                .to_string()
                .contains("run asr_ocr_pre first"),
            "应提示先跑 asr_ocr_pre"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
