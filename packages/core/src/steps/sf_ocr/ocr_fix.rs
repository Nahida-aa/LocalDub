//! sf_ocr_fix: 消费 sf_ocr 的 frames.json, 调 subtitle-ocr-post 统合管线 (adjust-box →
//! filter-box → merge → adjust-segment → filter-segment), 再叠加可选 LLM 修正, 写出最终字幕段。
//!
//! 镜像 TS `packages/core/steps/sf_ocr/ocr_fix.ts` (stepSfOcrFix)。
//!
//! 注意: LLM 修正通过 `llm` crate 的 `chat_completions` + `ocr_llm_fix` 实现 (`llmFix=true` 时),
//! 失败时 warn 并保留原文 (不中断 step)。

use crate::cmd::env::ensure_bin;
use crate::steps::sf_ocr::fix_args::SfOcrFixInput;
use crate::steps::utils::paths::{sf_ocr_fix, SrtJsonPath};
use crate::steps::utils::{now_iso, set_step_anyhow, StepPatch, StepStatus};
use std::process::Command;

/// 入口 (镜像 TS `stepSfOcrFix`)。
///
/// 单参 [`SfOcrFixInput`]：依赖（frames.json）+ 任务目录 + 本次参数都在里面。
pub fn step_sf_ocr_fix(input: &SfOcrFixInput) -> anyhow::Result<()> {
    let video_dir = input.video_dir.clone();
    tracing::info!(target: "sf_ocr", "start");

    let frames_file = input.frames_path.as_path();
    if !frames_file.exists() {
        return Err(anyhow::anyhow!(
            "frames.json not found: {}; run sf_ocr first",
            frames_file.display()
        ));
    }
    // 画面高度由 `subtitle-ocr-post` 自己从 frames.json 的 meta.video_height 读
    // (v0.1.1 起 subtitle-ocr 识别侧写入), 这里不再传 --video-height, 也不再为拿
    // 分辨率去碰 video_source_path。
    let out_dir = sf_ocr_fix::OutDir::new(&video_dir);
    std::fs::create_dir_all(out_dir.as_path())
        .map_err(|e| anyhow::anyhow!("创建 {} 失败: {}", out_dir.as_path().display(), e))?;

    let args = &input.args;

    let bin = ensure_bin("subtitle_ocr_post_bin").map_err(|e| {
        anyhow::anyhow!(
            "{e}\n若下载失败, 请手动执行: cargo run -p cli -- env --action ensure --targets subtitle_ocr_post_bin"
        )
    })?;

    let post_args = vec![
        "--frames".to_string(),
        frames_file.to_string_lossy().into_owned(),
        "--out".to_string(),
        out_dir.as_path().to_string_lossy().into_owned(),
        "--threshold".to_string(),
        args.adjusted_confidence_threshold.to_string(),
        "--stop-at".to_string(),
        "filter-segment".to_string(),
    ];
    tracing::info!(target: "sf_ocr", "subtitle-ocr-post {}", post_args.join(" "));
    let status = Command::new(&bin)
        .args(&post_args)
        .status()
        .map_err(|e| anyhow::anyhow!("spawn subtitle-ocr-post 失败: {e}"))?;
    if !status.success() {
        return Err(anyhow::anyhow!(
            "subtitle-ocr-post failed with exit code {:?}",
            status.code()
        ));
    }

    let filter_file = sf_ocr_fix::FilterPath::new(&video_dir);
    let filtered: serde_json::Value = {
        let raw = std::fs::read_to_string(filter_file.as_path())
            .map_err(|e| anyhow::anyhow!("读取 {} 失败: {}", filter_file.as_path().display(), e))?;
        serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("解析 {} 失败: {}", filter_file.as_path().display(), e))?
    };
    let segments = filtered
        .get("result")
        .and_then(|r| r.get("segments"))
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    tracing::info!(target: "sf_ocr",
        "subtitle-ocr-post → {} segments (filtered)",
        segments.len()
    );

    // === LLM 修正 (最后一层修复) ===
    let llm_fix_file = sf_ocr_fix::LlmFixPath::new(&video_dir);
    let mut final_segments = segments.clone();
    if args.llm_fix.llm_fix {
        let src_texts: Vec<String> = segments
            .iter()
            .filter_map(|s| s.get("text").and_then(|t| t.as_str()).map(String::from))
            .collect();
        // 源语言走统一的解析（`<asr>/out.json` > 配置 sourceLang > 兜底空串）。
        // 不在这里内联回退链——否则与 `src_language` 各算各的，容易分叉。
        let lang_label = llm::lang_label(input.source_lang.code());
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
        std::fs::write(llm_fix_file.as_path(), s).map_err(|e| {
            anyhow::anyhow!("写入 {} 失败: {}", llm_fix_file.as_path().display(), e)
        })?;
    }

    // 规范化字幕产物（下游 translate/split_audio/mix_video 的唯一契约）——
    // llmFix 与否已由上面消化，这里统一落最终结果。
    let srt_segs: Vec<subtitle::SubtitleSegment> = final_segments
        .iter()
        .filter_map(|s| {
            Some(subtitle::SubtitleSegment {
                text: s.get("text")?.as_str()?.to_string(),
                start_ms: s.get("start_ms")?.as_u64()? as u32,
                end_ms: s.get("end_ms")?.as_u64()? as u32,
            })
        })
        .collect();
    crate::steps::utils::write_srt_json(sf_ocr_fix::SrtPath::new(&video_dir).as_path(), &srt_segs)?;

    set_step_anyhow(
        &video_dir,
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
    use crate::steps::sf_ocr::fix_args::OcrFixArgs;
    use serde_json::json;

    fn input_at(input: serde_json::Value) -> crate::input::Input {
        serde_json::from_value(input).unwrap()
    }

    #[test]
    fn args_defaults() {
        let input = input_at(json!({"steps": {"sf_ocr_fix": {}}}));
        let cfg = OcrFixArgs::from_input(&input);
        assert_eq!(cfg.adjusted_confidence_threshold, 0.45);
        assert!(!cfg.llm_fix.llm_fix);
    }

    #[test]
    fn args_camel_case_and_llm_fix() {
        let input = input_at(json!({"steps": {"sf_ocr_fix": {
            "adjustedConfidenceThreshold": 0.6, "llmFix": true
        }}}));
        let cfg = OcrFixArgs::from_input(&input);
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
        let input = input_at(json!({}));
        let res = step_sf_ocr_fix(&SfOcrFixInput::new(&dir, &input));
        assert!(res.is_err());
        assert!(
            res.unwrap_err().to_string().contains("run sf_ocr first"),
            "应提示先跑 sf_ocr"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
