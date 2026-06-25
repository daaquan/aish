// SPDX-License-Identifier: MIT
//! `aish setup` — interactive configuration wizard, plus `--repair` to restore
//! the initial template config.
use crate::commands::emit_json;
use crate::config::{write_secure, CommitConfig, Config, ModelAlias, ProviderConfig};
use anyhow::{anyhow, Context, Result};
use std::collections::BTreeMap;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

/// Static knowledge about a provider that needs an API key. `anthropic` and
/// `google` use native adapters (no base_url); the rest are OpenAI-compatible
/// and ship with their public endpoint.
struct ProviderSpec {
    name: &'static str,
    base_url: Option<&'static str>,
    default_model: Option<&'static str>,
    env_var: &'static str,
}

const KEYED_PROVIDERS: &[ProviderSpec] = &[
    ProviderSpec {
        name: "anthropic",
        base_url: None,
        default_model: Some("claude-opus-4-8"),
        env_var: "ANTHROPIC_API_KEY",
    },
    ProviderSpec {
        name: "openai",
        base_url: None,
        default_model: Some("gpt-5-mini"),
        env_var: "OPENAI_API_KEY",
    },
    ProviderSpec {
        name: "google",
        base_url: None,
        default_model: Some("gemini-2.5-pro"),
        env_var: "GOOGLE_API_KEY",
    },
    ProviderSpec {
        name: "openrouter",
        base_url: Some("https://openrouter.ai/api/v1"),
        default_model: None,
        env_var: "OPENROUTER_API_KEY",
    },
    ProviderSpec {
        name: "deepseek",
        base_url: Some("https://api.deepseek.com/v1"),
        default_model: Some("deepseek-chat"),
        env_var: "DEEPSEEK_API_KEY",
    },
    ProviderSpec {
        name: "groq",
        base_url: Some("https://api.groq.com/openai/v1"),
        default_model: None,
        env_var: "GROQ_API_KEY",
    },
    ProviderSpec {
        name: "kilo",
        base_url: Some("https://api.kilo.ai/api/gateway"),
        default_model: None,
        env_var: "KILO_API_KEY",
    },
];

const OLLAMA_BASE_URL: &str = "http://localhost:11434/v1";
const OLLAMA_DEFAULT_MODEL: &str = "qwen3-coder";

/// One provider the user chose to enable, resolved to concrete config values.
pub struct EnabledProvider {
    pub name: String,
    /// Literal to store: a plaintext key, a `${VAR}` reference, or None (ollama).
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    /// Provider model string for this provider's alias.
    pub model: String,
}

pub fn run(repair: bool, json: bool) -> Result<()> {
    if repair {
        repair_config(json)
    } else {
        wizard(json)
    }
}

/// Restore the initial template config, backing up any existing file.
fn repair_config(json: bool) -> Result<()> {
    let path = Config::default_path();
    let backup = back_up_existing(&path)?;
    write_secure(&path, Config::template())
        .with_context(|| format!("writing config to {}", path.display()))?;
    report_written(&path, backup.as_deref(), &[], None, json);
    Ok(())
}

/// Interactive wizard. Requires a TTY on both stdin and stdout.
fn wizard(json: bool) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(anyhow!(
            "`aish setup` needs an interactive terminal; use `aish setup --repair` for a non-interactive reset"
        ));
    }

    let mut out = io::stdout();
    writeln!(
        out,
        "Configure aish providers. Leave a provider disabled by answering 'n'.\n"
    )?;

    let mut enabled: Vec<EnabledProvider> = Vec::new();

    for spec in KEYED_PROVIDERS {
        if !prompt_yes_no(&format!("Enable {}?", spec.name), false)? {
            continue;
        }
        let api_key = prompt_api_key(spec)?;
        let model = prompt_model(spec)?;
        enabled.push(EnabledProvider {
            name: spec.name.to_string(),
            api_key: Some(api_key),
            base_url: spec.base_url.map(str::to_string),
            model,
        });
    }

    if prompt_yes_no("Enable local Ollama (no API key)?", false)? {
        let model = prompt_line_default("  Ollama model", OLLAMA_DEFAULT_MODEL)?;
        enabled.push(EnabledProvider {
            name: "ollama".to_string(),
            api_key: None,
            base_url: Some(OLLAMA_BASE_URL.to_string()),
            model,
        });
    }

    if enabled.is_empty() {
        return Err(anyhow!("no providers enabled; nothing to write"));
    }

    let default_alias = prompt_default_alias(&enabled)?;
    let cfg = build_config(&enabled, &default_alias);
    let yaml = serde_yaml::to_string(&cfg).context("serializing config")?;

    let path = Config::default_path();
    let backup = back_up_existing(&path)?;
    write_secure(&path, &yaml).with_context(|| format!("writing config to {}", path.display()))?;

    let names: Vec<String> = enabled.iter().map(|p| p.name.clone()).collect();
    report_written(&path, backup.as_deref(), &names, Some(&default_alias), json);
    Ok(())
}

/// Build a `Config` from the chosen providers and the default alias. Pure: no
/// I/O, so it is unit-testable. Each enabled provider gets an alias named after
/// it, plus a `default` alias pointing at `default_alias`; `commit.model`
/// targets `default`.
pub fn build_config(providers: &[EnabledProvider], default_alias: &str) -> Config {
    let mut provider_map = BTreeMap::new();
    let mut models = BTreeMap::new();

    for p in providers {
        provider_map.insert(
            p.name.clone(),
            ProviderConfig {
                api_key: p.api_key.clone(),
                base_url: p.base_url.clone(),
            },
        );
        models.insert(
            p.name.clone(),
            ModelAlias {
                provider: p.name.clone(),
                model: p.model.clone(),
            },
        );
    }

    if let Some(chosen) = providers.iter().find(|p| p.name == default_alias) {
        models.insert(
            "default".to_string(),
            ModelAlias {
                provider: chosen.name.clone(),
                model: chosen.model.clone(),
            },
        );
    }

    Config {
        providers: provider_map,
        models,
        commit: CommitConfig {
            style: "conventional".to_string(),
            language: "en".to_string(),
            model: "default".to_string(),
            instructions: None,
        },
        pricing: BTreeMap::new(),
    }
}

/// Copy an existing config to `<path>.bak`. Returns the backup path if one was
/// made, or None if there was no file to back up.
fn back_up_existing(path: &Path) -> Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let mut bak = path.as_os_str().to_owned();
    bak.push(".bak");
    let bak = PathBuf::from(bak);
    std::fs::copy(path, &bak)
        .with_context(|| format!("backing up {} to {}", path.display(), bak.display()))?;
    Ok(Some(bak))
}

fn report_written(
    path: &Path,
    backup: Option<&Path>,
    providers: &[String],
    default_alias: Option<&str>,
    json: bool,
) {
    if json {
        emit_json(&serde_json::json!({
            "wrote": path.display().to_string(),
            "backup": backup.map(|b| b.display().to_string()),
            "providers": providers,
            "default": default_alias,
        }));
    } else {
        if let Some(b) = backup {
            println!("Backed up existing config to {}", b.display());
        }
        println!("Wrote config to {}", path.display());
        if let Some(alias) = default_alias {
            println!("Default model alias: {alias}");
        }
    }
}

// --- interactive prompt helpers ---

fn prompt_api_key(spec: &ProviderSpec) -> Result<String> {
    println!(
        "  How should the {} key be stored?\n    1) plaintext in config\n    2) ${{{}}} environment reference",
        spec.name, spec.env_var
    );
    let choice = prompt_line_default("  choice [1/2]", "1")?;
    if choice.trim() == "2" {
        println!("  Stored as ${{{}}} — remember to export it.", spec.env_var);
        Ok(format!("${{{}}}", spec.env_var))
    } else {
        let key = rpassword::prompt_password(format!("  {} API key: ", spec.name))
            .context("reading API key")?;
        let key = key.trim().to_string();
        if key.is_empty() {
            return Err(anyhow!("empty key for {}", spec.name));
        }
        println!("  Stored as plaintext in a 0600 config file.");
        Ok(key)
    }
}

fn prompt_model(spec: &ProviderSpec) -> Result<String> {
    match spec.default_model {
        Some(default) => prompt_line_default(&format!("  {} model", spec.name), default),
        None => loop {
            let m = prompt_line(&format!("  {} model (required): ", spec.name))?;
            if !m.is_empty() {
                break Ok(m);
            }
        },
    }
}

fn prompt_default_alias(enabled: &[EnabledProvider]) -> Result<String> {
    if enabled.len() == 1 {
        return Ok(enabled[0].name.clone());
    }
    println!("Choose the default provider:");
    for (i, p) in enabled.iter().enumerate() {
        println!("  {}) {}", i + 1, p.name);
    }
    loop {
        let pick = prompt_line_default("  default [1]", "1")?;
        if let Ok(n) = pick.trim().parse::<usize>() {
            if n >= 1 && n <= enabled.len() {
                return Ok(enabled[n - 1].name.clone());
            }
        }
    }
}

fn prompt_yes_no(question: &str, default_yes: bool) -> Result<bool> {
    let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
    let ans = prompt_line(&format!("{question} {hint} "))?;
    Ok(match ans.to_ascii_lowercase().as_str() {
        "y" | "yes" => true,
        "n" | "no" => false,
        "" => default_yes,
        _ => default_yes,
    })
}

/// Print `prompt`, read one trimmed line from stdin.
fn prompt_line(prompt: &str) -> Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

/// Like `prompt_line` but shows a default and returns it when the input is empty.
fn prompt_line_default(label: &str, default: &str) -> Result<String> {
    let v = prompt_line(&format!("{label} [{default}]: "))?;
    Ok(if v.is_empty() { default.to_string() } else { v })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled(name: &str, key: Option<&str>, base: Option<&str>, model: &str) -> EnabledProvider {
        EnabledProvider {
            name: name.to_string(),
            api_key: key.map(str::to_string),
            base_url: base.map(str::to_string),
            model: model.to_string(),
        }
    }

    #[test]
    fn build_config_round_trips_and_validates() {
        let providers = vec![
            enabled("anthropic", Some("sk-ant"), None, "claude-opus-4-8"),
            enabled(
                "openrouter",
                Some("${OPENROUTER_API_KEY}"),
                Some("https://openrouter.ai/api/v1"),
                "openai/gpt-4o",
            ),
        ];
        let cfg = build_config(&providers, "openrouter");
        // default alias points at the chosen provider
        assert_eq!(cfg.models["default"].provider, "openrouter");
        assert_eq!(cfg.commit.model, "default");
        // per-provider aliases exist
        assert!(cfg.models.contains_key("anthropic"));
        assert!(cfg.models.contains_key("openrouter"));

        let yaml = serde_yaml::to_string(&cfg).unwrap();
        let reloaded = Config::from_yaml(&yaml).unwrap();
        assert!(
            reloaded
                .validate()
                .iter()
                .all(|i| i.level != crate::config::IssueLevel::Error),
            "round-tripped config should have no errors: {:?}",
            reloaded.validate()
        );
    }

    #[test]
    fn build_config_single_provider() {
        let providers = vec![enabled(
            "ollama",
            None,
            Some(OLLAMA_BASE_URL),
            "qwen3-coder",
        )];
        let cfg = build_config(&providers, "ollama");
        assert_eq!(
            cfg.providers["ollama"].base_url.as_deref(),
            Some(OLLAMA_BASE_URL)
        );
        assert!(cfg.providers["ollama"].api_key.is_none());
        assert_eq!(cfg.models["default"].provider, "ollama");
    }
}
