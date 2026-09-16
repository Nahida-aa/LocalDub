//! mix_video 阶段 (镜像 TS `packages/core/steps/mix_video/index.ts`)。
//!
//! 两条分支:
//! - `subtitle` output_mode: 硬烧字幕到视频 (从 translation / srt)
//! - `dub` output_mode: 混流配音音频 (audio_dubbing.wav + bgm, sidechain 压缩) 再硬烧字幕

pub mod args;

use anyhow::{anyhow, Context};

use crate::context::WorkflowState;
use crate::steps::mix_audio::out::MixAudioOutputTimings;
use crate::steps::mix_video::args::{Alignment, MixVideoArgs, MixVideoInput};
use crate::steps::split_audio::out::SplitAudioTimingResult;
use crate::steps::utils::srt::{SubtitleSegments, SrtSeg};
use crate::steps::utils::{
    default_font, ensure_dir, ffmpeg_timeout, final_video_dir, probe_video_resolution,
    set_step_anyhow, set_workflow_anyhow, StepPatch, StepStatus, WorkflowPatch,
};
use crate::workflows::args::OutputMode;

/// 入口 (镜像 TS `stepMixVideo`)。
pub fn step_mix_video(ctx: &WorkflowState) -> anyhow::Result<()> {
    let video_dir = ctx.workflow.video_dir.clone();
    let video_id = ctx.workflow.id.clone();
    let cfg = MixVideoArgs::from_input(&ctx.input);

    if !cfg.enabled {
        tracing::info!(target: "mix_video", "disabled (mix_video.enabled=false), skipping");
        set_step_anyhow(
            &video_dir,
            "mix_video",
            StepPatch {
                status: Some(StepStatus::Success),
                completed_at: Some(crate::steps::utils::now_iso()),
                progress: Some(100.0),
                last_message: Some("Skipped".into()),
                ..Default::default()
            },
        )?;
        return Ok(());
    }

    let input = MixVideoInput::from_state(ctx)?;
    let video_file_path = match &input {
        MixVideoInput::Dub {
            video_source_path, ..
        }
        | MixVideoInput::Subtitle {
            video_source_path, ..
        } => video_source_path.to_string(),
    };
    let mix_video = std::path::Path::new(&video_dir).join("mix_video");
    ensure_dir(&mix_video)?;

    if !std::path::Path::new(&video_file_path).exists() {
        return Err(anyhow!("video_source.mp4 not found: {video_file_path}"));
    }

    let output_mode = OutputMode::from_input(&ctx.input);
    // 目标语言读**已落定的事实**（翻译步骤写的唯一写入点），不再现算：
    // `resolve_language` 是现算，推断逻辑一变就会与产物不一致。
    let target_lang = crate::steps::utils::paths::translate::LangPath::from_state(ctx)?
        .lang()
        .to_string();

    let no_translate = !ctx.input.steps.translate.enabled;

    let subtitle_source = ctx
        .input
        .workflow
        .as_ref()
        .map(|w| w.subtitle_source.as_str())
        .unwrap_or("asr");

    let final_dir_name = final_video_dir(&output_mode.as_ref(), subtitle_source, no_translate);
    let final_video_dir = mix_video.join(&final_dir_name);
    ensure_dir(&final_video_dir)?;
    let final_video = final_video_dir.join(format!("{video_id}.mp4"));

    // 对齐 → ffmpeg ass Alignment 数值
    let alignment = cfg.alignment.unwrap_or(Alignment::BottomCenter);
    let alignment_num = args::alignment_to_ffmpeg(alignment);

    let sub_path = mix_video.join(format!("{target_lang}.srt"));
    match input {
        // 字幕时间轴统一取 split_audio 的**意图时序** `timings.json`：
        // `split_audio.json` 里 padding 后的切块区间是给切音频用的，不是字幕显示
        // 时间。vad_align 打开时 timings.json 本身已含 VAD 修正，无需再分支。
        MixVideoInput::Subtitle {
            split_audio_timings_path,
            ..
        } => {
            let timings = SplitAudioTimingResult::read(&split_audio_timings_path)
                .map_err(|e| anyhow!("读取 split_audio 意图时序失败: {e}"))?;
            let segs: SubtitleSegments = timings.into();
            segs.write_srt(&sub_path)
                .map_err(|e| anyhow!("写字幕 SRT 失败: {e}"))?;

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
        }
        MixVideoInput::Dub {
            dubbing_path,
            bgm_path,
            mix_audio_timings_path,
            ..
        } => {
            if !dubbing_path.exists() {
                return Err(anyhow!(
                    "audio_dubbing.wav not found: {}; 请先运行 mix_audio",
                    dubbing_path.display()
                ));
            }
            let bgm_file = bgm_path.as_path().to_path_buf();

            build_subtitle_srt_dub_branch(&mix_audio_timings_path, &sub_path)?;

        let style = probe_style(&video_file_path, &target_lang, &cfg, alignment_num);
        let filter = sub_filter_arg(&sub_path.to_string_lossy(), &style);

        // 配音 + 背景音 sidechain 压缩混流
        let mixed_audio = mix_video.join("audio_mixed.m4a");
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
                dubbing_path.to_string_lossy().to_string(),
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
    }

    tracing::info!(target: "mix_video", "Wrote final video: {}", final_video.display());

    // 写回 ctx.workflow.final_video_path (镜像 TS mix_video 设置 finalVideoPath)
    set_workflow_anyhow(
        &video_dir,
        WorkflowPatch {
            final_video_path: Some(Some(final_video.to_string_lossy().into_owned())),
            ..Default::default()
        },
    )?;

    set_step_anyhow(
        &video_dir,
        "mix_video",
        StepPatch {
            status: Some(StepStatus::Success),
            completed_at: Some(crate::steps::utils::now_iso()),
            progress: Some(100.0),
            last_message: Some("Merged".into()),
            ..Default::default()
        },
    )?;

    Ok(())
}

/// win32 下 libass 路径转义 (镜像 TS `filterSubPath`): `\`→`/`、`:`→`\:` (冒号是路径分隔符)。
///
/// 独立 w 函数便于无平台依赖测试 (调用处用 [`filter_sub_path`] 按编译平台分发)。
fn win_filter_sub_path(sub_path: &str) -> String {
    sub_path.replace('\\', "/").replace(':', "\\:")
}

/// 按当前平台构造 libass 字幕路径 (镜像 TS `filterSubPath`)。
fn filter_sub_path(sub_path: &str) -> String {
    if cfg!(windows) {
        win_filter_sub_path(sub_path)
    } else {
        sub_path.to_string()
    }
}

/// 构造 subtitles 滤镜参数 (镜像 TS `subFilterArg`): filename= 包裹路径, 转义单引号。
fn sub_filter_arg(sub_path: &str, style: &str) -> String {
    let escaped = filter_sub_path(sub_path).replace('\'', "\\'");
    format!("subtitles=filename='{escaped}':force_style='{style}'")
}

/// 探测字幕样式 (字号/边距/对齐/描边/阴影/字体) (镜像 TS `probeStyle`)。
fn probe_style(video_file: &str, dst_lang: &str, cfg: &MixVideoArgs, alignment_num: u8) -> String {
    let (width, height) = probe_video_resolution(video_file);
    let is_portrait = height > width && height > 0;
    let font_size = cfg.font_size.unwrap_or(if is_portrait {
        if dst_lang == "zh" {
            12.0
        } else {
            9.0
        }
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

/// dub 分支: 从 mix_audio/timings.json 生成字幕 SRT (镜像 TS dub 分支)。
fn build_subtitle_srt_dub_branch(
    mix_audio_timings_path: &crate::steps::utils::paths::mix_audio::TimingsPath,
    sub_path: &std::path::Path,
) -> anyhow::Result<()> {
    let data = MixAudioOutputTimings::read(mix_audio_timings_path)
        .map_err(|e| anyhow!("读取 timings 失败: {e}"))?;
    // 时序 → 字幕的映射收在 `From<MixAudioOutputTimings> for SubtitleSegments`：
    // 时间轴取 actual_*（跟配音对齐），文本取 dst、空则回退 text（等价于旧的
    // `write_srt(.., use_source = false)`）。
    let segs: SubtitleSegments = data.into();
    segs.write_srt(sub_path)
        .map_err(|e| anyhow!("写字幕 SRT 失败: {e}"))
}

/// 读取已有 SRT 文件为 SrtSeg 列表 (subtitle 分支 noTranslate 路径 / generate_meta 复用)。
pub fn read_srt_file_to_segs(path: &str) -> anyhow::Result<Vec<SrtSeg>> {
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

fn parse_srt_time(t: &str) -> u32 {
    let parts: Vec<&str> = t.split(':').collect();
    if parts.len() != 3 {
        return 0;
    }
    let h: u32 = parts[0].parse().unwrap_or(0);
    let m: u32 = parts[1].parse().unwrap_or(0);
    let rest: Vec<&str> = parts[2].split(',').collect();
    let s: u32 = rest.first().and_then(|x| x.parse().ok()).unwrap_or(0);
    let ms: u32 = rest.get(1).and_then(|x| x.parse().ok()).unwrap_or(0);
    (h * 3600 + m * 60 + s) * 1000 + ms
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `mix_video` 的目标语言必须来自**已落定的事实**（`ctx.target_language`），
    /// 而不是 `resolve_language` 的现算值。
    ///
    /// 为什么这条重要：`target_lang` 不只用于命名，还驱动 `probe_style` →
    /// `default_font`（非 zh → Arial）。若它取现算值，推断逻辑一改（如直通时
    /// 目标=源），这里就会与产物实际的语言不一致——中文字幕配上 Arial。
    ///
    /// 构造「配置说 vi、事实是 zh」的错配，断言以**事实**为准。
    #[test]
    fn target_lang_comes_from_persisted_fact_not_config() {
        use crate::context::read_ctx_from_value;
        use crate::steps::utils::paths::translate::LangPath;

        let mut ctx = read_ctx_from_value(serde_json::json!({
            "workflow": {"id":"t","video_dir":"/w","url":"http://e","source":"remote",
                     "status":"running","created_at":"2024-01-01T00:00:00Z"},
            "input": {"workflow": {"targetLang": "vi"}}
        }))
        .unwrap();
        ctx.target_language = Some("zh".to_string());

        let lang = LangPath::from_state(&ctx).unwrap().lang().to_string();
        assert_eq!(lang, "zh", "应以 ctx 的事实为准，而非 input 里的 vi");

        // 与字体选择挂钩：zh 必须选到 CJK 字体而非 Arial
        let style = probe_style("/nonexistent.mp4", &lang, &MixVideoArgs::default(), 2);
        assert!(
            !style.contains("FontName=Arial"),
            "中文字幕不该配 Arial（无中文字形）: {style}"
        );
    }

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
    fn win_filter_sub_path_escapes_backslash_and_colon() {
        // 镜像 TS filterSubPath win32 分支: 先 \→/, 再 :→\:  (顺序敏感, 与 TS replace 链一致)
        let p = win_filter_sub_path(r"C:\videos\sub-title.srt");
        // C:\videos → C:/videos → C\:/videos (冒号被打上 \: 前缀)
        assert_eq!(p, "C\\:/videos/sub-title.srt");

        let p2 = win_filter_sub_path(r"D:\a\b\c.srt");
        assert_eq!(p2, "D\\:/a/b/c.srt");
    }

    #[test]
    fn win_filter_sub_path_replaces_all_backslashes() {
        let p = win_filter_sub_path(r"C:\a\b\c.srt");
        assert!(
            !p.contains('\\') || p.contains("\\:"),
            "仅保留 : 转义的 \\ 前缀"
        );
        let p3 = win_filter_sub_path(r"\relative\path\f.srt");
        // 无冒号: 全部 \ 变 /, 不引入 \:
        assert_eq!(p3, "/relative/path/f.srt");
    }

    #[test]
    fn filter_sub_path_non_windows_passthrough() {
        // 非 win32: 原样返回。win32 归 win_filter_sub_path 处理。
        if !cfg!(windows) {
            assert_eq!(filter_sub_path("/unix/path.srt"), "/unix/path.srt");
        }
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
