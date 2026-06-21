// SPDX-License-Identifier: MIT
//! Shared HTTP plumbing for the real providers: one place that sends a
//! request, maps status codes to [`ProviderError`], and decodes the JSON
//! body. Providers keep what actually differs between them — URL, auth
//! headers, and request/response shapes.

use crate::provider::ProviderError;
use serde::de::DeserializeOwned;

/// Strip a trailing `/` so providers can join paths with a plain `format!`.
pub(crate) fn normalize_base(base_url: String) -> String {
    base_url.trim_end_matches('/').to_string()
}

/// Try to extract a human-readable error message from a JSON error body.
///
/// Ollama double-encodes: `{"error":{"message":"{\"error\":{\"message\":\"...\"}}"}}`  
/// — so we try to parse the inner message too. Other providers differ.
/// Falls back to a truncated copy of the raw body.
fn extract_error_body(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(msg) = v["error"]["message"].as_str() {
            // Ollama may return the message as nested JSON — unwrap one level.
            if let Ok(inner) = serde_json::from_str::<serde_json::Value>(msg) {
                if let Some(inner_msg) = inner["error"]["message"].as_str() {
                    return inner_msg.to_string();
                }
            }
            return msg.to_string();
        }
    }
    // Keep it short — bodies can be large.
    let limit = 300;
    if body.chars().count() > limit {
        let truncated: String = body.chars().take(limit).collect();
        format!("{truncated}...")
    } else {
        body.to_string()
    }
}

/// Send a prepared request and decode the JSON response.
///
/// Status mapping is uniform across providers: 401/403 → `Auth`,
/// 429 → `RateLimited`, 404 → `BadModel(model)`, 400 → `BadRequest(detail)`,
/// anything else non-200 → `Request`. Transport errors are reported via
/// [`reqwest::Error::without_url`] so an api_key embedded in a URL can
/// never leak into error output.
pub(crate) async fn send_json<T: DeserializeOwned>(
    rb: reqwest::RequestBuilder,
    model: &str,
) -> Result<T, ProviderError> {
    let resp = rb
        .send()
        .await
        .map_err(|e| ProviderError::Request(format!("request failed: {}", e.without_url())))?;

    match resp.status().as_u16() {
        200 => {}
        401 | 403 => return Err(ProviderError::Auth),
        429 => return Err(ProviderError::RateLimited),
        400 => {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::BadRequest(extract_error_body(&body)));
        }
        404 => return Err(ProviderError::BadModel(model.to_string())),
        s => return Err(ProviderError::Request(format!("HTTP {s}"))),
    }

    resp.json::<T>()
        .await
        .map_err(|e| ProviderError::Decode(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[derive(Debug, serde::Deserialize)]
    struct Body {
        ok: bool,
    }

    async fn status_err(status: u16) -> ProviderError {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(status))
            .mount(&server)
            .await;
        let rb = reqwest::Client::new().post(server.uri());
        send_json::<Body>(rb, "test-model").await.unwrap_err()
    }

    #[tokio::test]
    async fn maps_statuses_to_provider_errors() {
        assert!(matches!(status_err(401).await, ProviderError::Auth));
        assert!(matches!(status_err(403).await, ProviderError::Auth));
        assert!(matches!(status_err(429).await, ProviderError::RateLimited));
        assert!(matches!(
            status_err(400).await,
            ProviderError::BadRequest(_)
        ));
        assert!(matches!(
            status_err(404).await,
            ProviderError::BadModel(m) if m == "test-model"
        ));
        assert!(matches!(status_err(500).await, ProviderError::Request(_)));
    }

    #[tokio::test]
    async fn decodes_ok_body_and_flags_bad_json() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
            .mount(&server)
            .await;
        let rb = reqwest::Client::new().post(server.uri());
        let body: Body = send_json(rb, "m").await.unwrap();
        assert!(body.ok);

        let server2 = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server2)
            .await;
        let rb2 = reqwest::Client::new().post(server2.uri());
        let err = send_json::<Body>(rb2, "m").await.unwrap_err();
        assert!(matches!(err, ProviderError::Decode(_)));
    }

    #[tokio::test]
    async fn transport_error_does_not_leak_url_contents() {
        // Closed port; key smuggled into the URL must not appear in the error.
        let rb = reqwest::Client::new().post("http://127.0.0.1:1/?key=SUPER_SECRET_KEY");
        let err = send_json::<Body>(rb, "m").await.unwrap_err();
        assert!(
            !err.to_string().contains("SUPER_SECRET_KEY"),
            "url leaked in error: {err}"
        );
    }

    #[test]
    fn extract_error_parses_ollama_style_json() {
        let body = r#"{"error":{"message":"request exceeds context size","type":"invalid"}}"#;
        let s = extract_error_body(body);
        assert_eq!(s, "request exceeds context size");
    }

    #[test]
    fn extract_error_unwraps_nested_ollama_message() {
        let body = r#"{"error":{"message":"{\"error\":{\"code\":400,\"message\":\"request (8583 tokens) exceeds the available context size (4096 tokens)\",\"type\":\"exceed_context_size_error\",\"n_prompt_tokens\":8583,\"n_ctx\":4096}}","type":"invalid_request_error"}}"#;
        let s = extract_error_body(body);
        assert_eq!(
            s,
            "request (8583 tokens) exceeds the available context size (4096 tokens)"
        );
    }

    #[test]
    fn extract_error_falls_back_to_raw_body() {
        let s = extract_error_body("plain text error");
        assert_eq!(s, "plain text error");
    }

    #[test]
    fn extract_error_truncates_long_body() {
        let long = format!("{}xxx", "x".repeat(500));
        let s = extract_error_body(&long);
        assert!(s.ends_with("..."));
        assert!(s.len() <= 303); // limit(300) + "..."
    }

    #[test]
    fn extract_error_truncates_multibyte_body_without_panic() {
        // A multibyte char straddling the 300-byte boundary would panic on byte slicing.
        let long = "あ".repeat(400);
        let s = extract_error_body(&long);
        assert!(s.ends_with("..."));
        assert_eq!(s.chars().count(), 303); // 300 chars + "..."
    }

    #[tokio::test]
    async fn bad_request_400_reads_error_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(400).set_body_string(
                r#"{"error":{"message":"context length exceeded (25000 tokens > 4096)"}}"#,
            ))
            .mount(&server)
            .await;
        let rb = reqwest::Client::new().post(server.uri());
        let err = send_json::<Body>(rb, "test-model").await.unwrap_err();
        assert!(matches!(err, ProviderError::BadRequest(ref s) if s.contains("context length")));
    }

    #[test]
    fn normalize_base_strips_trailing_slash() {
        assert_eq!(normalize_base("http://x/".into()), "http://x");
        assert_eq!(normalize_base("http://x".into()), "http://x");
    }
}
