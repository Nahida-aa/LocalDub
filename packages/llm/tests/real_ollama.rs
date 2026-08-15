use llm::{ChatOptions, chat_completions};

#[test]
fn real_call_to_ollama() {
    let opts = ChatOptions {
        api_base: Some("http://127.0.0.1:11434/v1".to_string()),
        ..Default::default()
    };
    match chat_completions("say OK", &opts) {
        Ok(s) => println!("OK response: {s}"),
        Err(e) => println!("ERR: {e:#}"),
    }
}
