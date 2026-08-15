//! asr 阶段修正参数 (镜像 TS `packages/core/stages/asr/fix_args.ts` AsrFixArgsSchema)。
//!
//! TS 端 `AsrFixArgsSchema` 在 `LlmFixArgsSchema` 基础上 spread 并追加 `asrFilePath`。
//! 这里复用 `llm::LlmFixArgs`, 用 `#[serde(flatten)]` 保持扁平 JSON 结构。
//!
//! 注意: 父结构用 `#[serde(default)]` 整体缺省时会调用 Rust `Default`, 故手写
//! `impl Default` 以保证默认值一致 (与 `input::stages::Asr` 同款处理)。

use llm::LlmFixArgs;
use serde::{Deserialize, Serialize};

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
