//! mix_audio: 把 TTS 各段音频合并成完整配音轨, 并做 timing 微调对齐原视频时间线。
//!
//! 镜像 TS `packages/core/stages/mix_audio/mix_audio.ts`。
//! 核心: drift 传播 + advance/delay 借间隙时间, 去尾静音 + rubberband 拉伸,
//! 段间静音填充, ffmpeg concat 输出 `mix_audio/audio_dubbing.wav`, 写 `mix_audio/timings.json`。

pub mod args;
pub mod out;

use std::path::Path;

use crate::context::TaskCtx;
use crate::stages::mix_audio::out::{Timing, TimingsFile};
use crate::stages::utils::{
    StagePatch, StageStatus, ensure_dir, ffmpeg, now_iso, probe_duration_ms,
    probe_sample_rate, read_split_audio_timings, set_stage_anyhow, split_audio_timings_path,
};

pub use args::MixAudioArgs;

/// 从 `ctx.input.stages.mix_audio` 解析配置 (镜像 TS `readInputArgs().stages.mix_audio`)。
fn read_args(ctx: &TaskCtx) -> MixAudioArgs {
    ctx.input
        .get("stages")
        .and_then(|v| v.get("mix_audio"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// 入口 (镜像 TS `stageMixAudio`)。
pub fn stage_mix_audio(ctx: &TaskCtx) -> anyhow::Result<()> {
    let task_dir = ctx.task.task_dir.clone();
    tracing::info!(target: "mix_audio", "mix_audio: start");

    let args = read_args(ctx);

    let merge_audio_dir = Path::new(&task_dir).join("mix_audio");
    let tts_dir = Path::new(&task_dir).join("tts").join("wavs");
    let stretched_dir = merge_audio_dir.join("stretched");
    let silence_dir = merge_audio_dir.join("silences");
    ensure_dir(&stretched_dir)?;
    ensure_dir(&silence_dir)?;
    ensure_dir(&merge_audio_dir)?;

    let dubbing_file = merge_audio_dir.join("audio_dubbing.wav");

    let data = read_split_audio_timings(ctx)?;
    let segments = data
        .get("segments")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    if segments.is_empty() {
        return Err(anyhow::anyhow!(
            "{} 无 segments",
            split_audio_timings_path(&task_dir).display()
        ));
    }

    let tts_files: Vec<String> = segments
        .iter()
        .enumerate()
        .map(|(i, _)| {
            tts_dir
                .join(format!("{:04}.wav", i + 1))
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    for f in &tts_files {
        if !Path::new(f).exists() {
            return Err(anyhow::anyhow!("缺少 TTS 段: {f}"));
        }
    }

    let sample_rate = probe_sample_rate(&tts_files[0]);

    let mut segment_inputs: Vec<String> = Vec::new();
    let mut last_end_ms: i64 = 0;
    let mut drift_ms: i64 = 0;

    let max_speed = args.max_speed;
    let max_advance_ms = args.max_advance_ms as i64;
    let max_delay_ms = args.max_delay_ms as i64;

    let mut new_translation: Vec<Timing> = Vec::with_capacity(segments.len());

    for (i, item) in segments.iter().enumerate() {
        let tts_file = &tts_files[i];
        let idx = format!("{:04}", i + 1);
        let stretched_file = stretched_dir.join(format!("{idx}.wav"));
        let stretched_file = stretched_file.to_string_lossy().into_owned();

        let start_ms = item.get("start_ms").and_then(|v| v.as_u64()).unwrap_or(0);
        let end_ms = item.get("end_ms").and_then(|v| v.as_u64()).unwrap_or(0);
        let text = item
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        let tts_ms = probe_duration_ms(tts_file);

        // 去尾静音 (areverse + silenceremove, 不伤内部停顿)
        let trimmed_file = stretched_dir.join(format!("{idx}_trimmed.wav"));
        let trimmed_file = trimmed_file.to_string_lossy().into_owned();
        ffmpeg(&[
            "-i".to_string(),
            tts_file.clone(),
            "-af".to_string(),
            "areverse,silenceremove=start_periods=1:start_threshold=-50dB:start_duration=0.05,areverse"
                .to_string(),
            trimmed_file.clone(),
        ])?;
        let trimmed_ms = probe_duration_ms(&trimmed_file);

        // advance: 从前间隙借时间
        let original_slot_base_ms = end_ms.saturating_sub(start_ms);
        let mut advance_ms: i64 = 0;
        if trimmed_ms <= original_slot_base_ms {
            let surplus_no_advance = drift_ms + (original_slot_base_ms - trimmed_ms) as i64;
            if surplus_no_advance < 500 {
                advance_ms = (500 - surplus_no_advance).min((max_advance_ms as f64 * 0.2) as i64);
            }
        } else {
            advance_ms = max_advance_ms.min(drift_ms.max(0));
        }

        let real_start_ms = (start_ms as i64 - advance_ms).max(last_end_ms).max(0);
        advance_ms = (start_ms as i64 - real_start_ms).max(0);
        let effective_drift_ms = drift_ms - advance_ms;

        // delay: 从后间隙借时间
        let next_start_ms = if i < segments.len() - 1 {
            segments[i + 1]
                .get("start_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(end_ms)
        } else {
            end_ms
        };
        let gap_ms = (next_start_ms as i64 - end_ms as i64).max(0);
        let delay_ms = gap_ms.min(max_delay_ms);

        if real_start_ms > last_end_ms {
            let gap_sec = (real_start_ms - last_end_ms) as f64 / 1000.0;
            let silence_file = silence_dir.join(format!("silence_{i}.wav"));
            let silence_file = silence_file.to_string_lossy().into_owned();
            ffmpeg(&[
                "-f".to_string(),
                "lavfi".to_string(),
                "-i".to_string(),
                format!("anullsrc=r={sample_rate}:cl=mono"),
                "-t".to_string(),
                format!("{gap_sec}"),
                silence_file.clone(),
            ])?;
            segment_inputs.push(silence_file);
        }

        let original_slot_ms = (end_ms + delay_ms as u64) as i64 - real_start_ms;
        let slot_ms = (50i64).max(original_slot_ms + effective_drift_ms);

        let (stretched_ms, speed): (f64, f64) = if trimmed_ms as i64 <= original_slot_ms {
            (trimmed_ms as f64, 1.0)
        } else if trimmed_ms as i64 <= slot_ms {
            (trimmed_ms as f64, 1.0)
        } else {
            let sp = (max_speed as f64).min(trimmed_ms as f64 / slot_ms as f64);
            (trimmed_ms as f64 / sp, sp)
        };

        if speed > 1.0 {
            ffmpeg(&[
                "-i".to_string(),
                trimmed_file.clone(),
                "-filter:a".to_string(),
                format!("rubberband=tempo={:.4}", speed),
                stretched_file.clone(),
            ])?;
        } else {
            ffmpeg(&[
                "-i".to_string(),
                trimmed_file.clone(),
                "-c".to_string(),
                "copy".to_string(),
                stretched_file.clone(),
            ])?;
        }

        let mut new_drift_ms = original_slot_ms - stretched_ms as i64;
        if new_drift_ms > max_advance_ms {
            new_drift_ms = max_advance_ms;
        }
        drift_ms = new_drift_ms;

        segment_inputs.push(stretched_file.clone());

        let real_end_ms = (real_start_ms as f64 + stretched_ms).floor() as i64;
        if real_end_ms <= real_start_ms {
            return Err(anyhow::anyhow!(
                "mix_audio #{} 生成了零时长段: tts_file={}, start={}ms end={}ms, tts={}ms trimmed={}ms",
                i + 1,
                tts_file,
                start_ms,
                end_ms,
                tts_ms,
                trimmed_ms
            ));
        }

        last_end_ms = real_end_ms;

        let stretch_ratio = if trimmed_ms as i64 <= slot_ms {
            1.0
        } else {
            speed
        };
        new_translation.push(Timing {
            timing: crate::stages::split_audio::out::SplitAudioTiming {
                seg_idx: (i + 1) as u32,
                text,
                start_ms,
                end_ms,
                dst: item
                    .get("dst")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string(),
                src_lang: item
                    .get("src_lang")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                dst_lang: item
                    .get("dst_lang")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                speaker: item
                    .get("speaker")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                text_confidence: None,
            },
            original_duration_ms: original_slot_base_ms,
            tts_duration_ms: tts_ms,
            stretched_duration_ms: stretched_ms as u64,
            stretch_ratio,
            drift_ms: drift_ms,
            advance_ms: advance_ms as u64,
            delay_ms: delay_ms as u64,
            actual_start: real_start_ms as u64,
            actual_end: real_end_ms as u64,
        });
    }

    if segment_inputs.is_empty() {
        return Err(anyhow::anyhow!("没有可合并的音频段"));
    }

    let concat_file = merge_audio_dir.join("concat_list.txt");
    let concat_content = segment_inputs
        .iter()
        .map(|f| format!("file '{f}'"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&concat_file, concat_content)
        .map_err(|e| anyhow::anyhow!("写入 {} 失败: {}", concat_file.display(), e))?;

    ffmpeg(&[
        "-f".to_string(),
        "concat".to_string(),
        "-safe".to_string(),
        "0".to_string(),
        "-i".to_string(),
        concat_file.to_string_lossy().into_owned(),
        "-acodec".to_string(),
        "pcm_s16le".to_string(),
        "-ar".to_string(),
        sample_rate.to_string(),
        "-ac".to_string(),
        "1".to_string(),
        dubbing_file.to_string_lossy().into_owned(),
    ])?;

    let timings_file = merge_audio_dir.join("timings.json");
    let out = TimingsFile {
        segments: new_translation,
    };
    let json = serde_json::to_string_pretty(&out)
        .map_err(|e| anyhow::anyhow!("序列化 mix_audio timings 失败: {e}"))?;
    std::fs::write(&timings_file, json)
        .map_err(|e| anyhow::anyhow!("写入 {} 失败: {}", timings_file.display(), e))?;

    set_stage_anyhow(
        &task_dir,
        "mix_audio",
        StagePatch {
            status: Some(StageStatus::Success),
            completed_at: Some(now_iso()),
            progress: Some(100.0),
            last_message: Some("Merged".to_string()),
            ..Default::default()
        },
    )?;
    tracing::info!(target: "mix_audio", "mix_audio: done");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::read_ctx_from_value;
    use serde_json::json;

    fn ctx_at(dir: &str, input: serde_json::Value) -> TaskCtx {
        let mut ctx = read_ctx_from_value(input).unwrap();
        ctx.task.task_dir = dir.to_string();
        ctx.pipeline = "dub".to_string();
        ctx
    }

    #[test]
    fn args_defaults() {
        let ctx = ctx_at(
            "/x",
            json!({
                "task": {"id":"t","task_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}
            }),
        );
        let cfg = read_args(&ctx);
        assert_eq!(cfg.max_speed, 1.35);
        assert_eq!(cfg.max_advance_ms, 500.0);
        assert_eq!(cfg.max_delay_ms, 500.0);
    }
}
