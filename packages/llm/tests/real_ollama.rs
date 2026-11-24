use llm::{ChatOptions, chat_completions};

#[tokio::test]
async fn real_call_to_ollama() {
    let opts = ChatOptions {
        api_base: Some("http://127.0.0.1:11434/v1".to_string()),
        ..Default::default()
    };
    match chat_completions("say OK", &opts).await {
        Ok(s) => println!("OK response: {s}"),
        Err(e) => println!("ERR: {e:#}"),
    }
}
