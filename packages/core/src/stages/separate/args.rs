use serde::{Deserialize, Serialize};

/// separate 推理运行时
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    Ggml,
    Ort,
    Burn,
    #[default]
    BurnTch,
}

/// 推理设备
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Device {
    Vulkan,
    Webgpu,
    #[default]
    Cuda,
    Cpu,
    Mps,
}

/// 可分离的 stem
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Stem {
    Drums,
    Bass,
    Other,
    Vocals,
}

/// separate 阶段参数 (镜像 TS `packages/core/stages/separate/args.ts` SeparateArgsSchema)
///
/// 枚举/数组默认值 TS 在写入 ctx.json 前已落定 (zod `.prefault({})`), 这里只需处理
/// 「对象存在但字段缺」: 字段级 `#[serde(default…)]` 兜底即可。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SeparateArgs {
    /// 推理运行时
    #[serde(default)]
    pub runtime: Runtime,
    /// torch:cuda (NVIDIA/ROCm), mps (Apple Silicon)
    #[serde(default)]
    pub device: Device,
    /// 效果(默认关闭): subtitle 模式下也始终分离人声, 保留 vocals 以便后续切换到 dub;
    /// dub 流程下始终需要分离人声以保证 tts-vc 的质量
    #[serde(default)]
    pub always: bool,
    /// 需分离的 stems; 暂不被消费
    #[serde(default = "default_stems")]
    pub stems: Vec<Stem>,
}

fn default_stems() -> Vec<Stem> {
    Vec::new()
}
