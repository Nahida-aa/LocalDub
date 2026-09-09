//! asr_ocr_fix: Fuse ASR + OCR (mirrors TS `stageAsrOcrFix`).
//!
//! Pipeline (in-process using `subtitle-ocr-post`):
//!   1. (Optional) Resample: collect candidate timestamps, extract frames, OCR via subtitle-ocr bin, merge
//!   2. adjust-box → filter-box → merge → adjust-segment → filter-segment
//!   3. ASR boundary alignment (stage 6)
//!   4. fixOverlap + final dedup (stage 7)
//!   5. Optional LLM fix (stage 8)

use crate::cmd::env::ensure_bin;
use crate::context::TaskCtx;
use crate::stages::asr::out::AsrResult;
use crate::stages::asr_ocr::fix_args::AsrOcrFixArgs;
use crate::stages::utils::{
    StagePatch, StageStatus, now_iso, set_stage_anyhow,
    asr_ocr_pre_dir, asr_ocr_dir, asr_ocr_fix_dir, asr_dir, video_source_path,
    probe_video_resolution,
};
use anyhow::Result;
use ocr_types::{
    FrameResult, OcrFramesResult, OcrSegment, SubtitleSegment,
    compute_box_x_stats, compute_box_y_stats, edit_distance,
};
use subtitle_ocr_post::{
    BoxAdjustedArgs, MergeFramesArgs, OcrSegmentAdjustArgs,
    merge_frames, ocr_frames_adjust_box, ocr_frames_filter_box,
    ocr_segment_adjust, ocr_segment_filter_with_meta, OcrSegmentWithAdjust,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;
use tracing::info;

/// Helper: read JSON file.
fn read_json_file<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let raw = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

/// Helper: write JSON file (pretty).
fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    fs::write(path, json)?;
    Ok(())
}

/// ASR split result (from asr_ocr_pre/asr_split.json).
#[derive(Debug, Clone, Deserialize, Serialize)]
struct AsrSplitResult {
    result: AsrSplitResultBody,
    meta: AsrSplitResultMeta,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
struct AsrSplitResultBody {
    text: String,
    segments: Vec<SubtitleSegment>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
struct AsrSplitResultMeta {
    original_segments_count: usize,
    segments_count: usize,
}

/// Stage 6: ASR boundary alignment result.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct AsrOcrMergedResult {
    audio_info: AudioInfo,
    #[serde(rename = "_engine")]
    engine: String,
    #[serde(rename = "_fusion_params")]
    fusion_params: serde_json::Value,
    result: AsrOcrMergedBody,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
struct AudioInfo {
    duration: u64,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
struct AsrOcrMergedBody {
    text: String,
    segments: Vec<OcrSegment>,
}

/// Stage 7: Fused result.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct AsrOcrFusedResult {
    #[serde(rename = "_engine")]
    engine: String,
    #[serde(rename = "_fusion_params")]
    fusion_params: serde_json::Value,
    result: AsrOcrFusedBody,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
struct AsrOcrFusedBody {
    text: String,
    segments: Vec<OcrSegment>,
}

/// Read asr_ocr_fix config (defaults to AsrOcrFixArgs::default).
fn read_args(ctx: &TaskCtx) -> AsrOcrFixArgs {
    ctx.input
        .get("stages")
        .and_then(|v| v.get("asr_ocr_fix"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Stage 1 (optional): Resample - extract additional frames at isolated high-confidence frames.
fn maybe_resample(
    ctx: &TaskCtx,
    ocr_frames: &OcrFramesResult,
    out_dir: &Path,
    args: &AsrOcrFixArgs,
) -> Result<OcrFramesResult> {
    if !args.is_resample {
        return Ok(ocr_frames.clone());
    }

    // Collect candidate timestamps (from resample.ts logic)
    let frames = &ocr_frames.frames;
    const RESAMPLE_CONF_THRESH: f64 = 0.6;
    const RESAMPLE_STEP_MS: u64 = 100;
    const RESAMPLE_RANGE_MS: u64 = 500;

    let has_nearby_same_text = |raw_frames: &[FrameResult], i: usize, f: &FrameResult| -> bool {
        raw_frames.iter().enumerate().any(|(j, other)| {
            j != i
                && other.text == f.text
                && (other.timestamp as i64 - f.timestamp as i64).abs() <= RESAMPLE_RANGE_MS as i64
        })
    };

    let mut candidate_ts = HashSet::new();
    for (i, f) in frames.iter().enumerate() {
        if f.text.is_empty() || f.text_confidence < RESAMPLE_CONF_THRESH {
            continue;
        }
        if has_nearby_same_text(frames, i, f) {
            continue;
        }
        // In ±RESAMPLE_RANGE_MS, step by RESAMPLE_STEP_MS
        let start = f.timestamp.saturating_sub(RESAMPLE_RANGE_MS);
        let end = f.timestamp + RESAMPLE_RANGE_MS;
        let mut t = start;
        while t <= end {
            candidate_ts.insert(t);
            t += RESAMPLE_STEP_MS;
        }
    }

    let existing_ts: HashSet<u64> = frames.iter().map(|f| f.timestamp).collect();
    let mut new_ts: Vec<u64> = candidate_ts.into_iter().filter(|t| !existing_ts.contains(t)).collect();
    if new_ts.is_empty() {
        info!(target: "asr_ocr", "Resample: no new candidates");
        return Ok(ocr_frames.clone());
    }
    new_ts.sort_unstable();

    // Extract frames to resampled_frames/
    let video_path = video_source_path(ctx)?;
    let resample_dir = out_dir.join("resampled_frames");
    fs::create_dir_all(&resample_dir)?;

    let mut extract_count = 0;
    for ms in &new_ts {
        let frame_path = resample_dir.join(format!("{:07}.jpg", ms));
        let ffmpeg_args = vec![
            "-ss".into(), format!("{:.3}", *ms as f64 / 1000.0),
            "-i".into(), video_path.clone(),
            "-frames:v".into(), "1".into(),
            "-qscale:v".into(), "2".into(),
            frame_path.to_string_lossy().into_owned(),
        ];
        if crate::stages::utils::ffmpeg(&ffmpeg_args).is_ok() {
            extract_count += 1;
        }
    }
    info!(target: "asr_ocr", "Resample: extracted {} frames", extract_count);

    if extract_count == 0 {
        return Ok(ocr_frames.clone());
    }

    // OCR the resampled frames using subtitle-ocr bin
    let bin = ensure_bin("subtitle_ocr_bin").map_err(|e| {
        anyhow::anyhow!(
            "{e}\nIf download fails, run: cargo run -p cli -- env --action ensure --targets subtitle_ocr_bin"
        )
    })?;

    let resample_out = out_dir.join("resampled_frames.json");
    let mut cmd = Command::new(&bin);
    cmd.arg("--dir").arg(&resample_dir);
    cmd.arg("--out").arg(&resample_out);
    cmd.arg("--text-confidence-threshold").arg("0.45");
    let status = cmd.status()?;
    if !status.success() {
        return Err(anyhow::anyhow!("resample subtitle-ocr failed"));
    }

    let resampled_data: OcrFramesResult = read_json_file(&resample_out)?;
    let mut merged_frames = frames.clone();
    merged_frames.extend(resampled_data.frames);
    merged_frames.sort_by_key(|f| f.timestamp);

    // Write merged frames to out_dir/ocr_frames.json
    let merged = OcrFramesResult {
        frames: merged_frames,
        meta: ocr_frames.meta.clone(),
    };
    write_json_file(&out_dir.join("ocr_frames.json"), &merged)?;

    Ok(merged)
}

/// fixOverlap implementation (mirrors TS `fixOverlap` in merge_frames.ts).
fn fix_overlap(
    asr_segs: &[OcrSegment],
    raw_frames: &[FrameResult],
    ocr_segs: &[OcrSegment],
    max_advance_ms: u64,
) -> Vec<OcrSegment> {
    let mut fix = asr_segs.to_vec();
    let mut sorted_frames = raw_frames.to_vec();
    sorted_frames.sort_by_key(|f| f.timestamp);

    // Step 1: boundary adjustment using raw frames
    for i in 1..fix.len() {
        let prev = fix[i - 1].clone();
        let cur = fix[i].clone();
        if cur.base.start_ms >= prev.base.end_ms {
            continue;
        }
        let overlap_end = prev.base.end_ms.min(cur.base.end_ms);
        for f in &sorted_frames {
            if f.timestamp < cur.base.start_ms {
                continue;
            }
            if f.timestamp > overlap_end {
                break;
            }
            let d_cur = edit_distance(&f.text, &cur.base.text);
            let d_prev = edit_distance(&f.text, &prev.base.text);
            if d_cur <= 2 && d_cur < d_prev {
                fix[i - 1].base.end_ms = f.timestamp;
                fix[i].base.start_ms = f.timestamp;
                break;
            }
        }
    }

    // Step 2: maxAdvanceMs check against ocrSegs
    for seg in &mut fix {
        let mut best_ocr: Option<&OcrSegment> = None;
        let mut best_overlap = 0;
        for o in ocr_segs {
            let overlap = if seg.base.start_ms == seg.base.end_ms {
                if seg.base.start_ms >= o.base.start_ms && seg.base.start_ms <= o.base.end_ms { 1 } else { 0 }
            } else {
                let ov = seg.base.end_ms.min(o.base.end_ms).saturating_sub(seg.base.start_ms.max(o.base.start_ms));
                if ov > 0 { ov } else { 0 }
            };
            if overlap > best_overlap && edit_distance(&seg.base.text, &o.base.text) <= 2 {
                best_overlap = overlap;
                best_ocr = Some(o);
            }
        }
        if let Some(o) = best_ocr {
            if seg.base.start_ms + max_advance_ms < o.base.start_ms {
                seg.base.start_ms = o.base.start_ms;
            }
        }
    }

    // Filter zero-length segments
    fix.retain(|s| s.base.end_ms > s.base.start_ms);
    fix
}

/// Final dedup: adjacent same text within 2000ms → merge.
fn final_dedup(segs: Vec<OcrSegment>) -> Vec<OcrSegment> {
    let mut merged: Vec<OcrSegment> = Vec::new();
    for s in segs {
        if let Some(prev) = merged.last_mut() {
            if prev.base.text.trim() == s.base.text.trim() && s.base.start_ms.saturating_sub(prev.base.end_ms) <= 2000 {
                prev.base.end_ms = s.base.end_ms;
                prev.text_confidence = (prev.text_confidence + s.text_confidence) / 2.0;
                continue;
            }
        }
        merged.push(s);
    }
    merged
}

/// Entry point (mirrors TS `stageAsrOcrFix`).
pub fn stage_asr_ocr_fix(ctx: &TaskCtx) -> Result<()> {
    let task_dir = ctx.task.task_dir.clone();
    info!(target: "asr_ocr", "stage_asr_ocr_fix: start");

    set_stage_anyhow(
        &task_dir,
        "asr_ocr_fix",
        StagePatch {
            last_message: Some("Fusing ASR + OCR...".into()),
            progress: Some(0.0),
            ..Default::default()
        },
    )?;

    let args = read_args(ctx);
    let out_dir = asr_ocr_fix_dir(&task_dir);
    fs::create_dir_all(&out_dir)?;

    // Read upstream artifacts
    let asr_file = asr_dir(&task_dir).join("asr.json");
    let asr_split_file = asr_ocr_pre_dir(&task_dir).join("asr_split.json");
    let ocr_frames_file = asr_ocr_dir(&task_dir).join("frames.json");

    if !asr_file.exists() {
        return Err(anyhow::anyhow!("asr.json not found: {}", asr_file.display()));
    }
    if !asr_split_file.exists() {
        return Err(anyhow::anyhow!("asr_split.json not found, run asr_ocr_pre first"));
    }
    if !ocr_frames_file.exists() {
        return Err(anyhow::anyhow!("frames.json not found, run asr_ocr first"));
    }

    let asr_data: AsrResult = read_json_file(&asr_file)?;
    let asr_split_data: AsrSplitResult = read_json_file(&asr_split_file)?;
    let ocr_frames_data: OcrFramesResult = read_json_file(&ocr_frames_file)?;

    let asr_raw_len = asr_data.result.segments.len();
    let asr_segs = asr_split_data.result.segments.clone();

    // Stage 1: Optional resample
    let ocr_frames = maybe_resample(ctx, &ocr_frames_data, &out_dir, &args)?;
    let frames = &ocr_frames.frames;

    // --- Pipeline: adjust-box → filter-box → merge → adjust-segment → filter-segment ---
    // 1. adjust-box
    let y_stats = compute_box_y_stats(frames);
    let x_stats = compute_box_x_stats(frames);
    let box_args = BoxAdjustedArgs {
        box_adjusted_threshold: Some(args.ocr_fix.box_adjusted_threshold as f32),
    };
    let adjust_result = ocr_frames_adjust_box(frames, &y_stats, &x_stats, &box_args);
    write_json_file(&out_dir.join("frames_box_adjust.json"), &adjust_result)?;

    // 2. filter-box
    let filtered = ocr_frames_filter_box(&adjust_result.frames);
    write_json_file(&out_dir.join("frames_box_filter.json"), &filtered)?;

    // 3. merge
    let merge_args = MergeFramesArgs {
        is_merge_substring: Some(args.ocr_fix.is_merge_substring),
        dedup_edit_distance: Some(args.ocr_fix.dedup_edit_distance),
    };
    let merged = merge_frames(&filtered.frames, &merge_args);
    write_json_file(&out_dir.join("frames_merged.json"), &merged)?;

    // 4. adjust-segment
    let (_, video_height) = probe_video_resolution(&video_source_path(ctx)?);
    let y_stats2 = compute_box_y_stats(&filtered.frames);
    let seg_args = OcrSegmentAdjustArgs {
        iso_threshold_ms: Some(args.ocr_fix.iso_threshold_ms as u64),
        adjust_y_weight: Some(args.ocr_fix.adjust_y_weight as f32),
        adjust_iso_weight: Some(args.ocr_fix.adjust_iso_weight as f32),
        adjust_y_factor: Some(args.ocr_fix.adjust_y_factor as f32),
    };
    let seg_adjust: Vec<OcrSegmentWithAdjust> = ocr_segment_adjust(
        &merged.segments,
        &filtered.frames,
        &y_stats2,
        video_height as f32,
        &seg_args,
    );
    write_json_file(&out_dir.join("segment_adjust.json"), &seg_adjust)?;

    // 5. filter-segment
    let seg_filter = ocr_segment_filter_with_meta(
        &seg_adjust,
        args.ocr_fix.adjusted_confidence_threshold as f32,
    );
    write_json_file(&out_dir.join("segment_filter.json"), &seg_filter)?;

    // === Stage 6: ASR boundary alignment ===
    // For each OCR segment, find best overlapping ASR segment, use ASR's start/end as new boundaries
    let asr_ocr_segs: Vec<OcrSegment> = seg_filter
        .result
        .segments
        .iter()
        .map(|seg| {
            let mut best_asr: Option<&SubtitleSegment> = None;
            let mut best_overlap = 0;
            for asr in &asr_segs {
                let overlap = if seg.base.base.start_ms == seg.base.base.end_ms {
                    if seg.base.base.start_ms >= asr.start_ms && seg.base.base.start_ms <= asr.end_ms {
                        1
                    } else {
                        0
                    }
                } else {
                    let ov = seg.base.base.end_ms.min(asr.end_ms).saturating_sub(seg.base.base.start_ms.max(asr.start_ms));
                    if ov > 0 { ov } else { 0 }
                };
                if overlap > best_overlap {
                    best_overlap = overlap;
                    best_asr = Some(asr);
                }
            }
            let mut s = seg.clone();
            if let Some(a) = best_asr {
                s.base.base.start_ms = a.start_ms;
                s.base.base.end_ms = a.end_ms;
            }
            // Convert OcrSegmentWithAdjust -> OcrSegment
            OcrSegment {
                base: s.base.base.clone(),
                y_range: s.base.y_range,
                text_confidence: s.base.text_confidence,
                frame_count: s.base.frame_count,
                frames: s.base.frames.clone(),
            }
        })
        .collect();

    let asr_ocr_text = asr_ocr_segs.iter().map(|s| s.base.text.as_str()).collect::<Vec<_>>().join(" ");
    let merged_result = AsrOcrMergedResult {
        audio_info: AudioInfo { duration: asr_ocr_segs.last().map(|s| s.base.end_ms).unwrap_or(0) },
        engine: "asr_ocr".into(),
        fusion_params: serde_json::json!({
            "strategy": "end2fps",
            "ocrCalls": asr_ocr_segs.len(),
            "asrSegs": asr_raw_len,
            "asrSplits": asr_segs.len(),
        }),
        result: AsrOcrMergedBody {
            text: asr_ocr_text,
            segments: asr_ocr_segs.clone(),
        },
    };
    write_json_file(&out_dir.join("asr_ocr_merged.json"), &merged_result)?;

    // === Stage 7: fixOverlap + final dedup ===
    let max_advance_ms = ctx
        .input
        .get("stages")
        .and_then(|v| v.get("mix_audio"))
        .and_then(|v| v.get("maxAdvanceMs"))
        .and_then(|v| v.as_u64())
        .unwrap_or(500);
    // Convert OcrSegmentWithAdjust to OcrSegment for fix_overlap
    let ocr_segs_for_fix: Vec<OcrSegment> = seg_filter
        .result
        .segments
        .iter()
        .map(|s| OcrSegment {
            base: s.base.base.clone(),
            y_range: s.base.y_range,
            text_confidence: s.base.text_confidence,
            frame_count: s.base.frame_count,
            frames: s.base.frames.clone(),
        })
        .collect();
    let fix = fix_overlap(&asr_ocr_segs, frames, &ocr_segs_for_fix, max_advance_ms);
    let deduped = final_dedup(fix);

    let fix_text = deduped.iter().map(|s| s.base.text.as_str()).collect::<Vec<_>>().join(" ");
    let fused_result = AsrOcrFusedResult {
        engine: "asr_ocr".into(),
        fusion_params: serde_json::json!({
            "strategy": "end2fps",
            "maxAdvanceMs": max_advance_ms,
            "ocrCalls": seg_filter.meta.segment_count,
            "asrSegs": asr_raw_len,
            "asrSplits": asr_segs.len(),
            "fixSegs": deduped.len(),
        }),
        result: AsrOcrFusedBody {
            text: fix_text,
            segments: deduped,
        },
    };
    write_json_file(&out_dir.join("asr_ocr_fused.json"), &fused_result)?;

    // === Stage 8 (optional): LLM fix ===
    if args.ocr_fix.llm_fix.llm_fix {
        let src_texts: Vec<String> = fused_result
            .result
            .segments
            .iter()
            .map(|s| s.base.text.clone())
            .collect();
        // Source language: ASR detected > task.sourceLang > default zh
        let src_lang = ctx
            .asr_language
            .clone()
            .or_else(|| {
                ctx.input
                    .get("task")
                    .and_then(|v| v.get("sourceLang"))
                    .and_then(|v| v.as_str())
                    .map(|s| crate::r#const::lang::Language::from(s))
            })
            .unwrap_or_else(crate::r#const::lang::default_lang);
        let lang_label = llm::lang_label(src_lang.code());
        info!(target: "asr_ocr",
            "asr_ocr_fix: LLM fix {} segs (model={})", src_texts.len(), args.ocr_fix.llm_fix.llm_model
        );
        match llm::ocr_llm_fix(&src_texts, &lang_label, &args.ocr_fix.llm_fix) {
            Ok(fixed) => {
                let mut llm_segments = fused_result.result.segments.clone();
                for (seg, text) in llm_segments.iter_mut().zip(fixed) {
                    seg.base.text = text;
                }
                let llm_text = llm_segments.iter().map(|s| s.base.text.as_str()).collect::<Vec<_>>().join(" ");
                let llm_result = serde_json::json!({
                    "result": {
                        "text": llm_text,
                        "segments": llm_segments,
                    }
                });
                write_json_file(&out_dir.join("asr_ocr_fused_llm_fix.json"), &llm_result)?;
            }
            Err(e) => {
                tracing::warn!(target: "asr_ocr", "asr_ocr_fix LLM fix failed, keeping original: {e}");
            }
        }
    }

    info!(target: "asr_ocr",
        "filter-segment done (threshold={}) → {} ASR → {} split → {} merged, {} fused",
        args.ocr_fix.adjusted_confidence_threshold,
        asr_raw_len,
        asr_segs.len(),
        asr_ocr_segs.len(),
        fused_result.result.segments.len()
    );

    set_stage_anyhow(
        &task_dir,
        "asr_ocr_fix",
        StagePatch {
            status: Some(StageStatus::Success),
            completed_at: Some(now_iso()),
            progress: Some(100.0),
            ..Default::default()
        },
    )?;
    info!(target: "asr_ocr", "done");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fix_overlap_adjusts_boundary() {
        // raw frame "世界" at 950 is closer to cur ("世界") than prev ("你好")
        // so boundary should move from 900 to 950
        let asr_segs = vec![
            OcrSegment { base: SubtitleSegment { text: "你好".into(), start_ms: 0, end_ms: 1000 }, y_range: None, text_confidence: 0.9, frame_count: None, frames: None },
            OcrSegment { base: SubtitleSegment { text: "世界".into(), start_ms: 900, end_ms: 2000 }, y_range: None, text_confidence: 0.9, frame_count: None, frames: None },
        ];
        let raw_frames = vec![
            FrameResult { text: "世界".into(), text_confidence: 0.9, boxes: vec![], x_range: [0.,0.], y_range: [0.,0.], timestamp: 950 },
        ];
        let ocr_segs = vec![
            OcrSegment { base: SubtitleSegment { text: "你好".into(), start_ms: 0, end_ms: 1000 }, y_range: None, text_confidence: 0.9, frame_count: None, frames: None },
            OcrSegment { base: SubtitleSegment { text: "世界".into(), start_ms: 950, end_ms: 2000 }, y_range: None, text_confidence: 0.9, frame_count: None, frames: None },
        ];
        let fix = fix_overlap(&asr_segs, &raw_frames, &ocr_segs, 500);
        assert_eq!(fix[0].base.end_ms, 950);
        assert_eq!(fix[1].base.start_ms, 950);
    }

    #[test]
    fn final_dedup_merges_adjacent_same_text() {
        let segs = vec![
            OcrSegment { base: SubtitleSegment { text: "hello".into(), start_ms: 0, end_ms: 1000 }, y_range: None, text_confidence: 0.9, frame_count: None, frames: None },
            OcrSegment { base: SubtitleSegment { text: "hello".into(), start_ms: 1500, end_ms: 2000 }, y_range: None, text_confidence: 0.8, frame_count: None, frames: None },
        ];
        let out = final_dedup(segs);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].base.end_ms, 2000);
    }
}