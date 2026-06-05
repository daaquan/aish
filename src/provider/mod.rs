// SPDX-License-Identifier: MIT
use async_trait::async_trait;
use thiserror::Error;

use crate::config::resolve::Resolved;

pub mod anthropic;
pub mod gemini;
pub mod mock;
pub mod openai;
pub mod retry;

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

/// Build a provider from a resolved model. `provider_name` selects the API shape:
/// `anthropic` and `google`/`gemini` use native adapters; everything else
/// (openai, ollama, kilo, …) is treated as OpenAI-compatible and uses `base_url`
/// (defaulting to the public OpenAI endpoint when absent).
pub fn build_provider(
    provider_name: &str,
    r: &Resolved,
) -> Result<Box<dyn Provider>, ProviderError> {
    let pc = r.provider;
    let inner: Box<dyn Provider> = match provider_name {
        "anthropic" => {
            let key = pc.api_key.clone().ok_or_else(|| {
                ProviderError::Request("anthropic provider missing api_key".into())
            })?;
            Box::new(anthropic::Anthropic::new(key))
        }
        "google" | "gemini" => {
            let key = pc
                .api_key
                .clone()
                .ok_or_else(|| ProviderError::Request("google provider missing api_key".into()))?;
            Box::new(gemini::Gemini::new(key))
        }
        _ => {
            let base = pc
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".into());
            Box::new(openai::OpenAiCompat::new(base, pc.api_key.clone()))
        }
    };
    // Every real provider is wrapped so transient 429s are retried with backoff.
    Ok(Box::new(retry::RetryProvider::new(inner)))
}

#[cfg(test)]
mod factory_tests {
    use super::*;
    use crate::config::resolve::resolve_model;
    use crate::config::Config;

    fn cfg() -> Config {
        Config::from_yaml(
            r#"
providers:
  anthropic: { api_key: sk-a }
  openai: { api_key: sk-o }
  ollama: { base_url: http://localhost:11434/v1 }
  google: { api_key: gk }
models:
  a: { provider: anthropic, model: claude-opus-4-8 }
  o: { provider: openai, model: gpt-5-mini }
  l: { provider: ollama, model: qwen3-coder }
  g: { provider: google, model: gemini-2.5-pro }
commit: { style: conventional, language: en, model: o }
"#,
        )
        .unwrap()
    }

    #[test]
    fn builds_each_provider_kind() {
        let c = cfg();
        for alias in ["a", "o", "l", "g"] {
            let r = resolve_model(&c, alias).unwrap();
            assert!(
                build_provider(&r.provider_name, &r).is_ok(),
                "failed to build provider for alias {alias}"
            );
        }
    }

    #[test]
    fn provider_missing_key_errors() {
        let bad = Config::from_yaml(
            r#"
providers:
  anthropic: {}
models:
  a: { provider: anthropic, model: claude-opus-4-8 }
commit: { style: conventional, language: en, model: a }
"#,
        )
        .unwrap();
        let r = resolve_model(&bad, "a").unwrap();
        assert!(build_provider("anthropic", &r).is_err());
    }
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
