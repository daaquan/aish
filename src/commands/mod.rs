// SPDX-License-Identifier: MIT
//! Command implementations behind the library seam. `main.rs` only parses
//! argv and forwards here, so every command is callable (and testable)
//! without spawning the binary.

pub mod cache;
pub mod changelog;
pub mod commit;
pub mod config;
pub(crate) mod generate;
pub mod pr;
pub mod review;
pub mod uninstall;
pub mod update;

use crate::audit;
use crate::cli::{CacheAction, Cli, Command, ConfigAction, ModelsAction, ProvidersAction};
use crate::config::Config;
use anyhow::{Context, Result};

pub async fn run(cli: Cli) -> Result<()> {
    let json = cli.json;
    match cli.command {
        Command::Config { action } => match action {
            ConfigAction::Init { force } => config::init(force, json),
            ConfigAction::Check { ping } => config::check(ping, json).await,
        },
        Command::Providers {
            action: ProvidersAction::List,
        } => providers_list(json),
        Command::Models {
            action: ModelsAction::List,
        } => models_list(json),
        Command::Usage => usage(json),
        Command::Completions { shell } => {
            use clap::CommandFactory;
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "aish", &mut std::io::stdout());
            Ok(())
        }
        Command::Cache { action } => match action {
            CacheAction::Stats => cache::stats(json),
            CacheAction::Clear { yes } => cache::clear(yes, json),
        },
        Command::Commit {
            apply,
            model,
            style,
            lang,
            signoff,
            no_cache,
        } => commit::run(apply, model, style, lang, signoff, no_cache, json).await,
        Command::Pr {
            apply,
            model,
            lang,
            base,
            no_cache,
        } => pr::run(apply, model, lang, base, no_cache, json).await,
        Command::Review {
            branch,
            base,
            model,
            lang,
            no_cache,
        } => review::run(branch, base, model, lang, no_cache, json).await,
        Command::Changelog {
            from,
            to,
            model,
            lang,
            no_cache,
        } => changelog::run(from, to, model, lang, no_cache, json).await,
        Command::Update { check, version } => update::run(check, version, json).await,
        Command::Uninstall { purge, yes } => uninstall::run(purge, yes, json),
    }
}

/// Print a JSON value to stdout (pretty-printed), the single sink for `--json` output.
pub(crate) fn emit_json(value: &serde_json::Value) {
    println!("{}", serde_json::to_string_pretty(value).unwrap());
}

fn providers_list(json: bool) -> Result<()> {
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

fn models_list(json: bool) -> Result<()> {
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

fn usage(json: bool) -> Result<()> {
    let cfg = Config::load()?;
    let lines = crate::usage::read_log(&audit::log_path()).with_context(|| "reading audit log")?;
    let summary = crate::usage::summarize(lines, &cfg.pricing);
    if json {
        emit_json(&crate::usage::to_json(&summary));
    } else {
        print!("{}", crate::usage::render(&summary));
    }
    Ok(())
}
