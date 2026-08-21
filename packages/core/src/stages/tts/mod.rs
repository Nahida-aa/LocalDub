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

use indicatif::{ProgressBar, ProgressStyle};

use crate::context::TaskCtx;
use crate::stages::tts::args::{TtsArgs, TtsDevice, TtsRuntime};
use crate::stages::tts::out::{TtsFile, TtsSegment};
use crate::stages::utils::{
    StagePatch, StageStatus, cargo_build_bin, ensure_dir, ffmpeg, find_release_bin,
    now_iso, probe_duration_ms, read_split_audio_timings, set_stage_anyhow, tts_filepath,
};

/// vocals 参考音"非静音"判定阈值: PCM 裸数据 > 该字节数才认为有实际声音。
/// 1200 采样帧 * 16bit * 2 声道 = 38400 bytes (~75ms @ 16kHz)。
const MIN_REF_BYTES: u64 = 1200 * 16 * 2;
/// refAudioX2 触发阈值: 参考音短于此则拼接自身翻倍。
const MIN_REF_DURATION_MS: u64 = 2500;

/// TTS 静音段统一采样率。VoxCPM cloud 归一化到 48000Hz (`voxlab::TARGET_SAMPLE_RATE`),
/// 本地二进制输出也为 48k, 故静音占位统一用 48000Hz 以保证与合成段、mix_audio 探测一致。
const SILENT_SAMPLE_RATE: u32 = 48000;

/// 写一段合法的静音 wav (47000Hz 单声道, 无采样数据)。
///
/// 用于「有意不合成」的段 (regenIndices 排除 / 空译文 / 无参考音),
/// 取代原先 `fs::write(vec![0u8;44])` 写的*非法零头* wav —— 后者无 RIFF 标记,
/// 会让后续 mix_audio 的 ffprobe/trim 步骤读取失败 (exit 183 "Invalid data")。
///
/// 合法静音 wav 可被 ffprobe 正常探测到 48000Hz, 且时长 0ms, mix_audio 会自然跳过该段。
fn write_silent_wav(out_path: &str) -> anyhow::Result<()> {
    voxlab::write_wav(&[], SILENT_SAMPLE_RATE, out_path)
        .map_err(|e| anyhow::anyhow!("写静音 wav {} 失败: {e}", out_path))
}

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
///
/// 仅用于非 cloud 运行时 (cloud 在 `stage_tts` 中直接走 `VoxCPMCloud`, 不调用本函数)。
/// 因此这里只按 device 选 GPU 后端, runtime 参数仅用于报错信息。
fn pick_voxcpm_bin(device: TtsDevice, runtime: TtsRuntime) -> anyhow::Result<String> {
    let candidates: Vec<&str> = match device {
        TtsDevice::Cpu => vec!["voxcpm-burn-cpu"],
        TtsDevice::Rocm => vec!["voxcpm-burn-vulkan", "voxcpm-burn-wgpu"],
        TtsDevice::Mps => vec!["voxcpm-burn-wgpu", "voxcpm-burn-cpu"],
        TtsDevice::Webgpu => vec!["voxcpm-burn-wgpu", "voxcpm-burn-cpu"],
        TtsDevice::Cuda => vec!["voxcpm-burn-vulkan", "voxcpm-burn-wgpu", "voxcpm-burn-cpu"],
    };
    for c in &candidates {
        if let Some(p) = find_release_bin(c) {
            return Ok(p.to_string_lossy().into_owned());
        }
    }
    // 阶段内自动编译缺失二进制 (用户选项: 阶段内自动编译): 取首个候选后端编译后重试
    let first = candidates
        .first()
        .copied()
        .ok_or_else(|| anyhow::anyhow!("无可用 voxcpm-burn 后端候选为空 (device={device:?})"))?;
    // 候选名 `voxcpm-burn-<feature>`, feature 即后缀
    let first_feat = first.strip_prefix("voxcpm-burn-").unwrap_or("wgpu");
    tracing::info!(target: "tts", 
        "未找到 voxcpm-burn 二进制, 尝试自动编译 {first} (--features {first_feat})..."
    );
    let _ = cargo_build_bin("voxcpm-burn", first, &[first_feat], false).map_err(|e| {
        anyhow::anyhow!(
            "{e}\n若编译失败, 请手动执行: cargo build --release -p voxcpm-burn --bin {first} --no-default-features --features {first_feat}"
        )
    })?;
    if let Some(p) = find_release_bin(first) {
        return Ok(p.to_string_lossy().into_owned());
    }
    // 回退: 编译成功但产物仍缺失 (理论上不会发生), 引导用户先 build
    Err(anyhow::anyhow!(
        "未找到 voxcpm-burn 二进制 (profile: {:?}/{:?})。请先 cargo build --release -p voxcpm-burn",
        device,
        runtime
    ))
}

/// 入口 (镜像 TS `stageTts`)。
pub fn stage_tts(ctx: &TaskCtx) -> anyhow::Result<()> {
    let task_dir = ctx.task.task_dir.clone();
    tracing::info!(target: "tts", "start");

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

    // cloud 运行时走 HTTP gradio (voxlab::VoxCPMCloud), 不 spawn 本地二进制。
    // 镜像 TS: runtime === "cloud" -> new VoxCPMCloud() (而非本地 onnx/pytorch 引擎)。
    let use_cloud = args.runtime == TtsRuntime::Cloud;
    let cloud = if use_cloud {
        Some(voxlab::VoxCPMCloud::new(voxlab::VoxCPMCloudConfig {
            api_url: None,
            control_instruction: None,
        })?)
    } else {
        None
    };
    let bin = if use_cloud {
        None
    } else {
        Some(pick_voxcpm_bin(args.device, args.runtime)?)
    };
    tracing::info!(target: "tts", 
        "Using voxcpm backend: {}",
        if use_cloud {
            "cloud (gradio)".to_string()
        } else {
            bin.clone().unwrap()
        }
    );

    // regenIndices 语义 (continue 模式精准重跑):
    // - 首次 start 时强制忽略, 全量跑; 仅 continue 重跑时用。
    // - 列表内的段: 无视 skipExisting, 强制重新合成 (先删旧 wav 再生成)。
    // - 列表外的段: 保留旧结果 —— 优先复用 tts.json 已有记录, 无旧记录才写合法静音占位。
    let is_start = ctx
        .input
        .get("task")
        .and_then(|t| t.get("action"))
        .and_then(|a| a.as_str())
        == Some("start");
    let regen_indices: Option<Vec<u32>> = if is_start {
        None
    } else {
        args.regen_indices.clone()
    };
    let regen_active = regen_indices
        .as_ref()
        .map(|o| !o.is_empty())
        .unwrap_or(false);

    // 找第一个非静音 vocals 作为 fallback 参考音
    let fallback_ref: Option<String> = (0..segments.len())
        .map(|i| vocals_dir.join(format!("{:04}.wav", i + 1)))
        .find(|p| p.exists() && fs::metadata(p).map(|m| m.len()).unwrap_or(0) > MIN_REF_BYTES)
        .map(|p| p.to_string_lossy().into_owned());

    // regenIndices 生效时载入已有 tts.json, 供列表外段复用旧结果 (避免重跑时覆盖其它段)。
    let existing_segments: std::collections::HashMap<u32, TtsSegment> = if regen_active {
        let p = tts_filepath(&task_dir);
        if p.exists() {
            match std::fs::read_to_string(&p) {
                Ok(raw) => serde_json::from_str::<TtsFile>(&raw)
                    .map(|f| {
                        f.segments
                            .into_iter()
                            .map(|s| (s.timing.seg_idx, s))
                            .collect()
                    })
                    .unwrap_or_default(),
                Err(_) => std::collections::HashMap::new(),
            }
        } else {
            std::collections::HashMap::new()
        }
    } else {
        std::collections::HashMap::new()
    };

    let mut tts_segments: Vec<TtsSegment> = Vec::with_capacity(segments.len());

    // 逐段合成进度条 (对齐 OCR/sf-cli 的 indicatif UX, 走 stderr, 非 TTY 自动隐藏)。
    // 右侧 {msg} 动态显示上一段的实时率 RTF (生成秒 / 音频秒)。
    let pb = ProgressBar::new(segments.len() as u64);
    pb.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] TTS 合成 [{bar:30.cyan/blue}] {pos}/{len} ({eta}) {msg}",
        )
        .unwrap_or_else(|_| ProgressStyle::default_bar())
        .progress_chars("=> "),
    );

    for (i, item) in segments.iter().enumerate() {
        pb.inc(1);
        let seg_idx = (i + 1) as u32;
        let idx = format!("{:04}", seg_idx);
        let out_path = tts_wav_dir.join(format!("{idx}.wav"));
        let out_path = out_path.to_string_lossy().into_owned();

        // regenIndices 语义 (continue 模式精准重跑):
        // - 仅作用于「存在有效旧结果」的段: 列表内 -> 强制重合成, 列表外 -> 保留旧结果。
        // - 「没有有效旧结果」的段 (无记录 / wav 缺失 / wav 零时长损坏): 无论是否
        //   在列表里, 都必须正常生成, 不能因为不在 regenIndices 中就被跳过复用坏结果。
        if let Some(regen) = &regen_indices {
            if !regen.is_empty() && !regen.contains(&seg_idx) {
                // 列表外: 仅在旧结果有效时复用; 否则 fall through 到下方正常生成。
                let old_valid = existing_segments
                    .get(&seg_idx)
                    .map(|_| Path::new(&out_path).exists() && probe_duration_ms(&out_path) > 0)
                    .unwrap_or(false);
                if old_valid {
                    tracing::info!(target: "tts", 
                        "[TTS] 段 {idx} 不在 regenIndices 中, 复用有效旧结果"
                    );
                    tts_segments.push(existing_segments.get(&seg_idx).unwrap().clone());
                    continue;
                }
                tracing::info!(target: "tts", 
                    "[TTS] 段 {idx} 无有效旧结果, 正常生成 (regenIndices 跳过不适用)"
                );
                // fall through: 走下方空译文/无参考音/正式合成逻辑
            }
        }

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

        // skipExisting: 输出比参考新且可被正常探测 (时长>0) 才跳过。
        // 加 probe_duration_ms>0 兜底: 历史损坏/零头 wav 即使 mtime 较新也不复用,
        // 否则会带着坏文件进 mix_audio (exit 183 Invalid data)。
        // regenIndices 命中的段无视 skipExisting, 强制重合成 (下方 rmSync 后重新生成)。
        if args.skip_existing && !regen_active && Path::new(&out_path).exists() {
            let out_mtime = fs::metadata(&out_path)
                .and_then(|m| {
                    m.modified().map(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0)
                    })
                })
                .unwrap_or(0);
            let out_dur = probe_duration_ms(&out_path);
            if out_mtime > ref_mtime && out_dur > 0 {
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

        // regenIndices 命中段: 先删旧 wav 再合成 (镜像 TS rmSync(outPath)),
        // 无视 skipExisting, 强制重新生成, 避免残留坏文件 / 旧结果干扰本次合成。
        if regen_active && Path::new(&out_path).exists() {
            let _ = fs::remove_file(&out_path);
        }

        // 空译文 -> 合法静音 wav
        if text.trim().is_empty() {
            write_silent_wav(&out_path)?;
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
            tracing::warn!(target: "tts", "[TTS] 段 {idx} 无参考音, 跳过");
            write_silent_wav(&out_path)?;
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
                progress: Some((i as f64 / segments.len() as f64) * 100.0),
                ..Default::default()
            },
        )
        .ok();

        // 调 voxcpm 合成: cloud -> HTTP gradio; 其他 -> 本地二进制
        let gen_t0 = std::time::Instant::now();
        if let Some(cloud) = &cloud {
            let samples = cloud
                .generate(&text, &ref_for_tts, Some(&item_text), 2.0)
                .map_err(|e| anyhow::anyhow!("voxcpm cloud 段 {idx} 失败: {e}"))?;
            voxlab::write_wav(&samples.samples, samples.sample_rate, &out_path)
                .map_err(|e| anyhow::anyhow!("写 cloud tts wav 段 {idx} 失败: {e}"))?;
        } else {
            let bin = bin
                .as_ref()
                .expect("non-cloud runtime must select a local voxcpm binary");
            let mut cmd = std::process::Command::new(bin);
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
        }

        let tts_duration = probe_duration_ms(&out_path);
        // 更新进度条右侧的实时率 (RTF = 生成耗时秒 / 音频时长秒, 越小越快)。
        {
            let gen_sec = gen_t0.elapsed().as_secs_f64();
            let audio_sec = tts_duration as f64 / 1000.0;
            if gen_sec > 0.0 && audio_sec > 0.0 {
                pb.set_message(format!("上一段 RTF {:.3}", gen_sec / audio_sec));
            } else {
                pb.set_message(format!("上一段 {}ms / {}s", tts_duration, gen_sec));
            }
        }
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
    pb.finish();

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
    tracing::info!(target: "tts", "done");
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
        assert_eq!(cfg.regen_indices, None);

        let ctx2 = ctx_at(
            "/x",
            json!({
                "task": {"id":"t","task_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"stages": {"tts": {"skipExisting": false, "refAudioX2": true, "regenIndices": [1,2,3]}}}
            }),
        );
        let cfg2 = read_args(&ctx2);
        assert!(!cfg2.skip_existing);
        assert!(cfg2.ref_audio_x2);
        assert_eq!(cfg2.regen_indices, Some(vec![1, 2, 3]));
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
