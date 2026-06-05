// SPDX-License-Identifier: MIT
use aish::audit;
use aish::cli::{Cli, Command, ConfigAction, ModelsAction, ProvidersAction};
use aish::config::resolve::resolve_model;
use aish::config::Config;
use aish::git;
use aish::provider::{build_provider, ChatRequest};
use aish::tool::commit::{build_messages, postprocess};
use anyhow::{anyhow, Context, Result};
use clap::Parser;
use std::io::Write;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let verbose = cli.verbose;
    if let Err(e) = run(cli).await {
        if verbose {
            eprintln!("error: {e:?}");
        } else {
            eprintln!("error: {e}");
        }
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    let json = cli.json;
    match cli.command {
        Command::Config { action } => match action {
            ConfigAction::Init { force } => {
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
            ConfigAction::Check => run_config_check(json),
        },
        Command::Providers {
            action: ProvidersAction::List,
        } => {
            let cfg = Config::load()?;
            if json {
                let rows: Vec<_> = cfg
                    .providers
                    .iter()
                    .map(|(name, p)| {
                        serde_json::json!({
                            "name": name,
                            "api_key": p.api_key.is_some(),
                            "base_url": p.base_url,
                        })
                    })
                    .collect();
                emit_json(&serde_json::json!(rows));
            } else {
                for (name, p) in &cfg.providers {
                    let status = if p.api_key.is_some() {
                        "key set"
                    } else if p.base_url.is_some() {
                        "endpoint set"
                    } else {
                        "unconfigured"
                    };
                    println!("{name:12} {status}");
                }
            }
            Ok(())
        }
        Command::Models {
            action: ModelsAction::List,
        } => {
            let cfg = Config::load()?;
            if json {
                let rows: Vec<_> = cfg
                    .models
                    .iter()
                    .map(|(alias, m)| {
                        serde_json::json!({
                            "alias": alias,
                            "provider": m.provider,
                            "model": m.model,
                        })
                    })
                    .collect();
                emit_json(&serde_json::json!(rows));
            } else {
                for (alias, m) in &cfg.models {
                    println!("{alias:10} -> {}/{}", m.provider, m.model);
                }
            }
            Ok(())
        }
        Command::Usage => {
            let cfg = Config::load()?;
            let lines =
                aish::usage::read_log(&audit::log_path()).with_context(|| "reading audit log")?;
            let summary = aish::usage::summarize(lines, &cfg.pricing);
            if json {
                emit_json(&aish::usage::to_json(&summary));
            } else {
                print!("{}", aish::usage::render(&summary));
            }
            Ok(())
        }
        Command::Commit {
            apply,
            model,
            style,
            lang,
            signoff,
            no_cache,
        } => run_commit(apply, model, style, lang, signoff, no_cache, json).await,
    }
}

/// Print a JSON value to stdout (pretty-printed), the single sink for `--json` output.
fn emit_json(value: &serde_json::Value) {
    println!("{}", serde_json::to_string_pretty(value).unwrap());
}

fn run_config_check(json: bool) -> Result<()> {
    use aish::config::IssueLevel;
    let cfg = Config::load()?;
    let issues = cfg.validate();
    let errors = issues
        .iter()
        .filter(|i| i.level == IssueLevel::Error)
        .count();

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
        emit_json(&serde_json::json!({
            "ok": errors == 0,
            "providers": cfg.providers.len(),
            "models": cfg.models.len(),
            "issues": rows,
        }));
    } else if issues.is_empty() {
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

    // Nonzero exit on errors regardless of format, so CI gates fail correctly.
    if errors > 0 {
        return Err(anyhow!("config has {errors} error(s)"));
    }
    Ok(())
}

async fn run_commit(
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

    let cache_dir = aish::cache::cache_dir();
    let cache_key = aish::cache::request_key(&resolved.provider_name, &resolved.model, &messages);

    // Deterministic cache: an identical request (same diff, model, style, language)
    // reuses the stored message and skips the model request entirely.
    let mut cached = false;
    let (message, usage) = match (!no_cache)
        .then(|| aish::cache::get(&cache_dir, &cache_key))
        .flatten()
    {
        Some(hit) => {
            cached = true;
            if !json {
                println!("(cached — no model request made)");
            }
            (hit, aish::provider::Usage::default())
        }
        None => {
            // Test hook: AISH_PROVIDER=mock returns a canned message without network.
            let provider: Box<dyn aish::provider::Provider> =
                if std::env::var("AISH_PROVIDER").as_deref() == Ok("mock") {
                    Box::new(aish::provider::mock::MockProvider::new(
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
                let _ = aish::cache::put(&cache_dir, &cache_key, &message);
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
        print!("Accept? [Y/n/e(dit)] ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        let n = std::io::stdin().read_line(&mut input)?;
        if n == 0 {
            // EOF / non-interactive (e.g. </dev/null): do not commit.
            println!("Aborted (no input).");
            "rejected"
        } else {
            let answer = input.trim().to_lowercase();
            if answer == "e" || answer == "edit" {
                let edited = aish::editor::edit(&message).map_err(|e| anyhow!(e))?;
                if edited.trim().is_empty() {
                    println!("Aborted (empty message).");
                    "rejected"
                } else {
                    git::commit(&cwd, &edited, signoff)?;
                    println!("Committed.");
                    "edited"
                }
            } else if answer.is_empty() || answer == "y" || answer == "yes" {
                git::commit(&cwd, &message, signoff)?;
                println!("Committed.");
                "applied"
            } else {
                println!("Aborted.");
                "rejected"
            }
        }
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

    let _ = audit::record(&audit::AuditEntry {
        tool: "git.commit.message.generate".into(),
        provider: resolved.provider_name.clone(),
        model: resolved.model.clone(),
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        decision: decision.into(),
    });
    Ok(())
}
