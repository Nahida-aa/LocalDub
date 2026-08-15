//! asr 阶段 (镜像 TS `packages/core/stages/asr/asr.ts`)。
//!
//! 本端口仅实现 ggml 运行时 (whisper.cpp / `whisper-vulkan`), 其余运行时
//! (pytorch / faster-whisper) 按需求暂不移植。

pub mod args;
pub mod fix;
pub mod fix_args;
pub mod out;

use std::process::Command;

use anyhow::{Context, anyhow};
use config_rs::path::models::{whisper_model_path, whisper_vulkan_path};

use crate::context::{TaskCtx, write_ctx};
use crate::stages::asr::args::{AsrArgs, VadModel};
use crate::stages::asr::out::*;
use crate::stages::utils::{
    StagePatch, StageStatus, asr_dir, emit_log, ensure_dir, ffmpeg, gated_vocals_path,
    mixed_vocals_path, now_iso, set_stage_anyhow, video_source_path, vocals_path,
};

/// 读取 asr 配置 (缺省用 AsrArgs::default)。
fn read_args(ctx: &TaskCtx) -> AsrArgs {
    ctx.input
        .get("stages")
        .and_then(|v| v.get("asr"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// 入口 (镜像 TS `stageAsr`)。
pub fn stage_asr(ctx: &TaskCtx) -> anyhow::Result<()> {
    let task_dir = ctx.task.task_dir.clone();
    emit_log("asr: start");

    set_stage_anyhow(
        &task_dir,
        "asr",
        StagePatch {
            last_message: Some("Transcribing...".into()),
            progress: Some(0.0),
            ..Default::default()
        },
    )?;

    let cfg = read_args(ctx);

    if !cfg.enabled {
        emit_log("[ASR] disabled (asr.enabled=false), skipping");
        return Ok(());
    }

    // —— 解析输入音频 (镜像 TS stageAsr 的 useSeparated / mixed / gated 逻辑) ——
    let audio_vocal = vocals_path(&task_dir);
    let video_source = video_source_path(ctx)?;

    let mut audio_path: String = if cfg.use_separated {
        audio_vocal.to_string_lossy().to_string()
    } else {
        video_source.clone()
    };
    if !std::path::Path::new(&audio_path).exists() {
        return Err(anyhow!(
            "ASR input not found: {audio_path}; 如果 asr.useSeparated=true, 请确保 target_3_vocals.wav 存在；如果 asr.useSeparated=false, 请确保 video_source 存在"
        ));
    }

    if cfg.use_separated {
        let mixed = mixed_vocals_path(&task_dir);
        let gated = gated_vocals_path(&task_dir);
        let mixed_or_gated = if gated.exists() {
            Some(gated)
        } else if mixed.exists() {
            Some(mixed)
        } else {
            None
        };
        if let Some(p) = mixed_or_gated {
            audio_path = p.to_string_lossy().to_string();
            emit_log(&format!("[ASR] Using pre-mixed audio: {audio_path}"));
        } else {
            emit_log("[ASR] No mixed audio found, using vocals-only");
        }
    }

    let runtime = "ggml";
    emit_log(&format!("[ASR] runtime={runtime} device=vulkan"));

    // —— 准备 whisper 输入 WAV (已是 .wav 则直接复用, 否则 ffmpeg 转单声道) ——
    let audio_dir = asr_dir(&task_dir);
    ensure_dir(&audio_dir)?;
    let tmp_audio: String = if audio_path.to_lowercase().ends_with(".wav") {
        emit_log(&format!("[ASR] Using existing WAV input: {audio_path}"));
        audio_path.clone()
    } else {
        let wav = audio_dir.join("whisper-input.wav");
        ffmpeg(&[
            "-i".into(),
            audio_path.clone(),
            "-ac".into(),
            "1".into(),
            wav.to_string_lossy().to_string(),
        ])
        .context("ffmpeg 转换输入音频为 WAV 失败")?;
        wav.to_string_lossy().to_string()
    };

    let whisper_cli = whisper_vulkan_path();
    if !whisper_cli.exists() {
        return Err(anyhow!(
            "whisper-vulkan 未构建: {}; 请先在 submodule/whisper.cpp 执行 `cmake -B build -DGGML_VULKAN=ON && cmake --build build --config Release -j4`",
            whisper_cli.display()
        ));
    }
    let model = whisper_model_path();
    if !model.exists() {
        return Err(anyhow!(
            "whisper 模型缺失: {}; 请从 huggingface.co/ggerganov/whisper.cpp 下载 ggml-large-v3-turbo.bin 放入 {}",
            model.display(),
            whisper_model_path().parent().unwrap().display()
        ));
    }

    // —— 组装 whisper-cli 参数 (镜像 TS asrWhisperCpp) ——
    let language = ctx
        .input
        .get("task")
        .and_then(|v| v.get("sourceLang"))
        .and_then(|v| v.as_str())
        .unwrap_or("auto")
        .to_string();

    let mut whisper_args: Vec<String> = vec![
        "-m".into(),
        model.to_string_lossy().to_string(),
        tmp_audio.clone(),
        "-l".into(),
        language.clone(),
        "-t".into(),
        "4".into(),
        "-ojf".into(),
    ];
    if cfg.vad {
        whisper_args.push("--vad".into());
        if let Some(vm) = cfg.vad_model {
            whisper_args.push("-vm".into());
            whisper_args.push(resolve_vad_model(vm));
        }
    }
    push_opt_f64(&mut whisper_args, "vad-threshold", cfg.vad_threshold);
    push_opt_f64(&mut whisper_args, "no-speech-thold", cfg.no_speech_thold);
    push_opt_f64(&mut whisper_args, "temperature", cfg.temperature);
    if cfg.max_len > 0 {
        whisper_args.push("--max-len".into());
        whisper_args.push(cfg.max_len.to_string());
    }
    if cfg.split_on_word {
        whisper_args.push("--split-on-word".into());
    }

    emit_log(&format!(
        "whisper-vulkan -m {} {} -l {} ...",
        model.display(),
        tmp_audio,
        language
    ));

    let t0 = std::time::Instant::now();
    let status = Command::new(&whisper_cli)
        .args(&whisper_args)
        .status()
        .map_err(|e| anyhow!("spawn whisper-vulkan 失败: {e}"))?;
    let elapsed_sec = t0.elapsed().as_secs_f64();
    if !status.success() {
        return Err(anyhow!(
            "whisper-vulkan failed with exit code {:?}",
            status.code()
        ));
    }

    // —— 读取 whisper 生成的 JSON (<tmpAudio>.json) ——
    let whisper_json = format!("{tmp_audio}.json");
    if !std::path::Path::new(&whisper_json).exists() {
        return Err(anyhow!("whisper-cli did not produce {whisper_json}"));
    }
    let raw_text = std::fs::read_to_string(&whisper_json)
        .with_context(|| format!("读取 {whisper_json} 失败"))?;
    let raw: WhisperJson = serde_json::from_str(&raw_text)
        .with_context(|| format!("解析 whisper JSON 失败: {whisper_json}"))?;

    let detected_language = if raw.result.language.is_empty() {
        "auto".to_string()
    } else {
        raw.result.language.clone()
    };

    // whisper.cpp `-ojf` 的 transcription 为段数组
    let transcription = raw.transcription.clone();

    let emit_words = cfg.words_output;

    let segments: Vec<AsrSegment> = transcription
        .iter()
        .map(|s| {
            let start_ms = s.offsets.from;
            let end_ms = s.offsets.to;
            let mut seg = AsrSegment {
                text: s.text.trim().to_string(),
                start_ms,
                end_ms,
                words: None,
                confidence: None,
            };
            if emit_words {
                let raw_words: Vec<AsrWord> = s
                    .tokens
                    .iter()
                    .filter(|t| {
                        let txt = t.text.trim();
                        !txt.is_empty() && !txt.starts_with('[') && t.offsets.from != 0
                    })
                    .map(|t| AsrWord {
                        word: t.text.trim().to_string(),
                        start: t.offsets.from,
                        end: t.offsets.to,
                        probability: t.p,
                    })
                    .collect();
                if !raw_words.is_empty() {
                    let offset = start_ms as i64 - raw_words[0].start as i64;
                    if offset.abs() > 500 {
                        emit_log(&format!(
                            "[ASR] VAD word timestamp shift: {} words offset by {}ms",
                            raw_words.len(),
                            offset
                        ));
                    }
                    let mut shifted = raw_words;
                    for w in &mut shifted {
                        w.start = (w.start as i64 + offset).max(0) as u64;
                        w.end = (w.end as i64 + offset).max(0) as u64;
                    }
                    let probs: Vec<f64> = shifted
                        .iter()
                        .map(|w| w.probability)
                        .filter(|p| *p >= 0.0)
                        .collect();
                    if !probs.is_empty() {
                        let sum: f64 = probs.iter().sum();
                        let min = probs.iter().cloned().fold(f64::INFINITY, f64::min);
                        seg.confidence = Some(AsrConfidence {
                            avg: (sum / probs.len() as f64),
                            min,
                        });
                    }
                    seg.words = Some(shifted);
                }
            }
            seg
        })
        .collect();

    let text = segments
        .iter()
        .map(|s| s.text.clone())
        .collect::<Vec<_>>()
        .join(" ");

    let last_end_ms = segments.last().map(|s| s.end_ms).unwrap_or(0);
    let rtf = if elapsed_sec > 0.0 && last_end_ms > 0 {
        elapsed_sec / (last_end_ms as f64 / 1000.0)
    } else {
        0.0
    };

    let asr_output = AsrResult {
        result: AsrResultBody { text, segments },
        meta: AsrResultMeta {
            audio_duration: last_end_ms,
            device: "vulkan".into(),
            detected_language: Some(detected_language.clone()),
            engine: "whisper.cpp".into(),
            model: model.to_string_lossy().to_string(),
            args: serde_json::to_value(&cfg).ok(),
            input_audio: audio_path.clone(),
            rtf,
        },
    };

    // 写回 asr/asr.json
    let asr_file = audio_dir.join("asr.json");
    let json = serde_json::to_string_pretty(&asr_output)
        .map_err(|e| anyhow!("序列化 asr 结果失败: {e}"))?;
    std::fs::write(&asr_file, json).with_context(|| format!("写入 {} 失败", asr_file.display()))?;

    // 持久化检测到的语言到 ctx (镜像 TS setCtx asr_language)
    set_asr_language(&task_dir, &detected_language)?;

    // —— 幻觉段后处理 (所有路径 shared) ——
    postprocess_hallucination(&asr_file, &audio_path)?;

    emit_log(&format!(
        "Transcribed in {:.1}s, RTF {:.3}, language {}",
        elapsed_sec, rtf, detected_language
    ));

    set_stage_anyhow(
        &task_dir,
        "asr",
        StagePatch {
            status: Some(StageStatus::Success),
            completed_at: Some(now_iso()),
            progress: Some(100.0),
            last_message: Some("Transcribed".into()),
            ..Default::default()
        },
    )?;

    Ok(())
}

/// 把检测语言写回 ctx.json (镜像 TS `setCtx(taskDir, { asr_language })`)。
fn set_asr_language(task_dir: &str, lang: &str) -> anyhow::Result<()> {
    let mut ctx = crate::context::read_ctx(task_dir).map_err(anyhow::Error::msg)?;
    ctx.asr_language = Some(lang.to_string());
    write_ctx(task_dir, &ctx).map_err(anyhow::Error::msg)
}

/// 把可选 f64 参数以 `--kebab` 形式追加 (数值等于默认值时也追加, 与 TS 行为一致)。
fn push_opt_f64(args: &mut Vec<String>, flag: &str, v: f64) {
    args.push(format!("--{flag}"));
    args.push(v.to_string());
}

/// 解析 VAD 模型实际文件路径 (镜像 TS `resolveVadModel` 的候选逻辑, 简化版)。
fn resolve_vad_model(vm: VadModel) -> String {
    let candidates: &[&str] = match vm {
        VadModel::SileroV5 => &["silero-v5.1.2", "silero-vad-v5"],
        VadModel::SileroV6 => &["silero-v6.2.0", "silero-vad-v6"],
    };
    let search_dirs = [
        dirs_home().join(".cache").join("pywhispercpp"),
        config_rs::root::repo_root()
            .join("submodule")
            .join("whisper.cpp")
            .join("models"),
    ];
    for dir in &search_dirs {
        for c in candidates {
            let p = dir.join(format!("ggml-{c}.bin"));
            if p.exists() {
                return p.to_string_lossy().to_string();
            }
            // fallback: 允许版本化文件名
            if let Ok(entries) = std::fs::read_dir(dir) {
                for e in entries.flatten() {
                    let fname = e.file_name().to_string_lossy().to_string();
                    if fname.starts_with("ggml-")
                        && fname.ends_with(".bin")
                        && (fname.contains(c) || fname.contains(&c.split('.').next().unwrap_or("")))
                    {
                        return e.path().to_string_lossy().to_string();
                    }
                }
            }
        }
    }
    // 回退: 直接给出首选候选路径 (运行时 whisper 会报缺失)
    search_dirs[1]
        .join(format!("ggml-{}.bin", candidates[0]))
        .to_string_lossy()
        .to_string()
}

fn dirs_home() -> std::path::PathBuf {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}

/// 幻觉段后处理:
/// 1. 过滤 start_ms >= 音频时长 或 end_ms <= 0 的段
/// 2. 末尾段 RMS 过低则判为幻觉剔除
fn postprocess_hallucination(asr_file: &std::path::Path, audio_path: &str) -> anyhow::Result<()> {
    let raw = std::fs::read_to_string(asr_file)
        .with_context(|| format!("读取 {} 失败", asr_file.display()))?;
    let mut data: AsrResult =
        serde_json::from_str(&raw).with_context(|| format!("解析 {} 失败", asr_file.display()))?;

    let duration_ms = data.meta.audio_duration;
    if duration_ms > 0 && !data.result.segments.is_empty() {
        let before = data.result.segments.len();
        data.result
            .segments
            .retain(|u| u.start_ms < duration_ms && u.end_ms > 0);
        if data.result.segments.len() < before {
            let removed = before - data.result.segments.len();
            emit_log(&format!(
                "Removed {removed} hallucinated segment(s) (start >= {duration_ms}ms or end <= 0ms)"
            ));
        }
    }

    // 末尾段 RMS 能量检测
    if let Some(last) = data.result.segments.last().cloned() {
        if std::path::Path::new(audio_path).exists() {
            let rms = segment_rms(audio_path, last.start_ms, last.end_ms);
            emit_log(&format!("[ASR] Last segment RMS: {rms:.5}"));
            if rms > 0.0 && rms < 0.005 {
                if let Some(removed) = data.result.segments.pop() {
                    emit_log(&format!(
                        "Removed low-energy hallucinated segment \"{}\" (RMS={:.5})",
                        &removed.text.chars().take(30).collect::<String>(),
                        rms
                    ));
                }
            }
        }
    }

    let json =
        serde_json::to_string_pretty(&data).map_err(|e| anyhow!("序列化 asr 结果失败: {e}"))?;
    std::fs::write(asr_file, json).with_context(|| format!("写回 {} 失败", asr_file.display()))?;
    Ok(())
}

/// 取某时间区间的 RMS 能量 (linear), 通过 ffmpeg astats 解析 (镜像 TS `segmentRms`)。
///
/// 返回 linear RMS; 解析失败返回 0。
fn segment_rms(audio_path: &str, start_ms: u64, end_ms: u64) -> f64 {
    let args = [
        "-y".to_string(),
        "-i".to_string(),
        audio_path.to_string(),
        "-ss".to_string(),
        (start_ms as f64 / 1000.0).to_string(),
        "-to".to_string(),
        (end_ms as f64 / 1000.0).to_string(),
        "-af".to_string(),
        "astats=metadata=1:reset=1,ametadata=print:key=lavfi.astats.Overall.RMS_level:file=-"
            .to_string(),
        "-f".to_string(),
        "null".to_string(),
        "-".to_string(),
    ];
    let bin = ffmpeg_bin();
    let out = Command::new(&bin).args(&args).output();
    let Ok(out) = out else {
        return 0.0;
    };
    if !out.status.success() {
        return 0.0;
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    // 解析 `lavfi.astats.Overall.RMS_level=<dB>` (无 regex 依赖)
    if let Some(pos) = stderr.find("lavfi.astats.Overall.RMS_level=") {
        let rest = &stderr[pos + "lavfi.astats.Overall.RMS_level=".len()..];
        let token = rest
            .split(|c: char| !(c.is_ascii_digit() || c == '-' || c == '.'))
            .next()
            .unwrap_or("");
        if let Ok(db) = token.parse::<f64>() {
            // dB -> linear: 10^(dB/20)
            return 10f64.powf(db / 20.0);
        }
    }
    0.0
}

fn ffmpeg_bin() -> String {
    // 复用 utils::ffmpeg 的查找逻辑: 优先 PATH 中的 ffmpeg
    std::env::var("FFMPEG_BIN")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "ffmpeg".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{read_ctx_from_value, write_ctx};
    use crate::stages::asr::args::MixMode;
    use crate::stages::asr::fix_args::AsrFixArgs;
    use serde_json::json;

    fn test_ctx(input: serde_json::Value) -> TaskCtx {
        let dir = std::env::temp_dir()
            .join(format!("ld_asr_test_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        std::fs::create_dir_all(&dir).unwrap();
        let mut ctx = read_ctx_from_value(input).unwrap();
        ctx.task.task_dir = dir.clone();
        ctx.pipeline = "dub".into();
        write_ctx(&dir, &ctx).unwrap();
        ctx
    }

    #[test]
    fn asr_args_defaults_and_enabled() {
        let a = AsrArgs::default();
        assert!(a.enabled, "asr 默认启用");
        assert!(!a.use_separated);
        assert_eq!(a.mix_mode, MixMode::Sidechain);
        assert_eq!(a.vad_threshold, 0.5);
        assert_eq!(a.no_speech_thold, 0.6);
        assert_eq!(a.temperature, 0.0);
        assert!(!a.vad);
    }

    #[test]
    fn asr_fix_args_defaults_and_enabled() {
        let a = AsrFixArgs::default();
        assert!(a.enabled, "asr_fix 默认启用");
        assert!(!a.llm_fix.llm_fix);
    }

    #[test]
    fn stage_asr_skips_when_disabled() {
        // enabled=false 时不触达 whisper 二进制, 直接 Ok
        let ctx = test_ctx(json!({
            "task": {"id":"t","task_dir":"/nonexistent_task_dir","url":"http://e","source":"remote",
                     "status":"running","created_at":"2024-01-01T00:00:00Z",
                     "videoSourcePath":"/nonexistent.mp4"},
            "input": {"stages": {"asr": {"enabled": false}}}
        }));
        let r = stage_asr(&ctx);
        assert!(r.is_ok(), "asr disabled 应直接跳过: {:?}", r.err());
    }

    #[test]
    fn stage_asr_missing_input_errors() {
        // useSeparated=false 且 video_source 不存在 → 报错 (不静默跳过)
        let ctx = test_ctx(json!({
            "task": {"id":"t","task_dir":"/nonexistent_task_dir","url":"http://e","source":"remote",
                     "status":"running","created_at":"2024-01-01T00:00:00Z"},
            "video_source_path": "/nonexistent_video.mp4",
            "input": {"stages": {"asr": {"enabled": true, "useSeparated": false}}}
        }));
        let r = stage_asr(&ctx);
        assert!(r.is_err(), "video source 缺失应报错");
        assert!(r.unwrap_err().to_string().contains("ASR input not found"));
    }
}
