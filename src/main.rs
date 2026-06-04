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
        Command::Config {
            action: ConfigAction::Init { force },
        } => {
            let path = Config::default_path();
            Config::write_template(&path, force)
                .with_context(|| format!("writing config to {}", path.display()))?;
            println!("Wrote config template to {}", path.display());
            Ok(())
        }
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
        Command::Commit {
            apply,
            model,
            style,
            lang,
            signoff,
        } => run_commit(apply, model, style, lang, signoff).await,
    }
}

async fn run_commit(
    apply: bool,
    model: Option<String>,
    style: Option<String>,
    lang: Option<String>,
    signoff: bool,
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
    // Test hook: AISH_PROVIDER=mock returns a canned message without network.
    let provider: Box<dyn aish::provider::Provider> =
        if std::env::var("AISH_PROVIDER").as_deref() == Ok("mock") {
            Box::new(aish::provider::mock::MockProvider::new(
                std::env::var("AISH_MOCK_REPLY").unwrap_or_else(|_| "feat: add thing".into()),
            ))
        } else {
            build_provider(&resolved.provider_name, &resolved).map_err(|e| anyhow!(e))?
        };

    let style = style.unwrap_or_else(|| cfg.commit.style.clone());
    let lang = lang.unwrap_or_else(|| cfg.commit.language.clone());
    let messages = build_messages(&style, &lang, &diff);

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

    println!("\nSuggested commit:\n\n{message}\n");

    let decision = if apply {
        git::commit(&cwd, &message, signoff)?;
        println!("Committed.");
        "applied"
    } else {
        print!("Accept? [Y/n] ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let answer = input.trim().to_lowercase();
        if answer.is_empty() || answer == "y" || answer == "yes" {
            git::commit(&cwd, &message, signoff)?;
            println!("Committed.");
            "applied"
        } else {
            println!("Aborted.");
            "rejected"
        }
    };

    let usage = resp.usage.unwrap_or_default();
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
