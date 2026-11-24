use serde::{Deserialize, Serialize};

use crate::steps::utils::paths::split_audio;

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
/// 枚举/字符串默认值 TS 在写入 state.json 前已落定 (zod `.prefault({})` / `.default(...)`),
/// 这里只需处理「对象存在但字段缺」: 字段级 `#[serde(default…)]` 兜底即可。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
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
    /// resume 模式下强制重新生成的 segment 索引 (1-based); 列表外段保留旧结果。
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

/// tts 步骤的输入契约（**依赖清单**）：本步**消费**的路径。
///
/// Phase 1 事实面：`step_tts` 的显式依赖清单（由装配层算好后注入）。
///
/// 只装依赖——`split_audio` 步骤产出的两个产物（见各字段注释的来源）。本步的
/// **产出**（逐段配音 wav / 翻倍参考音 / `tts.json`）属于「产出」而非「依赖」，
/// 由步骤体自行构造。
///
/// 意图面（`video_dir` / `args` / `is_start`）走 `sc.ctx().input()`，不进 deps。
#[derive(Debug, Clone)]
pub struct TtsDeps {
    /// 来自 split_audio 的人声目录 `{video_dir}/split_audio/vocals`（参考音来源）。
    pub split_audio_vocals_dir: split_audio::VocalsDir,
    /// 来自 split_audio 的意图时序 `{video_dir}/split_audio/timings.json`（带类型读入）。
    pub split_audio_timings_path: split_audio::TimingsPath,
}

impl TtsDeps {
    pub fn from_input(input: &crate::input::WorkflowInput) -> Self {
        let video_dir = input.video_dir();
        Self {
            split_audio_vocals_dir: split_audio::VocalsDir::new(video_dir),
            split_audio_timings_path: split_audio::TimingsPath::new(video_dir),
        }
    }
}
