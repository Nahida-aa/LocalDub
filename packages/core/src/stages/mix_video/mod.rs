//! mix_video 阶段 (镜像 TS `packages/core/stages/mix_video/index.ts`)。
//!
//! 两条分支:
//! - `subtitle` pipeline: 硬烧字幕到视频 (从 translation / srt)
//! - `dub` pipeline: 混流配音音频 (audio_dubbing.wav + bgm, sidechain 压缩) 再硬烧字幕

pub mod args;

use std::path::PathBuf;

use anyhow::{Context, anyhow};

use crate::context::TaskCtx;
use crate::stages::mix_video::args::{Alignment, MixVideoArgs};
use crate::stages::utils::srt::{SrtSeg, write_srt};
use crate::stages::utils::{
    StagePatch, StageStatus, bgm_path, default_font, dubbing_path, emit_log, ensure_dir,
    ffmpeg_timeout, final_video_dir, probe_video_resolution, read_timings, read_translation_result,
    resolve_language, set_stage_anyhow, subtitle_file_path, video_source_path,
};

/// 读取 mix_video 配置 (缺省用 MixVideoArgs::default)。
fn read_args(ctx: &TaskCtx) -> MixVideoArgs {
    ctx.input
        .get("stages")
        .and_then(|v| v.get("mix_video"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// 入口 (镜像 TS `stageMixVideo`)。
pub fn stage_mix_video(ctx: &TaskCtx) -> anyhow::Result<()> {
    let task_dir = ctx.task.task_dir.clone();
    let task_id = ctx.task.id.clone();
    let cfg = read_args(ctx);

    if !cfg.enabled {
        emit_log(
            Some(&task_dir),
            "[mix_video] disabled (mix_video.enabled=false), skipping",
        );
        set_stage_anyhow(
            &task_dir,
            "mix_video",
            StagePatch {
                status: Some(StageStatus::Success),
                completed_at: Some(crate::stages::utils::now_iso()),
                progress: Some(100.0),
                last_message: Some("Skipped".into()),
                ..Default::default()
            },
        )?;
        return Ok(());
    }

    let video_file_path = video_source_path(ctx)?;
    let merge_dir = std::path::Path::new(&task_dir).join("mix_video");
    ensure_dir(&merge_dir)?;

    if !std::path::Path::new(&video_file_path).exists() {
        return Err(anyhow!("video_source.mp4 not found: {video_file_path}"));
    }

    let pipeline = ctx.pipeline.clone();
    let (_, target_lang) = resolve_language(ctx)?;

    let no_translate = ctx
        .input
        .get("stages")
        .and_then(|v| v.get("translate"))
        .and_then(|v| v.get("enabled"))
        .and_then(|v| v.as_bool())
        == Some(false);

    let subtitle_source = ctx
        .input
        .get("task")
        .and_then(|v| v.get("subtitleSource"))
        .and_then(|v| v.as_str())
        .unwrap_or("asr");

    let final_dir_name = final_video_dir(&pipeline, subtitle_source, no_translate);
    let final_video_dir = merge_dir.join(&final_dir_name);
    ensure_dir(&final_video_dir)?;
    let final_video = final_video_dir.join(format!("{task_id}.mp4"));

    // 对齐 → ffmpeg ass Alignment 数值
    let alignment = cfg.alignment.unwrap_or(Alignment::BottomCenter);
    let alignment_num = args::alignment_to_ffmpeg(alignment);

    if pipeline == "subtitle" {
        let sub_path = merge_dir.join(format!("subtitles.{target_lang}.srt"));
        build_subtitle_srt_subtitle_branch(ctx, &sub_path, &cfg, no_translate)?;

        let style = probe_style(&video_file_path, &target_lang, &cfg, alignment_num);
        let filter = sub_filter_arg(&sub_path.to_string_lossy(), &style);

        ffmpeg_timeout(
            &[
                "-i".into(),
                video_file_path.clone(),
                "-vf".into(),
                filter,
                "-map".into(),
                "0:v:0".into(),
                "-map".into(),
                "0:a:0".into(),
                "-c:v".into(),
                "libx264".into(),
                "-preset".into(),
                "fast".into(),
                "-crf".into(),
                "23".into(),
                "-c:a".into(),
                "copy".into(),
                "-movflags".into(),
                "+faststart".into(),
                final_video.to_string_lossy().to_string(),
            ],
            300_000,
        )
        .map_err(|e| anyhow!("mix_video (subtitle) ffmpeg 失败: {e}"))?;
    } else {
        let dubbing_file = dubbing_path(&task_dir);
        if !dubbing_file.exists() {
            return Err(anyhow!(
                "audio_dubbing.wav not found: {}; 请先运行 mix_audio",
                dubbing_file.display()
            ));
        }
        let bgm_file = cfg
            .bgm_path
            .clone()
            .map(PathBuf::from)
            .unwrap_or_else(|| bgm_path(&task_dir));

        let sub_path = merge_dir.join(format!("{target_lang}.srt"));
        build_subtitle_srt_dub_branch(ctx, &sub_path, &cfg)?;

        let style = probe_style(&video_file_path, &target_lang, &cfg, alignment_num);
        let filter = sub_filter_arg(&sub_path.to_string_lossy(), &style);

        // 配音 + 背景音 sidechain 压缩混流
        let mixed_audio = merge_dir.join("audio_mixed.m4a");
        let dub_gain = cfg.dub_gain;
        let bgm_gain = cfg.bgm_gain;
        let filter_complex = format!(
            "[0:a]volume={dub_gain}dB[adub];\
             [1:a]volume={bgm_gain}dB[abgm];\
             [adub]asplit[adub_mix][adub_key];\
             [abgm][adub_key]sidechaincompress=threshold=-24dB:ratio=4:attack=5:release=300[abgm_sc];\
             [adub_mix][abgm_sc]amix=inputs=2:duration=longest:normalize=0,\
             acompressor=threshold=-24dB:ratio=2,alimiter=limit=-1dB[aout]"
        );
        ffmpeg_timeout(
            &[
                "-i".into(),
                dubbing_file.to_string_lossy().to_string(),
                "-i".into(),
                bgm_file.to_string_lossy().to_string(),
                "-filter_complex".into(),
                filter_complex,
                "-map".into(),
                "[aout]".into(),
                "-c:a".into(),
                "aac".into(),
                mixed_audio.to_string_lossy().to_string(),
            ],
            300_000,
        )
        .map_err(|e| anyhow!("mix_video 音频混流 ffmpeg 失败: {e}"))?;

        ffmpeg_timeout(
            &[
                "-i".into(),
                video_file_path.clone(),
                "-i".into(),
                mixed_audio.to_string_lossy().to_string(),
                "-vf".into(),
                filter,
                "-map".into(),
                "0:v:0".into(),
                "-map".into(),
                "1:a:0".into(),
                "-c:v".into(),
                "libx264".into(),
                "-preset".into(),
                "fast".into(),
                "-crf".into(),
                "23".into(),
                "-c:a".into(),
                "aac".into(),
                "-movflags".into(),
                "+faststart".into(),
                "-shortest".into(),
                final_video.to_string_lossy().to_string(),
            ],
            300_000,
        )
        .map_err(|e| anyhow!("mix_video (dub) ffmpeg 失败: {e}"))?;
    }

    emit_log(
        Some(&task_dir),
        &format!("Wrote final video: {}", final_video.display()),
    );

    set_stage_anyhow(
        &task_dir,
        "mix_video",
        StagePatch {
            status: Some(StageStatus::Success),
            completed_at: Some(crate::stages::utils::now_iso()),
            progress: Some(100.0),
            last_message: Some("Merged".into()),
            ..Default::default()
        },
    )?;

    Ok(())
}

/// 构造 subtitles 滤镜参数 (镜像 TS `subFilterArg`)。
fn sub_filter_arg(sub_path: &str, style: &str) -> String {
    let escaped = sub_path.replace('\'', "\\'");
    format!("subtitles=filename='{escaped}':force_style='{style}'")
}

/// 探测字幕样式 (字号/边距/对齐/描边/阴影/字体) (镜像 TS `probeStyle`)。
fn probe_style(video_file: &str, dst_lang: &str, cfg: &MixVideoArgs, alignment_num: u8) -> String {
    let (width, height) = probe_video_resolution(video_file);
    let is_portrait = height > width && height > 0;
    let font_size = cfg.font_size.unwrap_or(if is_portrait {
        if dst_lang == "zh" { 12.0 } else { 9.0 }
    } else if dst_lang == "zh" {
        24.0
    } else {
        18.0
    });
    let margin_v = cfg.margin_v.unwrap_or(if is_portrait { 70.0 } else { 5.0 });
    let outline = cfg.outline;
    let shadow = cfg.shadow;
    let font = cfg.font.clone().unwrap_or_else(|| default_font(dst_lang));
    format!(
        "FontName={font},FontSize={font_size},PrimaryColour=&H00FFFFFF,OutlineColour=&H00000000,BorderStyle={},Outline={outline},Shadow={shadow},Alignment={alignment_num},MarginV={margin_v}",
        if outline > 0.0 { 1 } else { 0 }
    )
}

/// subtitle 分支: 从 translation / srt 生成字幕 SRT (镜像 TS subtitle 分支)。
fn build_subtitle_srt_subtitle_branch(
    ctx: &TaskCtx,
    sub_path: &std::path::Path,
    cfg: &MixVideoArgs,
    no_translate: bool,
) -> anyhow::Result<()> {
    let task_dir = ctx.task.task_dir.clone();
    let (_, target_lang) = resolve_language(ctx)?;

    let segs: Vec<SrtSeg> = if no_translate {
        // 用已有 srt 或 subtitle file
        let srt_path = cfg
            .srt_path
            .clone()
            .unwrap_or_else(|| subtitle_file_path(ctx));
        read_srt_file_to_segs(&srt_path)?
    } else {
        let tr_file = crate::stages::utils::translation_file_path(&task_dir, &target_lang);
        let data = read_translation_result(ctx)
            .with_context(|| format!("读取翻译结果失败: {}", tr_file.display()))?;
        let segments = data
            .get("segments")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        segments
            .iter()
            .map(|s| {
                let start_ms = s.get("start_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                let end_ms = s.get("end_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                let text = s
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let dst = s
                    .get("dst")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                SrtSeg {
                    start_ms,
                    end_ms,
                    dst,
                    text,
                    actual_start: None,
                    actual_end: None,
                }
            })
            .collect()
    };
    write_srt(&segs, sub_path, no_translate).map_err(|e| anyhow!("写字幕 SRT 失败: {e}"))
}

/// dub 分支: 从 mix_audio/timings.json 生成字幕 SRT (镜像 TS dub 分支)。
fn build_subtitle_srt_dub_branch(
    ctx: &TaskCtx,
    sub_path: &std::path::Path,
    _cfg: &MixVideoArgs,
) -> anyhow::Result<()> {
    let task_dir = ctx.task.task_dir.clone();
    let data = read_timings(&task_dir).map_err(|e| anyhow!("读取 timings 失败: {e}"))?;
    let segs: Vec<SrtSeg> = data
        .segments
        .iter()
        .map(|t| {
            let base = &t.timing;
            SrtSeg {
                start_ms: base.start_ms,
                end_ms: base.end_ms,
                dst: base.dst.clone(),
                text: base.text.clone(),
                actual_start: Some(t.actual_start),
                actual_end: Some(t.actual_end),
            }
        })
        .collect();
    write_srt(&segs, sub_path, false).map_err(|e| anyhow!("写字幕 SRT 失败: {e}"))
}

/// 读取已有 SRT 文件为 SrtSeg 列表 (subtitle 分支 noTranslate 路径)。
fn read_srt_file_to_segs(path: &str) -> anyhow::Result<Vec<SrtSeg>> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("读取 SRT {path} 失败"))?;
    let mut segs = Vec::new();
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in raw.split('\n') {
        if line.trim().is_empty() {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    for b in blocks {
        if b.len() < 3 {
            continue;
        }
        // b[1] = "HH:MM:SS,mmm --> HH:MM:SS,mmm"
        let times: Vec<&str> = b[1].split(" --> ").collect();
        if times.len() != 2 {
            continue;
        }
        let s = parse_srt_time(times[0]);
        let e = parse_srt_time(times[1]);
        let text = b[2..].join("\n");
        segs.push(SrtSeg {
            start_ms: s,
            end_ms: e,
            dst: text.clone(),
            text,
            actual_start: None,
            actual_end: None,
        });
    }
    Ok(segs)
}

fn parse_srt_time(t: &str) -> u64 {
    let parts: Vec<&str> = t.split(':').collect();
    if parts.len() != 3 {
        return 0;
    }
    let h: u64 = parts[0].parse().unwrap_or(0);
    let m: u64 = parts[1].parse().unwrap_or(0);
    let rest: Vec<&str> = parts[2].split(',').collect();
    let s: u64 = rest.first().and_then(|x| x.parse().ok()).unwrap_or(0);
    let ms: u64 = rest.get(1).and_then(|x| x.parse().ok()).unwrap_or(0);
    (h * 3600 + m * 60 + s) * 1000 + ms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_video_args_defaults() {
        let a = MixVideoArgs::default();
        assert_eq!(a.outline, 0.0);
        assert_eq!(a.shadow, 1.0);
        assert_eq!(a.bgm_gain, -6.0);
        assert_eq!(a.dub_gain, 3.0);
        assert_eq!(a.alignment, None);
    }

    #[test]
    fn alignment_to_ffmpeg_maps_correctly() {
        assert_eq!(args::alignment_to_ffmpeg(Alignment::BottomLeft), 1);
        assert_eq!(args::alignment_to_ffmpeg(Alignment::BottomCenter), 2);
        assert_eq!(args::alignment_to_ffmpeg(Alignment::TopRight), 9);
    }

    #[test]
    fn final_video_dir_variants() {
        assert_eq!(final_video_dir("dub", "asr", false), "dub");
        assert_eq!(final_video_dir("subtitle", "asr", false), "subtitle");
        assert_eq!(final_video_dir("dub", "asr", true), "dub_ntl");
        assert_eq!(final_video_dir("dub", "sf_ocr", false), "dub_sf_ocr");
        assert_eq!(final_video_dir("dub", "asr_ocr", true), "dub_asr_ocr_ntl");
    }

    #[test]
    fn probe_style_uses_defaults_for_landscape_zh() {
        // 横屏 (宽>高) 中文 → fontSize 24, marginV 5, alignment 2
        let cfg = MixVideoArgs::default();
        let style = probe_style(
            "/nonexistent.mp4", // probe 失败返回 (0,0) → is_portrait=false
            "zh",
            &cfg,
            2,
        );
        assert!(style.contains("FontSize=24"));
        assert!(style.contains("MarginV=5"));
        assert!(style.contains("Alignment=2"));
        assert!(style.contains("FontName=Noto Sans CJK SC") || style.contains("FontName=Arial"));
    }

    #[test]
    fn sub_filter_arg_escapes_quotes() {
        let f = sub_filter_arg("/path/with'quote/sub.srt", "FontName=Arial");
        assert!(f.starts_with("subtitles=filename='"));
        assert!(!f.contains("'sub.srt")); // 单引号被转义
        assert!(f.contains("\\'"));
    }

    #[test]
    fn mix_video_args_accepts_decimals() {
        // 镜像 TS z.number(): fontSize/shadow/marginV/outline 支持小数
        let cfg: MixVideoArgs =
            serde_json::from_str(r#"{"fontSize":21.4,"shadow":1.1,"marginV":45.5,"outline":0.5}"#)
                .expect("小数应可解析");
        assert_eq!(cfg.font_size, Some(21.4));
        assert_eq!(cfg.shadow, 1.1);
        assert_eq!(cfg.margin_v, Some(45.5));
        assert_eq!(cfg.outline, 0.5);

        let style = probe_style("/nonexistent.mp4", "zh", &cfg, 2);
        assert!(style.contains("FontSize=21.4"));
        assert!(style.contains("MarginV=45.5"));
        assert!(style.contains("Shadow=1.1"));
        assert!(style.contains("Outline=0.5"));
    }
}
