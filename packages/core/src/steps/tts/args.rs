use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::steps::utils::paths::{split_audio, tts};

/// TTS 运行时后端 (镜像 TS `packages/core/steps/07_tts/args.ts` TtsStepArgsSchema.runtime)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum TtsRuntime {
    #[default]
    Ggml,
    Cloud,
    VoxcpmTorchGradio,
}

/// TTS 计算设备 (镜像 TS `packages/core/steps/07_tts/args.ts` TtsStepArgsSchema.device)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum TtsDevice {
    Webgpu,
    #[default]
    Cuda,
    Rocm,
    Cpu,
    Mps,
}

/// tts 阶段参数 (镜像 TS `packages/core/steps/07_tts/args.ts` TtsStepArgsSchema)
///
/// 枚举/字符串默认值 TS 在写入 ctx.json 前已落定 (zod `.prefault({})` / `.default(...)`),
/// 这里只需处理「对象存在但字段缺」: 字段级 `#[serde(default…)]` 兜底即可。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TtsArgs {
    /// 运行时后端; 默认 cloud
    #[serde(default = "default_runtime")]
    pub runtime: TtsRuntime,
    /// 计算设备; 默认 cuda
    #[serde(default = "default_device")]
    pub device: TtsDevice,
    /// 跳过已存在的段 (按 mtime 比对参考音); 默认 true
    #[serde(default = "default_true")]
    pub skip_existing: bool,
    /// continue 模式下强制重新生成的 segment 索引 (1-based); 列表外段保留旧结果。
    /// 命中段无视 skipExisting, 强制重合成 (先删旧 wav 再生成)。None/空 = 全量按 skipExisting 走。
    #[serde(default)]
    pub regen_indices: Option<Vec<u32>>,
    /// 将短参考音频 (< 2500ms) 拼接一倍再送 TTS, 帮助稳定输出音色; 默认 false
    #[serde(default)]
    pub ref_audio_x2: bool,
}

impl Default for TtsArgs {
    fn default() -> Self {
        Self {
            runtime: default_runtime(),
            device: default_device(),
            skip_existing: default_true(),
            regen_indices: None,
            ref_audio_x2: false,
        }
    }
}

impl TtsArgs {
    /// 从输入契约取本步参数：`input.steps.tts`。
    pub fn from_input(input: &crate::input::Input) -> Self {
        input.steps.tts.clone()
    }
}

fn default_true() -> bool {
    true
}

fn default_runtime() -> TtsRuntime {
    TtsRuntime::Cloud
}

fn default_device() -> TtsDevice {
    TtsDevice::Cuda
}

/// tts 步骤的输入契约：本步要读/写的全部路径。
///
/// 两个字段都是 `{video_dir}/split_audio/...` 的**约定拼法**，不读 ctx 里落定的
/// 事实（不像 `VideoSourcePath` / `LangPath` 那种），故入口是
/// [`from_video_dir`](Self::from_video_dir) 而不是 `from_state`——与
/// [`crate::steps::utils::paths::AudioSourcePath::from_video_dir`] 同款区分：
/// 名字本身就在说明「这是约定，不是事实」。
pub struct TtsInput {
    pub split_audio_vocals_dir: split_audio::VocalsDir,
    pub split_audio_timings_path: split_audio::TimingsPath,
    /// 本步产出：逐段配音 wav。
    pub tts_wavs_dir: tts::WavsDir,
    /// 本步中间态：短参考音翻倍产物（仅 `refAudioX2` 时用到）。
    pub ref_doubled_dir: tts::RefDoubledDir,
    /// 本步产物清单 `tts/tts.json`（regen 时也要读回）。
    pub tts_path: tts::TtsPath,
}

impl TtsInput {
    pub fn from_video_dir(video_dir: impl AsRef<Path>) -> Self {
        Self {
            split_audio_vocals_dir: split_audio::VocalsDir::new(&video_dir),
            split_audio_timings_path: split_audio::TimingsPath::new(&video_dir),
            tts_wavs_dir: tts::WavsDir::new(&video_dir),
            ref_doubled_dir: tts::RefDoubledDir::new(&video_dir),
            tts_path: tts::TtsPath::new(&video_dir),
        }
    }
}
