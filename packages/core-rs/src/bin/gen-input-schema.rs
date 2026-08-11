//! 用 specta + specta-jsonschema 生成 input 的 JSON Schema。
//!
//! 对标 TS 侧 `packages/cli/scripts/gen-input-schema.ts`（zod `toJSONSchema({io:'input'})`）。
//! 类型不限定 CLI 场景，Tauri RPC / pipeline 均可复用。
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
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

/// 任务操作
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
enum TaskAction {
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "resume")]
    Resume,
    #[serde(rename = "rerun_stage")]
    RerunStage,
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "get_group_list")]
    GetGroupList,
    #[serde(rename = "get_task_ctx")]
    GetTaskCtx,
}

/// 目标语言 (langList)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
enum TargetLang {
    En,
    Zh,
    Vi,
    Ja,
    Ko,
    Fr,
    De,
    Es,
    Pt,
    Ru,
    Ar,
    Hi,
    Th,
    Id,
    Ms,
    Tl,
    My,
    Km,
    Lo,
    Mn,
    Ne,
    Ur,
    Bn,
}

/// pipeline 阶段名 (stagesList)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
enum StageName {
    Separate,
    SeparateAfter,
    Asr,
    AsrFix,
    SfOcrPre,
    SfOcr,
    SfOcrFix,
    AsrOcrPre,
    AsrOcr,
    AsrOcrFix,
    Translate,
    SplitAudio,
    Tts,
    MergeAudio,
    MergeVideo,
}

/// 任务模式: dub=配音, subtitle=仅字幕
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
enum Pipeline {
    Dub,
    Subtitle,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::Dub
    }
}

/// 字幕源
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
enum SubtitleSource {
    #[serde(rename = "asr")]
    Asr,
    #[serde(rename = "sf_ocr")]
    SfOcr,
    #[serde(rename = "asr_ocr")]
    AsrOcr,
}

impl Default for SubtitleSource {
    fn default() -> Self {
        Self::Asr
    }
}

/// 任务参数 (镜像 taskArgsSchema)
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
struct TaskArgs {
    /// 任务操作: start=开始, resume=继续, rerun_stage=重新运行某步骤, status=显示状态, get_group_list=列出分组
    action: Option<TaskAction>,
    /// 本地文件路径或云端文件 url、youtubeUrl、bilibiliUrl
    url: Option<String>,
    source_lang: Option<TargetLang>,
    target_lang: Option<TargetLang>,
    /// 继续任务专业参数, 可指定 resumeFrom 从某步骤开始, 不指定则从上次中断的步骤开始
    resume_from: Option<StageName>,
    task_dir: Option<String>,
    /// rerunStage 专业参数, 指定要重新运行的步骤
    stage_name: Option<StageName>,
    /// 任务模式, dub 配音, subtitle 仅字幕
    #[serde(default)]
    pipeline: Pipeline,
    /// 字幕源: asr (whisper, 默认), sf_ocr (关键帧策略硬字幕提取), asr_ocr (ASR 时序+OCR 文本融合)
    #[serde(default)]
    subtitle_source: SubtitleSource,
    /// 目标步骤, pipeline 跑到此步骤后自动停止, 不指定则跑完所有步骤
    target_stage: Option<StageName>,
}

impl Default for TaskArgs {
    fn default() -> Self {
        Self {
            action: None,
            url: None,
            source_lang: None,
            target_lang: None,
            resume_from: None,
            task_dir: None,
            stage_name: None,
            pipeline: Pipeline::default(),
            subtitle_source: SubtitleSource::default(),
            target_stage: None,
        }
    }
}

/// 顶层输入 (不限定 CLI 场景, Tauri RPC / pipeline 均可复用)
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
struct Input {
    /// 任务参数, 仅 command=task 时必须
    task: Option<TaskArgs>,
    /// 执行命令 (默认 env)
    #[serde(default)]
    command: Command,
    #[serde(default)]
    stages: Stages,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            task: None,
            command: Command::default(),
            stages: Stages::default(),
        }
    }
}

impl Input {
    /// command=task 时 task 必填
    ///
    /// 目前仅被测试引用；迁移到正式解析入口后由 CLI/RPC 调用。
    #[allow(dead_code)]
    pub fn validate(&self) -> Result<(), String> {
        if self.command == Command::Task && self.task.is_none() {
            return Err("command=task 时 task 必填".into());
        }
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut types = Types::default();
    let root = Input::definition(&mut types);
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
        .title("LocalDub 输入")
        .export_ref_value(&phased, NoopFormat, &name)?;
    std::fs::write(&out, serde_json::to_string_pretty(&schema).unwrap())?;
    println!("Generated: {} (root {name})", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_partial_fills_defaults() {
        let input: Input =
            serde_json::from_str(r#"{"command":"env","task":{"pipeline":"subtitle"}}"#).unwrap();
        assert_eq!(input.command, Command::Env);
        assert!(input.stages.asr.mix_mode == MixMode::Sidechain);
        assert_eq!(input.stages.asr.reduce_bgm, -12.0);
        assert_eq!(input.task.as_ref().unwrap().pipeline, Pipeline::Subtitle);
        assert_eq!(
            input.task.as_ref().unwrap().subtitle_source,
            SubtitleSource::Asr
        );
    }

    #[test]
    fn camel_case_field_names() {
        let input: Input =
            serde_json::from_str(r#"{"task":{"sourceLang":"zh","targetStage":"merge_video"}}"#)
                .unwrap();
        assert_eq!(
            input.task.as_ref().unwrap().source_lang,
            Some(TargetLang::Zh)
        );
        assert_eq!(
            input.task.as_ref().unwrap().target_stage,
            Some(StageName::MergeVideo)
        );
    }

    #[test]
    fn validate_task_required_for_task_command() {
        let ok: Input = serde_json::from_str(r#"{"command":"task","task":{}}"#).unwrap();
        assert!(ok.validate().is_ok());

        let missing: Input = serde_json::from_str(r#"{"command":"task"}"#).unwrap();
        assert!(missing.validate().is_err());

        let env: Input = serde_json::from_str(r#"{"command":"env"}"#).unwrap();
        assert!(env.validate().is_ok());
    }

    #[test]
    fn schema_marks_defaulted_fields_optional() {
        let mut types = Types::default();
        let root = Input::definition(&mut types);
        let phased = PhasesFormat
            .map_types(&types)
            .map_err(|e| format!("phases: {e}"))
            .unwrap()
            .into_owned();
        let deser = select_phase_datatype(&root, &phased, Phase::Deserialize);
        let DataType::Reference(Reference::Named(r)) = &deser else {
            panic!("expected named deserialize root");
        };
        let ndt = phased.get(r).unwrap();
        let DataType::Struct(strct) = &ndt.ty.as_ref().unwrap() else {
            panic!("expected struct root");
        };
        let fields = match &strct.fields {
            specta::datatype::Fields::Named(f) => &f.fields,
            _ => panic!("expected named fields"),
        };
        let optional = fields
            .iter()
            .filter(|(_, f)| f.optional)
            .map(|(n, _)| n.as_ref())
            .collect::<Vec<_>>();
        assert!(optional.contains(&"task"), "task 应可选: {optional:?}");
        assert!(
            optional.contains(&"command"),
            "command 应可选: {optional:?}"
        );
    }
}
