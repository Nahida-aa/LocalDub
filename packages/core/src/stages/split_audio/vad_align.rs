//! 可选 vadAlign: 用静音检测把每段起点前移到真实语音处 (镜像 TS `vad_align.ts`)。

use std::path::Path;

use crate::stages::split_audio::out::{SplitAudioSegment, SplitAudioTiming};
use crate::stages::utils::{ffmpeg, probe_duration_ms};

/// 检测一段已切好 wav 开头被静音削掉的时长 (ms)。
/// 做法: 用 ffmpeg silenceremove 裁掉 >0.1s 且 < -30dB 的起始静音, 对比原时长减裁后时长。
fn detect_speech_start_ms(wav_path: &str) -> u64 {
    let orig = probe_duration_ms(wav_path);
    if orig == 0 {
        return 0;
    }
    let tmp = wav_path.replace(".wav", ".trim.wav");
    let _ = ffmpeg(&[
        "-i".to_string(),
        wav_path.to_string(),
        "-af".to_string(),
        "silenceremove=start_periods=1:start_threshold=-30dB:start_duration=0.1".to_string(),
        "-y".to_string(),
        tmp.clone(),
    ]);
    let removed = if Path::new(&tmp).exists() {
        orig.saturating_sub(probe_duration_ms(&tmp))
    } else {
        0
    };
    let _ = std::fs::remove_file(&tmp);
    removed
}

/// 同 detect_speech_start_ms, 但直接 seek 源音轨 [startMs, endMs) 裁临时 wav 再测。
fn detect_speech_start_ms_seek(source: &str, start_ms: u64, end_ms: u64, work_dir: &str) -> u64 {
    let dur = end_ms.saturating_sub(start_ms);
    if dur == 0 {
        return 0;
    }
    let tmp = Path::new(work_dir).join(".vad_trim.wav");
    let tmp = tmp.to_string_lossy().into_owned();
    let _ = ffmpeg(&[
        "-ss".to_string(),
        format!("{}", start_ms as f64 / 1000.0),
        "-to".to_string(),
        format!("{}", end_ms as f64 / 1000.0),
        "-i".to_string(),
        source.to_string(),
        "-vn".to_string(),
        "-af".to_string(),
        "silenceremove=start_periods=1:start_threshold=-30dB:start_duration=0.1".to_string(),
        "-y".to_string(),
        tmp.clone(),
    ]);
    let removed = if Path::new(&tmp).exists() {
        dur.saturating_sub(probe_duration_ms(&tmp))
    } else {
        0
    };
    let _ = std::fs::remove_file(&tmp);
    removed
}

/// 用静音检测把每段起点前移到真实语音处, 返回是否修正了任何段 (改写 segments/timings)。
pub fn apply_vad_align(
    segments: &mut [SplitAudioSegment],
    timings: &mut [SplitAudioTiming],
    source_audio: &str,
    total_ms: u64,
    vocals_segment_dir: &str,
    has_vocals: bool,
) -> bool {
    let mut corrected = false;
    for i in 0..segments.len() {
        let (start_ms, end_ms) = (segments[i].split_start_ms, segments[i].split_end_ms);
        if start_ms >= end_ms {
            continue;
        }
        let wav_path = Path::new(vocals_segment_dir)
            .join(format!("{:04}.wav", i + 1))
            .to_string_lossy()
            .into_owned();
        let removed = if Path::new(&wav_path).exists() {
            detect_speech_start_ms(&wav_path)
        } else {
            detect_speech_start_ms_seek(
                source_audio,
                start_ms,
                total_ms.min(end_ms),
                vocals_segment_dir,
            )
        };
        if removed <= 500 {
            continue; // 开头静音不足 500ms, 不值得修正
        }

        let cut_start = segments[i].split_start_ms;
        let new_cut_start = cut_start + removed - 80; // 保留 80ms 呼吸余量
        if new_cut_start >= end_ms {
            tracing::warn!(
                "vadAlign #{}: would exceed end ({} >= {}), skipping",
                i + 1,
                new_cut_start,
                end_ms
            );
            continue;
        }
        tracing::info!(
            "vadAlign #{}: start {} -> {} (removed {}ms)",
            i + 1,
            cut_start,
            new_cut_start,
            removed
        );

        if has_vocals {
            let new_end = (total_ms.min(end_ms + 160)).min(new_cut_start + 1);
            if new_end > new_cut_start {
                let _ = crate::stages::split_audio::util::cut_audio_range(
                    source_audio,
                    new_cut_start,
                    new_end,
                    &wav_path,
                );
            }
        }

        segments[i].split_start_ms = new_cut_start;
        if let Some(t) = timings.get_mut(i) {
            t.start_ms = t.start_ms.saturating_add(removed).saturating_sub(80);
        }
        corrected = true;
    }
    corrected
}
