use serde::{Deserialize, Serialize};

use crate::steps::utils::paths::AudioSourcePath;
use crate::workflows::args::OutputMode;
use anyhow::Result;

/// separate 推理运行时
///
/// 四种运行时分属 vox-lab 的两个世代:
/// - `burn` / `burn-tch`: Burn 框架路径 (`demucs-burn-*` 二进制), 保留作性能基线;
/// - `tch` / `wgpu`: 独立运行时——`demucs-tch` 直连 tch-rs (不依赖 burn),
///   `demucs-wgpu` 走自研 WGSL/Vulkan kernel (不依赖 libtorch)。
///
/// kebab-case 序列化 (`BurnTch` → `"burn-tch"`, `Wgpu` → `"wgpu"`)。原先误用
/// `lowercase` 会得到 `"burntch"`, 故这里必须保持 kebab-case。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum SeparateRuntime {
    /// Burn + wgpu/vulkan/cuda/cpu 后端 (`demucs-burn-<dev>`)
    Burn,
    /// Burn + tch 后端 (`demucs-burn-tch`), 默认
    #[default]
    BurnTch,
    /// 独立 tch-rs 运行时 (`demucs-tch`, 按设备分 cpu/cuda 变体)
    Tch,
    /// 独立自研 WGSL/Vulkan 运行时 (`demucs-wgpu`)
    Wgpu,
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

/// demucs 模型变体
///
/// 取值与 demucs-burn 二进制的 `-m/--model` 参数一致 (见 demucs-core
/// `model/metadata.rs` 的 `ALL_MODELS`), 故直接透传给子进程即可, 无需二次映射。
/// 权重文件同名 `<id>.safetensors`, 存放在 `data/models/demucs/`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum DemucsModel {
    /// 标准 4 stems (84MB), 速度与质量平衡
    Htdemucs,
    /// 6 stems (84MB), 额外分出 guitar / piano
    #[serde(rename = "htdemucs_6s")]
    Htdemucs6s,
    /// fine-tuned 4 stems (333MB), 质量最好但推理更慢
    #[default]
    #[serde(rename = "htdemucs_ft")]
    HtdemucsFt,
}

impl DemucsModel {
    /// demucs-burn 的 model id, 同时是权重文件名主干 (`<id>.safetensors`)。
    pub fn as_id(self) -> &'static str {
        match self {
            DemucsModel::Htdemucs => "htdemucs",
            DemucsModel::Htdemucs6s => "htdemucs_6s",
            DemucsModel::HtdemucsFt => "htdemucs_ft",
        }
    }

    /// 该模型产出的 stem 数 (6s 多出 guitar / piano)。
    pub fn stem_count(self) -> usize {
        match self {
            DemucsModel::Htdemucs6s => 6,
            DemucsModel::Htdemucs | DemucsModel::HtdemucsFt => 4,
        }
    }
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
    /// demucs 模型变体, 透传为 demucs-burn 的 `-m <id>`; 默认 fine-tuned (沿用历史行为)
    #[serde(default)]
    pub model: DemucsModel,
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

/// separate 实际使用的二进制 + 其变体后缀。
///
/// runtime 决定二进制家族, device 在有多种构建的家族内挑变体:
/// - `Burn`    → `demucs-burn-<cpu|cuda|vulkan|wgpu>` (mps 复用 burn 的 cpu 降级路径)
/// - `BurnTch` → `demucs-burn-tch` (device 无关)
/// - `Tch`     → `demucs-tch-<cpu|cuda>` (tch-rs 直连; vulkan/webgpu/mps 回退 cpu)
/// - `Wgpu`    → `demucs-wgpu` (经系统 Vulkan, device 无关)
///
/// `suffix` 既是二进制名的后缀, 也是 `data/bin` 下的文件名主干;
/// `env_key` 对应 env 检查项, `None` 表示该组合没有发布资产。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemucsBinary {
    /// 完整二进制名, 如 `demucs-tch-cuda`。
    pub binary: String,
    /// name 后缀 / `data/bin` 文件名主干。
    pub suffix: &'static str,
    /// 对应 env 检查项的 key; `None` 表示该组合没有发布资产。
    pub env_key: Option<&'static str>,
}

impl SeparateRuntime {
    /// 结合 device 解析出要跑的 demucs 二进制。见 [`DemucsBinary`]。
    pub fn resolve(self, device: Device) -> DemucsBinary {
        match self {
            SeparateRuntime::Burn => {
                let suffix = match device {
                    Device::Cpu | Device::Mps => "cpu",
                    Device::Cuda => "cuda",
                    Device::Vulkan => "vulkan",
                    Device::Webgpu => "wgpu",
                };
                DemucsBinary {
                    binary: format!("demucs-burn-{suffix}"),
                    suffix,
                    env_key: match suffix {
                        "wgpu" => Some("demucs_burn_wgpu_bin"),
                        _ => None,
                    },
                }
            }
            SeparateRuntime::BurnTch => DemucsBinary {
                binary: "demucs-burn-tch".into(),
                suffix: "tch",
                env_key: Some("demucs_burn_tch_bin"),
            },
            SeparateRuntime::Tch => {
                let suffix = match device {
                    Device::Cuda => "cuda",
                    Device::Cpu | Device::Mps | Device::Vulkan | Device::Webgpu => "cpu",
                };
                DemucsBinary {
                    binary: format!("demucs-tch-{suffix}"),
                    suffix,
                    env_key: Some(match suffix {
                        "cuda" => "demucs_tch_cuda_bin",
                        _ => "demucs_tch_cpu_bin",
                    }),
                }
            }
            SeparateRuntime::Wgpu => DemucsBinary {
                binary: "demucs-wgpu".into(),
                suffix: "wgpu",
                env_key: Some("demucs_wgpu_bin"),
            },
        }
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
