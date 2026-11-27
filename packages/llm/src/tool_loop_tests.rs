//! tool loop 的假提供商测试：不连真实 LLM，注入脚本化响应验证完整链路。
//! 验证：① 归一化钩子被勾住 ② feedback 回传让模型改 ③ 成功即终止 ④ max_turns 兜底。

use super::*;
use std::sync::{Arc, Mutex};

/// 把一轮响应封装成 ChatCompletion（带一个 submit tool call）。
fn tool_completion(id: &str, arguments: &str) -> ChatCompletion {
    ChatCompletion {
        content: String::new(),
        finish_reason: Some("tool_calls".into()),
        tool_calls: vec![ToolCall {
            id: id.into(),
            function: ToolCallFunction {
                name: "submit".into(),
                arguments: arguments.into(),
            },
        }],
    }
}

/// 脚本化 fake：按调用次数依次返回预设响应，并记录每次请求的 messages。
type SendFn<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = anyhow::Result<ChatCompletion>> + Send + 'a>
>;

fn scripted_fake(responses: Vec<ChatCompletion>) -> (
    Box<dyn for<'a> Fn(&'a Vec<ChatMessage>, &'a ChatOptions) -> SendFn<'a>>,
    Arc<Mutex<Vec<Vec<ChatMessage>>>>,
) {
    let calls = Arc::new(Mutex::new(0usize));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let calls2 = calls.clone();
    let seen2 = seen.clone();
    let move_responses = Arc::new(Mutex::new(responses));

    let fake: Box<dyn for<'a> Fn(&'a Vec<ChatMessage>, &'a ChatOptions) -> SendFn<'a>> =
        Box::new(move |messages: &Vec<ChatMessage>, _opts: &ChatOptions| {
            let i = {
                let mut c = calls2.lock().unwrap();
                let i = *c;
                *c += 1;
                i
            };
            seen2.lock().unwrap().push(messages.clone());
            let resp = move_responses.lock().unwrap().get(i).cloned();
            Box::pin(async move {
                resp.ok_or_else(|| anyhow::anyhow!("fake 响应脚本用尽 (call {i})"))
            }) as SendFn<'_>
        });
    (fake, seen)
}

fn opts() -> ChatOptions {
    ChatOptions {
        model: Some("fake".into()),
        ..Default::default()
    }
}

#[tokio::test]
async fn normalizer_applied_and_success_terminates() {
    // 第 1 轮模型返回 agnes 自造结构，归一化器应解出 {"1":..,"2":..} 并成功终止。
    let (fake, seen) = scripted_fake(vec![tool_completion(
        "c1",
        r#"{"arguments":"{\"1\":\"译1\",\"2\":\"译2\"}"}"#,
    )]);
    let handler = TranslationToolHandler::new(2);
    let out = chat_with_tool_loop_inner("sys", "1. 原文1\n2. 原文2", &handler, &opts(), 3, &fake)
        .await
        .unwrap();
    assert_eq!(out, r#"{"1":"译1","2":"译2"}"#);
    // 只发了 1 轮（成功即终止）。
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn feedback_flows_back_and_model_recovers() {
    // 第 1 轮缺字段（归一化失败）→ feedback 回传 → 第 2 轮正确 → 终止。
    let (fake, seen) = scripted_fake(vec![
        tool_completion("c1", r#"{"foo":"只有一句"}"#), // 归一化失败
        tool_completion("c2", r#"{"1":"译1","2":"译2"}"#), // 修正
    ]);
    let handler = TranslationToolHandler::new(2);
    let out = chat_with_tool_loop_inner("sys", "1. 原文1\n2. 原文2", &handler, &opts(), 3, &fake)
        .await
        .unwrap();
    assert_eq!(out, r#"{"1":"译1","2":"译2"}"#);

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2, "应发两轮");
    // 第 2 轮请求里应包含上一轮的 tool result（feedback），且带 tool_call_id。
    let round2 = &seen[1];
    let tool_results: Vec<_> = round2.iter().filter(|m| m.role == "tool").collect();
    assert_eq!(tool_results.len(), 1);
    assert_eq!(tool_results[0].tool_call_id.as_deref(), Some("c1"));
    assert!(tool_results[0].content.as_deref().unwrap_or("").contains("无法解析译文"));
    // 也应有 assistant(tool_calls) 消息。
    assert!(round2.iter().any(|m| m.role == "assistant" && m.tool_calls.is_some()));
}

#[tokio::test]
async fn max_turns_budget_is_enforced() {
    // 每轮都失败 → max_turns 用尽 → 报错。
    let responses: Vec<_> = (0..3)
        .map(|i| tool_completion(&format!("c{i}"), r#"{"bad":"x"}"#))
        .collect();
    let (fake, seen) = scripted_fake(responses);
    let handler = TranslationToolHandler::new(2);
    let err = chat_with_tool_loop_inner("sys", "1. 原文1\n2. 原文2", &handler, &opts(), 3, &fake)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("max_turns"));
    assert_eq!(seen.lock().unwrap().len(), 3, "应发 3 轮后停止");
}

#[tokio::test]
async fn no_normalizer_passes_raw_through() {
    // 没有归一化钩子的工具：raw 原样传给 handle。
    struct EchoHandler;
    impl ToolHandler for EchoHandler {
        fn name(&self) -> &str { "echo" }
        fn description(&self) -> &str { "echo input" }
        fn schema(&self) -> serde_json::Value { serde_json::json!({"type":"object"}) }
        fn handle(&self, input: &serde_json::Value) -> Result<String, String> {
            Ok(format!("got:{}", input.as_object().unwrap().len()))
        }
    }
    let (fake, _) = scripted_fake(vec![tool_completion("c1", r#"{"a":1,"b":2}"#)]);
    let out = chat_with_tool_loop_inner("sys", "hi", &EchoHandler, &opts(), 3, &fake)
        .await
        .unwrap();
    assert_eq!(out, "got:2"); // raw 原样（2 个 key）传给 handle
}
