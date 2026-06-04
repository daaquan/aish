// SPDX-License-Identifier: AGPL-3.0-only
use async_trait::async_trait;
use thiserror::Error;

pub mod anthropic;
pub mod gemini;
pub mod mock;
pub mod openai;

#[derive(Debug, Clone, Copy)]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn system(c: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: c.into(),
        }
    }

    pub fn user(c: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: c.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: String,
    pub usage: Option<Usage>,
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("authentication failed (HTTP 401) — check the provider api_key")]
    Auth,
    #[error("rate limited (HTTP 429) — retry later")]
    RateLimited,
    #[error("model `{0}` was rejected by the provider")]
    BadModel(String),
    #[error("provider request failed: {0}")]
    Request(String),
    #[error("could not parse provider response: {0}")]
    Decode(String),
}

#[async_trait]
pub trait Provider: Send + Sync {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_constructors_set_roles() {
        let s = Message::system("sys");
        let u = Message::user("hi");
        assert!(matches!(s.role, Role::System));
        assert!(matches!(u.role, Role::User));
        assert_eq!(u.content, "hi");
    }
}
