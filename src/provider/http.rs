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

/// Send a prepared request and decode the JSON response.
///
/// Status mapping is uniform across providers: 401/403 → `Auth`,
/// 429 → `RateLimited`, 400/404 → `BadModel(model)`, anything else
/// non-200 → `Request`. Transport errors are reported via
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
        400 | 404 => return Err(ProviderError::BadModel(model.to_string())),
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
            ProviderError::BadModel(m) if m == "test-model"
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
    fn normalize_base_strips_trailing_slash() {
        assert_eq!(normalize_base("http://x/".into()), "http://x");
        assert_eq!(normalize_base("http://x".into()), "http://x");
    }
}
