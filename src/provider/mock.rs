// SPDX-License-Identifier: AGPL-3.0-only
use crate::provider::{ChatRequest, ChatResponse, Provider, ProviderError, Usage};
use async_trait::async_trait;

/// Deterministic provider for tests. Returns a fixed message.
pub struct MockProvider {
    pub reply: String,
}

impl MockProvider {
    pub fn new(reply: impl Into<String>) -> Self {
        Self {
            reply: reply.into(),
        }
    }
}

#[async_trait]
impl Provider for MockProvider {
    async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, ProviderError> {
        Ok(ChatResponse {
            content: self.reply.clone(),
            usage: Some(Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
            }),
        })
    }
}
