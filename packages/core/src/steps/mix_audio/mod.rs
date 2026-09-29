//! mix_audio: 把 TTS 各段音频合并成完整配音轨, 并做 timing 微调对齐原视频时间线。
//!
//! 镜像 TS `packages/core/steps/mix_audio/mix_audio.ts`。
//! 核心: drift 传播 + advance/delay 借间隙时间, 去尾静音 + rubberband 拉伸,
//! 段间静音填充, ffmpeg concat 输出 `mix_audio/audio_dubbing.wav`, 写 `mix_audio/timings.json`。

pub mod args;
pub mod out;

use std::path::Path;

use crate::state::State;
use crate::steps::mix_audio::out::{MixAudioOutputTimings, Timing};
use crate::steps::split_audio::out::SplitAudioTimingResult;
use crate::steps::utils::paths::mix_audio::TimingsPath;
use crate::steps::utils::{
    ensure_dir, ffmpeg, now_iso, probe_duration_ms, probe_sample_rate, set_step_anyhow, StepPatch,
    StepStatus,
};

use args::MixAudioInput;

pub use args::MixAudioArgs;

/// 入口 (镜像 TS `stepMixAudio`)。
pub fn step_mix_audio(state: &State) -> anyhow::Result<()> {
    let video_dir = state.init.workflow.video_dir.clone();
    tracing::info!(target: "mix_audio", "start");

    let args = MixAudioArgs::from_input(&state.init.input);

    let input = MixAudioInput::from_state(state)?;
    let segments = SplitAudioTimingResult::read(&input.split_audio_timings_path)?.segments;

    let mix_audio_dir = Path::new(&video_dir).join("mix_audio");
    let timings_path = TimingsPath::new(&video_dir);
    let stretched_dir = mix_audio_dir.join("stretched");
    let silence_dir = mix_audio_dir.join("silences");
    ensure_dir(&stretched_dir)?;
    ensure_dir(&silence_dir)?;
    ensure_dir(&mix_audio_dir)?;

    let dubbing_file = mix_audio_dir.join("audio_dubbing.wav");

    let tts_files: Vec<String> = segments
        .iter()
        .enumerate()
        .map(|(i, _)| {
            input
                .tts_wavs_dir
                .segment_path(i)
                .to_string_lossy()
                .into_owned()
        })
        .collect();

    let sample_rate = probe_sample_rate(&tts_files[0]);

    let mut segment_inputs: Vec<String> = Vec::new();
    let mut last_end_ms: i64 = 0;
    let mut drift_ms: i64 = 0;

    let max_speed = args.max_speed;
    let max_advance_ms = args.max_advance_ms as i64;
    let max_delay_ms = args.max_delay_ms as i64;

    let mut new_translation: Vec<Timing> = Vec::with_capacity(segments.len());

    for (i, seg) in segments.iter().enumerate() {
        let tts_file = &tts_files[i];
        let idx = format!("{:04}", i + 1);
        let stretched_file = stretched_dir.join(format!("{idx}.wav"));
        let stretched_file = stretched_file.to_string_lossy().into_owned();

        let start_ms = seg.start_ms as u64;
        let end_ms = seg.end_ms as u64;

        let tts_ms = probe_duration_ms(tts_file);

        // 去尾静音 (areverse + silenceremove, 不伤内部停顿)
        let trimmed_file = stretched_dir.join(format!("{idx}_trimmed.wav"));
        let trimmed_file = trimmed_file.to_string_lossy().into_owned();
        ffmpeg(&[
            "-i".to_string(),
            tts_file.to_string(),
            "-af".to_string(),
            "areverse,silenceremove=start_periods=1:start_threshold=-50dB:start_duration=0.05,areverse"
                .to_string(),
            trimmed_file.clone(),
        ])?;
        let trimmed_ms = probe_duration_ms(&trimmed_file);

        // 零时长段跳过 (TTS 无效输出/静音占位): 不进 concat、不写 timings,
        // last_end/drift 保持不动, 该槽位自然留白。替代原先的硬失败 ——
        // 单个坏段不应让整条 pipeline 失败 (空译文/无参考音的占位 wav 同样零时长)。
        if trimmed_ms <= 0 {
            tracing::warn!(
                target: "mix_audio",
                "#{} 零时长段, 跳过 (tts={}ms): {}",
                i + 1,
                tts_ms,
                tts_file
            );
            continue;
        }

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
            segments[i + 1].start_ms as u64
        } else {
            end_ms
        };
        let gap_ms = (next_start_ms as i64 - end_ms as i64).max(0);
        let delay_ms = gap_ms.min(max_delay_ms);

        let pushed_silence = real_start_ms > last_end_ms;
        if pushed_silence {
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
            // 上方 trimmed<=0 早退后理论上不可达 (slot>=50ms 保证拉伸结果 >=1ms),
            // 这里仅作极端兜底: 回滚本段 concat 条目并跳过, 不再硬失败整条 pipeline。
            segment_inputs.pop(); // stretched_file
            if pushed_silence {
                segment_inputs.pop(); // silence_file
            }
            tracing::warn!(
                target: "mix_audio",
                "#{} 生成了零时长段, 跳过: tts_file={}, start={}ms end={}ms, tts={}ms trimmed={}ms",
                i + 1,
                tts_file,
                start_ms,
                end_ms,
                tts_ms,
                trimmed_ms
            );
            continue;
        }

        last_end_ms = real_end_ms;

        let stretch_ratio = if trimmed_ms as i64 <= slot_ms {
            1.0
        } else {
            speed
        };
        new_translation.push(Timing {
            timing: seg.clone(),
            original_duration_ms: original_slot_base_ms as u32,
            tts_duration_ms: tts_ms as u32,
            stretched_duration_ms: stretched_ms as u32,
            stretch_ratio,
            drift_ms: drift_ms as i32,
            advance_ms: advance_ms as u32,
            delay_ms: delay_ms as u32,
            actual_start: real_start_ms as u32,
            actual_end: real_end_ms as u32,
        });
    }

    if segment_inputs.is_empty() {
        return Err(anyhow::anyhow!("没有可合并的音频段"));
    }

    let concat_file = mix_audio_dir.join("concat_list.txt");
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

    let out = MixAudioOutputTimings {
        segments: new_translation,
    };
    out.write(&timings_path)?;

    set_step_anyhow(
        &video_dir,
        "mix_audio",
        StepPatch {
            status: Some(StepStatus::Success),
            completed_at: Some(now_iso()),
            progress: Some(100.0),
            last_message: Some("Merged".to_string()),
            ..Default::default()
        },
    )?;
    tracing::info!(target: "mix_audio", "done");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::read_ctx_from_value;
    use serde_json::json;

    fn ctx_at(dir: &str, input: serde_json::Value) -> State {
        let mut ctx = read_ctx_from_value(input).unwrap();
        ctx.init.workflow.video_dir = dir.to_string();
        ctx.init.output_mode = "dub".to_string();
        ctx
    }

    #[test]
    fn args_defaults() {
        let ctx = ctx_at(
            "/x",
            json!({
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}
            }),
        );
        let cfg = MixAudioArgs::from_input(&ctx.init.input);
        assert_eq!(cfg.max_speed, 1.35);
        assert_eq!(cfg.max_advance_ms, 500.0);
        assert_eq!(cfg.max_delay_ms, 500.0);
    }
}
