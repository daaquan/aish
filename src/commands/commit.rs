// SPDX-License-Identifier: MIT
use crate::commands::emit_json;
use crate::config::resolve::resolve_model;
use crate::config::Config;
use crate::git;
use crate::provider::{build_provider, ChatRequest};
use crate::tool::commit::{build_messages, postprocess};
use anyhow::{anyhow, Result};
use std::io::Write;

#[allow(clippy::too_many_arguments)]
pub async fn run(
    apply: bool,
    model: Option<String>,
    style: Option<String>,
    lang: Option<String>,
    signoff: bool,
    no_cache: bool,
    json: bool,
) -> Result<()> {
    let cfg = Config::load()?;
    let cwd = std::env::current_dir()?;

    let diff = git::staged_diff(&cwd)?;
    if diff.trim().is_empty() {
        if json {
            emit_json(&serde_json::json!({
                "committed": false,
                "message": serde_json::Value::Null,
                "note": "nothing staged",
            }));
        } else {
            println!("Nothing staged. Run `git add` first.");
        }
        return Ok(());
    }

    let alias = model.unwrap_or_else(|| cfg.commit.model.clone());
    let resolved = resolve_model(&cfg, &alias)?;

    let style = style.unwrap_or_else(|| cfg.commit.style.clone());
    let lang = lang.unwrap_or_else(|| cfg.commit.language.clone());
    let messages = build_messages(&style, &lang, &diff);

    let cache_dir = crate::cache::cache_dir();
    let cache_key = crate::cache::request_key(&resolved.provider_name, &resolved.model, &messages);

    // Deterministic cache: an identical request (same diff, model, style, language)
    // reuses the stored message and skips the model request entirely.
    let mut cached = false;
    let (message, usage) = match (!no_cache)
        .then(|| crate::cache::get(&cache_dir, &cache_key))
        .flatten()
    {
        Some(hit) => {
            cached = true;
            if !json {
                println!("(cached — no model request made)");
            }
            (hit, crate::provider::Usage::default())
        }
        None => {
            // Test hook: AISH_PROVIDER=mock returns a canned message without network.
            let provider: Box<dyn crate::provider::Provider> =
                if std::env::var("AISH_PROVIDER").as_deref() == Ok("mock") {
                    Box::new(crate::provider::mock::MockProvider::new(
                        std::env::var("AISH_MOCK_REPLY")
                            .unwrap_or_else(|_| "feat: add thing".into()),
                    ))
                } else {
                    build_provider(&resolved.provider_name, &resolved).map_err(|e| anyhow!(e))?
                };

            let resp = provider
                .chat(ChatRequest {
                    model: resolved.model.clone(),
                    messages,
                    temperature: Some(0.2),
                })
                .await
                .map_err(|e| anyhow!(e))?;

            let message = postprocess(&resp.content);
            if message.is_empty() {
                return Err(anyhow!(
                    "model returned an empty/unusable message; not committing. raw: {:?}",
                    resp.content
                ));
            }
            if !no_cache {
                let _ = crate::cache::put(&cache_dir, &cache_key, &message);
            }
            (message, resp.usage.unwrap_or_default())
        }
    };

    if !json {
        println!("\nSuggested commit:\n\n{message}\n");
    }

    let decision = if apply {
        git::commit(&cwd, &message, signoff)?;
        if !json {
            println!("Committed.");
        }
        "applied"
    } else if json {
        // JSON mode is non-interactive: emit the suggestion without committing.
        // CI that wants to commit passes `--apply --json`.
        "suggested"
    } else {
        confirm_loop(&cwd, message.clone(), signoff)?
    };

    if json {
        emit_json(&serde_json::json!({
            "message": message,
            "decision": decision,
            "committed": decision == "applied" || decision == "edited",
            "cached": cached,
            "provider": resolved.provider_name.clone(),
            "model": resolved.model.clone(),
            "prompt_tokens": usage.prompt_tokens,
            "completion_tokens": usage.completion_tokens,
        }));
    }

    let _ = crate::audit::record(&crate::audit::AuditEntry {
        tool: "git.commit.message.generate".into(),
        provider: resolved.provider_name.clone(),
        model: resolved.model.clone(),
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        decision: decision.into(),
    });
    Ok(())
}

/// Interactive accept/edit/reject loop. Re-prompts after each edit so the user
/// confirms the edited message before it is committed.
fn confirm_loop(cwd: &std::path::Path, mut message: String, signoff: bool) -> Result<&'static str> {
    let mut edited = false;
    loop {
        print!("Accept? [Y/n/e(dit)] ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        let n = std::io::stdin().read_line(&mut input)?;
        if n == 0 {
            // EOF / non-interactive (e.g. </dev/null): do not commit.
            println!("Aborted (no input).");
            return Ok("rejected");
        }
        match input.trim().to_lowercase().as_str() {
            "" | "y" | "yes" => {
                git::commit(cwd, &message, signoff)?;
                println!("Committed.");
                return Ok(if edited { "edited" } else { "applied" });
            }
            "e" | "edit" => {
                message = crate::editor::edit(&message).map_err(|e| anyhow!(e))?;
                if message.trim().is_empty() {
                    println!("Aborted (empty message).");
                    return Ok("rejected");
                }
                edited = true;
                println!("\nEdited commit:\n\n{message}\n");
            }
            _ => {
                println!("Aborted.");
                return Ok("rejected");
            }
        }
    }
}
