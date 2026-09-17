use serde::{Deserialize, Serialize};

use crate::state::WorkflowState;
use crate::steps::utils::paths::{AudioSourcePath, VideoSourcePath};
use anyhow::Result;

/// separate 推理运行时
///
/// TS 侧为 `z.enum(["burn", "burn-tch"])` (kebab-case), 故用 `rename_all = "kebab-case"`
/// (`BurnTch` → `"burn-tch"`)。原先误用 `lowercase` 会得到 `"burntch"`, 与 TS 对不上,
/// 导致运行时 `"burn-tch"` 解析失败回落默认 (see `from_input_parses_camel_case_fields`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum SeparateRuntime {
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

/// separate 阶段参数 (镜像 TS `packages/core/steps/separate/args.ts` SeparateArgsSchema)
///
/// 枚举/数组默认值 TS 在写入 ctx.json 前已落定 (zod `.prefault({})`), 这里只需处理
/// 「对象存在但字段缺」: 字段级 `#[serde(default…)]` 兜底即可。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SeparateArgs {
    /// 推理运行时
    #[serde(default)]
    pub runtime: SeparateRuntime,
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

impl SeparateArgs {
    /// 从输入契约取本步参数：`input.steps.separate`。
    pub fn from_input(input: &crate::input::Input) -> Self {
        input.steps.separate.clone()
    }
}

fn default_stems() -> Vec<Stem> {
    Vec::new()
}

/// separate 步骤的跨步骤依赖清单(代码即 DAG 视角)。
///
/// 与 `AsrOcrPreInput` 等兄弟 `*Input` 同款定位: 只声明**被消费的跨步骤产物**,
/// 自身输出(stems)、`*Args::from_input` 另行获取。separate 直接对 import 落地的
/// 音视频做分轨, 两条输入都是 import 写进 ctx 的事实:
/// - `video_source_path`: 原视频 `video_source.mp4`(import 产出);
/// - `audio_source_path`: 原音频 `audio_source.wav`(import 产出), demucs 实际分轨的输入。
pub struct SeparateInput {
    pub video_source_path: VideoSourcePath,
    pub audio_source_path: AudioSourcePath,
}

impl SeparateInput {
    /// 两条输入均为 import 写进 ctx 的事实, 故走 `from_state`(与 `AsrOcrPreInput` 同款)。
    pub fn from_state(state: &WorkflowState) -> Result<Self> {
        Ok(Self {
            video_source_path: VideoSourcePath::from_state(state)?,
            audio_source_path: AudioSourcePath::from_state(state)?,
        })
    }
}
