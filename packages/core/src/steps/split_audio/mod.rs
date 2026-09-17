//! split_audio: 按字幕/翻译时间轴把音频切块, 供 tts 逐段合成。
//!
//! 镜像 TS `packages/core/steps/06_split_audio/split_audio.ts`。
//! - 权威字幕文件 (asr_fix / sf_ocr / asr_ocr 按 subtitleSource) 取时间轴
//! - translate.enabled 时译文来自 {lang}.json, 否则退回原文
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

use crate::state::WorkflowState;
use crate::steps::split_audio::out::{
    SplitAudioResult, SplitAudioSegment, SplitAudioTimingResult, TranslateResultMeta,
};
use crate::steps::split_audio::pad_segment::{pad_segments, SegmentBounds};
use crate::steps::translate::out::{TranslateResult, TranslateSegment};
use crate::steps::utils::{
    ensure_dir, now_iso, probe_duration_ms, set_step_anyhow, split_audio_path,
    split_audio_timings_path, StepPatch, StepStatus,
};
use crate::utils::file_ops::write_json_pretty;

pub use args::SplitAudioArgs;

/// SplitAudioTiming 实现 SegmentBounds (pad_segments 用 start/end)。
impl SegmentBounds for TranslateSegment {
    fn start_ms(&self) -> u32 {
        self.start_ms
    }
    fn end_ms(&self) -> u32 {
        self.end_ms
    }
}

/// 入口 (镜像 TS `stepSplitAudio`)。
/// 本步骤耗时在 0~2s, 对于流程中的步骤来说没有必要考虑 如何设计成 与 某些步骤并行
pub fn step_split_audio(ctx: &WorkflowState) -> anyhow::Result<()> {
    let video_dir = ctx.workflow.video_dir.clone();
    tracing::info!(target: "split_audio", "start");

    let args = SplitAudioArgs::from_input(&ctx.input);

    // 输入契约一次性构建：切块音频源 + 翻译产物路径，后续只读它，
    // 不再各自 `LangPath::from_state` / `audio_from_state` 拼两次。
    // 本步既要用音频源也要用 translate 路径，故走完整 `from_state`；
    // 仅要音频源的消费者才用 `audio_from_state`。
    let args::SplitAudioInput {
        audio_path,
        translate_path,
    } = args::SplitAudioInput::from_state(ctx)?;

    // 字幕时间轴 + 文本：**统一来自 translate 的产物**（translate 恒执行；
    // enabled=false 时 dst=原文直通）。焦点文件不再是下游输入。
    //
    // 目标语言读**已落定的事实**（翻译步骤写的），不现算；
    // 源语言独立解析（它有自己的三级回退，与目标语言无关）。
    let src_lang = crate::steps::utils::src_language(ctx).code().to_string();
    let trans = TranslateResult::read(&translate_path).map_err(|e| {
        anyhow::anyhow!("{e}; 请先运行 translate（或从该步骤 continue 以重新生成）")
    })?;
    if trans.segments.is_empty() {
        return Err(anyhow::anyhow!("translation 无 segments"));
    }

    // 切块源：已在 `SplitAudioInput::from_state` 内按优先级解析
    // （vocalsFilePath 覆盖 → 人声 stem → 原始音频），此处不再手拼。
    // NB: 旧逻辑曾支持 `sourceFilePath` 调试覆盖，但它被 vocals 检查遮蔽，
    // 实际只对「无 vocals + vad_align」窄路径生效且与 `vocalsFilePath` 概念混淆，
    // 已从 `SplitAudioArgs` 移除（自定义切块源走 `vocalsFilePath`）。
    let source_audio = audio_path.as_path().to_string_lossy().into_owned();

    // 是否 dub 流程由 **output_mode 输入契约**决定，不再靠「源文件是否碰巧存在」
    // 推断——后者把「有没有 separate 产物」当成了流程模式，是个副作用推断
    // （subtitle 模式若残留了旧的 vocals stem，就会误判成 dub）。
    let is_dub = crate::workflows::args::OutputMode::from_input(&ctx.input)
        == crate::workflows::args::OutputMode::Dub;

    // 意图时序 =  translate 的 segments **原样搬运**。
    //
    // `SplitAudioTiming` 已是 `TranslateSegment` 的别名，所以这里不需要任何转换
    // ——此前的 30 行逐字段 `seg.get(..).unwrap_or(0)` 就是手写一个 serde，
    // 还把坏字段静默吞成 0。现在解析只在 `TranslateResult::read` 一处，
    // 字段缺失/类型不对在那里就会失败。
    let mut segments: Vec<TranslateSegment> = trans.segments.clone();

    // padSegments 得到真实切块时序
    let bounds = pad_segments(&segments, args.start_pad_ms, args.end_pad_ms);
    let mut split_segments: Vec<SplitAudioSegment> = segments
        .iter()
        .zip(bounds.iter())
        .map(|(t, (s, e))| SplitAudioSegment {
            timing: t.clone(),
            split_start_ms: *s,
            split_end_ms: *e,
        })
        .collect();

    let total_ms = probe_duration_ms(&source_audio);

    let split_audio_dir = Path::new(&video_dir).join("split_audio");
    let vocals_segment_dir = split_audio_dir.join("vocals");
    ensure_dir(&vocals_segment_dir)?;
    ensure_dir(&split_audio_dir)?;

    // dub 模式才切人声块; subtitle 模式跳过, 只产出时序文件。
    if is_dub {
        cut_vocal_segments(
            &vocals_segment_dir,
            translate_path.as_path(),
            &split_segments,
            &source_audio,
            total_ms,
        )?;
    }

    // meta 与 translate 产物的 meta 对齐：src/target 都取**产物自述**的值
    // （`trans` 在函数开头已读入），而不是重新解析——本步骤处理的就是那份
    // segments，meta 理应与之逐字一致。
    let meta = TranslateResultMeta {
        // 产物 meta 为空串时回退到独立解析（老产物可能没写过该字段）
        src_lang: if trans.meta.src_lang.is_empty() {
            src_lang.clone()
        } else {
            trans.meta.src_lang.clone()
        },
        target_lang: translate_path.lang().to_string(),
    };

    // VAD alignment (可选)：原地修正 split_segments 与 segments 的时序。
    // 是否实际修正由内部决定，这里不需要它的返回值——
    // 最终态在下方统一各写一次盘（corrected 与否内容一致，省掉一次冗余写盘）。
    if args.vad_align {
        vad_align::apply_vad_align(
            &mut split_segments,
            &mut segments,
            &source_audio,
            total_ms,
            &vocals_segment_dir.to_string_lossy(),
            is_dub,
        );
    }

    // split_audio.json：总是写最终（可能经 VAD 修正）的切块时序。
    let split_result = SplitAudioResult {
        segments: split_segments.clone(),
        meta: meta.clone(),
    };
    let split_path = split_audio_path(&video_dir);
    write_json_pretty(&split_path, &split_result)?;

    // timings.json：意图时序（同样可能被 VAD 修正），总是写。
    let timing_result = SplitAudioTimingResult {
        segments: segments.clone(),
    };
    let timings_path = split_audio_timings_path(&video_dir);
    write_json_pretty(&timings_path, &timing_result)?;

    set_step_anyhow(
        &video_dir,
        "split_audio",
        StepPatch {
            status: Some(StepStatus::Success),
            completed_at: Some(now_iso()),
            progress: Some(100.0),
            last_message: Some("Split".to_string()),
            ..Default::default()
        },
    )?;
    tracing::info!(target: "split_audio", "done");
    Ok(())
}

/// dub 模式切块：把人声按 `split_segments` 切到 `vocals_segment_dir`。
///
/// - 若翻译产物比已切出的块更新（重跑翻译），清空旧块重切，避免 stale 块。
/// - 无效段（起点 ≥ 终点）写 44 字节空 wav 头占位，不报错，让下游自行跳过。
///
/// `source_audio` 是实际要切的音频（dub 模式下即 vocals stem，
/// 见 `step_split_audio` 的 `is_dub` 推导）。`total_ms` 仅用于把末尾段
/// 截断到音频实际长度内。
fn cut_vocal_segments(
    vocals_segment_dir: &Path,
    translation_file: &Path,
    split_segments: &[SplitAudioSegment],
    source_audio: &str,
    total_ms: u64,
) -> anyhow::Result<()> {
    // 若翻译文件比已切出的块更新 (重跑翻译), 清空旧块重新切
    let has_seg = fs::read_dir(vocals_segment_dir)
        .ok()
        .map(|d| {
            d.filter_map(|e| e.ok())
                .any(|e| e.path().extension().map(|x| x == "wav").unwrap_or(false))
        })
        .unwrap_or(false);
    if has_seg && translation_file.exists() {
        let trans_mtime = fs::metadata(translation_file)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        // 任意旧块的最新 mtime
        let oldest_seg_is_newer = fs::read_dir(vocals_segment_dir)
            .ok()
            .map(|d| {
                d.filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().map(|x| x == "wav").unwrap_or(false))
                    .filter_map(|e| fs::metadata(e.path()).ok().and_then(|m| m.modified().ok()))
                    .all(|m| m < trans_mtime)
            })
            .unwrap_or(false);
        if oldest_seg_is_newer {
            let _ = fs::remove_dir_all(vocals_segment_dir);
            ensure_dir(vocals_segment_dir)?;
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
        let start = start_ms as u64;
        let end = total_ms.min(end_ms as u64);
        if end <= start {
            fs::write(&out_path, vec![0u8; 44]).ok();
            continue;
        }
        util::cut_audio_range(source_audio, start, end, &out_path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx_with_input(input: serde_json::Value) -> WorkflowState {
        crate::state::read_ctx_from_value(input).unwrap()
    }

    #[test]
    fn field_defaults_when_absent() {
        let ctx = ctx_with_input(json!({
            "workflow": {"id": "t", "video_dir": "/x", "url": "http://e", "source": "remote",
                     "status": "running", "created_at": "2024-01-01T00:00:00Z"},
            "input": {"steps": {"split_audio": {}}}
        }));
        let cfg = SplitAudioArgs::from_input(&ctx.input);
        assert!(!cfg.vad_align);
        assert_eq!(cfg.start_pad_ms, 100);
        assert_eq!(cfg.end_pad_ms, 300);
        assert!(cfg.vocals_file_path.is_none());
    }

    #[test]
    fn read_config_parses_camel_case_fields() {
        let ctx = ctx_with_input(json!({
            "workflow": {"id": "t", "video_dir": "/x", "url": "http://e", "source": "remote",
                     "status": "running", "created_at": "2024-01-01T00:00:00Z"},
            "input": {"steps": {"split_audio": {
                "vadAlign": true,
                "startPadMs": 150,
                "endPadMs": 400,
                "vocalsFilePath": "/v.wav"
            }}}
        }));
        let cfg = SplitAudioArgs::from_input(&ctx.input);
        assert!(cfg.vad_align);
        assert_eq!(cfg.start_pad_ms, 150);
        assert_eq!(cfg.end_pad_ms, 400);
        assert_eq!(cfg.vocals_file_path.as_deref(), Some("/v.wav"));
    }
}
