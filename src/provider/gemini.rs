// SPDX-License-Identifier: AGPL-3.0-only
use crate::provider::{ChatRequest, ChatResponse, Provider, ProviderError, Role, Usage};
use async_trait::async_trait;
use serde::Deserialize;

const DEFAULT_BASE: &str = "https://generativelanguage.googleapis.com";

pub struct Gemini {
    base_url: String,
    api_key: String,
    client: reqwest::Client,
}

impl Gemini {
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
    candidates: Vec<Candidate>,
    #[serde(rename = "usageMetadata")]
    usage: Option<UsageBody>,
}

#[derive(Deserialize)]
struct Candidate {
    content: Content,
}

#[derive(Deserialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Deserialize)]
struct Part {
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct UsageBody {
    #[serde(rename = "promptTokenCount")]
    prompt: u32,
    #[serde(rename = "candidatesTokenCount")]
    candidates: u32,
}

#[async_trait]
impl Provider for Gemini {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, ProviderError> {
        // Gemini folds system text into systemInstruction; other messages → contents.
        let system: String = req
            .messages
            .iter()
            .filter(|m| matches!(m.role, Role::System))
            .map(|m| m.content.clone())
            .collect::<Vec<_>>()
            .join("\n");

        let contents: Vec<_> = req
            .messages
            .iter()
            .filter(|m| !matches!(m.role, Role::System))
            .map(|m| {
                serde_json::json!({
                    "role": match m.role { Role::Assistant => "model", _ => "user" },
                    "parts": [ { "text": m.content } ]
                })
            })
            .collect();

        let mut body = serde_json::json!({ "contents": contents });
        if !system.is_empty() {
            body["systemInstruction"] = serde_json::json!({ "parts": [ { "text": system } ] });
        }

        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            self.base_url, req.model, self.api_key
        );
        let resp = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await
            // Strip the URL: it carries the api_key in its query string.
            .map_err(|e| {
                ProviderError::Request(format!("request to Gemini failed: {}", e.without_url()))
            })?;

        match resp.status().as_u16() {
            200 => {}
            401 | 403 => return Err(ProviderError::Auth),
            429 => return Err(ProviderError::RateLimited),
            400 | 404 => return Err(ProviderError::BadModel(req.model.clone())),
            s => return Err(ProviderError::Request(format!("HTTP {s}"))),
        }

        let parsed: RespBody = resp
            .json()
            .await
            .map_err(|e| ProviderError::Decode(e.to_string()))?;

        let content = parsed
            .candidates
            .into_iter()
            .next()
            .and_then(|c| c.content.parts.into_iter().next())
            .map(|p| p.text)
            .ok_or_else(|| ProviderError::Decode("no candidate text".into()))?;

        let usage = parsed.usage.map(|u| Usage {
            prompt_tokens: u.prompt,
            completion_tokens: u.candidates,
        });

        Ok(ChatResponse { content, usage })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ChatRequest, Message, Provider};
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn parses_candidate_text() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "candidates": [ { "content": { "parts": [ { "text": "chore: bump deps" } ] } } ],
                "usageMetadata": { "promptTokenCount": 12, "candidatesTokenCount": 5 }
            })))
            .mount(&server)
            .await;

        let p = Gemini::with_base(server.uri(), "gk".into());
        let resp = p
            .chat(ChatRequest {
                model: "gemini-2.5-pro".into(),
                messages: vec![Message::user("diff")],
                temperature: None,
            })
            .await
            .unwrap();

        assert_eq!(resp.content, "chore: bump deps");
        assert_eq!(resp.usage.unwrap().prompt_tokens, 12);
    }

    #[tokio::test]
    async fn request_error_does_not_leak_api_key() {
        // Point at a closed port so the request fails at the transport layer.
        let p = Gemini::with_base("http://127.0.0.1:1".into(), "SUPER_SECRET_KEY".into());
        let err = p
            .chat(ChatRequest {
                model: "gemini-2.5-pro".into(),
                messages: vec![Message::user("x")],
                temperature: None,
            })
            .await
            .unwrap_err();
        assert!(
            !err.to_string().contains("SUPER_SECRET_KEY"),
            "api_key leaked in error: {err}"
        );
    }
}
