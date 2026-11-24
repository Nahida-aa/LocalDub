//! LLM 修正参数 (镜像 TS `packages/llm/llm_fix_args.ts` 的 `LlmFixArgsSchema`)。

use serde::{Deserialize, Serialize};

/// LLM 修正参数。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub struct LlmFixArgs {
    /// 引用的 LLM provider 名 (见 input.llm_providers)
    #[serde(default = "default_provider")]
    pub provider: String,
    /// LLM 模型名。不写则用 provider.default_model。
    #[serde(default)]
    pub model: Option<String>,
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
            provider: default_provider(),
            model: None,
            domain_hint: None,
            llm_fix: false,
        }
    }
}

fn default_provider() -> String {
    "default".to_string()
}
