//! split_audio: 按字幕/翻译时间轴把音频切块, 供 tts 逐段合成。
//!
//! 镜像 TS `packages/core/stages/06_split_audio/split_audio.ts`。
//! - 权威字幕文件 (asr_fix / sf_ocr / asr_ocr 按 subtitleSource) 取时间轴
//! - translate.enabled 时译文来自 translation.{lang}.json, 否则退回原文
//! - padSegments 加前后 padding 得到切块时序 -> split_audio.json
//! - 意图时序 (未 padding) -> timings.json
//! - 仅 dub 模式有 vocals 时切 wav 块; subtitle 模式只产出时序文件
//! - 可选 vadAlign 用静音检测前移每段起点

pub mod args;
pub mod out;
pub mod pad_segment;
pub mod util;
pub mod vad_align;

use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::context::TaskCtx;
use crate::stages::split_audio::out::{
    SplitAudioResult, SplitAudioSegment, SplitAudioTiming, SplitAudioTimingResult,
    TranslateResultMeta,
};
use crate::stages::split_audio::pad_segment::{pad_segments, SegmentBounds};
use crate::stages::utils::{
    ensure_dir, now_iso, probe_duration_ms, read_translation_result, resolve_language,
    set_stage_anyhow, split_audio_path, split_audio_timings_path, subtitle_file_path,
    video_source_path, vocals_path, StagePatch, StageStatus,
};

pub use args::SplitAudioArgs;

/// 从 `ctx.input.stages.split_audio` 解析配置, 不存在时返回默认 (与 TS default 对齐)
pub fn read_args(ctx: &TaskCtx) -> SplitAudioArgs {
    ctx.input
        .get("stages")
        .and_then(|v| v.get("split_audio"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// SplitAudioTiming 实现 SegmentBounds (pad_segments 用 start/end)。
impl SegmentBounds for SplitAudioTiming {
    fn start_ms(&self) -> u64 {
        self.start_ms
    }
    fn end_ms(&self) -> u64 {
        self.end_ms
    }
}

/// 入口 (镜像 TS `stageSplitAudio`)。
pub fn stage_split_audio(ctx: &TaskCtx) -> anyhow::Result<()> {
    let task_dir = ctx.task.task_dir.clone();
    tracing::info!(target: "split_audio", "start");

    let args = read_args(ctx);

    // 权威字幕文件 (时间轴)
    let srt_file = subtitle_file_path(ctx);
    if !Path::new(&srt_file).exists() {
        return Err(anyhow::anyhow!("字幕文件不存在: {srt_file}"));
    }
    let srt_raw = std::fs::read_to_string(&srt_file)
        .map_err(|e| anyhow::anyhow!("读取字幕文件 {srt_file} 失败: {e}"))?;
    let srt: Value = serde_json::from_str(&srt_raw)
        .map_err(|e| anyhow::anyhow!("解析字幕文件 {srt_file} 失败: {e}"))?;
    let srt_segments = srt
        .get("result")
        .and_then(|r| r.get("segments"))
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    if srt_segments.is_empty() {
        return Err(anyhow::anyhow!("{srt_file} 无 segments"));
    }

    // 源音频: 可被 split_audio.sourceFilePath 覆盖; 否则有 vocals 用干净人声
    let (src_lang, target_lang) = resolve_language(ctx)?;
    let fallback_source = video_source_path(ctx)?;
    let source_audio = args
        .source_file_path
        .clone()
        .unwrap_or_else(|| fallback_source);

    let vocals_file_path = args.vocals_file_path.clone().unwrap_or_else(|| {
        let p = vocals_path(&task_dir);
        p.to_string_lossy().into_owned()
    });
    let has_vocals = Path::new(&vocals_file_path).exists();
    let source_audio = if has_vocals {
        vocals_file_path
    } else {
        source_audio
    };

    let translate_enabled = ctx
        .input
        .get("stages")
        .and_then(|v| v.get("translate"))
        .and_then(|v| v.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    // 拼 timings: 有翻译取 dst 文本, 否则退回原文
    let timings: Vec<SplitAudioTiming> = if translate_enabled {
        let trans = read_translation_result(ctx)?;
        let segs = trans
            .get("segments")
            .and_then(|s| s.as_array())
            .cloned()
            .unwrap_or_default();
        if segs.is_empty() {
            return Err(anyhow::anyhow!("translation 无 segments"));
        }
        segs.iter()
            .enumerate()
            .map(|(i, seg)| SplitAudioTiming {
                seg_idx: (i + 1) as u32,
                text: seg
                    .get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string(),
                start_ms: seg.get("start_ms").and_then(|v| v.as_u64()).unwrap_or(0),
                end_ms: seg.get("end_ms").and_then(|v| v.as_u64()).unwrap_or(0),
                dst: seg
                    .get("dst")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string(),
                src_lang: seg
                    .get("src_lang")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                dst_lang: seg
                    .get("dst_lang")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                speaker: seg
                    .get("speaker")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                text_confidence: None,
            })
            .collect()
    } else {
        srt_segments
            .iter()
            .enumerate()
            .map(|(i, seg)| {
                let text = seg
                    .get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                SplitAudioTiming {
                    seg_idx: (i + 1) as u32,
                    text: text.clone(),
                    start_ms: seg.get("start_ms").and_then(|v| v.as_u64()).unwrap_or(0),
                    end_ms: seg.get("end_ms").and_then(|v| v.as_u64()).unwrap_or(0),
                    dst: text, // 未翻译时 dst 直接用原文
                    src_lang: Some(src_lang.clone()),
                    dst_lang: Some(src_lang.clone()),
                    speaker: None,
                    text_confidence: None,
                }
            })
            .collect()
    };

    // padSegments 得到真实切块时序
    let bounds = pad_segments(&timings, args.start_pad_ms, args.end_pad_ms);
    let mut split_segments: Vec<SplitAudioSegment> = timings
        .iter()
        .zip(bounds.iter())
        .map(|(t, (s, e))| SplitAudioSegment {
            timing: t.clone(),
            split_start_ms: *s,
            split_end_ms: *e,
        })
        .collect();

    let total_ms = probe_duration_ms(&source_audio);

    let split_audio_dir = Path::new(&task_dir).join("split_audio");
    let vocals_segment_dir = split_audio_dir.join("vocals");
    ensure_dir(&vocals_segment_dir)?;
    ensure_dir(&split_audio_dir)?;

    // 切块 (仅 dub 模式有 vocals 时执行; subtitle 模式跳过, 只产出时序文件)
    if has_vocals {
        // 若翻译文件比已切出的块更新 (重跑翻译), 清空旧块重新切
        let translation_file = crate::stages::utils::translation_file_path(&task_dir, &target_lang);
        let has_seg = fs::read_dir(&vocals_segment_dir)
            .ok()
            .map(|d| {
                d.filter_map(|e| e.ok())
                    .any(|e| e.path().extension().map(|x| x == "wav").unwrap_or(false))
            })
            .unwrap_or(false);
        if has_seg && translation_file.exists() {
            let trans_mtime = fs::metadata(&translation_file)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            // 任意旧块的最新 mtime
            let oldest_seg_is_newer = fs::read_dir(&vocals_segment_dir)
                .ok()
                .map(|d| {
                    d.filter_map(|e| e.ok())
                        .filter(|e| e.path().extension().map(|x| x == "wav").unwrap_or(false))
                        .filter_map(|e| fs::metadata(e.path()).ok().and_then(|m| m.modified().ok()))
                        .all(|m| m < trans_mtime)
                })
                .unwrap_or(false);
            if oldest_seg_is_newer {
                let _ = fs::remove_dir_all(&vocals_segment_dir);
                ensure_dir(&vocals_segment_dir)?;
            }
        }

        for (i, seg) in split_segments.iter().enumerate() {
            let idx = format!("{:04}", i + 1);
            let out_path = vocals_segment_dir.join(format!("{idx}.wav"));
            let out_path = out_path.to_string_lossy().into_owned();
            let start_ms = seg.split_start_ms;
            let end_ms = seg.split_end_ms;
            if start_ms >= end_ms {
                // 无效段: 写 44 字节空 wav 头占位
                fs::write(&out_path, vec![0u8; 44]).ok();
                tracing::info!("#{} invalid ({} >= {}), empty wav", i + 1, start_ms, end_ms);
                continue;
            }
            let start = start_ms;
            let end = total_ms.min(end_ms);
            if end <= start {
                fs::write(&out_path, vec![0u8; 44]).ok();
                continue;
            }
            util::cut_audio_range(&source_audio, start, end, &out_path)?;
        }
    }

    let meta = TranslateResultMeta {
        src_lang: src_lang.clone(),
        target_lang: if translate_enabled {
            target_lang.clone()
        } else {
            src_lang.clone()
        },
    };
    let split_result = SplitAudioResult {
        segments: split_segments.clone(),
        meta: meta.clone(),
    };
    let split_path = split_audio_path(&task_dir);
    let json = serde_json::to_string_pretty(&split_result)
        .map_err(|e| anyhow::anyhow!("序列化 split_audio 结果失败: {e}"))?;
    fs::write(&split_path, json)
        .map_err(|e| anyhow::anyhow!("写入 {} 失败: {}", split_path.display(), e))?;

    // VAD alignment (可选)
    if args.vad_align {
        let mut timings_mut = timings.clone();
        let corrected = vad_align::apply_vad_align(
            &mut split_segments,
            &mut timings_mut,
            &source_audio,
            total_ms,
            &vocals_segment_dir.to_string_lossy(),
            has_vocals,
        );
        if corrected {
            let split_result = SplitAudioResult {
                segments: split_segments.clone(),
                meta: meta.clone(),
            };
            let json = serde_json::to_string_pretty(&split_result)
                .map_err(|e| anyhow::anyhow!("序列化 split_audio 结果失败: {e}"))?;
            fs::write(&split_path, json)
                .map_err(|e| anyhow::anyhow!("写入 {} 失败: {}", split_path.display(), e))?;
        }
    }

    let timing_result = SplitAudioTimingResult {
        segments: timings.clone(),
    };
    let timings_path = split_audio_timings_path(&task_dir);
    let json = serde_json::to_string_pretty(&timing_result)
        .map_err(|e| anyhow::anyhow!("序列化 timings 失败: {e}"))?;
    fs::write(&timings_path, json)
        .map_err(|e| anyhow::anyhow!("写入 {} 失败: {}", timings_path.display(), e))?;

    set_stage_anyhow(
        &task_dir,
        "split_audio",
        StagePatch {
            status: Some(StageStatus::Success),
            completed_at: Some(now_iso()),
            progress: Some(100.0),
            last_message: Some("Split".to_string()),
            ..Default::default()
        },
    )?;
    tracing::info!(target: "split_audio", "done");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::read_ctx_from_value;
    use serde_json::json;

    fn ctx_with_input(input: serde_json::Value) -> TaskCtx {
        crate::context::read_ctx_from_value(input).unwrap()
    }

    #[test]
    fn field_defaults_when_absent() {
        let ctx = ctx_with_input(json!({
            "task": {"id": "t", "task_dir": "/x", "url": "http://e", "source": "remote",
                     "status": "running", "created_at": "2024-01-01T00:00:00Z"},
            "input": {"stages": {"split_audio": {}}}
        }));
        let cfg = read_args(&ctx);
        assert!(!cfg.vad_align);
        assert_eq!(cfg.start_pad_ms, 100);
        assert_eq!(cfg.end_pad_ms, 300);
        assert!(cfg.vocals_file_path.is_none());
        assert!(cfg.source_file_path.is_none());
    }

    #[test]
    fn read_config_parses_camel_case_fields() {
        let ctx = ctx_with_input(json!({
            "task": {"id": "t", "task_dir": "/x", "url": "http://e", "source": "remote",
                     "status": "running", "created_at": "2024-01-01T00:00:00Z"},
            "input": {"stages": {"split_audio": {
                "vadAlign": true,
                "startPadMs": 150,
                "endPadMs": 400,
                "vocalsFilePath": "/v.wav",
                "sourceFilePath": "/s.mp4"
            }}}
        }));
        let cfg = read_args(&ctx);
        assert!(cfg.vad_align);
        assert_eq!(cfg.start_pad_ms, 150);
        assert_eq!(cfg.end_pad_ms, 400);
        assert_eq!(cfg.vocals_file_path.as_deref(), Some("/v.wav"));
        assert_eq!(cfg.source_file_path.as_deref(), Some("/s.mp4"));
    }
}
