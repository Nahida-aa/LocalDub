//! asr_ocr: OCR frames extracted by asr_ocr_pre, consuming subtitle-ocr CLI.
//!
//! Mirrors TS `packages/core/steps/04_asr_ocr/ocr.ts` (stepAsrOcr).
//! Writes `<videoDir>/asr_ocr/frames.json` (OcrFramesResult).

use crate::cmd::env::ensure_bin;
use crate::steps::asr_ocr::args::AsrOcrInput;
use crate::steps::utils::paths::asr_ocr;
use crate::steps::utils::{asr_ocr_dir, now_iso, set_step_anyhow, StepPatch, StepStatus};
use anyhow::Result;
use std::fs;
use std::process::Command;

/// Entry point (mirrors TS `stepAsrOcr`).
///
/// 单参 [`AsrOcrInput`]：依赖（帧目录）+ 任务目录 + 本次参数都在里面。
pub fn step_asr_ocr(input: &AsrOcrInput) -> Result<()> {
    let video_dir = input.video_dir.clone();
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

    let cfg = &input.args;
    let frame_dir = input.frames_dir.as_path();
    if !frame_dir.exists() {
        return Err(anyhow::anyhow!(
            "Frame directory not found: {} — run asr_ocr_pre first",
            frame_dir.display()
        ));
    }

    let bin = ensure_bin("subtitle_ocr_bin").map_err(|e| {
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_frames_errors() {
        let dir = std::env::temp_dir()
            .join(format!("ld_asrocr_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // 本步不读 state.json，故 input 用默认即可（帧目录缺失是最先被拦的条件）
        let input = crate::input::Input::default();
        let res = step_asr_ocr(&AsrOcrInput::new(&dir, &input));
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
