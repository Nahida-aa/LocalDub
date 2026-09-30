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
#[serde(rename_all = "camelCase")]
pub struct AsrFixArgs {
    /// LLM 修正参数 (flatten, 扁平展开为 llmModel / llmApiBase / domainHint / llmFix)
    #[serde(default, flatten)]
    pub llm_fix: LlmFixArgs,
    /// ASR 结果文件路径, 调试使用
    #[serde(default)]
    pub asr_file_path: Option<String>,
    /// 是否启用本阶段 (缺省 true; 设为 false 可跳过 asr_fix)
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl Default for AsrFixArgs {
    fn default() -> Self {
        Self {
            llm_fix: LlmFixArgs::default(),
            asr_file_path: None,
            enabled: default_enabled(),
        }
    }
}

/// asr_fix 步骤的输入契约：本步要消费的**跨步骤输入**路径。
///
/// 只装真正的输入 `asr/asr.json`——asr 步骤的识别结果。其余路径
/// （`asr_fix/` 下的产物与 `srt.json`）由步骤内部 `::new(video_dir)` 构造，拼法同样
/// 只有一处来源。与 `SfOcrFixInput` 同款：`StepInput` 是依赖清单。
///
/// 该路径是 `{video_dir}/asr/asr.json` 的**约定拼法**（非拍平的事实副本），故入口
/// 是 [`new`](Self::new)`(video_dir, input)`。
pub struct AsrFixInput {
    /// 任务目录（写侧 step status 需要）。
    pub video_dir: String,
    /// 来自 asr 步骤的识别结果 `asr/asr.json`。
    pub asr_path: AsrPath,
    /// 源语言（`<asr>/out.json` > `input.workflow.sourceLang` > 未知）：LLM 提示词用它。
    pub source_lang: crate::r#const::lang::LangCode,
    /// 本次运行参数（`input.steps.asr_fix`）。
    pub args: AsrFixArgs,
}

impl AsrFixInput {
    pub fn new(video_dir: &str, input: &crate::input::Input) -> Self {
        Self {
            video_dir: video_dir.to_string(),
            asr_path: AsrPath::new(video_dir),
            source_lang: crate::steps::utils::src_language(video_dir, input),
            args: input.steps.asr_fix.clone(),
        }
    }
}
