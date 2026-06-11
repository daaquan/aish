// SPDX-License-Identifier: MIT
//! Shared "messages -> model reply" pipeline used by every generating command:
//! deterministic cache lookup, mock-provider test hook, provider request, and
//! cache write-back.

use crate::config::resolve::Resolved;
use crate::provider::{build_provider, ChatRequest, Message, Usage};
use anyhow::{anyhow, Result};

pub(crate) struct Generated {
    pub raw: String,
    pub usage: Usage,
    pub cached: bool,
}

/// Run `messages` through the cache and the resolved provider.
///
/// An identical request (same provider, model, and messages) is served from
/// the cache without a model call unless `no_cache` is set. In non-JSON mode
/// a cache hit prints a note so the user knows no request was made.
pub(crate) async fn generate(
    resolved: &Resolved<'_>,
    messages: Vec<Message>,
    no_cache: bool,
    json: bool,
) -> Result<Generated> {
    let cache_dir = crate::cache::cache_dir();
    let cache_key = crate::cache::request_key(&resolved.provider_name, &resolved.model, &messages);

    if let Some(hit) = (!no_cache)
        .then(|| crate::cache::get(&cache_dir, &cache_key))
        .flatten()
    {
        if !json {
            println!("(cached — no model request made)");
        }
        return Ok(Generated {
            raw: hit,
            usage: Usage::default(),
            cached: true,
        });
    }

    // Test hook: AISH_PROVIDER=mock returns a canned reply without network.
    let provider: Box<dyn crate::provider::Provider> =
        if std::env::var("AISH_PROVIDER").as_deref() == Ok("mock") {
            Box::new(crate::provider::mock::MockProvider::new(
                std::env::var("AISH_MOCK_REPLY").unwrap_or_else(|_| "feat: add thing".into()),
            ))
        } else {
            build_provider(&resolved.provider_name, resolved).map_err(|e| anyhow!(e))?
        };

    let resp = provider
        .chat(ChatRequest {
            model: resolved.model.clone(),
            messages,
            temperature: Some(0.2),
        })
        .await
        .map_err(|e| anyhow!(e))?;

    if !no_cache {
        let _ = crate::cache::put(&cache_dir, &cache_key, &resp.content);
    }
    Ok(Generated {
        raw: resp.content,
        usage: resp.usage.unwrap_or_default(),
        cached: false,
    })
}
