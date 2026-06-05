// SPDX-License-Identifier: AGPL-3.0-only
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
    match cli.command {
        Command::Config { action } => match action {
            ConfigAction::Init { force } => {
                let path = Config::default_path();
                Config::write_template(&path, force)
                    .with_context(|| format!("writing config to {}", path.display()))?;
                println!("Wrote config template to {}", path.display());
                Ok(())
            }
            ConfigAction::Check => run_config_check(),
        },
        Command::Providers {
            action: ProvidersAction::List,
        } => {
            let cfg = Config::load()?;
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
            Ok(())
        }
        Command::Models {
            action: ModelsAction::List,
        } => {
            let cfg = Config::load()?;
            for (alias, m) in &cfg.models {
                println!("{alias:10} -> {}/{}", m.provider, m.model);
            }
            Ok(())
        }
        Command::Usage => {
            let cfg = Config::load()?;
            let lines =
                aish::usage::read_log(&audit::log_path()).with_context(|| "reading audit log")?;
            let summary = aish::usage::summarize(lines, &cfg.pricing);
            print!("{}", aish::usage::render(&summary));
            Ok(())
        }
        Command::Commit {
            apply,
            model,
            style,
            lang,
            signoff,
            no_cache,
        } => run_commit(apply, model, style, lang, signoff, no_cache).await,
    }
}

fn run_config_check() -> Result<()> {
    use aish::config::IssueLevel;
    let cfg = Config::load()?;
    let issues = cfg.validate();
    if issues.is_empty() {
        println!(
            "Config OK: {} provider(s), {} model alias(es).",
            cfg.providers.len(),
            cfg.models.len()
        );
        return Ok(());
    }
    let mut errors = 0usize;
    for issue in &issues {
        let tag = match issue.level {
            IssueLevel::Error => {
                errors += 1;
                "error"
            }
            IssueLevel::Warning => "warning",
        };
        println!("{tag}: {}", issue.message);
    }
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
) -> Result<()> {
    let cfg = Config::load()?;
    let cwd = std::env::current_dir()?;

    let diff = git::staged_diff(&cwd)?;
    if diff.trim().is_empty() {
        println!("Nothing staged. Run `git add` first.");
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
    let (message, usage) = match (!no_cache)
        .then(|| aish::cache::get(&cache_dir, &cache_key))
        .flatten()
    {
        Some(cached) => {
            println!("(cached — no model request made)");
            (cached, aish::provider::Usage::default())
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

    println!("\nSuggested commit:\n\n{message}\n");

    let decision = if apply {
        git::commit(&cwd, &message, signoff)?;
        println!("Committed.");
        "applied"
    } else {
        print!("Accept? [Y/n] ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        let n = std::io::stdin().read_line(&mut input)?;
        if n == 0 {
            // EOF / non-interactive (e.g. </dev/null): do not commit.
            println!("Aborted (no input).");
            "rejected"
        } else {
            let answer = input.trim().to_lowercase();
            if answer.is_empty() || answer == "y" || answer == "yes" {
                git::commit(&cwd, &message, signoff)?;
                println!("Committed.");
                "applied"
            } else {
                println!("Aborted.");
                "rejected"
            }
        }
    };

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
