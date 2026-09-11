//! asr_ocr_pre: Split ASR segments by punctuation (mirroring TS `stepAsrOcrPre`)
//! and extract frames via ffmpeg.

use crate::context::WorkflowCtx;
use crate::steps::asr::out::{AsrResult, AsrSegment};
use crate::steps::utils::ffmpeg;
use crate::steps::utils::{
    asr_dir, asr_ocr_pre_dir, now_iso, set_step_anyhow, video_source_path, StepPatch, StepStatus,
};
use anyhow::Result;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use tracing::info;

const SPLIT_PAT: &str = r"[，,。！？.!?]";
const MIN_SUB_DUR: u32 = 800;

/// Find word indices where seg.text contains spaces (e.g. "陆 陆直循").
fn find_space_splits(text: &str, words: &[crate::steps::asr::out::AsrWord]) -> Vec<usize> {
    let chars: Vec<char> = text.chars().collect();
    let mut splits = Vec::new();
    let mut word_idx = 0;
    let mut word_pos = 0;
    for i in 0..chars.len() {
        if word_idx >= words.len() {
            break;
        }
        if chars[i] == ' ' {
            if word_idx > 0 {
                splits.push(word_idx - 1);
            }
            continue;
        }
        word_pos += 1;
        if word_pos >= words[word_idx].word.chars().count() {
            word_idx += 1;
            word_pos = 0;
        }
    }
    splits
}

static SPLIT_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(SPLIT_PAT).expect("Invalid SPLIT_PAT regex"));

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AsrSplitResult {
    result: AsrSplitResultBody,
    meta: AsrSplitResultMeta,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AsrSplitResultBody {
    text: String,
    segments: Vec<SubtitleSegment>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AsrSplitResultMeta {
    original_segments_count: usize,
    segments_count: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SubtitleSegment {
    pub text: String,
    pub start_ms: u32,
    pub end_ms: u32,
}

/// Split long ASR segments by punctuation using word-level timestamps.
fn split_asr_by_words(segs: &[AsrSegment]) -> Vec<SubtitleSegment> {
    segs.iter()
        .flat_map(|seg| {
            let ws = match &seg.words {
                Some(w) if w.len() >= 2 => w,
                _ => {
                    return vec![SubtitleSegment {
                        text: seg.text.clone(),
                        start_ms: seg.start_ms,
                        end_ms: seg.end_ms,
                    }];
                }
            };
            let space_splits = find_space_splits(&seg.text, ws);
            let mut punct_splits = Vec::new();
            for (i, w) in ws.iter().enumerate() {
                if SPLIT_RE.is_match(&w.word) {
                    punct_splits.push(i);
                }
            }
            let has_space_split = !space_splits.is_empty();
            let mut split_idx: Vec<usize> = space_splits;
            split_idx.extend(punct_splits);
            split_idx.sort_unstable();
            split_idx.dedup();
            if split_idx.len() <= 1 && !has_space_split {
                return vec![SubtitleSegment {
                    text: seg.text.clone(),
                    start_ms: seg.start_ms,
                    end_ms: seg.end_ms,
                }];
            }
            let total_end = ws.last().map(|w| w.end).unwrap_or(seg.end_ms);
            let mut use_idx = Vec::new();
            for i in 0..split_idx.len().saturating_sub(1) {
                let end_ms = ws[split_idx[i]].end;
                if total_end.saturating_sub(end_ms) >= MIN_SUB_DUR {
                    use_idx.push(split_idx[i]);
                }
            }
            use_idx.push(split_idx[split_idx.len() - 1]);
            if use_idx.len() <= 1 && !has_space_split {
                return vec![SubtitleSegment {
                    text: seg.text.clone(),
                    start_ms: seg.start_ms,
                    end_ms: seg.end_ms,
                }];
            }
            let mut sub_segs = Vec::new();
            let mut prev_idx = 0;
            for &end_idx in &use_idx {
                sub_segs.push(SubtitleSegment {
                    text: ws[prev_idx..=end_idx]
                        .iter()
                        .map(|w| w.word.as_str())
                        .collect::<Vec<_>>()
                        .join(""),
                    start_ms: ws[prev_idx].start,
                    end_ms: ws[end_idx].end,
                });
                prev_idx = end_idx + 1;
            }
            if prev_idx < ws.len() {
                sub_segs.push(SubtitleSegment {
                    text: ws[prev_idx..]
                        .iter()
                        .map(|w| w.word.as_str())
                        .collect::<Vec<_>>()
                        .join(""),
                    start_ms: ws[prev_idx].start,
                    end_ms: total_end,
                });
            }
            sub_segs
        })
        .collect()
}

/// Entry point (mirrors TS `stepAsrOcrPre`).
pub fn step_asr_ocr_pre(ctx: &WorkflowCtx) -> Result<()> {
    let workflow_dir = ctx.workflow.workflow_dir.clone();
    info!(target: "asr_ocr", "step_asr_ocr_pre: start");

    set_step_anyhow(
        &workflow_dir,
        "asr_ocr_pre",
        StepPatch {
            last_message: Some("Splitting ASR segments by punctuation...".into()),
            progress: Some(0.0),
            ..Default::default()
        },
    )?;

    let video_path = video_source_path(ctx)?;
    if !Path::new(&video_path).exists() {
        return Err(anyhow::anyhow!("Video not found: {}", video_path));
    }

    let asr_file = asr_dir(&workflow_dir).join("asr.json");
    if !asr_file.exists() {
        return Err(anyhow::anyhow!(
            "asr.json not found: {}",
            asr_file.display()
        ));
    }

    let asr_data: AsrResult = {
        let raw = fs::read_to_string(&asr_file)
            .map_err(|e| anyhow::anyhow!("读取 {} 失败: {e}", asr_file.display()))?;
        serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("解析 {} 失败: {e}", asr_file.display()))?
    };
    let asr_segs_raw = asr_data.result.segments;

    if asr_segs_raw.is_empty() {
        return Err(anyhow::anyhow!("No ASR segments found"));
    }

    // Step 1: Split ASR segments by punctuation
    info!(target: "asr_ocr", "{} Split ASR segments by punctuation", asr_segs_raw.len());
    let asr_segs = split_asr_by_words(&asr_segs_raw);

    let pre_dir = asr_ocr_pre_dir(&workflow_dir);
    fs::create_dir_all(&pre_dir)
        .map_err(|e| anyhow::anyhow!("创建 {} 失败: {}", pre_dir.display(), e))?;

    let asr_split_result = AsrSplitResult {
        result: AsrSplitResultBody {
            text: asr_segs
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            segments: asr_segs.clone(),
        },
        meta: AsrSplitResultMeta {
            original_segments_count: asr_segs_raw.len(),
            segments_count: asr_segs.len(),
        },
    };
    // Write asr_split.json
    let asr_split_path = pre_dir.join("asr_split.json");
    let json = serde_json::to_string_pretty(&asr_split_result)
        .map_err(|e| anyhow::anyhow!("序列化 asr_split.json 失败: {e}"))?;
    fs::write(&asr_split_path, json)
        .map_err(|e| anyhow::anyhow!("写入 {} 失败: {}", asr_split_path.display(), e))?;

    info!(target: "asr_ocr",
        "{} ASR segs → {} split segs", asr_segs_raw.len(), asr_segs.len()
    );

    // Step 2: Generate frame timestamps (end2fps strategy)
    set_step_anyhow(
        &workflow_dir,
        "asr_ocr_pre",
        StepPatch {
            last_message: Some(format!(
                "Extracting {} split segments frames...",
                asr_segs.len()
            )),
            progress: Some(10.0),
            ..Default::default()
        },
    )?;

    let mut all_timestamps = HashSet::new();
    let first_seg_step_ms = 100; // 首段双向密采步长
    let regular_step_ms = 500; // 其余段抽帧步长

    for (i, seg) in asr_segs.iter().enumerate() {
        if i == 0 {
            let mut fwd = (seg.start_ms as f64).round() as u64;
            let mut bwd = (seg.end_ms as f64).round() as u64;
            while fwd <= bwd {
                all_timestamps.insert(fwd);
                if fwd != bwd {
                    all_timestamps.insert(bwd);
                }
                fwd = fwd.saturating_add(first_seg_step_ms);
                bwd = bwd.saturating_sub(first_seg_step_ms);
            }
        } else {
            let mut t = (seg.end_ms as f64).round() as u64;
            let start = seg.start_ms;
            while t >= start as u64 {
                all_timestamps.insert(t);
                if t < regular_step_ms {
                    break;
                }
                t = t.saturating_sub(regular_step_ms);
            }
        }
    }
    let mut sorted_ts: Vec<u64> = all_timestamps.into_iter().collect();
    sorted_ts.sort_unstable();

    info!(target: "asr_ocr", "{} split segs → {} frame positions", asr_segs.len(), sorted_ts.len());

    // Step 3: Extract frames via ffmpeg
    let frame_dir = pre_dir.join("frames");
    fs::create_dir_all(&frame_dir)
        .map_err(|e| anyhow::anyhow!("创建 {} 失败: {}", frame_dir.display(), e))?;

    let mut extract_count = 0;
    for (idx, ms) in sorted_ts.iter().enumerate() {
        let frame_path = frame_dir.join(format!("{:07}.jpg", ms));
        let args = vec![
            "-ss".into(),
            format!("{:.3}", *ms as f64 / 1000.0),
            "-i".into(),
            video_path.clone(),
            "-frames:v".into(),
            "1".into(),
            "-qscale:v".into(),
            "2".into(),
            frame_path.to_string_lossy().into_owned(),
        ];
        if ffmpeg(&args).is_ok() {
            extract_count += 1;
        }
        if (idx + 1) % 50 == 0 || idx == sorted_ts.len() - 1 {
            info!(target: "asr_ocr", "Extracted {}/{} frames", idx + 1, sorted_ts.len());
        }
    }

    if extract_count == 0 {
        return Err(anyhow::anyhow!("No frames extracted"));
    }

    info!(target: "asr_ocr", "{} frames extracted to {}", extract_count, frame_dir.display());

    set_step_anyhow(
        &workflow_dir,
        "asr_ocr_pre",
        StepPatch {
            status: Some(StepStatus::Success),
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
    use crate::steps::asr::out::AsrWord;

    fn make_asr_seg(text: &str, start: u32, end: u32, words: Option<Vec<AsrWord>>) -> AsrSegment {
        AsrSegment {
            text: text.into(),
            start_ms: start,
            end_ms: end,
            words,
            confidence: None,
        }
    }

    #[test]
    fn split_simple_no_words() {
        let segs = vec![make_asr_seg("hello world", 0, 1000, None)];
        let out = split_asr_by_words(&segs);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "hello world");
    }

    #[test]
    fn split_by_punctuation() {
        // Space + punctuation triggers multiple splits
        let words = vec![
            AsrWord {
                word: "你好".into(),
                start: 0,
                end: 300,
                probability: 0.9,
            },
            AsrWord {
                word: "，".into(),
                start: 300,
                end: 400,
                probability: 0.9,
            },
            AsrWord {
                word: "世界".into(),
                start: 400,
                end: 1200,
                probability: 0.9,
            },
        ];
        let segs = vec![make_asr_seg("你好 ， 世界", 0, 1200, Some(words))];
        let out = split_asr_by_words(&segs);
        // With spaces around punctuation, both space splits and punctuation splits fire
        assert!(out.len() >= 2);
        // First segment contains "你好"
        assert!(out[0].text.contains("你好"));
        // Last segment contains "世界"
        assert!(out.last().unwrap().text.contains("世界"));
    }
}
