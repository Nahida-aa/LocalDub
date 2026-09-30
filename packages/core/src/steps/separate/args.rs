use serde::{Deserialize, Serialize};

use crate::steps::utils::paths::AudioSourcePath;
use crate::workflows::args::OutputMode;
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
/// 枚举/数组默认值 TS 在写入 state.json 前已落定 (zod `.prefault({})`), 这里只需处理
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
/// 与 `AsrOcrPreInput` 等兄弟 `*Input` 同款定位: 只声明**被消费的跨步骤产物**;
/// separate 直接对 import 落地的音频做分轨(原视频 `video_source.mp4` 仅作来源、
/// demucs 实际只吃 `audio_source.wav`, 故此处不把 video_source 列为依赖):
/// - `audio_source_path`: 原音频 `audio_source.wav`(import 产出), demucs 实际分轨的输入。
///
/// `output_mode` 也在此：它决定本步是"真分轨"还是"跳过"（subtitle + !always），
/// 属于本步的**决策输入**而非被消费的产物——跳过时本步不读任何上游文件。
///
/// 构造器统一为 [`new`](Self::new)`(video_dir, input)`——`video_dir` 给身份与写侧
/// 用，`input` 给本次运行参数([`SeparateArgs`])用；两参数取代原先的组合态 `&State`。
#[derive(Debug, Clone)]
pub struct SeparateInput {
    /// 任务目录（写侧 ready / step status 需要）。
    pub video_dir: String,
    pub audio_source_path: AudioSourcePath,
    /// 本次运行参数（`input.steps.separate`）。
    pub args: SeparateArgs,
    /// 任务模式（`input.workflow.output_mode`）：决定 subtitle + !always 的跳过分支。
    pub output_mode: OutputMode,
}

impl SeparateInput {
    pub fn new(video_dir: &str, input: &crate::input::Input) -> Result<Self> {
        Ok(Self {
            video_dir: video_dir.to_string(),
            audio_source_path: AudioSourcePath::from_video_dir(video_dir),
            args: SeparateArgs::from_input(input),
            output_mode: OutputMode::from_input(input),
        })
    }
}
