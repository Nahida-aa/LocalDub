use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::steps::utils::paths::{separate, translate};

/// split_audio 阶段参数 (镜像 TS `packages/core/steps/06_split_audio/args.ts` SplitAudioArgsSchema)
///
/// 默认值带业务含义 (startPadMs=100 / endPadMs=300)。TS 端 zod `.prefault({})`
/// 在写入 state.json 前已落定默认值, 因此这里只需处理「对象存在但字段缺」:
/// 用 `#[serde(default = "…")]` 兜底。
///
/// 注意: `#[serde(default = "fn")]` 仅在字段级反序列化时生效; 当父结构用
/// `#[serde(default)]` 整体缺省时会调用 Rust `Default`, 故手写 `impl Default`
/// 以保证两种路径下默认值一致 (vadAlign=false / startPadMs=100 / endPadMs=300)。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SplitAudioArgs {
    /// 是否启用静音检测对齐: 修正 segments 前后静音导致的偏移
    #[serde(default)]
    pub vad_align: bool,
    /// 段落切块前缘 padding (ms), 避免语音被截断
    #[serde(default = "default_start_pad_ms")]
    pub start_pad_ms: u32,
    /// 段落切块后缘 padding (ms), 避免语音被截断
    #[serde(default = "default_end_pad_ms")]
    pub end_pad_ms: u32,
    /// 人声文件路径, 调试使用（作为 [`crate::steps::split_audio::SplitAudioSourcePath::Custom]）
    pub vocals_file_path: Option<String>,
}

impl Default for SplitAudioArgs {
    fn default() -> Self {
        Self {
            vad_align: false,
            start_pad_ms: default_start_pad_ms(),
            end_pad_ms: default_end_pad_ms(),
            vocals_file_path: None,
        }
    }
}

impl SplitAudioArgs {
    /// 从输入契约取本步参数：`input.steps.split_audio`。
    ///
    /// 与 `WorkflowState` 无关——只读 `Input`，故 `from_input` 直接吃 `&Input`。
    /// 因 [`Steps`](crate::input::Steps) 里该字段**就是** `SplitAudioArgs`，
    /// 这里是 `clone` 而非转换；存在的意义是给本类型一个与
    /// [`OutputMode::from_input`](crate::workflows::args::OutputMode::from_input)
    /// 一致的构造入口（读输入契约，而非 import 落定的事实）。
    pub fn from_input(input: &crate::input::Input) -> Self {
        input.steps.split_audio.clone()
    }
}

fn default_start_pad_ms() -> u32 {
    100
}

fn default_end_pad_ms() -> u32 {
    300
}

/// split_audio 的**输入契约**：切块要用的音频源 + 字幕时序来源 + 切块参数。
///
/// 消费者只需读这里，不必再自己去 ctx 里拼路径 / 判断文件存在性。
#[derive(Debug, Clone)]
pub struct SplitAudioInput {
    /// 任务目录（写侧 step status 需要）。
    pub video_dir: String,
    /// 切块时要切哪个音频。
    pub audio_path: SplitAudioSourcePath,
    /// 字幕时序来源（意图时序 + 文本 + 译文）。
    pub translate_path: translate::LangPath,
    /// 本次运行参数（`input.steps.split_audio`）。
    pub args: SplitAudioArgs,
    /// 源语言（`<asr>/out.json` > `input.workflow.sourceLang` > 未知）：写 meta 用。
    pub source_lang: crate::r#const::lang::LangCode,
    /// 是否 dub 流程（`input.workflow.output_mode`）：决定切不切 wav 块。
    pub is_dub: bool,
}

/// 切块用的音频源（三选一）。
///
/// **顺序即优先级**（对应 `SplitAudioArgs` 的既有逻辑）：
/// 1. [`Custom`](Self::Custom) —— `splitAudio.vocalsFilePath` 显式指定
/// 2. [`Vocals`](Self::Vocals) —— `separate/target_3_vocals.wav` 人声 stem
/// 3. [`SourceAudio`](Self::SourceAudio) —— `audio_source.wav` 原始音频
///
/// **为什么 `Vocals` 是「可能不存在」的**：subtitle 模式不做 separate，没有
/// 人声 stem，此时回退到原始音频是**正常路径**而非错误。所以存在性检查留在
/// 选择逻辑里（[`SplitAudioInput::audio_source`]），不进类型。
///
/// **历史说明（已核实：不构成产物差异）**：`step_split_audio` 旧逻辑的 fallback 是
/// `video_source_path(ctx)`（`video_source.mp4`）。但 `audio_source.wav` 本就是
/// import 阶段 `extract_audio(video_source.mp4)` 产出的同名音轨
/// （pcm_s16le / 44100 / 2ch，见 `workflows/import/download.rs`），两者是同一段音频。
/// 且旧代码里 `cut_audio_range`（`-c copy`）只在 `has_vocals` 时调用，彼时
/// `source_audio` 已被改写为 `separate/target_3_vocals.wav`——
/// `video_source.mp4` 实际只喂给 `probe_duration_ms`，其返回时长与 wav 一致。
/// 故本契约的 `SourceAudio` 与旧 fallback **产物相同**，无需实测比对。
///
/// **`SplitAudioArgs` 已移除 `source_file_path`**：旧逻辑里它被 vocals 检查遮蔽，
/// 无法真正作为「从任意源音频切块」的覆盖（仅在「无 vocals + vad_align」窄路径影响
/// VAD 参考音频），且与 `vocals_file_path` 概念混淆。调试自定义切块源统一走
/// `vocals_file_path` → [`crate::steps::split_audio::SplitAudioSourcePath::Custom`]。
#[derive(Debug, Clone)]
pub enum SplitAudioSourcePath {
    /// 人声 stem `separate/target_3_vocals.wav`。
    ///
    /// 注意是 **`separate/`**（demucs 原始人声），不是 `separate_after/` 的
    /// 混音版——`separate_after` 那个是给 asr 用的。
    Vocals(crate::steps::utils::paths::separate::Target3VocalsPath),
    /// 原始音频 `audio_source.wav`。
    SourceAudio(crate::steps::utils::paths::AudioSourcePath),
    /// 调试用：`splitAudio.vocalsFilePath` 指定的任意路径。
    Custom(PathBuf),
}

impl SplitAudioInput {
    /// 音乐源解析：`(video_dir, input)` → 先定音频源（按 `SplitAudioSourcePath`
    /// 的优先级），再取翻译产物路径（`<translate>/out.json`）。
    ///
    /// **优先级与存在性检查的现状对齐**（`step_split_audio` 原有逻辑）：
    ///
    /// 1. `splitAudio.vocalsFilePath` 显式指定 → [`Custom`](SplitAudioSourcePath::Custom)
    /// 2. `separate/target_3_vocals.wav` **存在** → [`Vocals`](SplitAudioSourcePath::Vocals)
    /// 3. 否则 → [`SourceAudio`](SplitAudioSourcePath::SourceAudio)（`audio_source.wav`）
    ///
    /// 第 2 步的「存在才用」是刻意的：subtitle 模式不做 separate，没有人声 stem，
    /// 回退到原始音频是**正常路径**。故存在性判断留在这里，类型只表达结论。
    ///
    /// `translate` 没跑过时 `<translate>/out.json` 不存在 → Err（无从知道该读哪个文件）。
    pub fn new(video_dir: &str, input: &crate::input::Input) -> anyhow::Result<Self> {
        let args = SplitAudioArgs::from_input(input);
        Ok(Self {
            video_dir: video_dir.to_string(),
            audio_path: Self::audio_source(video_dir, &args),
            translate_path: translate::LangPath::from_video_dir(video_dir)?,
            args,
            source_lang: crate::steps::utils::src_language(video_dir, input),
            is_dub: crate::workflows::args::OutputMode::from_input(input)
                == crate::workflows::args::OutputMode::Dub,
        })
    }

    /// 只解析**切块音频源**（优先级见 [`SplitAudioSourcePath`]）。
    ///
    /// 拆出来是给「只要音频源、不要 translate 路径」的消费者用——它不必为了
    /// 拿 audio 而顺带构造一个用不到的 `LangPath`（后者会在 translate 没跑过时
    /// 报错，把与该消费者无关的失败条件引进来）。
    pub fn audio_source(video_dir: &str, args: &SplitAudioArgs) -> SplitAudioSourcePath {
        match &args.vocals_file_path {
            Some(p) => SplitAudioSourcePath::Custom(PathBuf::from(p)),
            None => {
                let vocals = separate::Target3VocalsPath::new(video_dir);

                if vocals.exists() {
                    SplitAudioSourcePath::Vocals(vocals)
                } else {
                    SplitAudioSourcePath::SourceAudio(
                        crate::steps::utils::paths::AudioSourcePath::from_video_dir(video_dir),
                    )
                }
            }
        }
    }
}

impl SplitAudioSourcePath {
    /// 实际要切的那个文件。
    pub fn as_path(&self) -> &std::path::Path {
        match self {
            SplitAudioSourcePath::Vocals(p) => p.as_path(),
            SplitAudioSourcePath::SourceAudio(p) => p.as_path(),
            SplitAudioSourcePath::Custom(p) => p.as_path(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 只验证**音频源优先级**的测试不需要 translate 产物，故走
    /// [`SplitAudioInput::audio_source`] 而不构造完整的 `SplitAudioInput`
    /// （后者要求 `<translate>/out.json` 已存在）。
    fn audio_of(dir: &str, split_audio_cfg: serde_json::Value) -> SplitAudioSourcePath {
        let input: crate::input::Input =
            serde_json::from_value(serde_json::json!({"steps": {"split_audio": split_audio_cfg}}))
                .unwrap();
        SplitAudioInput::audio_source(dir, &SplitAudioArgs::from_input(&input))
    }

    /// 优先级 1：`vocalsFilePath` 显式指定 → 用它，**无视其它**。
    ///
    /// 即使 `separate/target_3_vocals.wav` 存在（真实文件），也该走 Custom。
    #[test]
    fn explicit_vocals_file_path_wins() {
        let dir = std::env::temp_dir()
            .join(format!("ld_sa_custom_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::create_dir_all(&dir);

        let audio_path = audio_of(
            &dir,
            serde_json::json!({"vocalsFilePath": "/tmp/my_vocals.wav"}),
        );
        assert!(matches!(audio_path, SplitAudioSourcePath::Custom(_)));
        assert_eq!(audio_path.as_path().to_string_lossy(), "/tmp/my_vocals.wav");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 优先级 2：`separate/target_3_vocals.wav` **存在** → 用人声 stem。
    #[test]
    fn existing_vocals_stem_is_preferred() {
        let dir = std::env::temp_dir()
            .join(format!("ld_sa_vocals_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        // 造出 separate/target_3_vocals.wav
        std::fs::create_dir_all(std::path::Path::new(&dir).join("separate")).unwrap();
        std::fs::write(
            std::path::Path::new(&dir)
                .join("separate")
                .join("target_3_vocals.wav"),
            b"x",
        )
        .unwrap();

        let audio_path = audio_of(&dir, serde_json::json!({}));
        assert!(
            matches!(audio_path, SplitAudioSourcePath::Vocals(_)),
            "存在 vocals 时应优先用它, 实际 {:?}",
            audio_path.as_path()
        );
        // 必须是 separate/（原始人声），不是 separate_after/
        assert!(
            audio_path
                .as_path()
                .to_string_lossy()
                .contains("/separate/"),
            "该用 separate/ 的原始人声: {:?}",
            audio_path.as_path()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 优先级 3：没有 vocals stem → 回退 `audio_source.wav`。
    ///
    /// 这条是 subtitle 模式的**正常路径**（不做 separate），不是错误。
    #[test]
    fn falls_back_to_audio_source_when_no_vocals() {
        let dir = std::env::temp_dir()
            .join(format!("ld_sa_fallback_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let audio_path = audio_of(&dir, serde_json::json!({}));
        assert!(
            matches!(audio_path, SplitAudioSourcePath::SourceAudio(_)),
            "无 vocals 应回退音频源, 实际 {:?}",
            audio_path.as_path()
        );
        // 约定拼法：`{video_dir}/audio_source.wav`
        assert_eq!(
            audio_path.as_path().to_string_lossy(),
            format!("{dir}/audio_source.wav")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `audio_source.wav` 现在是约定拼法（`{video_dir}/audio_source.wav`），
    /// 不再有「没填路径」这种状态——缺文件是 produce 侧的事，不是契约构造失败。
    /// 契约构造唯一的失败条件是 `translate` 没跑过（`<translate>/out.json` 缺失）。
    #[test]
    fn missing_translate_output_is_error() {
        let dir = std::env::temp_dir()
            .join(format!("ld_sa_missing_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let input: crate::input::Input = serde_json::from_value(serde_json::json!({})).unwrap();
        let err = SplitAudioInput::new(&dir, &input).expect_err("translate 没跑过应报错");
        assert!(
            err.to_string().contains("translate"),
            "错误应点名缺失的 translate 产物: {err}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
