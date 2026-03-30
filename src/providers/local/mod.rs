use serde_json::json;
use serde_json::Value;
use crate::providers::{LlmProvider, Message, Role};
use async_trait::async_trait;

pub struct LocalMock {}

impl LocalMock {
    pub fn new() -> Self { Self {} }
}

#[async_trait]
impl LlmProvider for LocalMock {
    async fn complete_chat(&self, messages: Vec<Message>) -> Value {
        // Simple deterministic mock response: echo last user message
        let m = messages.into_iter().rev().find(|m| matches!(m.role, Role::User)).expect("no user message");
        let echo = format!("echo: {}", m.content);
        json!({
            "source": "local-mock",
            "response": echo
        })
    }
}
