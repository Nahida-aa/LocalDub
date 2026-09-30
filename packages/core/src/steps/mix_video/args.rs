use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::steps::utils::paths::{
    mix_audio, separate_after, split_audio, translate, VideoSourcePath,
};
use crate::workflows::args::OutputMode;

/// 字幕对齐位置 (镜像 TS `packages/core/steps/mix_video/args.ts` alignmentList)
///
/// 顺序即 ffmpeg ass `Alignment` 数值 (1..9), 见 [`alignment_to_ffmpeg`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum Alignment {
    BottomLeft,
    #[default]
    BottomCenter,
    BottomRight,
    MiddleLeft,
    Center,
    MiddleRight,
    TopLeft,
    TopCenter,
    TopRight,
}

/// 对齐位置 → ffmpeg ass `Alignment` 数值 (1..9)
pub fn alignment_to_ffmpeg(alignment: Alignment) -> u8 {
    match alignment {
        Alignment::BottomLeft => 1,
        Alignment::BottomCenter => 2,
        Alignment::BottomRight => 3,
        Alignment::MiddleLeft => 4,
        Alignment::Center => 5,
        Alignment::MiddleRight => 6,
        Alignment::TopLeft => 7,
        Alignment::TopCenter => 8,
        Alignment::TopRight => 9,
    }
}

/// mix_video 阶段参数 (镜像 TS `packages/core/steps/mix_video/args.ts` MixVideoArgsSchema)
///
/// 枚举/字符串默认值 TS 在写入 state.json 前已落定 (zod `.prefault({})` / `.default(...)`),
/// 这里只需处理「对象存在但字段缺」: 字段级 `#[serde(default…)]` 兜底即可。
///
/// 注意: `#[serde(default = "fn")]` 仅在字段级反序列化时生效; 当父结构用
/// `#[serde(default)]` 整体缺省时会调用 Rust `Default`, 故这里手写 `impl Default`
/// 以保证两种路径下默认值一致 (与 `input::steps::Asr` 同款处理)。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MixVideoArgs {
    /// 字幕字号, 不填则自动: 竖屏 12(zh)/9(其他) ← 横屏 24(zh)/18(其他)
    ///
    /// 镜像 TS `z.number()` (允许小数, 与 ASS `FontSize` 一致)。
    #[serde(default)]
    pub font_size: Option<f64>,
    /// 垂直边距(像素), 不填则自动: 竖屏 70 / 横屏 5
    ///
    /// 镜像 TS `z.number().min(0)` (允许小数)。
    #[serde(default)]
    pub margin_v: Option<f64>,
    /// 对齐位置; 默认 bottom-center
    #[serde(default)]
    pub alignment: Option<Alignment>,
    /// 描边宽度; 默认 0
    ///
    /// 镜像 TS `z.number().min(0).default(0)` (允许小数)。
    #[serde(default = "default_outline")]
    pub outline: f64,
    /// 阴影宽度; 默认 1
    ///
    /// 镜像 TS `z.number().min(0).default(1)` (允许小数)。
    #[serde(default = "default_shadow")]
    pub shadow: f64,
    /// ASS 字幕字体名 (须系统已安装), 默认 Noto Sans CJK SC
    #[serde(default)]
    pub font: Option<String>,
    /// 调试使用: 外部 srt 路径
    #[serde(default)]
    pub srt_path: Option<String>,
    /// 调试使用: 外部 bgm 路径
    #[serde(default)]
    pub bgm_path: Option<String>,
    /// 背景音乐增益(dB), 0=不变, 负值=衰减; 默认 -6
    #[serde(default = "default_bgm_gain")]
    pub bgm_gain: f64,
    /// 配音增益(dB), 补偿合成语音偏小的听感差; 默认 3
    #[serde(default = "default_dub_gain")]
    pub dub_gain: f64,
    /// 是否启用本阶段 (缺省 true; 设为 false 可跳过 mix_video)
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl Default for MixVideoArgs {
    fn default() -> Self {
        Self {
            font_size: None,
            margin_v: None,
            alignment: None,
            outline: default_outline(),
            shadow: default_shadow(),
            font: None,
            srt_path: None,
            bgm_path: None,
            bgm_gain: default_bgm_gain(),
            dub_gain: default_dub_gain(),
            enabled: default_enabled(),
        }
    }
}

impl MixVideoArgs {
    /// 从输入契约取本步参数：`input.steps.mix_video`。
    pub fn from_input(input: &crate::input::Input) -> Self {
        input.steps.mix_video.clone()
    }
}

fn default_outline() -> f64 {
    0.0
}

fn default_shadow() -> f64 {
    1.0
}

fn default_bgm_gain() -> f64 {
    -6.0
}

fn default_dub_gain() -> f64 {
    3.0
}

/// dub 分支的背景音来源。
///
/// | 变体 | 谁用 |
/// | --- | --- |
/// | [`Separate`](Self::Separate) | 默认：`separate_after/target_bgm.wav`（demucs 分离产物） |
/// | [`Custom`](Self::Custom) | `mix_video.bgmPath` 调试覆盖指定的任意路径 |
pub enum BgmPath {
    Separate(separate_after::TargetBgmPath),
    Custom(PathBuf),
}

impl BgmPath {
    pub fn as_path(&self) -> &std::path::Path {
        match self {
            Self::Separate(p) => p.as_path(),
            Self::Custom(p) => p.as_path(),
        }
    }
}

/// mix_video 的两条分支**各自需要的路径**。
///
/// 与 `AsrInput` 的 `AsrSourceAudioPath` 同款：分支不同 → 读集不同，用枚举表达，
/// 让 step 体的 `match` 是穷尽的。共享部分（身份 / 语言 / 参数）在
/// [`MixVideoInput`] 里，不放进枚举——否则两条分支各存一份、还要写访问器。
pub enum MixVideoBranch {
    Dub {
        mix_audio_timings_path: mix_audio::TimingsPath,
        dubbing_path: mix_audio::DubbingPath,
        bgm_path: BgmPath,
    },
    Subtitle {
        split_audio_timings_path: split_audio::TimingsPath,
    },
}

/// mix_video 步骤的输入契约。
pub struct MixVideoInput {
    /// 任务目录（写侧 step status 与 `<mix_video>/out.json` 需要）。
    pub video_dir: String,
    /// 任务 id（成片文件名用它）——与 [`crate::steps::utils::video_id`] 同源。
    pub video_id: String,
    pub video_source_path: VideoSourcePath,
    /// 任务模式（决定走哪条分支、成片目录名）。
    pub output_mode: OutputMode,
    /// 目标语言（`<translate>/out.json` 的**落定事实**，不现算）。
    pub target_lang: String,
    /// `!input.steps.translate.enabled`（直通）：成片目录名的一部分。
    pub no_translate: bool,
    /// 字幕来源（`input.workflow.subtitleSource`）：成片目录名的一部分。
    pub subtitle_source: String,
    /// 本次运行参数（`input.steps.mix_video`）。
    pub args: MixVideoArgs,
    /// 分支专属的读集。
    pub branch: MixVideoBranch,
}

impl MixVideoInput {
    /// 分支由 `OutputMode` 决定，两条分支**需要的输入不同**：
    /// Dub 要配音轨 + BGM + mix_audio 对齐时序；Subtitle 只要 split_audio 意图时序。
    ///
    /// 路径全部交给 `paths` 里各类型的构造函数，本函数不自己拼字符串；
    /// `video_source_path` 走 [`VideoSourcePath::from_video_dir`] 的约定拼法。
    pub fn new(video_dir: &str, input: &crate::input::Input) -> anyhow::Result<Self> {
        let args = MixVideoArgs::from_input(input);
        let video_source_path = VideoSourcePath::from_video_dir(video_dir);
        let branch = match OutputMode::from_input(input) {
            OutputMode::Dub => {
                let bgm_path = match &args.bgm_path {
                    Some(p) => BgmPath::Custom(PathBuf::from(p)),
                    None => BgmPath::Separate(separate_after::TargetBgmPath::new(video_dir)),
                };
                MixVideoBranch::Dub {
                    mix_audio_timings_path: mix_audio::TimingsPath::new(video_dir),
                    dubbing_path: mix_audio::DubbingPath::new(video_dir),
                    bgm_path,
                }
            }
            OutputMode::Subtitle => MixVideoBranch::Subtitle {
                split_audio_timings_path: split_audio::TimingsPath::new(video_dir),
            },
        };
        Ok(Self {
            video_dir: video_dir.to_string(),
            video_id: crate::steps::utils::video_id(video_dir).unwrap_or_default(),
            video_source_path,
            output_mode: OutputMode::from_input(input),
            target_lang: translate::LangPath::from_video_dir(video_dir)?
                .lang()
                .to_string(),
            no_translate: !input.steps.translate.enabled,
            subtitle_source: input
                .workflow
                .as_ref()
                .map(|w| w.subtitle_source.as_str())
                .unwrap_or("asr")
                .to_string(),
            args,
            branch,
        })
    }
}
