use serde::{Deserialize, Serialize};

use crate::stages::mix_audio::args::MixAudioArgs;
use crate::stages::mix_video::args::MixVideoArgs;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    Ggml,
    FasterWhisper,
    Pytorch,
}

impl Default for Runtime {
    fn default() -> Self {
        Self::Pytorch
    }
}

/// ASR 音频源: vocals=纯分离人声, raw-sum=直接叠加, sidechain=侧链压缩背景音
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum MixMode {
    Vocals,
    #[serde(rename = "raw-sum")]
    RawSum,
    Sidechain,
}

impl Default for MixMode {
    fn default() -> Self {
        Self::Sidechain
    }
}

/// 侧链压缩器参数
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SidechainCompress {
    /// 压缩器阈值, 默认 0.1
    #[serde(default)]
    pub threshold: f64,
    /// 压缩比, 默认 20
    #[serde(default)]
    pub ratio: f64,
}

impl Default for SidechainCompress {
    fn default() -> Self {
        Self {
            threshold: 0.1,
            ratio: 20.0,
        }
    }
}

/// asr: 语音转写
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Asr {
    /// 推理运行时, 默认 pytorch
    #[serde(default)]
    pub runtime: Runtime,
    /// ASR 音频源 (默认 sidechain)
    #[serde(default)]
    pub mix_mode: MixMode,
    /// 背景音降低量(dB), raw-sum 叠加前衰减
    #[serde(default = "default_reduce_bgm")]
    pub reduce_bgm: f64,
    /// ASR 结果是否包含词级时间戳 (默认关闭, 调试时开启)
    #[serde(default)]
    pub words_output: bool,
    /// mixMode=sidechain 时压缩器参数
    #[serde(default)]
    pub sidechain_compress: SidechainCompress,
}

impl Default for Asr {
    fn default() -> Self {
        Self {
            runtime: Runtime::default(),
            mix_mode: MixMode::default(),
            reduce_bgm: -12.0,
            words_output: false,
            sidechain_compress: SidechainCompress::default(),
        }
    }
}

/// 与 TS `reduceBgm.default(-12)` 对应，避免 `#[serde(default)]` 填成 0.0
fn default_reduce_bgm() -> f64 {
    -12.0
}

/// 各处理阶段的入参 (镜像 TS `packages/core/input/types.ts` StagesSchema)
///
/// mix_audio / mix_video 直接使用 `crate::stages::{mix_audio,mix_video}::args` 的
/// 完整定义，与 TS `MixAudioArgsSchema` / `MixVideoArgsSchema` 对齐。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Stages {
    #[serde(default)]
    pub asr: Asr,
    #[serde(default)]
    pub mix_audio: MixAudioArgs,
    #[serde(default)]
    pub mix_video: MixVideoArgs,
}

impl Default for Stages {
    fn default() -> Self {
        Self {
            asr: Asr::default(),
            mix_audio: MixAudioArgs::default(),
            mix_video: MixVideoArgs::default(),
        }
    }
}
