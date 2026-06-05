// SPDX-License-Identifier: AGPL-3.0-only
use std::time::Duration;

use async_trait::async_trait;

use crate::provider::{ChatRequest, ChatResponse, Provider, ProviderError};

/// Default number of retries attempted after the initial request.
pub const DEFAULT_MAX_RETRIES: u32 = 3;
/// Default backoff before the first retry; doubles each subsequent attempt.
pub const DEFAULT_BASE_DELAY: Duration = Duration::from_millis(500);

/// Wraps a provider and transparently retries transient HTTP 429 responses
/// (`ProviderError::RateLimited`) with exponential backoff. Other errors are
/// non-transient and propagate immediately.
pub struct RetryProvider {
    inner: Box<dyn Provider>,
    max_retries: u32,
    base_delay: Duration,
}

impl RetryProvider {
    /// Wrap `inner` with the default retry policy.
    pub fn new(inner: Box<dyn Provider>) -> Self {
        Self::with_policy(inner, DEFAULT_MAX_RETRIES, DEFAULT_BASE_DELAY)
    }

    /// Wrap `inner` with an explicit policy. Used by tests to shrink delays.
    pub fn with_policy(inner: Box<dyn Provider>, max_retries: u32, base_delay: Duration) -> Self {
        Self {
            inner,
            max_retries,
            base_delay,
        }
    }

    /// Backoff before the retry following `attempt` (0-based): base * 2^attempt.
    fn backoff(&self, attempt: u32) -> Duration {
        self.base_delay.saturating_mul(1u32 << attempt.min(16))
    }
}

#[async_trait]
impl Provider for RetryProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, ProviderError> {
        let mut attempt = 0;
        loop {
            match self.inner.chat(req.clone()).await {
                Err(ProviderError::RateLimited) if attempt < self.max_retries => {
                    tokio::time::sleep(self.backoff(attempt)).await;
                    attempt += 1;
                }
                other => return other,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Message, Usage};
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Fails with `RateLimited` for the first `fail_times` calls, then succeeds.
    struct FlakyProvider {
        fail_times: u32,
        calls: AtomicU32,
    }

    #[async_trait]
    impl Provider for FlakyProvider {
        async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, ProviderError> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if n < self.fail_times {
                Err(ProviderError::RateLimited)
            } else {
                Ok(ChatResponse {
                    content: "ok".into(),
                    usage: Some(Usage::default()),
                })
            }
        }
    }

    fn req() -> ChatRequest {
        ChatRequest {
            model: "m".into(),
            messages: vec![Message::user("hi")],
            temperature: None,
        }
    }

    fn flaky(fail_times: u32) -> Box<FlakyProvider> {
        Box::new(FlakyProvider {
            fail_times,
            calls: AtomicU32::new(0),
        })
    }

    #[tokio::test]
    async fn retries_until_success() {
        let inner = flaky(2);
        let p = RetryProvider::with_policy(inner, 3, Duration::from_millis(1));
        let resp = p.chat(req()).await.unwrap();
        assert_eq!(resp.content, "ok");
    }

    #[tokio::test]
    async fn gives_up_after_max_retries() {
        // Fails more times than the policy allows: 1 initial + 2 retries = 3 calls.
        let inner = flaky(100);
        let p = RetryProvider::with_policy(inner, 2, Duration::from_millis(1));
        let err = p.chat(req()).await.unwrap_err();
        assert!(matches!(err, ProviderError::RateLimited));
    }

    #[tokio::test]
    async fn does_not_retry_non_rate_limit_errors() {
        use std::sync::Arc;
        struct AlwaysAuth {
            calls: Arc<AtomicU32>,
        }
        #[async_trait]
        impl Provider for AlwaysAuth {
            async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, ProviderError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Err(ProviderError::Auth)
            }
        }
        let calls = Arc::new(AtomicU32::new(0));
        let inner = Box::new(AlwaysAuth {
            calls: calls.clone(),
        });
        let p = RetryProvider::with_policy(inner, 5, Duration::from_millis(1));
        let err = p.chat(req()).await.unwrap_err();
        assert!(matches!(err, ProviderError::Auth));
        // Only the initial attempt — no retries on a non-transient error.
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn backoff_grows_exponentially() {
        let p = RetryProvider::with_policy(flaky(0), 3, Duration::from_millis(100));
        assert_eq!(p.backoff(0), Duration::from_millis(100));
        assert_eq!(p.backoff(1), Duration::from_millis(200));
        assert_eq!(p.backoff(2), Duration::from_millis(400));
    }
}
