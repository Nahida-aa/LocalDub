//! asr 阶段修正参数 (镜像 TS `packages/core/steps/asr/fix_args.ts` AsrFixArgsSchema)。
//!
//! TS 端 `AsrFixArgsSchema` 在 `LlmFixArgsSchema` 基础上 spread 并追加 `asrFilePath`。
//! 这里复用 `llm::LlmFixArgs`, 用 `#[serde(flatten)]` 保持扁平 JSON 结构。
//!
//! 注意: 父结构用 `#[serde(default)]` 整体缺省时会调用 Rust `Default`, 故手写
//! `impl Default` 以保证默认值一致 (与 `input::steps::Asr` 同款处理)。

use llm::LlmFixArgs;
use serde::{Deserialize, Serialize};

use crate::steps::utils::paths::asr::AsrPath;

/// asr 阶段修正参数。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub struct AsrFixArgs {
    /// LLM 修正参数 (flatten, 扁平展开为 llmModel / llmApiBase / domainHint / llmFix)
    #[serde(default, flatten)]
    pub llm_fix: LlmFixArgs,
    /// ASR 结果文件路径, 调试使用
    #[serde(default)]
    pub asr_file_path: Option<String>,
}

impl Default for AsrFixArgs {
    fn default() -> Self {
        Self {
            llm_fix: LlmFixArgs::default(),
            asr_file_path: None,
        }
    }
}

/// Phase 1 事实面：`step_asr_fix` 的显式依赖清单（由装配层算好后注入）。
///
/// - `asr_path`：asr 步骤产出的识别结果 `asr/asr.json`；
/// - `source_lang`：源语言（`<asr>/out.json` 实测 > `input.workflow.source_lang`
///   配置 > 未知），由装配层算好，LLM 提示词用它。
///
/// 意图面（`video_dir` / `args`）走 `sc.ctx().input()`，不进 deps。
#[derive(Debug, Clone)]
pub struct AsrFixDeps {
    /// 来自 asr 步骤的识别结果 `asr/asr.json`。
    pub asr_path: AsrPath,
    /// 源语言（三级回退算好）：LLM 提示词用它。
    pub source_lang: crate::r#const::lang::LangStr,
}

/// `step_asr_fix` 的**全部产物清单**（作为步骤函数返回值，不写盘）。
///
/// 两个产物都是无条件写出的：`asr_fix.json`（详细结果）+ `srt.json`
/// （规范化字幕，下游 translate / split_audio / mix_video 的唯一契约）。
#[derive(Debug, Clone, PartialEq, Eq, specta::Type)]
pub struct AsrFixOutput {
    /// 详细结果 `asr_fix/asr_fix.json`。
    pub result_path: std::path::PathBuf,
    /// 规范化字幕 `asr_fix/srt.json`。
    pub srt_path: std::path::PathBuf,
}

impl AsrFixDeps {
    pub fn from_input(input: &crate::input::WorkflowInput) -> Self {
        let video_dir = input.video_dir();
        // 与 translate / asr 同款三级回退，读 WorkflowInput 的 workflow 字段即可。
        let asr_language = crate::steps::asr::out::asr_language(video_dir);
        let source_lang = crate::state::lang::resolve_source_language(
            asr_language.as_ref(),
            input.workflow.source_lang.as_ref(),
        );
        Self {
            asr_path: AsrPath::new(video_dir),
            source_lang,
        }
    }
}
