// SPDX-License-Identifier: MIT
use crate::provider::http::{normalize_base, send_json};
use crate::provider::{ChatRequest, ChatResponse, Provider, ProviderError, Role, Usage};
use async_trait::async_trait;
use serde::Deserialize;

pub struct OpenAiCompat {
    base_url: String,
    api_key: Option<String>,
    client: reqwest::Client,
}

impl OpenAiCompat {
    pub fn new(base_url: String, api_key: Option<String>) -> Self {
        Self {
            base_url: normalize_base(base_url),
            api_key,
            client: reqwest::Client::new(),
        }
    }
}

fn role_str(r: Role) -> &'static str {
    match r {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

#[derive(Deserialize)]
struct RespBody {
    choices: Vec<Choice>,
    usage: Option<UsageBody>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMsg,
}

#[derive(Deserialize)]
struct ChoiceMsg {
    content: String,
}

#[derive(Deserialize)]
struct UsageBody {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[async_trait]
impl Provider for OpenAiCompat {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, ProviderError> {
        let messages: Vec<_> = req
            .messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": role_str(m.role),
                    "content": m.content
                })
            })
            .collect();
        let mut body = serde_json::json!({ "model": req.model, "messages": messages });
        if let Some(t) = req.temperature {
            body["temperature"] = serde_json::json!(t);
        }

        let url = format!("{}/chat/completions", self.base_url);
        let mut rb = self.client.post(url).json(&body);
        if let Some(key) = &self.api_key {
            rb = rb.bearer_auth(key);
        }

        let parsed: RespBody = send_json(rb, &req.model).await?;
        let content = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| ProviderError::Decode("no choices in response".into()))?
            .message
            .content;
        let usage = parsed.usage.map(|u| Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
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
    async fn sends_request_and_parses_choice() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [ { "message": { "role": "assistant", "content": "feat: add x" } } ],
                "usage": { "prompt_tokens": 10, "completion_tokens": 4 }
            })))
            .mount(&server)
            .await;

        let p = OpenAiCompat::new(server.uri(), Some("sk-test".into()));
        let resp = p
            .chat(ChatRequest {
                model: "gpt-5-mini".into(),
                messages: vec![Message::user("hi")],
                temperature: Some(0.2),
            })
            .await
            .unwrap();

        assert_eq!(resp.content, "feat: add x");
        assert_eq!(resp.usage.unwrap().completion_tokens, 4);
    }

    #[tokio::test]
    async fn maps_401_to_auth_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let p = OpenAiCompat::new(server.uri(), Some("bad".into()));
        let err = p
            .chat(ChatRequest {
                model: "m".into(),
                messages: vec![],
                temperature: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, crate::provider::ProviderError::Auth));
    }
}
