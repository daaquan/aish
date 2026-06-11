// SPDX-License-Identifier: MIT
use crate::commands::emit_json;
use crate::config::{Config, IssueLevel};
use anyhow::{anyhow, Context, Result};

pub fn init(force: bool, json: bool) -> Result<()> {
    let path = Config::default_path();
    Config::write_template(&path, force)
        .with_context(|| format!("writing config to {}", path.display()))?;
    if json {
        emit_json(&serde_json::json!({ "wrote": path.display().to_string() }));
    } else {
        println!("Wrote config template to {}", path.display());
    }
    Ok(())
}

pub async fn check(ping: bool, json: bool) -> Result<()> {
    let cfg = Config::load()?;
    let issues = cfg.validate();
    let errors = issues
        .iter()
        .filter(|i| i.level == IssueLevel::Error)
        .count();

    // Live checks only run when static validation found no errors.
    let pings = if ping && errors == 0 {
        Some(ping_providers(&cfg).await)
    } else {
        None
    };
    let failed_pings = pings
        .as_deref()
        .map(|p| p.iter().filter(|r| r.error.is_some()).count())
        .unwrap_or(0);

    if json {
        let rows: Vec<_> = issues
            .iter()
            .map(|i| {
                let level = match i.level {
                    IssueLevel::Error => "error",
                    IssueLevel::Warning => "warning",
                };
                serde_json::json!({ "level": level, "message": i.message })
            })
            .collect();
        let mut envelope = serde_json::json!({
            "ok": errors == 0 && failed_pings == 0,
            "providers": cfg.providers.len(),
            "models": cfg.models.len(),
            "issues": rows,
        });
        if let Some(pings) = &pings {
            let rows: Vec<_> = pings
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "provider": r.provider,
                        "model": r.model,
                        "status": if r.error.is_none() { "ok" } else { "fail" },
                        "error": r.error,
                    })
                })
                .collect();
            envelope["ping"] = serde_json::json!(rows);
        }
        emit_json(&envelope);
    } else {
        if issues.is_empty() {
            println!(
                "Config OK: {} provider(s), {} model alias(es).",
                cfg.providers.len(),
                cfg.models.len()
            );
        } else {
            for issue in &issues {
                let tag = match issue.level {
                    IssueLevel::Error => "error",
                    IssueLevel::Warning => "warning",
                };
                println!("{tag}: {}", issue.message);
            }
        }
        if let Some(pings) = &pings {
            for r in pings {
                match (&r.model, &r.error) {
                    (_, Some(e)) => println!("{}: FAIL — {e}", r.provider),
                    (Some(m), None) => println!("{} ({m}): OK", r.provider),
                    (None, None) => println!("{}: SKIPPED (no model alias)", r.provider),
                }
            }
        }
    }

    // Nonzero exit on errors regardless of format, so CI gates fail correctly.
    if errors > 0 {
        return Err(anyhow!("config has {errors} error(s)"));
    }
    if failed_pings > 0 {
        return Err(anyhow!("{failed_pings} provider ping(s) failed"));
    }
    Ok(())
}

struct PingResult {
    provider: String,
    /// Model used for the request; None when the provider has no alias.
    model: Option<String>,
    error: Option<String>,
}

/// Send one minimal chat request per configured provider, using the first
/// model alias that maps to it. Providers without an alias are skipped.
async fn ping_providers(cfg: &Config) -> Vec<PingResult> {
    let mut results = Vec::new();
    for name in cfg.providers.keys() {
        let alias = cfg.models.iter().find(|(_, m)| &m.provider == name);
        let Some((alias, _)) = alias else {
            results.push(PingResult {
                provider: name.clone(),
                model: None,
                error: None,
            });
            continue;
        };
        let resolved = match crate::config::resolve::resolve_model(cfg, alias) {
            Ok(r) => r,
            Err(e) => {
                results.push(PingResult {
                    provider: name.clone(),
                    model: None,
                    error: Some(e.to_string()),
                });
                continue;
            }
        };
        results.push(PingResult {
            provider: name.clone(),
            model: Some(resolved.model.clone()),
            error: ping_one(name, &resolved).await.err(),
        });
    }
    results
}

/// One minimal request against a single provider. Returns the failure reason.
async fn ping_one(
    name: &str,
    resolved: &crate::config::resolve::Resolved<'_>,
) -> Result<(), String> {
    use crate::provider::{ChatRequest, Message, ProviderError};

    // Test hooks: AISH_PROVIDER=mock answers without network;
    // AISH_MOCK_FAIL=<provider> simulates an auth failure for that provider.
    let provider: Box<dyn crate::provider::Provider> =
        if std::env::var("AISH_PROVIDER").as_deref() == Ok("mock") {
            if std::env::var("AISH_MOCK_FAIL").as_deref() == Ok(name) {
                return Err(ProviderError::Auth.to_string());
            }
            Box::new(crate::provider::mock::MockProvider::new("OK"))
        } else {
            crate::provider::build_provider(name, resolved).map_err(|e| e.to_string())?
        };

    provider
        .chat(ChatRequest {
            model: resolved.model.clone(),
            messages: vec![Message::user("Reply with exactly: OK")],
            temperature: Some(0.0),
        })
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}
