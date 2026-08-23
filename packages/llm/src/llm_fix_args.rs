//! LLM 修正参数 (镜像 TS `packages/llm/llm_fix_args.ts` 的 `LlmFixArgsSchema`)。

use serde::{Deserialize, Serialize};

/// LLM 修正参数。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LlmFixArgs {
    /// LLM 模型名
    #[serde(default = "default_llm_model")]
    pub llm_model: String,
    /// LLM API 地址
    #[serde(default = "default_llm_api_base")]
    pub llm_api_base: String,
    /// 领域提示, 帮助 LLM 理解上下文，例如"仙侠题材，角色：叶白、慧天、夜白"
    #[serde(default)]
    pub domain_hint: Option<String>,
    /// 是否启用 LLM 修正
    #[serde(default)]
    pub llm_fix: bool,
}

impl Default for LlmFixArgs {
    fn default() -> Self {
        Self {
            llm_model: default_llm_model(),
            llm_api_base: default_llm_api_base(),
            domain_hint: None,
            llm_fix: false,
        }
    }
}

fn default_llm_model() -> String {
    "gemma4:31b-cloud".to_string()
}

fn default_llm_api_base() -> String {
    "http://localhost:11434/v1".to_string()
}
