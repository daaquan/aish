// SPDX-License-Identifier: MIT
use crate::provider::{ChatRequest, ChatResponse, Provider, ProviderError, Role, Usage};
use async_trait::async_trait;
use serde::Deserialize;

const DEFAULT_BASE: &str = "https://api.anthropic.com";
const API_VERSION: &str = "2023-06-01";
const MAX_TOKENS: u32 = 1024;

pub struct Anthropic {
    base_url: String,
    api_key: String,
    client: reqwest::Client,
}

impl Anthropic {
    pub fn new(api_key: String) -> Self {
        Self::with_base(DEFAULT_BASE.to_string(), api_key)
    }

    pub fn with_base(base_url: String, api_key: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            client: reqwest::Client::new(),
        }
    }
}

#[derive(Deserialize)]
struct RespBody {
    content: Vec<Block>,
    usage: Option<UsageBody>,
}

#[derive(Deserialize)]
struct Block {
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct UsageBody {
    input_tokens: u32,
    output_tokens: u32,
}

#[async_trait]
impl Provider for Anthropic {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, ProviderError> {
        // Anthropic takes `system` as a top-level field, not a message.
        let system: String = req
            .messages
            .iter()
            .filter(|m| matches!(m.role, Role::System))
            .map(|m| m.content.clone())
            .collect::<Vec<_>>()
            .join("\n");

        let messages: Vec<_> = req
            .messages
            .iter()
            .filter(|m| !matches!(m.role, Role::System))
            .map(|m| {
                serde_json::json!({
                    "role": match m.role { Role::Assistant => "assistant", _ => "user" },
                    "content": m.content
                })
            })
            .collect();

        let mut body = serde_json::json!({
            "model": req.model,
            "max_tokens": MAX_TOKENS,
            "messages": messages
        });
        if !system.is_empty() {
            body["system"] = serde_json::json!(system);
        }
        if let Some(t) = req.temperature {
            body["temperature"] = serde_json::json!(t);
        }

        let url = format!("{}/v1/messages", self.base_url);
        let resp = self
            .client
            .post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Request(e.to_string()))?;

        match resp.status().as_u16() {
            200 => {}
            401 => return Err(ProviderError::Auth),
            429 => return Err(ProviderError::RateLimited),
            400 | 404 => return Err(ProviderError::BadModel(req.model.clone())),
            s => return Err(ProviderError::Request(format!("HTTP {s}"))),
        }

        let parsed: RespBody = resp
            .json()
            .await
            .map_err(|e| ProviderError::Decode(e.to_string()))?;

        let content = parsed
            .content
            .into_iter()
            .map(|b| b.text)
            .collect::<Vec<_>>()
            .join("");

        let usage = parsed.usage.map(|u| Usage {
            prompt_tokens: u.input_tokens,
            completion_tokens: u.output_tokens,
        });

        Ok(ChatResponse { content, usage })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ChatRequest, Message, Provider};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn sends_system_separately_and_parses_text() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "content": [ { "type": "text", "text": "fix: handle nil" } ],
                "usage": { "input_tokens": 8, "output_tokens": 3 }
            })))
            .mount(&server)
            .await;

        let p = Anthropic::with_base(server.uri(), "sk-ant".into());
        let resp = p
            .chat(ChatRequest {
                model: "claude-opus-4-8".into(),
                messages: vec![Message::system("be terse"), Message::user("diff...")],
                temperature: None,
            })
            .await
            .unwrap();

        assert_eq!(resp.content, "fix: handle nil");
        assert_eq!(resp.usage.unwrap().completion_tokens, 3);
    }
}
