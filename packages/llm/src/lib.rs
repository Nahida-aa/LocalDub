//! LLM 客户端 (镜像 TS `packages/core/ml/llm/openai.ts` 的 `chat_completions`)。
//!
//! 提供 OpenAI 兼容的 `/chat/completions` 同步调用, 供 sf_ocr_fix / asr_fix / translate
//! 等 stage 的 LLM 修正使用。参数来源为各 stage 的 `LlmFixArgs` (llmModel / llmApiBase),
//! 不依赖全局 env 单例 (与 Rust core 设计一致)。

mod llm_fix_args;
mod ocr_fix;

pub use llm_fix_args::LlmFixArgs;
pub use ocr_fix::{build_ocr_fix_system_prompt, ocr_llm_fix, ocr_segments_to_prompt, parse_lines};

/// 语言代码 -> 展示名 (供 LLM 系统提示), 镜像 TS `t(sourceLang)` 的常用映射。
/// 未命中时缺省 "中文"。
pub fn lang_label(code: &str) -> &str {
    match code {
        "zh" => "中文",
        "en" => "English",
        "ja" => "日本語",
        "ko" => "한국어",
        _ => "中文",
    }
}

/// `chat_completions` 选项 (镜像 TS `opts`)。
#[derive(Debug, Clone)]
pub struct ChatOptions {
    /// 模型名 (缺省 "gemma4:31b-cloud", 见 LlmFixArgs::default)
    pub model: Option<String>,
    /// API base (含协议与 host, 不含 /chat/completions), 缺省 "http://localhost:11434/v1"
    pub api_base: Option<String>,
    /// 系统提示
    pub system_prompt: String,
    /// API key (Authorization Bearer), 缺省空
    pub api_key: Option<String>,
    /// 最大生成 token, 缺省 4096
    pub max_tokens: Option<u32>,
    /// 采样温度, 缺省 0.1
    pub temperature: Option<f64>,
}

impl Default for ChatOptions {
    fn default() -> Self {
        Self {
            model: Some("gemma4:31b-cloud".to_string()),
            api_base: Some("http://localhost:11434/v1".to_string()),
            system_prompt: String::new(),
            api_key: None,
            max_tokens: Some(4096),
            temperature: Some(0.1),
        }
    }
}

/// OpenAI `/chat/completions` 请求体 (仅用到字段)。
#[derive(serde::Serialize)]
struct ChatRequest {
    model: String,
    max_tokens: u32,
    temperature: f64,
    messages: Vec<ChatMessage>,
}

#[derive(serde::Serialize)]
struct ChatMessage {
    role: &'static str,
    content: String,
}

/// `/chat/completions` 响应体 (仅解析用到的字段)。
#[derive(serde::Deserialize)]
struct ChatResponse {
    choices: Option<Vec<Choice>>,
}

#[derive(serde::Deserialize)]
struct Choice {
    message: MessageContent,
}

#[derive(serde::Deserialize)]
struct MessageContent {
    content: Option<String>,
}

/// 调用 OpenAI 兼容的 chat completion, 返回 `choices[0].message.content` (已 trim)。
///
/// 镜像 TS `chat_completions`: POST `{apiBase}/chat/completions`, 失败时抛错。
pub fn chat_completions(prompt: &str, opts: &ChatOptions) -> anyhow::Result<String> {
    let api_base = opts
        .api_base
        .clone()
        .unwrap_or_else(|| "http://localhost:11434/v1".to_string());
    let model = opts
        .model
        .clone()
        .unwrap_or_else(|| "gemma4:31b-cloud".to_string());
    let max_tokens = opts.max_tokens.unwrap_or(4096);
    let temperature = opts.temperature.unwrap_or(0.1);

    let url = format!("{api_base}/chat/completions");
    let body = ChatRequest {
        model,
        max_tokens,
        temperature,
        messages: vec![
            ChatMessage {
                role: "system",
                content: opts.system_prompt.clone(),
            },
            ChatMessage {
                role: "user",
                content: prompt.to_string(),
            },
        ],
    };

    let mut builder = reqwest::blocking::Client::new()
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body);
    if let Some(key) = &opts.api_key {
        builder = builder.header("Authorization", format!("Bearer {key}"));
    }

    let resp = builder
        .send()
        .map_err(|e| anyhow::anyhow!("LLM API 请求失败 ({url}): {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let text = resp.text().unwrap_or_default();
        return Err(anyhow::anyhow!("LLM API {status}: {text}"));
    }
    let json: ChatResponse = resp
        .json()
        .map_err(|e| anyhow::anyhow!("解析 LLM 响应失败: {e}"))?;
    let content = json
        .choices
        .and_then(|c| c.into_iter().next())
        .and_then(|c| c.message.content)
        .unwrap_or_default();
    Ok(content.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_options_are_sane() {
        let o = ChatOptions::default();
        assert_eq!(o.model.as_deref(), Some("gemma4:31b-cloud"));
        assert_eq!(o.api_base.as_deref(), Some("http://localhost:11434/v1"));
        assert_eq!(o.max_tokens, Some(4096));
        assert_eq!(o.temperature, Some(0.1));
    }

    #[test]
    fn llm_fix_args_defaults() {
        let a = LlmFixArgs::default();
        assert_eq!(a.llm_model, "gemma4:31b-cloud");
        assert_eq!(a.llm_api_base, "http://localhost:11434/v1");
        assert!(!a.llm_fix);
    }
}
