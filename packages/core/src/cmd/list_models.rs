//! listModels 命令 (镜像 TS `packages/cli/run-workflow.ts` 的 `listModels` 分支)。
//!
//! 列出 OpenAI 兼容端点的可用模型:
//! - apiBase 取 `input.stages.translate.apiBase` (其 default 为 `OPENAI_BASE_URL` env, 再回退 ollama)
//! - 需要 `OPENAI_API_KEY` (对齐 TS: 缺失即报错退出)
//! - GET `{apiBase}/models` 逐个打印 `data[].id`

use anyhow::{anyhow, Context};
use config_rs::env::{openai_api_key, openai_base_url};
use reqwest::blocking::Client;

use crate::input::Input;

/// 命令入口 (镜像 TS `listModels` 分支): 列出模型 id, 失败返回 Err (CLI 打印退出码 1)。
pub fn cmd_list_models(input: &Input) -> anyhow::Result<()> {
    let api_key = openai_api_key()
        .ok_or_else(|| anyhow!("OPENAI_API_KEY not configured"))?;

    // apiBase 三级回退 (镜像 TS `apiBase || env.OPENAI_BASE_URL || 默认`):
    // - 显式配置 (input.stages.translate.apiBase) 优先;
    // - 空串 (缺省) → openai_base_url() = env.OPENAI_BASE_URL 或本地 ollama 默认。
    let cfg_base = input.stages.translate.api_base.clone();
    let api_base = if cfg_base.is_empty() {
        openai_base_url()
    } else {
        cfg_base
    };
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
    fn api_base_defaults_to_openai_base_url_or_ollama() {
        // 输入缺 stages.translate 时 (serde 反序列化 `{}`), apiBase 走 openai_base_url()
        // (env 或 ollama 默认), 与 TS `stages.translate.apiBase || env.OPENAI_BASE_URL` 对齐。
        let input: Input = serde_json::from_str(r#"{"command":"listModels"}"#).unwrap();
        let base = &input.stages.translate.api_base;
        let resolved = if base.is_empty() {
            openai_base_url()
        } else {
            base.clone()
        };
        assert_eq!(resolved, openai_base_url());
    }

    #[test]
    fn api_base_respects_explicit_value() {
        // 显式配置时优先 (镜像 TS `apiBase || env` 首项)。
        let input: Input = serde_json::from_str(
            r#"{"command":"listModels","stages":{"translate":{"apiBase":"http://example.com/v1"}}}"#,
        )
        .unwrap();
        let cfg_base = &input.stages.translate.api_base;
        let resolved = if cfg_base.is_empty() {
            openai_base_url()
        } else {
            cfg_base.clone()
        };
        assert_eq!(resolved, "http://example.com/v1");
    }
}