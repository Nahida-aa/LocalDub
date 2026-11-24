//! listModels 命令 (镜像 TS `packages/cli/run-workflow.ts` 的 `listModels` 分支)。
//!
//! 列出 OpenAI 兼容端点的可用模型:
//! - apiBase 取 `input.steps.translate` 引用的 provider.base_url
//! - apiKey 取 provider.api_key，缺失回退 `OPENAI_API_KEY` (缺失即报错退出)
//! - GET `{apiBase}/models` 逐个打印 `data[].id`

use anyhow::{anyhow, Context};
use config_rs::env::openai_api_key;
use reqwest::blocking::Client;

use crate::input::Input;

/// 命令入口 (镜像 TS `listModels` 分支): 列出模型 id, 失败返回 Err (CLI 打印退出码 1)。
pub fn cmd_list_models(input: &Input) -> anyhow::Result<()> {
    let provider_name = &input.steps.translate.provider;
    let provider = input
        .llm_providers
        .get(provider_name)
        .ok_or_else(|| anyhow!("未配置 llm provider: {provider_name}"))?;
    let api_base = provider.base_url.clone();
    let api_key = provider
        .api_key
        .clone()
        .or_else(openai_api_key)
        .ok_or_else(|| anyhow!("provider {provider_name} 与 OPENAI_API_KEY 均未提供 api_key"))?;
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .context("构造 HTTP 客户端失败")?;

    let resp = client
        .get(format!("{api_base}/models"))
        .bearer_auth(&api_key)
        .send()
        .context("请求 /models 失败")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(anyhow!("API {status}: {body}"));
    }

    let data: serde_json::Value = resp.json().context("解析 /models 响应失败")?;
    if let Some(models) = data.get("data").and_then(|v| v.as_array()) {
        for m in models {
            if let Some(id) = m.get("id").and_then(|v| v.as_str()) {
                println!("  {id}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_lookup_resolves_base_url() {
        let input: Input = serde_json::from_str(
            r#"{"command":"listModels","llm_providers":{"agnes":{"base_url":"http://example.com/v1"}},"steps":{"translate":{"provider":"agnes"}}}"#,
        )
        .unwrap();
        let name = &input.steps.translate.provider;
        let provider = input.llm_providers.get(name).unwrap();
        assert_eq!(provider.base_url, "http://example.com/v1");
        assert!(provider.api_key.is_none());
    }

    #[test]
    fn provider_lookup_missing_is_error_at_cmd() {
        // 未配置 provider 时 cmd_list_models 报明确错误（不静默回退环境变量）。
        let input: Input = serde_json::from_str(r#"{"command":"listModels"}"#).unwrap();
        let name = &input.steps.translate.provider;
        assert!(input.llm_providers.get(name).is_none());
    }
}
