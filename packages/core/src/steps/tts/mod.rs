//! tts 阶段 (镜像 TS `packages/core/steps/07_tts/tts.ts`)。
//!
//! 从 split_audio/timings.json 取每段时间轴 + 译文, 调 VoxCPM 引擎逐段合成配音。
//! Rust 侧复用预构建的 `voxcpm-burn` 二进制 (镜像 TS `newVoxCPMEngine` 的进程外等价):
//! 每段 spawn 一次二进制, 传入 text / output / --ref-audio (人声参考音)。
//!
//! 注: TS 引擎进程内 load 一次后逐段 synthesize; 进程外 spawn 每段会重复 load 模型,
//! 对长视频较慢。这是 Rust core 二进制编排模式下的已知取舍 (与其他 step 一致),
//! 后续可改为批量模式二进制消除。

pub mod args;
pub mod out;
pub mod util;

use std::fs;
use std::path::Path;

use indicatif::{ProgressBar, ProgressStyle};

use crate::context::WorkflowState;
use crate::steps::split_audio::out::SplitAudioTimingResult;
use crate::steps::tts::args::{TtsArgs, TtsInput, TtsRuntime};
use crate::steps::tts::out::{TtsSegment, TtsSegmentsResult};
use crate::steps::tts::util::{
    ensure_no_failed_segments, pick_voxcpm_bin, should_skip_segment, write_silent_wav,
};
use crate::steps::utils::{
    ensure_dir, ffmpeg, now_iso, probe_duration_ms, set_step_anyhow, StepPatch, StepStatus,
};
use crate::utils::file_ops::{ensure_parent_dir, write_json_pretty};

/// vocals 参考音"非静音"判定阈值: PCM 裸数据 > 该字节数才认为有实际声音。
/// 1200 采样帧 * 16bit * 2 声道 = 38400 bytes (~75ms @ 16kHz)。
const MIN_REF_BYTES: u64 = 1200 * 16 * 2;
/// refAudioX2 触发阈值: 参考音短于此则拼接自身翻倍。
const MIN_REF_DURATION_MS: u64 = 2500;

/// 合成结果有效性阈值: 短于此视为无效音频 (cloud 对超短文本可能返回 ~1ms 近空 wav,
/// 曾导致 mix_audio 零时长硬失败, 见 workfolder 大/55 #25)。正常单音节约 300ms+。
const MIN_TTS_DURATION_MS: u64 = 100;

/// 入口 (镜像 TS `stepTts`)。
pub fn step_tts(ctx: &WorkflowState) -> anyhow::Result<()> {
    let video_dir = ctx.workflow.video_dir.clone();
    tracing::info!(target: "tts", "start");

    let args = TtsArgs::from_input(&ctx.input);
    // 输入契约一次性构建：本步只读 split_audio 的两个产物，路径全由 `paths` 里的
    // 类型给出，本函数不再自己拼字符串。
    let input = TtsInput::from_video_dir(&video_dir);
    // 本步产出不在 StepInput 依赖清单内，步骤体自行构造。
    let tts_wavs_dir = crate::steps::utils::paths::tts::WavsDir::new(&video_dir);
    let ref_doubled_dir = crate::steps::utils::paths::tts::RefDoubledDir::new(&video_dir);
    let tts_path = crate::steps::utils::paths::tts::TtsPath::new(&video_dir);
    ensure_dir(tts_wavs_dir.as_path())?;
    if args.ref_audio_x2 {
        ensure_dir(ref_doubled_dir.as_path())?;
    }

    // 意图时序**带类型**读入：字段缺失/类型不对在这里就失败，不再被 `unwrap_or(0)`
    // 静默吞成 0（此前逐字段 `item.get(..)` 等于手写一个 serde）。
    let timings = SplitAudioTimingResult::read(&input.split_audio_timings_path)?;
    let segments = &timings.segments;
    if segments.is_empty() {
        return Err(anyhow::anyhow!("split_audio/timings.json 无 segments"));
    }

    // cloud 运行时走 HTTP gradio (voxcpm_cloud::VoxCPMCloud), 不 spawn 本地二进制。
    // 镜像 TS: runtime === "cloud" -> new VoxCPMCloud() (而非本地 onnx/pytorch 引擎)。
    let use_cloud = args.runtime == TtsRuntime::Cloud;
    let cloud = if use_cloud {
        Some(voxcpm_cloud::VoxCPMCloud::new(
            voxcpm_cloud::VoxCPMCloudConfig {
                api_url: None,
                control_instruction: None,
            },
        )?)
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
    let is_start = ctx.input.workflow.as_ref().and_then(|w| w.action)
        == Some(crate::workflows::args::WorkflowAction::Start);
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
        .map(|i| input.split_audio_vocals_dir.segment_path(i))
        .find(|p| p.exists() && fs::metadata(p).map(|m| m.len()).unwrap_or(0) > MIN_REF_BYTES)
        .map(|p| p.to_string_lossy().into_owned());

    // regenIndices 生效时载入已有 tts.json, 供列表外段复用旧结果 (避免重跑时覆盖其它段)。
    // graceful 退化（缺失/损坏→空 map, 损坏时留 warn）封装在
    // [`TtsSegmentsResult::read_existing_segments`]。
    let existing_segments: std::collections::HashMap<u32, TtsSegment> = if regen_active {
        TtsSegmentsResult::read_existing_segments(&tts_path)
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
        let out_path = tts_wavs_dir.segment_path(i);
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

        let start_ms = item.start_ms;
        let end_ms = item.end_ms;
        let text = item.dst.clone();
        let item_text = item.text.clone();

        // 参考音: 优先本段 vocals, 否则 fallback
        let mut ref_wav = input.split_audio_vocals_dir.segment_path(i);
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

        // skipExisting 判定见 [`should_skip_segment`] (五个条件的 AND, 含时长探针兜底)。
        // regenIndices 命中的段无视 skipExisting, 强制重合成 (下方删旧 wav 后重新生成)。
        let out_exists = Path::new(&out_path).exists();
        let out_mtime = if out_exists {
            fs::metadata(&out_path)
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
        let out_dur = if out_exists {
            probe_duration_ms(&out_path)
        } else {
            0
        };
        if should_skip_segment(
            args.skip_existing,
            regen_active,
            out_exists,
            out_mtime,
            ref_mtime,
            out_dur,
        ) {
            let dur = out_dur;
            tts_segments.push(TtsSegment {
                timing: crate::steps::split_audio::out::SplitAudioTiming {
                    idx: (i + 1) as u32,
                    text: item_text.clone(),
                    start_ms: start_ms as u32,
                    end_ms: start_ms + dur as u32,
                    dst: text.clone(),
                    src_lang: item.src_lang.clone(),
                    dst_lang: item.dst_lang.clone(),
                    speaker: item.speaker.clone(),
                },
                slot_end_ms: end_ms,
                tts_duration_ms: dur as u32,
                status: "skipped".to_string(),
            });
            continue;
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
                timing: crate::steps::split_audio::out::SplitAudioTiming {
                    idx: (i + 1) as u32,
                    text: String::new(),
                    start_ms,
                    end_ms: start_ms,
                    dst: String::new(),
                    src_lang: item.src_lang.clone(),
                    dst_lang: item.dst_lang.clone(),
                    speaker: item.speaker.clone(),
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
                timing: crate::steps::split_audio::out::SplitAudioTiming {
                    idx: (i + 1) as u32,
                    text: item_text.clone(),
                    start_ms,
                    end_ms: start_ms,
                    dst: text.clone(),
                    src_lang: item.src_lang.clone(),
                    dst_lang: item.dst_lang.clone(),
                    speaker: item.speaker.clone(),
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
                let doubled = ref_doubled_dir.x2_path(i);
                let doubled = doubled.to_string_lossy().into_owned();
                if !Path::new(&doubled).exists() {
                    let list_path = ref_doubled_dir.list_path(i);
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

        set_step_anyhow(
            &video_dir,
            "tts",
            StepPatch {
                last_message: Some(format!("Generating {}/{}...", i + 1, segments.len())),
                progress: Some((i as f64 / segments.len() as f64) * 100.0),
                ..Default::default()
            },
        )
        .ok();

        // 调 voxcpm 合成: cloud -> HTTP gradio; 其他 -> 本地二进制。
        // 合成 + 有效性校验 (<MIN_TTS_DURATION_MS 视为失败) 整体走 Retryer —— 网络抖动
        // 与「超短文本返回近空音频」同一机制兜底 (镜像 TS voxcpm.ts AsyncRetryer 包住
        // synthesize)。重试耗尽则写静音占位并标 error, 不再让整条 pipeline 失败
        // (mix_audio 会跳过零时长段)。
        let gen_t0 = std::time::Instant::now();
        let out_path_synth = out_path.clone();
        let synth_once = || -> anyhow::Result<()> {
            // 先清掉上一轮可能的坏产物, 避免写失败时 probe 读到旧文件
            let _ = fs::remove_file(&out_path_synth);
            if let Some(cloud) = &cloud {
                let samples = cloud
                    .generate(&text, &ref_for_tts, Some(&item_text), 2.0)
                    .map_err(|e| anyhow::anyhow!("voxcpm cloud 段 {idx} 失败: {e}"))?;
                voxcpm_cloud::write_wav(&samples.samples, samples.sample_rate, &out_path_synth)
                    .map_err(|e| anyhow::anyhow!("写 cloud tts wav 段 {idx} 失败: {e}"))?;
            } else {
                let bin = bin
                    .as_ref()
                    .expect("non-cloud runtime must select a local voxcpm binary");
                let mut cmd = std::process::Command::new(bin);
                cmd.arg("--ref-audio").arg(&ref_for_tts);
                cmd.arg(&text);
                cmd.arg(&out_path_synth);
                let out = cmd
                    .output()
                    .map_err(|e| anyhow::anyhow!("spawn {bin} 失败: {e}"))?;
                if !out.status.success() {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    anyhow::bail!(
                        "voxcpm-burn 段 {idx} 失败 (exit {:?}): {}",
                        out.status.code(),
                        stderr
                    );
                }
            }
            let dur = probe_duration_ms(&out_path_synth);
            if dur < MIN_TTS_DURATION_MS {
                anyhow::bail!("合成音频时长 {dur}ms < {MIN_TTS_DURATION_MS}ms, 视为无效结果");
            }
            Ok(())
        };
        let retrier = pacer_rs::Retryer::new()
            .max_attempts(2)
            .base_wait_ms(500)
            .max_wait_ms(2000)
            .jitter(0.3);
        if let Err(e) = retrier.execute(synth_once, |attempt, err| {
            tracing::warn!(
                target: "tts",
                "[TTS] 段 {idx} 合成失败 (第 {attempt} 次, 将重试): {err:#}"
            );
        }) {
            tracing::warn!(
                target: "tts",
                "[TTS] 段 {idx} 重试后仍无效 ({e:#}), 写静音占位并标 error"
            );
            write_silent_wav(&out_path)?;
            tts_segments.push(TtsSegment {
                timing: crate::steps::split_audio::out::SplitAudioTiming {
                    idx: (i + 1) as u32,
                    text: item_text.clone(),
                    start_ms,
                    end_ms: start_ms,
                    dst: text.clone(),
                    src_lang: item.src_lang.clone(),
                    dst_lang: item.dst_lang.clone(),
                    speaker: item.speaker.clone(),
                },
                slot_end_ms: end_ms,
                tts_duration_ms: 0,
                status: "error".to_string(),
            });
            continue;
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
            timing: crate::steps::split_audio::out::SplitAudioTiming {
                idx: (i + 1) as u32,
                text: item_text.clone(),
                start_ms,
                end_ms: start_ms + tts_duration as u32,
                dst: text.clone(),
                src_lang: item.src_lang.clone(),
                dst_lang: item.dst_lang.clone(),
                speaker: item.speaker.clone(),
            },
            slot_end_ms: end_ms,
            tts_duration_ms: tts_duration as u32,
            status: "success".to_string(),
        });
    }
    pb.finish();

    ensure_parent_dir(tts_path.as_path())
        .map_err(|e| anyhow::anyhow!("创建目录失败: {e}"))?;
    let result = TtsSegmentsResult {
        segments: tts_segments,
    };
    write_json_pretty(tts_path.as_path(), &result)?;

    // tts.json 已落盘, 这里只做终局判定。
    ensure_no_failed_segments(&result)?;

    set_step_anyhow(
        &video_dir,
        "tts",
        StepPatch {
            status: Some(StepStatus::Success),
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

    fn ctx_at(dir: &str, input: serde_json::Value) -> WorkflowState {
        let mut ctx = read_ctx_from_value(input).unwrap();
        ctx.workflow.video_dir = dir.to_string();
        ctx.output_mode = "dub".to_string();
        ctx
    }

    #[test]
    fn args_defaults_and_camelcase() {
        let ctx = ctx_at(
            "/x",
            json!({
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}
            }),
        );
        let cfg = TtsArgs::from_input(&ctx.input);
        assert!(cfg.skip_existing);
        assert!(!cfg.ref_audio_x2);
        assert_eq!(cfg.regen_indices, None);

        let ctx2 = ctx_at(
            "/x",
            json!({
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"steps": {"tts": {"skipExisting": false, "refAudioX2": true, "regenIndices": [1,2,3]}}}
            }),
        );
        let cfg2 = TtsArgs::from_input(&ctx2.input);
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
                "workflow": {"id":"t","video_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}
            }),
        );
        crate::context::write_ctx(&dir, &ctx).unwrap();
        let res = step_tts(&ctx);
        assert!(res.is_err());
        assert!(
            res.unwrap_err().to_string().contains("timings.json"),
            "应提示 timings.json 读取失败"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
