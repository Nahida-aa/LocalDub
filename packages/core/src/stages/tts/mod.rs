//! tts 阶段 (镜像 TS `packages/core/stages/07_tts/tts.ts`)。
//!
//! 从 split_audio/timings.json 取每段时间轴 + 译文, 调 VoxCPM 引擎逐段合成配音。
//! Rust 侧复用预构建的 `voxcpm-burn` 二进制 (镜像 TS `newVoxCPMEngine` 的进程外等价):
//! 每段 spawn 一次二进制, 传入 text / output / --ref-audio (人声参考音)。
//!
//! 注: TS 引擎进程内 load 一次后逐段 synthesize; 进程外 spawn 每段会重复 load 模型,
//! 对长视频较慢。这是 Rust core 二进制编排模式下的已知取舍 (与其他 stage 一致),
//! 后续可改为批量模式二进制消除。

pub mod args;
pub mod out;

use std::fs;
use std::path::Path;

use crate::context::TaskCtx;
use crate::stages::tts::args::{TtsArgs, TtsDevice, TtsRuntime};
use crate::stages::tts::out::{TtsFile, TtsSegment};
use crate::stages::utils::{
    StagePatch, StageStatus, emit_log, ensure_dir, ffmpeg, find_release_bin, now_iso,
    probe_duration_ms, read_split_audio_timings, set_stage_anyhow, tts_filepath,
};

/// vocals 参考音"非静音"判定阈值: PCM 裸数据 > 该字节数才认为有实际声音。
/// 1200 采样帧 * 16bit * 2 声道 = 38400 bytes (~75ms @ 16kHz)。
const MIN_REF_BYTES: u64 = 1200 * 16 * 2;
/// refAudioX2 触发阈值: 参考音短于此则拼接自身翻倍。
const MIN_REF_DURATION_MS: u64 = 2500;

/// 从 `ctx.input.stages.tts` 解析配置 (镜像 TS `ctx.input.stages.tts`)。
fn read_args(ctx: &TaskCtx) -> TtsArgs {
    ctx.input
        .get("stages")
        .and_then(|v| v.get("tts"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// 选择 voxcpm-burn 后端二进制名 (镜像 TS runtime/device -> 二进制)。
/// 优先返回 workspace 中已构建的二进制。
fn pick_voxcpm_bin(device: TtsDevice, runtime: TtsRuntime) -> anyhow::Result<String> {
    // runtime=cloud 无独立后端, 退回按 device 选 GPU 后端
    let candidates: Vec<&str> = match (runtime, device) {
        (_, TtsDevice::Cpu) => vec!["voxcpm-burn-cpu"],
        (_, TtsDevice::Rocm) => vec!["voxcpm-burn-vulkan", "voxcpm-burn-wgpu"],
        (_, TtsDevice::Mps) => vec!["voxcpm-burn-wgpu", "voxcpm-burn-cpu"],
        (TtsRuntime::VoxcpmTorchGradio, _) => vec!["voxcpm-burn-tch", "voxcpm-burn-wgpu"],
        (_, TtsDevice::Webgpu) => vec!["voxcpm-burn-wgpu", "voxcpm-burn-cpu"],
        (_, TtsDevice::Cuda) => vec!["voxcpm-burn-vulkan", "voxcpm-burn-wgpu", "voxcpm-burn-cpu"],
        // 默认 cloud: 优先 GPU
        _ => vec!["voxcpm-burn-wgpu", "voxcpm-burn-vulkan", "voxcpm-burn-cpu"],
    };
    for c in candidates {
        if let Some(p) = find_release_bin(c) {
            return Ok(p.to_string_lossy().into_owned());
        }
    }
    // 回退: 期望默认 profile 二进制存在 (引导用户先 build)
    Err(anyhow::anyhow!(
        "未找到 voxcpm-burn 二进制 (profile: {:?}/{:?})。请先 cargo build --release -p voxcpm-burn",
        device,
        runtime
    ))
}

/// 入口 (镜像 TS `stageTts`)。
pub fn stage_tts(ctx: &TaskCtx) -> anyhow::Result<()> {
    let task_dir = ctx.task.task_dir.clone();
    emit_log(Some(&task_dir), "tts: start");

    let args = read_args(ctx);
    let vocals_dir = Path::new(&task_dir).join("split_audio").join("vocals");
    let tts_wav_dir = Path::new(&task_dir).join("tts").join("wavs");
    let doubled_dir = Path::new(&task_dir).join("tts").join("ref_doubled");
    ensure_dir(&tts_wav_dir)?;
    if args.ref_audio_x2 {
        ensure_dir(&doubled_dir)?;
    }

    let data = read_split_audio_timings(ctx)?;
    let segments = data
        .get("segments")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    if segments.is_empty() {
        return Err(anyhow::anyhow!("split_audio/timings.json 无 segments"));
    }

    let bin = pick_voxcpm_bin(args.device, args.runtime)?;
    emit_log(Some(&task_dir), &format!("Using voxcpm backend: {bin}"));

    // 找第一个非静音 vocals 作为 fallback 参考音
    let fallback_ref: Option<String> = (0..segments.len())
        .map(|i| vocals_dir.join(format!("{:04}.wav", i + 1)))
        .find(|p| p.exists() && fs::metadata(p).map(|m| m.len()).unwrap_or(0) > MIN_REF_BYTES)
        .map(|p| p.to_string_lossy().into_owned());

    let mut tts_segments: Vec<TtsSegment> = Vec::with_capacity(segments.len());

    for (i, item) in segments.iter().enumerate() {
        let idx = format!("{:04}", i + 1);
        let out_path = tts_wav_dir.join(format!("{idx}.wav"));
        let out_path = out_path.to_string_lossy().into_owned();

        let start_ms = item.get("start_ms").and_then(|v| v.as_u64()).unwrap_or(0);
        let end_ms = item.get("end_ms").and_then(|v| v.as_u64()).unwrap_or(0);
        let text = item
            .get("dst")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        let item_text = item
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        // 参考音: 优先本段 vocals, 否则 fallback
        let mut ref_wav = vocals_dir.join(format!("{idx}.wav"));
        if !ref_wav.exists() || fs::metadata(&ref_wav).map(|m| m.len()).unwrap_or(0) < MIN_REF_BYTES
        {
            if let Some(fb) = &fallback_ref {
                ref_wav = Path::new(fb).to_path_buf();
            }
        }
        let ref_wav = ref_wav.to_string_lossy().into_owned();
        let ref_mtime = if Path::new(&ref_wav).exists() {
            fs::metadata(&ref_wav)
                .and_then(|m| {
                    m.modified().map(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0)
                    })
                })
                .unwrap_or(0)
        } else {
            0
        };

        // skipExisting: 输出比参考新则跳过
        if args.skip_existing && Path::new(&out_path).exists() {
            let out_mtime = fs::metadata(&out_path)
                .and_then(|m| {
                    m.modified().map(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0)
                    })
                })
                .unwrap_or(0);
            if out_mtime > ref_mtime {
                let dur = probe_duration_ms(&out_path);
                tts_segments.push(TtsSegment {
                    timing: crate::stages::split_audio::out::SplitAudioTiming {
                        seg_idx: (i + 1) as u32,
                        text: item_text.clone(),
                        start_ms,
                        end_ms: start_ms + dur,
                        dst: text.clone(),
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
                    slot_end_ms: end_ms,
                    tts_duration_ms: dur,
                    status: "skipped".to_string(),
                });
                continue;
            }
        }

        // 空译文 -> 空 wav
        if text.trim().is_empty() {
            fs::write(&out_path, vec![0u8; 44]).ok();
            tts_segments.push(TtsSegment {
                timing: crate::stages::split_audio::out::SplitAudioTiming {
                    seg_idx: (i + 1) as u32,
                    text: String::new(),
                    start_ms,
                    end_ms: start_ms,
                    dst: String::new(),
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
                slot_end_ms: end_ms,
                tts_duration_ms: 0,
                status: "empty".to_string(),
            });
            continue;
        }

        // 无参考音 -> 跳过
        if !Path::new(&ref_wav).exists() {
            emit_log(
                Some(&task_dir),
                &format!("[WARN] [TTS] 段 {idx} 无参考音, 跳过"),
            );
            fs::write(&out_path, vec![0u8; 44]).ok();
            tts_segments.push(TtsSegment {
                timing: crate::stages::split_audio::out::SplitAudioTiming {
                    seg_idx: (i + 1) as u32,
                    text: item_text.clone(),
                    start_ms,
                    end_ms: start_ms,
                    dst: text.clone(),
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
                slot_end_ms: end_ms,
                tts_duration_ms: 0,
                status: "skipped".to_string(),
            });
            continue;
        }

        // refAudioX2: 短参考音翻倍
        let mut ref_for_tts = ref_wav.clone();
        if args.ref_audio_x2 {
            let ref_ms = probe_duration_ms(&ref_wav);
            if ref_ms > 0 && ref_ms < MIN_REF_DURATION_MS {
                let doubled = doubled_dir.join(format!("ref_{idx}_x2.wav"));
                let doubled = doubled.to_string_lossy().into_owned();
                if !Path::new(&doubled).exists() {
                    let list_path = doubled_dir.join(format!("ref_{idx}_list.txt"));
                    fs::write(&list_path, format!("file '{ref_wav}'\nfile '{ref_wav}'"))
                        .map_err(|e| anyhow::anyhow!("写 ref list 失败: {e}"))?;
                    ffmpeg(&[
                        "-f".to_string(),
                        "concat".to_string(),
                        "-safe".to_string(),
                        "0".to_string(),
                        "-i".to_string(),
                        list_path.to_string_lossy().into_owned(),
                        "-c".to_string(),
                        "copy".to_string(),
                        doubled.clone(),
                    ])?;
                }
                ref_for_tts = doubled;
            }
        }

        set_stage_anyhow(
            &task_dir,
            "tts",
            StagePatch {
                last_message: Some(format!("Generating {}/{}...", i + 1, segments.len())),
                ..Default::default()
            },
        )
        .ok();

        // 调 voxcpm-burn 合成
        let mut cmd = std::process::Command::new(&bin);
        cmd.arg("--ref-audio").arg(&ref_for_tts);
        cmd.arg(&text);
        cmd.arg(&out_path);
        let out = cmd
            .output()
            .map_err(|e| anyhow::anyhow!("spawn {bin} 失败: {e}"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(anyhow::anyhow!(
                "voxcpm-burn 段 {idx} 失败 (exit {:?}): {}",
                out.status.code(),
                stderr
            ));
        }

        let tts_duration = probe_duration_ms(&out_path);
        tts_segments.push(TtsSegment {
            timing: crate::stages::split_audio::out::SplitAudioTiming {
                seg_idx: (i + 1) as u32,
                text: item_text.clone(),
                start_ms,
                end_ms: start_ms + tts_duration,
                dst: text.clone(),
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
            slot_end_ms: end_ms,
            tts_duration_ms: tts_duration,
            status: "success".to_string(),
        });
    }

    let tts_file = tts_filepath(&task_dir);
    ensure_dir(Path::new(&tts_file).parent().unwrap())?;
    let result = TtsFile {
        segments: tts_segments,
    };
    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| anyhow::anyhow!("序列化 tts 结果失败: {e}"))?;
    fs::write(&tts_file, json)
        .map_err(|e| anyhow::anyhow!("写入 {} 失败: {}", tts_file.display(), e))?;

    set_stage_anyhow(
        &task_dir,
        "tts",
        StagePatch {
            status: Some(StageStatus::Success),
            completed_at: Some(now_iso()),
            progress: Some(100.0),
            last_message: Some("TTS done".to_string()),
            ..Default::default()
        },
    )?;
    emit_log(Some(&task_dir), "tts: done");
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
    fn args_defaults_and_camelcase() {
        let ctx = ctx_at(
            "/x",
            json!({
                "task": {"id":"t","task_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}
            }),
        );
        let cfg = read_args(&ctx);
        assert!(cfg.skip_existing);
        assert!(!cfg.ref_audio_x2);
        assert_eq!(cfg.only_indices, None);

        let ctx2 = ctx_at(
            "/x",
            json!({
                "task": {"id":"t","task_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"stages": {"tts": {"skipExisting": false, "refAudioX2": true, "onlyIndices": [1,2,3]}}}
            }),
        );
        let cfg2 = read_args(&ctx2);
        assert!(!cfg2.skip_existing);
        assert!(cfg2.ref_audio_x2);
        assert_eq!(cfg2.only_indices, Some(vec![1, 2, 3]));
    }

    #[test]
    fn missing_timings_errors() {
        let dir = std::env::temp_dir()
            .join(format!("ld_tts_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = ctx_at(
            &dir,
            json!({
                "task": {"id":"t","task_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}
            }),
        );
        crate::context::write_ctx(&dir, &ctx).unwrap();
        let res = stage_tts(&ctx);
        assert!(res.is_err());
        assert!(
            res.unwrap_err().to_string().contains("timings.json"),
            "应提示 timings.json 读取失败"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
