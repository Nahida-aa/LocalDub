//! sf_ocr: 关键帧 OCR, 消费 sf_ocr_pre 落盘的关键帧, 经 subtitle-ocr CLI 批量识别。
//!
//! 镜像 TS `packages/core/steps/sf_ocr/ocr.ts` (stepSfOcr)。
//! 只写出 `<videoDir>/sf_ocr/frames.json` (OcrFramesResult 原始逐帧结果);
//! 段合并 / 时间调整 / LLM 修正由下游 sf_ocr_fix 负责。

use crate::cmd::env::ensure_bin;
use crate::context::WorkflowCtx;
use crate::steps::sf_ocr::args::SfOcrArgs;
use crate::steps::utils::{
    now_iso, set_step_anyhow, sf_ocr_dir, sf_ocr_pre_dir, StepPatch, StepStatus,
};
use std::process::Command;

/// 读取 sf_ocr 配置 (缺省用 SfOcrArgs::default)。
fn read_args(ctx: &WorkflowCtx) -> SfOcrArgs {
    ctx.input
        .get("steps")
        .and_then(|v| v.get("sf_ocr"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// 入口 (镜像 TS `stepSfOcr`)。
pub fn step_sf_ocr(ctx: &WorkflowCtx) -> anyhow::Result<()> {
    let workflow_dir = ctx.workflow.workflow_dir.clone();
    tracing::info!(target: "sf_ocr", "start");

    set_step_anyhow(
        &workflow_dir,
        "sf_ocr",
        StepPatch {
            last_message: Some("OCR'ing keyframes...".into()),
            progress: Some(0.0),
            ..Default::default()
        },
    )?;

    let cfg = read_args(ctx);

    let frame_dir = sf_ocr_pre_dir(&workflow_dir).join("frames");
    if !frame_dir.exists() {
        return Err(anyhow::anyhow!(
            "Keyframe dir not found: {} — run sf_ocr_pre first",
            frame_dir.display()
        ));
    }

    let bin = ensure_bin("subtitle_ocr_bin").map_err(|e| {
        anyhow::anyhow!(
            "{e}\n若下载失败, 请手动执行: cargo run -p cli -- env --action ensure --targets subtitle_ocr_bin"
        )
    })?;

    let out_dir = sf_ocr_dir(&workflow_dir);
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| anyhow::anyhow!("创建 {} 失败: {}", out_dir.display(), e))?;
    let out_file = out_dir.join("frames.json");

    let mut cmd = Command::new(&bin);
    cmd.arg("--dir").arg(&frame_dir);
    cmd.arg("--out").arg(&out_file);
    cmd.arg("--text-confidence-threshold");
    cmd.arg(cfg.text_confidence_threshold.to_string());
    if cfg.subtitle_only {
        cmd.arg("--subtitle-only");
    }

    tracing::info!(target: "sf_ocr",
        "subtitle-ocr --dir {} --out {} --text-confidence-threshold {} {}",
        frame_dir.display(),
        out_file.display(),
        cfg.text_confidence_threshold,
        if cfg.subtitle_only {
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
        let raw = std::fs::read_to_string(&out_file)
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
        return Err(anyhow::anyhow!("sf_ocr: no OCR results from keyframes"));
    }
    tracing::info!(target: "sf_ocr",
        "{} frame results -> {}",
        frames.len(),
        out_file.display()
    );

    if cfg.cleanup_frames {
        let _ = std::fs::remove_dir_all(&frame_dir);
        tracing::info!(target: "sf_ocr", "Keyframes cleaned up");
    }

    set_step_anyhow(
        &workflow_dir,
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
    fn args_defaults() {
        let dir = "/x";
        let ctx = ctx_at(
            dir,
            json!({
                "workflow": {"id":"t","workflow_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"steps": {"sf_ocr": {}}}
            }),
        );
        let cfg = read_args(&ctx);
        assert_eq!(cfg.text_confidence_threshold, 0.45);
        assert!(cfg.subtitle_only);
        assert!(!cfg.cleanup_frames);
    }

    #[test]
    fn args_camel_case() {
        let dir = "/x";
        let ctx = ctx_at(
            dir,
            json!({
                "workflow": {"id":"t","workflow_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"steps": {"sf_ocr": {
                    "textConfidenceThreshold": 0.7, "subtitleOnly": false, "cleanupFrames": true
                }}}
            }),
        );
        let cfg = read_args(&ctx);
        assert_eq!(cfg.text_confidence_threshold, 0.7);
        assert!(!cfg.subtitle_only);
        assert!(cfg.cleanup_frames);
    }

    #[test]
    fn missing_frame_dir_errors() {
        let dir = std::env::temp_dir()
            .join(format!("ld_sfocr_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = ctx_at(
            &dir,
            json!({
                "workflow": {"id":"t","workflow_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}
            }),
        );
        crate::context::write_ctx(&dir, &ctx).unwrap();
        let res = step_sf_ocr(&ctx);
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
