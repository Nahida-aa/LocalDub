//! LLM 客户端 (镜像 TS `packages/core/ml/llm/openai.ts` 的 `chat_completions`)。
//!
//! 提供 OpenAI 兼容的 `/chat/completions` 同步调用, 供 sf_ocr_fix / asr_fix / translate
//! 等 step 的 LLM 修正使用。参数来源为各 step 的 `LlmFixArgs` (llmModel / llmApiBase),
//! 不依赖全局 env 单例 (与 Rust core 设计一致)。

mod llm_fix_args;
mod ocr_fix;

pub use llm_fix_args::LlmFixArgs;
pub use ocr_fix::{
    build_ocr_fix_system_prompt, ocr_llm_fix, ocr_segments_to_prompt, parse_fix, parse_lines,
};

/// 语言代码 -> 展示名 (供 LLM 系统提示), 镜像 TS `t(source_lang)`。
/// 覆盖 const/lang.rs 的 23 种支持语言; 未命中时**透传语言码** ——
/// LLM 认识 ISO 码, 硬塞"中文"会把外语原文误导成中文修正。
pub fn lang_label(code: &str) -> String {
    match code {
        "zh" => "中文".into(),
        "en" => "English".into(),
        "ja" => "日本語".into(),
        "ko" => "한국어".into(),
        "vi" => "Tiếng Việt".into(),
        "fr" => "Français".into(),
        "de" => "Deutsch".into(),
        "es" => "Español".into(),
        "pt" => "Português".into(),
        "ru" => "Русский".into(),
        "ar" => "العربية".into(),
        "hi" => "हिन्दी".into(),
        "th" => "ไทย".into(),
        "id" => "Bahasa Indonesia".into(),
        "ms" => "Bahasa Melayu".into(),
        "tl" => "Filipino".into(),
        "my" => "မြန်မာ".into(),
        "km" => "ខ្មែរ".into(),
        "lo" => "ລາວ".into(),
        "mn" => "Монгол".into(),
        "ne" => "नेपाली".into(),
        "ur" => "اردو".into(),
        "bn" => "বাংলা".into(),
        other => other.into(),
    }
}

/// `chat_completions` 选项 (镜像 TS `opts`)。
#[derive(Debug, Clone)]
pub struct ChatOptions {
    /// 模型名 (必填；由调用方从步骤 model / provider.default_model 解析后传入)
    pub model: Option<String>,
    /// API base (含协议与 host, 不含 /chat/completions), 缺省 "http://localhost:11434/v1"
    pub api_base: Option<String>,
    /// 系统提示
    pub system_prompt: String,
    /// API key (Authorization Bearer), 缺省空
    pub api_key: Option<String>,
    /// 最大生成 token；None = 不发送该字段，由服务端按模型默认上限决定（不硬编码截断）。
    pub max_tokens: Option<u32>,
    /// 采样温度, 缺省 0.1
    pub temperature: Option<f64>,
    /// OpenAI 兼容的 `tools`（function calling 定义），用 schema 约束模型输出结构。
    /// None = 不发送该字段，退回纯提示词约定。
    pub tools: Option<Vec<serde_json::Value>>,
    /// 强制调用指定 tool（如 `{"type":"function","function":{"name":"emit"}}`）。
    /// None = 不发送，由模型自行决定。
    pub tool_choice: Option<serde_json::Value>,
}

impl Default for ChatOptions {
    fn default() -> Self {
        Self {
            model: None,
            api_base: Some("http://localhost:11434/v1".to_string()),
            system_prompt: String::new(),
            api_key: None,
            max_tokens: None,
            temperature: Some(0.1),
            tools: None,
            tool_choice: None,
        }
    }
}

/// OpenAI `/chat/completions` 请求体 (仅用到字段)。
#[derive(serde::Serialize)]
struct ChatRequest {
    model: String,
    /// None = 不发送该字段，由服务端用模型默认上限（不再硬编码 4096 截断）。
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    temperature: f64,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<serde_json::Value>,
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
    /// 结束原因（如 `stop` / `length`）——`length` 表示被 max tokens 截断。
    finish_reason: Option<String>,
}

#[derive(serde::Deserialize)]
struct MessageContent {
    content: Option<String>,
    /// function calling 的返回；模型按 schema 输出时正文在 arguments 里。
    #[serde(default)]
    tool_calls: Vec<ToolCall>,
}

/// 一次 function calling 的返回（`name` + JSON 字符串 `arguments`）。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ToolCall {
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    /// JSON 字符串（模型按 schema 填的参数）。
    pub arguments: String,
}

/// 把 `localhost` 规范化为 `127.0.0.1`。
///
/// 原因: `reqwest`(基于 hyper) 解析 `localhost` 时可能优先尝试 IPv6 `::1`,
/// 而 Ollama 等本地服务通常只监听 IPv4, 导致连接被拒 (error sending request),
/// 且瞬间失败而非超时。curl / Node 对 localhost 的解析更宽容 (优先 IPv4), 故 TS 版
/// 能连而 Rust 版不能。统一换成 127.0.0.1 与 curl/Node 行为对齐。
fn normalize_localhost(url: &str) -> String {
    url.replace("localhost", "127.0.0.1")
}

/// 一次 chat completion 的结果：正文 + 结束原因 + tool calls（供调用方诊断截断）。
pub struct ChatCompletion {
    pub content: String,
    /// `stop` = 正常结束；`length` = 被 max tokens 截断。
    pub finish_reason: Option<String>,
    /// function calling 返回（模型按 schema 结构化输出时正文在 `arguments`）。
    pub tool_calls: Vec<ToolCall>,
}

/// 调用 OpenAI 兼容的 chat completion, 返回 `choices[0].message.content` (已 trim)。
///
/// 镜像 TS `chat_completions`: POST `{apiBase}/chat/completions`, 失败时抛错。
pub async fn chat_completions(prompt: &str, opts: &ChatOptions) -> anyhow::Result<String> {
    Ok(chat_completions_detailed(prompt, opts).await?.content)
}

/// 同 [`chat_completions`]，但额外带回 `finish_reason`（诊断截断用）。
pub async fn chat_completions_detailed(
    prompt: &str,
    opts: &ChatOptions,
) -> anyhow::Result<ChatCompletion> {
    let api_base = opts
        .api_base
        .clone()
        .unwrap_or_else(|| "http://localhost:11434/v1".to_string());
    let api_base = normalize_localhost(&api_base);
    let model = opts
        .model
        .clone()
        .ok_or_else(|| anyhow::anyhow!("chat_completions: 未指定 model"))?;
    let max_tokens = opts.max_tokens;
    let temperature = opts.temperature.unwrap_or(0.1);
    let tools = opts.tools.clone();
    let tool_choice = opts.tool_choice.clone();

    let url = format!("{api_base}/chat/completions");
    let body = ChatRequest {
        model,
        max_tokens,
        temperature,
        tools,
        tool_choice,
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

    // 本地 LLM (Ollama 等) 通常监听 127.0.0.1/localhost, 不应走系统代理
    // (如 CodeBuddy 注入的 127.0.0.1:7897), 否则会 connect refused。
    // 仅当 host 为回环地址时关闭代理, 远程 LLM 仍走系统代理。
    let host = url
        .split("://")
        .nth(1)
        .and_then(|r| r.split('/').next())
        .and_then(|a| a.rsplit_once(':').map(|(h, _)| h).or(Some(a)))
        .unwrap_or("");
    let is_loopback = host == "127.0.0.1" || host == "localhost" || host == "::1";
    let mut client_builder = reqwest::Client::builder();
    if is_loopback {
        client_builder = client_builder.no_proxy();
    }
    let client = client_builder
        .build()
        .map_err(|e| anyhow::anyhow!("构建 LLM HTTP client 失败: {e}"))?;

    let mut req_builder = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body);
    if let Some(key) = &opts.api_key {
        req_builder = req_builder.header("Authorization", format!("Bearer {key}"));
    }

    let resp = req_builder.send().await.map_err(|e| {
        let kind = if e.is_connect() {
            "connect"
        } else if e.is_timeout() {
            "timeout"
        } else if e.is_request() {
            "request"
        } else if e.is_body() {
            "body"
        } else if e.is_decode() {
            "decode"
        } else {
            "other"
        };
        let proxy = std::env::var("HTTPS_PROXY")
            .or_else(|_| std::env::var("https_proxy"))
            .or_else(|_| std::env::var("HTTP_PROXY"))
            .or_else(|_| std::env::var("http_proxy"))
            .unwrap_or_default();
        anyhow::anyhow!(
            "LLM API 请求失败 ({url}): kind={kind} loopback={is_loopback} proxy={proxy:?} err={e}"
        )
    })?;
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("LLM API {status}: {text}"));
    }
    let json: ChatResponse = resp
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("解析 LLM 响应失败: {e}"))?;
    let choice = json.choices.and_then(|c| c.into_iter().next());
    let finish_reason = choice.as_ref().and_then(|c| c.finish_reason.clone());
    let tool_calls = choice
        .as_ref()
        .map(|c| c.message.tool_calls.clone())
        .unwrap_or_default();
    let content = choice
        .and_then(|c| c.message.content)
        .unwrap_or_default();
    Ok(ChatCompletion {
        content: content.trim().to_string(),
        finish_reason,
        tool_calls,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_options_are_sane() {
        let o = ChatOptions::default();
        assert_eq!(o.model, None); // model 必填，由调用方解析后传入
        assert_eq!(o.api_base.as_deref(), Some("http://localhost:11434/v1"));
        assert_eq!(o.max_tokens, None);
        assert_eq!(o.temperature, Some(0.1));
    }

    #[test]
    fn llm_fix_args_defaults() {
        let a = LlmFixArgs::default();
        assert_eq!(a.model, None); // 不写则用 provider.default_model
        assert_eq!(a.provider, "default");
        assert!(!a.llm_fix);
    }
}
