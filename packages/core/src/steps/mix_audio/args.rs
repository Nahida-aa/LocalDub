use serde::{Deserialize, Serialize};

use crate::steps::split_audio::out::SplitAudioTimingResult;
use crate::steps::utils::paths::{split_audio, tts};

/// mix_audio 阶段参数 (镜像 TS `packages/core/steps/mix_audio/args.ts` MixAudioArgsSchema)
///
/// 枚举/字符串默认值 TS 在写入 state.json 前已落定 (zod `.prefault({})` / `.default(...)`),
/// 这里只需处理「对象存在但字段缺」: 字段级 `#[serde(default…)]` 兜底即可。
///
/// 注意: `#[serde(default = "fn")]` 仅在字段级反序列化时生效; 当父结构用
/// `#[serde(default)]` 整体缺省时会调用 Rust `Default`, 故这里手写 `impl Default`
/// 以保证两种路径下默认值一致 (与 `input::steps::Asr` 同款处理)。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MixAudioArgs {
    /// TTS 音频最大变速比, 1.0=不变速; 默认 1.35
    #[serde(default = "default_max_speed")]
    pub max_speed: f64,
    /// 字幕允许提前显示的最大毫秒数, 利用前段剩余时间; 默认 500
    #[serde(default = "default_max_advance_ms")]
    pub max_advance_ms: f64,
    /// 字幕允许延迟显示的最大毫秒数, 借用后段留白; 默认 500
    #[serde(default = "default_max_delay_ms")]
    pub max_delay_ms: f64,
}

impl Default for MixAudioArgs {
    fn default() -> Self {
        Self {
            max_speed: default_max_speed(),
            max_advance_ms: default_max_advance_ms(),
            max_delay_ms: default_max_delay_ms(),
        }
    }
}

impl MixAudioArgs {
    /// 从输入契约取本步参数：`input.steps.mix_audio`。
    pub fn from_input(input: &crate::input::Input) -> Self {
        input.steps.mix_audio.clone()
    }
}

fn default_max_speed() -> f64 {
    1.35
}

fn default_max_advance_ms() -> f64 {
    500.0
}

fn default_max_delay_ms() -> f64 {
    500.0
}

pub struct MixAudioInput {
    /// 任务目录（写侧 step status 需要）。
    pub video_dir: String,
    pub split_audio_timings_path: split_audio::TimingsPath,
    pub tts_wavs_dir: tts::WavsDir,
    /// 本次运行参数（`input.steps.mix_audio`）。
    pub args: MixAudioArgs,
}

impl MixAudioInput {
    /// mix_audio 阶段的两个产物依赖：split_audio 意图时序 + TTS 各段 wav 目录。
    ///
    /// 顺便做前向校验：时序文件存在且非空、每段 TTS 文件存在。这样 step 主体不必
    /// 再手写「文件不存在」判断——失败得早、错得清楚（与 `SplitAudioInput::new`
    /// 同款思路：输入契约在入口一次性查干净）。
    pub fn new(video_dir: &str, input: &crate::input::Input) -> anyhow::Result<Self> {
        let split_audio_timings_path = split_audio::TimingsPath::new(video_dir);
        let tts_wavs_dir = tts::WavsDir::new(video_dir);

        let timings = SplitAudioTimingResult::read(&split_audio_timings_path)?;
        if timings.segments.is_empty() {
            return Err(anyhow::anyhow!(
                "{} 无 segments",
                split_audio_timings_path.as_path().display()
            ));
        }
        for (i, seg) in timings.segments.iter().enumerate() {
            let f = tts_wavs_dir.segment_path(i);
            if !f.exists() {
                return Err(anyhow::anyhow!("缺少 TTS 段 #{}: {}", seg.idx, f.display()));
            }
        }
        Ok(Self {
            video_dir: video_dir.to_string(),
            split_audio_timings_path,
            tts_wavs_dir,
            args: MixAudioArgs::from_input(input),
        })
    }
}
