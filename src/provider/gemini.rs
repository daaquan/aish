// SPDX-License-Identifier: MIT
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
        if let Some(t) = req.temperature {
            body["generationConfig"] = serde_json::json!({ "temperature": t });
        }

        let url = format!(
            "{}/v1beta/models/{}:generateContent",
            self.base_url, req.model
        );
        let resp = self
            .client
            .post(url)
            .header("x-goog-api-key", &self.api_key)
            .json(&body)
            .send()
            .await
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
        use wiremock::matchers::header;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("x-goog-api-key", "gk"))
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
    async fn sends_temperature_in_generation_config() {
        use wiremock::matchers::body_partial_json;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_partial_json(
                serde_json::json!({ "generationConfig": {} }),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "candidates": [ { "content": { "parts": [ { "text": "chore: x" } ] } } ]
            })))
            .mount(&server)
            .await;
        let p = Gemini::with_base(server.uri(), "gk".into());
        let resp = p
            .chat(ChatRequest {
                model: "gemini-2.5-pro".into(),
                messages: vec![Message::user("d")],
                temperature: Some(0.2),
            })
            .await
            .unwrap();
        assert_eq!(resp.content, "chore: x");
        // Also verify the temperature value is correct in the sent body.
        let reqs = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
        let temp = body["generationConfig"]["temperature"]
            .as_f64()
            .expect("generationConfig.temperature must be a number");
        assert!(
            (temp - 0.2_f64).abs() < 1e-4,
            "expected temperature ~0.2, got {temp}"
        );
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
