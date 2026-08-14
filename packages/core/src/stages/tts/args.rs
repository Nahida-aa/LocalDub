use serde::{Deserialize, Serialize};

/// TTS 运行时后端 (镜像 TS `packages/core/stages/07_tts/args.ts` TtsStageArgsSchema.runtime)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum TtsRuntime {
    #[default]
    Ggml,
    Cloud,
    VoxcpmTorchGradio,
}

/// TTS 计算设备 (镜像 TS `packages/core/stages/07_tts/args.ts` TtsStageArgsSchema.device)
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

/// tts 阶段参数 (镜像 TS `packages/core/stages/07_tts/args.ts` TtsStageArgsSchema)
///
/// 枚举/字符串默认值 TS 在写入 ctx.json 前已落定 (zod `.prefault({})` / `.default(...)`),
/// 这里只需处理「对象存在但字段缺」: 字段级 `#[serde(default…)]` 兜底即可。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
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
    /// 仅处理指定索引的 segment (其余跳过), 可用于精准重跑指定段
    #[serde(default)]
    pub only_indices: Option<Vec<u32>>,
    /// 将短参考音频 (< 2500ms) 拼接一倍再送 TTS, 帮助稳定输出音色; 默认 false
    #[serde(default)]
    pub ref_audio_x2: bool,
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
