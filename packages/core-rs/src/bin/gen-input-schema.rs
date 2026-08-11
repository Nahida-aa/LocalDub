//! 用 specta + specta-jsonschema 生成 CLI input 的 JSON Schema。
//!
//! 对标 TS 侧 `packages/cli/scripts/gen-input-schema.ts`（zod `toJSONSchema({io:'input'})`）。
//!
//! input 语义由 `specta_serde::PhasesFormat` 驱动：`#[serde(default)]` 的字段在
//! Deserialize 面（input）可选、在 Serialize 面（output）必填，对齐 zod 的 io 区分。
//! 运行时由 `#[serde(default)]` 补齐默认值。specta 无 `Ranged`/min/max 约束，
//! 数值范围需在解析时自行校验（见下文 TODO）。

use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use specta::datatype::{DataType, Reference};
use specta::{Format as _, Type, Types};
use specta_jsonschema::JsonSchema;
use specta_serde::{Phase, PhasesFormat, select_phase_datatype};

/// 占位 formatter：types 已由 [`PhasesFormat`] 预映射，导出时不再二次改写。
struct NoopFormat;

impl specta::Format for NoopFormat {
    fn map_types(&self, types: &Types) -> Result<Cow<'_, Types>, specta::FormatError> {
        Ok(Cow::Owned(types.clone()))
    }

    fn map_type(
        &self,
        _types: &Types,
        dt: &DataType,
    ) -> Result<Cow<'_, DataType>, specta::FormatError> {
        Ok(Cow::Owned(dt.clone()))
    }
}

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
    threshold: f64,
    /// 压缩比, 默认 20
    #[serde(default)]
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
    runtime: Runtime,
    /// ASR 音频源 (默认 sidechain)
    #[serde(default)]
    mix_mode: MixMode,
    /// 背景音降低量(dB), raw-sum 叠加前衰减
    #[serde(default = "default_reduce_bgm")]
    reduce_bgm: f64,
    /// ASR 结果是否包含词级时间戳 (默认关闭, 调试时开启)
    #[serde(default)]
    words_output: bool,
    /// mixMode=sidechain 时压缩器参数
    #[serde(default)]
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
    asr: Asr,
    #[serde(default)]
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
    command: Command,
    #[serde(default)]
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
    let mut types = Types::default();
    let root = CliInput::definition(&mut types);
    let phased = PhasesFormat
        .map_types(&types)
        .map_err(|e| format!("phases: {e}"))?
        .into_owned();
    let deser = select_phase_datatype(&root, &phased, Phase::Deserialize);
    let name = match &deser {
        DataType::Reference(Reference::Named(r)) => phased
            .get(r)
            .map(|ndt| ndt.name.clone())
            .ok_or_else(|| "deserialize root ref not in phased types".to_string())?,
        other => return Err(format!("unexpected deserialize root: {other:?}").into()),
    };
    let out = config_rs::root::repo_root().join("input.schema.json");
    let schema = JsonSchema::default()
        .allow_additional_properties(true)
        .title("LocalDub CLI 输入")
        .export_ref_value(&phased, NoopFormat, &name)?;
    std::fs::write(&out, serde_json::to_string_pretty(&schema).unwrap())?;
    println!("Generated: {} (root {name})", out.display());
    Ok(())
}
