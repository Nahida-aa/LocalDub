//! 用 specta + specta-jsonschema 生成 CLI input 的 JSON Schema。
//!
//! 对标 TS 侧 `packages/cli/scripts/gen-input-schema.ts`（zod `toJSONSchema({io:'input'})`）。
//!
//! 这里仅演示 input 语义：输入 JSON 允许缺省字段（注释里的 *可缺省* 字段进不了 `required`），
//! 但读到 Rust 运行时由 `#[serde(default)]` 补齐默认值。specta 没有 `Ranged`/min/max 约束，
//! 数值范围需在解析时自行校验（见下文 TODO）。

use serde::{Deserialize, Serialize};
use specta::Type;
use specta::Types;
use specta_jsonschema::JsonSchema;
use specta_serde::Format;

// ---- 镜像 packages/core/input/types.ts 的子集（command + stages.asr + merge_audio） ----

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
enum Command {
    Task,
    Env,
    Servers,
}

impl Default for Command {
    fn default() -> Self {
        Self::Env
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
enum Runtime {
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
enum MixMode {
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
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
struct SidechainCompress {
    /// 压缩器阈值, 默认 0.1
    #[serde(default)]
    #[specta(optional)]
    threshold: f64,
    /// 压缩比, 默认 20
    #[serde(default)]
    #[specta(optional)]
    ratio: f64,
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
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
struct Asr {
    /// 推理运行时, 默认 pytorch
    #[serde(default)]
    #[specta(optional)]
    runtime: Runtime,
    /// ASR 音频源 (默认 sidechain)
    #[serde(default)]
    #[specta(optional)]
    mix_mode: MixMode,
    /// 背景音降低量(dB), raw-sum 叠加前衰减
    #[serde(default = "default_reduce_bgm")]
    #[specta(optional)]
    reduce_bgm: f64,
    /// ASR 结果是否包含词级时间戳 (默认关闭, 调试时开启)
    #[serde(default)]
    #[specta(optional)]
    words_output: bool,
    /// mixMode=sidechain 时压缩器参数
    #[serde(default)]
    #[specta(optional)]
    sidechain_compress: SidechainCompress,
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

/// merge_audio 参数
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
struct MergeAudio {
    /// 最大变速比, 1.0=不变速
    #[serde(default = "default_max_speed")]
    #[specta(optional)]
    max_speed: f64,
}

fn default_max_speed() -> f64 {
    1.35
}

/// 与 TS `reduceBgm.default(-12)` 对应，避免 `#[serde(default)]` 填成 0.0
fn default_reduce_bgm() -> f64 {
    -12.0
}

impl Default for MergeAudio {
    fn default() -> Self {
        Self {
            max_speed: default_max_speed(),
        }
    }
}

/// 各处理阶段的入参
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
struct Stages {
    #[serde(default)]
    #[specta(optional)]
    asr: Asr,
    #[serde(default)]
    #[specta(optional)]
    merge_audio: MergeAudio,
}

impl Default for Stages {
    fn default() -> Self {
        Self {
            asr: Asr::default(),
            merge_audio: MergeAudio::default(),
        }
    }
}

/// CLI 顶层输入
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
struct CliInput {
    /// 任务参数 (必需, 无默认; 演示 required 与可选字段的区别)
    task: String,
    /// 执行命令 (默认 env)
    #[serde(default)]
    #[specta(optional)]
    command: Command,
    #[serde(default)]
    #[specta(optional)]
    stages: Stages,
}

impl Default for CliInput {
    fn default() -> Self {
        Self {
            task: String::new(),
            command: Command::default(),
            stages: Stages::default(),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let types = Types::default().register::<CliInput>();
    let out = config_rs::root::repo_root().join("input.schema.json");
    let schema = JsonSchema::default()
        .title("LocalDub CLI 输入")
        .export_ref_value(&types, Format, "CliInput")
        .unwrap();
    std::fs::write(&out, serde_json::to_string_pretty(&schema).unwrap())?;
    println!("Generated: {}", out.display());
    Ok(())
}
