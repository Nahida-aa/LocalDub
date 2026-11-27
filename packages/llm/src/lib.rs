//! LLM 客户端 (镜像 TS `packages/core/ml/llm/openai.ts` 的 `chat_completions`)。
//!
//! 提供 OpenAI 兼容的 `/chat/completions` 同步调用, 供 sf_ocr_fix / asr_fix / translate
//! 等 step 的 LLM 修正使用。参数来源为各 step 的 `LlmFixArgs` (llmModel / llmApiBase),
//! 不依赖全局 env 单例 (与 Rust core 设计一致)。

mod arguments_parse;
mod llm_fix_args;
mod ocr_fix;
#[cfg(test)]
mod tool_loop_tests;
pub mod translation_normalize;

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

/// 一条 chat 消息。tool loop 需要多轮历史，所以支持三种 OpenAI 兼容形态：
/// - `system` / `user` / `assistant`：纯文本，`content` 必填
/// - `assistant` with `tool_calls`：模型发起的工具调用，`tool_calls` 必填
/// - `tool`：`tool_call_id` + `content`（工具结果/校验 feedback 回传）
#[derive(serde::Serialize, Clone, Debug)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn system(text: impl Into<String>) -> Self {
        Self { role: "system".into(), content: Some(text.into()), tool_calls: None, tool_call_id: None }
    }
    pub fn user(text: impl Into<String>) -> Self {
        Self { role: "user".into(), content: Some(text.into()), tool_calls: None, tool_call_id: None }
    }
    pub fn assistant(text: impl Into<String>) -> Self {
        Self { role: "assistant".into(), content: Some(text.into()), tool_calls: None, tool_call_id: None }
    }
    /// assistant 发起工具调用（content 可为 None）。
    pub fn assistant_tool_calls(calls: Vec<serde_json::Value>) -> Self {
        Self { role: "assistant".into(), content: None, tool_calls: Some(calls), tool_call_id: None }
    }
    /// 工具结果回传。
    pub fn tool(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self { role: "tool".into(), content: Some(content.into()), tool_calls: None, tool_call_id: Some(call_id.into()) }
    }
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

/// 一次 function calling 的返回（`id` + `name` + JSON 字符串 `arguments`）。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ToolCall {
    /// 调用 id，tool result 必须带它回传。
    pub id: String,
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
/// 日志截断：UTF-8 安全，超长加 "..."。
fn truncate_for_log(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let end = (0..=max).rev().find(|&i| s.is_char_boundary(i)).unwrap_or(0);
    format!("{}...", &s[..end])
}

fn normalize_localhost(url: &str) -> String {
    url.replace("localhost", "127.0.0.1")
}

/// 一次 chat completion 的结果：正文 + 结束原因 + tool calls（供调用方诊断截断）。
#[derive(Clone)]
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
    let messages = vec![
        ChatMessage::system(opts.system_prompt.clone()),
        ChatMessage::user(prompt),
    ];
    send_chat_request(messages, opts).await
}

/// 用完整 messages 数组调用（tool loop 多轮历史用）。
pub async fn chat_completions_messages(
    messages: Vec<ChatMessage>,
    opts: &ChatOptions,
) -> anyhow::Result<ChatCompletion> {
    send_chat_request(messages, opts).await
}

/// 可选钩子：把工具 arguments 的原始形状归一化成工具 Input schema 期望的形状。
///
/// 勾在「arguments 字符串反序列化为 `Value` 之后、工具 Input 反序列化之前」这一层：
/// `arguments (&str) → serde_json::from_str → Value → [ToolInputNormalizer] → Value → tool Input → typed`
/// provider 若严格遵守 schema，这个钩子跳过（raw 原样传给工具）；形状不可信时由
/// 特定工具提供归一化逻辑（如把 agnes 自造的 `{"arguments":"..."}` 解包成标准对象）。
///
/// 这是扩展系统的一种实现形式：钩子可选、每个工具可提供不同实现。
pub trait ToolInputNormalizer: Send + Sync {
    /// 归一化：Ok 返回符合工具 Input schema 形状的 Value；Err 返回 feedback 让模型重试。
    fn normalize(&self, raw: &serde_json::Value) -> Result<serde_json::Value, String>;
}

/// 通用 tool loop 的回调：处理一次 tool call 的 arguments。
///
/// - `Ok(output)`：成功，loop 立即终止，返回 output。
/// - `Err(feedback)`：失败，feedback 作为 tool result 回传给模型，下一轮修正。
pub trait ToolHandler: Send + Sync {
    /// tool 名（给 LLM 看的 token）。
    fn name(&self) -> &str;
    /// tool 描述。
    fn description(&self) -> &str;
    /// 入参 JSON Schema。
    fn schema(&self) -> serde_json::Value;
    /// 可选输入归一化钩子，默认 None（遵守 schema 的 provider 直接跳过）。
    fn input_normalizer(&self) -> Option<&dyn ToolInputNormalizer> {
        None
    }
    /// 执行工具，接收归一化后的 Input Value：Ok 终止并返回值，Err 作为 feedback 回传重试。
    fn handle(&self, input: &serde_json::Value) -> Result<String, String>;
}

/// 自己实现的 tool loop：驱动「模型调 tool → handler 处理 → 结果/feedback 回传」，
/// 直到 handler 成功（终止）或 max_turns 用尽。
///
/// 比框架 output tool 模式的优势：handler 可做自定义归一化/校验（如把 agnes 自造结构
/// 解析成目标格式），不被框架内置的 schema 校验绑死。
pub async fn chat_with_tool_loop(
    system: &str,
    user: &str,
    handler: &dyn ToolHandler,
    opts: &ChatOptions,
    max_turns: usize,
) -> anyhow::Result<String> {
    chat_with_tool_loop_inner(system, user, handler, opts, max_turns, |messages, opts| {
        let opts = opts.clone();
        let messages = messages.clone();
        Box::pin(async move { chat_completions_messages(messages, &opts).await })
    }).await
}

/// tool loop 核心，发送函数可注入（测试时喂 fake provider）。
async fn chat_with_tool_loop_inner<F>(
    system: &str,
    user: &str,
    handler: &dyn ToolHandler,
    opts: &ChatOptions,
    max_turns: usize,
    send: F,
) -> anyhow::Result<String>
where
    F: for<'a> Fn(
        &'a Vec<ChatMessage>,
        &'a ChatOptions,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<ChatCompletion>> + Send + 'a>>,
{
    let tool_def = serde_json::json!({
        "type": "function",
        "function": {
            "name": handler.name(),
            "description": handler.description(),
            "parameters": handler.schema(),
        }
    });
    let mut opts = opts.clone();
    opts.system_prompt = system.to_string();
    opts.tools = Some(vec![tool_def]);
    opts.tool_choice = Some(serde_json::json!({
        "type": "function",
        "function": { "name": handler.name() }
    }));

    let mut messages = vec![ChatMessage::system(system), ChatMessage::user(user)];

    for turn in 0..max_turns {
        let completion = send(&messages, &opts).await?;
        if completion.tool_calls.is_empty() {
            // 模型没调 tool：若有正文就当最终答复，否则继续（最多 max_turns）。
            if !completion.content.is_empty() {
                return Ok(completion.content);
            }
            messages.push(ChatMessage::assistant("(no tool call)"));
            continue;
        }

        // 本轮所有 tool call：逐个处理，拼 assistant(tool_calls) + tool result 回传。
        let mut assistant_calls = Vec::new();
        let mut results = Vec::new();
        let mut success_outputs = Vec::new();
        for call in &completion.tool_calls {
            assistant_calls.push(serde_json::json!({
                "id": call.id,
                "type": "function",
                "function": { "name": call.function.name, "arguments": call.function.arguments }
            }));

            // ① arguments 字符串 → Value（容错解析：repair_json + partial 补括号，
            //    参考 rig；保证进入 normalizer 的一定是合法 JSON）。
            let raw: serde_json::Value = match arguments_parse::parse_arguments_json(&call.function.arguments) {
                Ok(v) => v,
                Err(e) => {
                    // 打原始 arguments 全文（截断到 2000 字符）——看到模型到底填了什么，
                    // 方便改进钩子/容错解析。
                    let fb = format!("arguments 不是合法 JSON: {e}");
                    tracing::warn!(
                        target: "llm", tool = handler.name(), turn,
                        "{fb}; 原始 arguments={}", truncate_for_log(&call.function.arguments, 2000)
                    );
                    results.push((call.id.clone(), fb));
                    continue;
                }
            };

            // ② 可选输入归一化钩子（特定工具提供；把不可信形状归一到 Input schema）
            let input = match handler.input_normalizer() {
                Some(n) => match n.normalize(&raw) {
                    Ok(v) => v,
                    Err(fb) => {
                        // 打归一化前的原始 Value 全文——这是改进钩子的关键诊断信息。
                        let raw_json = serde_json::to_string(&raw).unwrap_or_default();
                        tracing::warn!(
                            target: "llm", tool = handler.name(), turn,
                            "归一化失败: {fb}; 原始 arguments Value={}", truncate_for_log(&raw_json, 2000)
                        );
                        results.push((call.id.clone(), fb));
                        continue;
                    }
                },
                None => raw,
            };

            // ③ 工具执行（接收归一化后的 Input Value）
            match handler.handle(&input) {
                Ok(output) => {
                    tracing::info!(target: "llm", tool = handler.name(), turn, "tool call succeeded");
                    results.push((call.id.clone(), output.clone()));
                    success_outputs.push(output);
                }
                Err(feedback) => {
                    tracing::warn!(target: "llm", tool = handler.name(), turn, "tool call rejected: {feedback}");
                    results.push((call.id.clone(), feedback));
                }
            }
        }

        // 回填 assistant(tool_calls) + 每条 tool result，让模型看到自己做了什么。
        messages.push(ChatMessage::assistant_tool_calls(assistant_calls));
        for (id, content) in results {
            messages.push(ChatMessage::tool(id, content));
        }

        if !success_outputs.is_empty() && success_outputs.len() == completion.tool_calls.len() {
            // 本轮所有 tool call 都成功 → 第一个成功结果即最终译文（透穿）。
            return Ok(success_outputs.remove(0));
        }
    }

    Err(anyhow::anyhow!("tool loop 用尽 max_turns={max_turns} 轮仍未成功"))
}

/// 实际发起 HTTP 请求。所有公开入口都走这里。
async fn send_chat_request(
    messages: Vec<ChatMessage>,
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
        messages,
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

/// 翻译场景的输入归一化钩子：把 provider 自造结构 → `{"1":"译1",...}`。
///
/// `n` = 期望句数（每个视频不同，运行时传入）。这是挂在 TranslationToolHandler 上的
/// 可选钩子，勾在 arguments 解析成 Value 之后、工具 Input 反序列化之前。
pub struct TranslationInputNormalizer {
    n: usize,
    /// 失败诊断持久化路径（可选）。每次归一化失败 append 一行 JSON，
    /// 让不可复现的模型随机失败也能留档分析。
    failure_log: Option<std::path::PathBuf>,
}

impl ToolInputNormalizer for TranslationInputNormalizer {
    fn normalize(&self, raw: &serde_json::Value) -> Result<serde_json::Value, String> {
        match translation_normalize::normalize(raw, self.n) {
            Ok(out) => serde_json::to_value(&out)
                .map_err(|e| format!("序列化归一化结果失败: {e}")),
            Err(e) => {
                let feedback = format!(
                    "无法解析译文。请用 {{\"1\": \"译1\", ...}} 或编号行 \"1. 译1\"。问题: {}",
                    e.0
                );
                // 失败留档：模型随机失败不可复现，攒几次才能看出失败模式分布。
                if let Some(path) = &self.failure_log {
                    self.append_failure(path, raw, &feedback);
                }
                Err(feedback)
            }
        }
    }
}

impl TranslationInputNormalizer {
    fn append_failure(&self, path: &std::path::Path, raw: &serde_json::Value, feedback: &str) {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let line = serde_json::json!({
            "ts_unix": ts,
            "expected_n": self.n,
            "raw_arguments": raw,
            "feedback": feedback,
        });
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::OpenOptions::new().create(true).append(true).open(path) {
            Ok(mut f) => {
                let _ = writeln!(f, "{}", line);
            }
            Err(e) => tracing::warn!(target: "llm", "写归一化失败诊断失败: {e}"),
        }
    }
}

/// 翻译场景的 ToolHandler：提供归一化钩子 + 透穿归一化后的 input。
pub struct TranslationToolHandler {
    n: usize,
    normalizer: TranslationInputNormalizer,
}

impl TranslationToolHandler {
    pub fn new(n: usize) -> Self {
        Self::with_failure_log(n, None)
    }

    /// `failure_log`：归一化失败时 append 诊断 JSONL 的路径（可选）。
    pub fn with_failure_log(n: usize, failure_log: Option<std::path::PathBuf>) -> Self {
        Self {
            n,
            normalizer: TranslationInputNormalizer { n, failure_log },
        }
    }
}

impl ToolHandler for TranslationToolHandler {
    fn name(&self) -> &str {
        "submit"
    }

    fn description(&self) -> &str {
        "Submit the sentence-by-sentence translations. Keys are sentence numbers (1..N), values are translations. Every key is required."
    }

    fn schema(&self) -> serde_json::Value {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();
        for i in 1..=self.n {
            let k = i.to_string();
            properties.insert(k.clone(), serde_json::json!({ "type": "string" }));
            required.push(serde_json::json!(k));
        }
        serde_json::json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        })
    }

    fn input_normalizer(&self) -> Option<&dyn ToolInputNormalizer> {
        Some(&self.normalizer)
    }

    fn handle(&self, input: &serde_json::Value) -> Result<String, String> {
        // 归一化已由钩子完成，这里透穿成字符串（tool loop 据此终止）。
        serde_json::to_string(input).map_err(|e| format!("序列化结果失败: {e}"))
    }
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
