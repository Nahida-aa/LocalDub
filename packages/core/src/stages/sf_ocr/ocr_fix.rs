//! sf_ocr_fix: 消费 sf_ocr 的 frames.json, 调 ocr-post 统合管线 (adjust-box → filter-box →
//! merge → adjust-segment → filter-segment), 再叠加可选 LLM 修正, 写出最终字幕段。
//!
//! 镜像 TS `packages/core/stages/sf_ocr/ocr_fix.ts` (stageSfOcrFix)。
//!
//! 注意: LLM 修正通过 `llm` crate 的 `chat_completions` + `ocr_llm_fix` 实现 (`llmFix=true` 时),
//! 失败时 warn 并保留原文 (不中断 stage)。

use crate::cmd::env::ensure_bin;
use crate::context::WorkflowCtx;
use crate::stages::sf_ocr::fix_args::OcrFixArgs;
use crate::stages::utils::{
    now_iso, set_stage_anyhow, sf_ocr_dir, sf_ocr_fix_dir, video_source_path, StepPatch, StepStatus,
};
use std::process::Command;

/// 读取 sf_ocr_fix 配置 (缺省用 OcrFixArgs::default)。
fn read_args(ctx: &WorkflowCtx) -> OcrFixArgs {
    ctx.input
        .get("stages")
        .and_then(|v| v.get("sf_ocr_fix"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// 入口 (镜像 TS `stageSfOcrFix`)。
pub fn stage_sf_ocr_fix(ctx: &WorkflowCtx) -> anyhow::Result<()> {
    let workflow_dir = ctx.workflow.workflow_dir.clone();
    tracing::info!(target: "sf_ocr", "start");

    let frames_file = sf_ocr_dir(&workflow_dir).join("frames.json");
    if !frames_file.exists() {
        return Err(anyhow::anyhow!(
            "frames.json not found: {}; run sf_ocr first",
            frames_file.display()
        ));
    }
    let video_file = video_source_path(ctx)?;
    let out_dir = sf_ocr_fix_dir(&workflow_dir);
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| anyhow::anyhow!("创建 {} 失败: {}", out_dir.display(), e))?;

    let args = read_args(ctx);

    let bin = ensure_bin("ocr_post_bin").map_err(|e| {
        anyhow::anyhow!(
            "{e}\n若下载失败, 请手动执行: cargo run -p cli -- env --action ensure --targets ocr_post_bin"
        )
    })?;

    let post_args = [
        "--frames".to_string(),
        frames_file.to_string_lossy().into_owned(),
        "--video".to_string(),
        video_file.clone(),
        "--out".to_string(),
        out_dir.to_string_lossy().into_owned(),
        "--threshold".to_string(),
        args.adjusted_confidence_threshold.to_string(),
        "--stop-at".to_string(),
        "filter-segment".to_string(),
    ];
    tracing::info!(target: "sf_ocr", "ocr-post {}", post_args.join(" "));
    let status = Command::new(&bin)
        .args(&post_args)
        .status()
        .map_err(|e| anyhow::anyhow!("spawn ocr-post 失败: {e}"))?;
    if !status.success() {
        return Err(anyhow::anyhow!(
            "ocr-post failed with exit code {:?}",
            status.code()
        ));
    }

    let filter_file = out_dir.join("segment_filter.json");
    let filtered: serde_json::Value = {
        let raw = std::fs::read_to_string(&filter_file)
            .map_err(|e| anyhow::anyhow!("读取 {} 失败: {}", filter_file.display(), e))?;
        serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("解析 {} 失败: {}", filter_file.display(), e))?
    };
    let segments = filtered
        .get("result")
        .and_then(|r| r.get("segments"))
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    tracing::info!(target: "sf_ocr",
        "ocr-post → {} segments (filtered)",
        segments.len()
    );

    // === LLM 修正 (最后一层修复) ===
    let llm_fix_file = out_dir.join("segment_filter_llm_fix.json");
    let mut final_segments = segments.clone();
    if args.llm_fix.llm_fix {
        let src_texts: Vec<String> = segments
            .iter()
            .filter_map(|s| s.get("text").and_then(|t| t.as_str()).map(String::from))
            .collect();
        // 源语言: ASR 实测 (ctx.asr_language) > input.workflow.sourceLang > 默认 zh。
        // (stage 级 sf_ocr_fix.sourceLang 已移除, 统一到任务级)
        let legacy = ctx
            .input
            .get("stages")
            .and_then(|v| v.get("sfOcrFix"))
            .and_then(|v| v.get("sourceLang"))
            .is_some();
        if legacy {
            tracing::warn!(target: "sf_ocr", "stages.sfOcrFix.sourceLang 已移除, 请在 workflow.sourceLang 配置源语言");
        }
        let src_lang: crate::r#const::lang::Language = ctx
            .asr_language
            .clone()
            .or_else(|| {
                ctx.input
                    .get("workflow")
                    .and_then(|v| v.get("sourceLang"))
                    .and_then(|v| v.as_str())
                    .map(crate::r#const::lang::Language::from)
            })
            .unwrap_or_else(crate::r#const::lang::default_lang);
        let lang_label = llm::lang_label(src_lang.code());
        tracing::info!(target: "sf_ocr",
            "sf_ocr_fix: LLM 修正 {} segs (model={})",
            src_texts.len(),
            args.llm_fix.llm_model
        );
        match llm::ocr_llm_fix(&src_texts, &lang_label, &args.llm_fix) {
            Ok(fixed) => {
                // 逐段回填修正文本 (保持原段结构/时间戳)
                for (seg, text) in final_segments.iter_mut().zip(fixed.into_iter()) {
                    if let Some(obj) = seg.as_object_mut() {
                        obj.insert("text".to_string(), serde_json::Value::String(text));
                    }
                }
            }
            Err(e) => {
                tracing::warn!(target: "sf_ocr", "sf_ocr_fix LLM 修正失败, 保留原文: {e}");
            }
        }
    }
    if args.llm_fix.llm_fix {
        let texts: Vec<String> = final_segments
            .iter()
            .filter_map(|s| s.get("text").and_then(|t| t.as_str()).map(String::from))
            .collect();
        let out = serde_json::json!({
            "result": {
                "text": texts.join(" "),
                "segments": final_segments,
            }
        });
        let s = serde_json::to_string_pretty(&out)
            .map_err(|e| anyhow::anyhow!("序列化 llm_fix 结果失败: {e}"))?;
        std::fs::write(&llm_fix_file, s)
            .map_err(|e| anyhow::anyhow!("写入 {} 失败: {}", llm_fix_file.display(), e))?;
    }

    set_stage_anyhow(
        &workflow_dir,
        "sf_ocr_fix",
        StepPatch {
            status: Some(StepStatus::Success),
            completed_at: Some(now_iso()),
            progress: Some(100.0),
            last_message: if args.llm_fix.llm_fix {
                Some(format!("LLM fixed {} segs", segments.len()))
            } else {
                Some(format!("Merged {} segs", segments.len()))
            },
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
        ctx.video_source_path = Some("/x/video.mp4".to_string());
        ctx
    }

    #[test]
    fn args_defaults() {
        let ctx = ctx_at(
            "/x",
            json!({
                "workflow": {"id":"t","workflow_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"stages": {"sf_ocr_fix": {}}}
            }),
        );
        let cfg = read_args(&ctx);
        assert_eq!(cfg.adjusted_confidence_threshold, 0.45);
        assert!(!cfg.llm_fix.llm_fix);
    }

    #[test]
    fn args_camel_case_and_llm_fix() {
        let ctx = ctx_at(
            "/x",
            json!({
                "workflow": {"id":"t","workflow_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"stages": {"sf_ocr_fix": {
                    "adjustedConfidenceThreshold": 0.6, "llmFix": true
                }}}
            }),
        );
        let cfg = read_args(&ctx);
        assert_eq!(cfg.adjusted_confidence_threshold, 0.6);
        assert!(cfg.llm_fix.llm_fix);
    }

    #[test]
    fn missing_frames_errors() {
        let dir = std::env::temp_dir()
            .join(format!("ld_sffix_{}", std::process::id()))
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
        let res = stage_sf_ocr_fix(&ctx);
        assert!(res.is_err());
        assert!(
            res.unwrap_err().to_string().contains("run sf_ocr first"),
            "应提示先跑 sf_ocr"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
