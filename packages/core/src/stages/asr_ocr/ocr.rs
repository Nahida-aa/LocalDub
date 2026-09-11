//! asr_ocr: OCR frames extracted by asr_ocr_pre, consuming subtitle-ocr CLI.
//!
//! Mirrors TS `packages/core/stages/04_asr_ocr/ocr.ts` (stageAsrOcr).
//! Writes `<workflowDir>/asr_ocr/frames.json` (OcrFramesResult).

use crate::cmd::env::ensure_bin;
use crate::context::WorkflowCtx;
use crate::stages::asr_ocr::args::AsrOcrArgs;
use crate::stages::utils::{
    StagePatch, StageStatus, now_iso, set_stage_anyhow, asr_ocr_pre_dir, asr_ocr_dir,
};
use anyhow::Result;
use std::fs;
use std::process::Command;

/// Read asr_ocr config (defaults to AsrOcrArgs::default).
fn read_args(ctx: &WorkflowCtx) -> AsrOcrArgs {
    ctx.input
        .get("stages")
        .and_then(|v| v.get("asr_ocr"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Entry point (mirrors TS `stageAsrOcr`).
pub fn stage_asr_ocr(ctx: &WorkflowCtx) -> Result<()> {
    let workflow_dir = ctx.workflow.workflow_dir.clone();
    tracing::info!(target: "asr_ocr", "stage_asr_ocr: start");

    set_stage_anyhow(
        &workflow_dir,
        "asr_ocr",
        StagePatch {
            last_message: Some("OCR'ing frames...".into()),
            progress: Some(0.0),
            ..Default::default()
        },
    )?;

    let cfg = read_args(ctx);

    let frame_dir = asr_ocr_pre_dir(&workflow_dir).join("frames");
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

    let out_dir = asr_ocr_dir(&workflow_dir);
    fs::create_dir_all(&out_dir)
        .map_err(|e| anyhow::anyhow!("创建 {} 失败: {}", out_dir.display(), e))?;
    let out_file = out_dir.join("frames.json");

    let mut cmd = Command::new(&bin);
    cmd.arg("--dir").arg(&frame_dir);
    cmd.arg("--out").arg(&out_file);
    cmd.arg("--text-confidence-threshold");
    cmd.arg(cfg.ocr.text_confidence_threshold.to_string());
    if cfg.ocr.subtitle_only {
        cmd.arg("--subtitle-only");
    }

    tracing::info!(target: "asr_ocr",
        "subtitle-ocr --dir {} --out {} --text-confidence-threshold {} {}",
        frame_dir.display(),
        out_file.display(),
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
        let raw = fs::read_to_string(&out_file)
            .map_err(|e| anyhow::anyhow!("读取 {} 失败: {}", out_file.display(), e))?;
        serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("解析 {} 失败: {}", out_file.display(), e))?
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
        "{} frame results -> {}", frames.len(), out_file.display()
    );

    if cfg.ocr.cleanup_frames {
        let _ = fs::remove_dir_all(&frame_dir);
        tracing::info!(target: "asr_ocr", "Frames cleaned up");
    }

    set_stage_anyhow(
        &workflow_dir,
        "asr_ocr",
        StagePatch {
            status: Some(StageStatus::Success),
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
    use crate::context::read_ctx_from_value;
    use serde_json::json;

    fn ctx_at(dir: &str, input: serde_json::Value) -> WorkflowCtx {
        let mut ctx = read_ctx_from_value(input).unwrap();
        ctx.workflow.workflow_dir = dir.to_string();
        ctx.pipeline = "dub".to_string();
        ctx
    }

    #[test]
    fn missing_frames_errors() {
        let dir = std::env::temp_dir()
            .join(format!("ld_asrocr_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let ctx = ctx_at(&dir, json!({
            "workflow": {"id":"t","workflow_dir":dir,"url":"http://e","source":"remote",
                     "status":"running","created_at":"2024-01-01T00:00:00Z"},
            "input": {}
        }));
        crate::context::write_ctx(&dir, &ctx).unwrap();
        let res = stage_asr_ocr(&ctx);
        assert!(res.is_err());
        assert!(
            res.unwrap_err().to_string().contains("run asr_ocr_pre first"),
            "应提示先跑 asr_ocr_pre"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}