// SPDX-License-Identifier: MIT
use aish::audit;
use aish::cli::{Cli, Command, ConfigAction, ModelsAction, PluginAction, ProvidersAction};
use aish::config::Config;
use aish::plugin::host::run_plugin;
use aish::plugin::install::{self, RegistrySource};
use aish::plugin::manifest::{InstalledRegistry, Manifest};
use anyhow::{anyhow, Context, Result};
use clap::Parser;
use std::io::Write;

const DEFAULT_REGISTRY: &str = "git@github.com:daaquan/aish-plugins.git";

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
        Command::Plugin { action } => run_plugin_cmd(action).await,
        Command::External(args) => dispatch_external(args).await,
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

fn registry_source() -> RegistrySource {
    let value = std::env::var("AISH_REGISTRY").unwrap_or_else(|_| DEFAULT_REGISTRY.to_string());
    RegistrySource::parse(&value)
}

async fn run_plugin_cmd(action: PluginAction) -> Result<()> {
    match action {
        PluginAction::Install { name, yes } => {
            let source = registry_source();
            if !yes {
                println!(
                    "Installing `{name}` builds and runs code from:\n  {source:?}\n\
                     Plugins are trusted native executables (install runs build scripts).\n\
                     Continue? [y/N] "
                );
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !matches!(input.trim().to_lowercase().as_str(), "y" | "yes") {
                    println!("Aborted.");
                    return Ok(());
                }
            }
            let entry = install::install_from_registry(&source, &name)?;
            println!(
                "Installed `{name}` {} (revision {}).",
                entry.version, entry.revision
            );
            Ok(())
        }
        PluginAction::List => {
            let reg = InstalledRegistry::load(&install::plugins_toml())?;
            if reg.plugins.is_empty() {
                println!("No plugins installed. Try `aish plugin install commit`.");
            }
            for (name, e) in &reg.plugins {
                let state = if e.enabled { "enabled" } else { "disabled" };
                println!(
                    "{name:14} {:8} {state:8} [{}]",
                    e.version,
                    e.subcommands.join(",")
                );
            }
            Ok(())
        }
        PluginAction::Enable { name } => set_enabled(&name, true),
        PluginAction::Disable { name } => set_enabled(&name, false),
        PluginAction::Uninstall { name } => {
            let path = install::plugins_toml();
            let mut reg = InstalledRegistry::load(&path)?;
            let entry = reg
                .plugins
                .remove(&name)
                .ok_or_else(|| anyhow!("plugin `{name}` is not installed"))?;
            if let Some(dir) = entry.path.parent() {
                let _ = std::fs::remove_dir_all(dir);
            }
            reg.save(&path)?;
            println!("Uninstalled `{name}`.");
            Ok(())
        }
    }
}

fn set_enabled(name: &str, enabled: bool) -> Result<()> {
    let path = install::plugins_toml();
    let mut reg = InstalledRegistry::load(&path)?;
    let subs = reg
        .plugins
        .get(name)
        .ok_or_else(|| anyhow!("plugin `{name}` is not installed"))?
        .subcommands
        .clone();
    if enabled {
        reg.check_conflicts(name, &subs)?;
    }
    reg.plugins.get_mut(name).unwrap().enabled = enabled;
    reg.save(&path)?;
    println!("{} `{name}`.", if enabled { "Enabled" } else { "Disabled" });
    Ok(())
}

async fn dispatch_external(args: Vec<String>) -> Result<()> {
    let subcommand = args
        .first()
        .cloned()
        .ok_or_else(|| anyhow!("no subcommand given"))?;
    let rest = &args[1..];
    let cfg = Config::load()?;
    let reg = InstalledRegistry::load(&install::plugins_toml())?;
    let (_name, entry) = reg.find_by_subcommand(&subcommand).ok_or_else(|| {
        anyhow!(
            "no enabled plugin provides `{subcommand}` — try `aish plugin install {subcommand}`"
        )
    })?;
    // Load the installed manifest for permission + abi info.
    let manifest_path = entry.path.parent().unwrap().join("aish-plugin.toml");
    let manifest = Manifest::from_toml(&std::fs::read_to_string(&manifest_path)?)
        .map_err(|e| anyhow!("reading installed manifest: {e}"))?;
    let cwd = std::env::current_dir()?;
    let code = run_plugin(entry, &manifest, &subcommand, rest, &cwd, &cfg).await?;
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}
