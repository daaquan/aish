// SPDX-License-Identifier: AGPL-3.0-only
pub mod resolve;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config file not found at {0} — run `aish config init`")]
    NotFound(PathBuf),
    #[error("invalid config: {0}")]
    Parse(String),
    #[error("io error reading {0}: {1}")]
    Io(PathBuf, String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelAlias {
    pub provider: String,
    pub model: String,
}

/// Per-model price in USD per million tokens, keyed by the provider's model
/// string (e.g. `claude-opus-4-8`). Used by `aish usage` to estimate cost.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ModelPricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitConfig {
    #[serde(default = "default_style")]
    pub style: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_model")]
    pub model: String,
}

fn default_style() -> String {
    "conventional".into()
}
fn default_language() -> String {
    "en".into()
}
fn default_model() -> String {
    "default".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub providers: BTreeMap<String, ProviderConfig>,
    pub models: BTreeMap<String, ModelAlias>,
    #[serde(default = "default_commit")]
    pub commit: CommitConfig,
    /// Optional model pricing for `aish usage` cost estimates. Keyed by model string.
    #[serde(default)]
    pub pricing: BTreeMap<String, ModelPricing>,
}

fn default_commit() -> CommitConfig {
    CommitConfig {
        style: default_style(),
        language: default_language(),
        model: default_model(),
    }
}

impl Config {
    /// Default path: `~/.aish/config.yaml`, override with `$AISH_CONFIG`.
    pub fn default_path() -> PathBuf {
        if let Ok(p) = std::env::var("AISH_CONFIG") {
            return PathBuf::from(p);
        }
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".aish").join("config.yaml")
    }

    pub fn load() -> Result<Self, ConfigError> {
        let path = Self::default_path();
        if !path.exists() {
            return Err(ConfigError::NotFound(path));
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| ConfigError::Io(path.clone(), e.to_string()))?;
        Self::from_yaml(&raw)
    }

    pub fn from_yaml(raw: &str) -> Result<Self, ConfigError> {
        let expanded = expand_env(raw)?;
        let mut cfg: Config =
            serde_yaml::from_str(&expanded).map_err(|e| ConfigError::Parse(e.to_string()))?;
        for p in cfg.providers.values_mut() {
            if p.api_key
                .as_deref()
                .map(str::trim)
                .is_some_and(str::is_empty)
            {
                p.api_key = None;
            }
            if p.base_url
                .as_deref()
                .map(str::trim)
                .is_some_and(str::is_empty)
            {
                p.base_url = None;
            }
        }
        Ok(cfg)
    }
}

impl Config {
    /// Commented YAML template for `aish config init`.
    pub fn template() -> &'static str {
        r#"# aish configuration (~/.aish/config.yaml)
#
# Only providers you leave uncommented are loaded. The default template keeps
# Anthropic and local Ollama available, while other example providers are
# commented so unset optional API keys never block config loading.
providers:
  anthropic: { api_key: ${ANTHROPIC_API_KEY} }
  ollama:    { base_url: http://localhost:11434/v1 }
  # openai: { api_key: ${OPENAI_API_KEY} }
  # google: { api_key: ${GOOGLE_API_KEY} }
  # kilo:   { api_key: ${KILO_API_KEY}, base_url: https://gateway.kilo.example/v1 }

models:
  default: { provider: anthropic, model: claude-opus-4-8 }
  local:   { provider: ollama,    model: qwen3-coder }
  # fast:   { provider: openai,   model: gpt-5-mini }

commit:
  style: conventional
  language: en
  model: default

# Optional. Prices in USD per 1,000,000 tokens, keyed by model string.
# `aish usage` uses these to estimate cost; models without an entry show tokens only.
# pricing:
#   claude-opus-4-8: { input_per_mtok: 5.0, output_per_mtok: 25.0 }
#   gpt-5-mini:      { input_per_mtok: 0.25, output_per_mtok: 2.0 }
"#
    }

    /// Write the template to `path`. Refuses to overwrite unless `force`.
    pub fn write_template(path: &std::path::Path, force: bool) -> std::io::Result<()> {
        if path.exists() && !force {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "{} already exists (use --force to overwrite)",
                    path.display()
                ),
            ));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, Self::template())
    }
}

/// Expand `${VAR}` occurrences. Missing variable → empty string (validated later when the
/// provider is actually used). Unterminated `${` → Parse error.
fn expand_env(input: &str) -> Result<String, ConfigError> {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find('}')
            .ok_or_else(|| ConfigError::Parse("unterminated ${ in config".into()))?;
        let var = &after[..end];
        let val = std::env::var(var).unwrap_or_default(); // missing var → empty; validated later when the provider is actually used
        out.push_str(&val);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_config() {
        let yaml = r#"
providers:
  openai: { api_key: sk-test }
models:
  default: { provider: openai, model: gpt-5-mini }
commit: { style: conventional, language: en, model: default }
"#;
        let cfg = Config::from_yaml(yaml).unwrap();
        assert_eq!(cfg.commit.model, "default");
        assert_eq!(cfg.models["default"].model, "gpt-5-mini");
    }

    #[test]
    fn expands_env_vars_in_secrets() {
        std::env::set_var("AISH_TEST_KEY", "secret-123");
        let yaml = r#"
providers:
  openai: { api_key: ${AISH_TEST_KEY} }
models:
  default: { provider: openai, model: gpt-5-mini }
commit: { style: conventional, language: en, model: default }
"#;
        let cfg = Config::from_yaml(yaml).unwrap();
        assert_eq!(
            cfg.providers["openai"].api_key.as_deref(),
            Some("secret-123")
        );
    }

    #[test]
    fn missing_env_var_expands_to_empty_not_error() {
        std::env::remove_var("AISH_UNSET_XYZ_1");
        let out = super::expand_env("key: ${AISH_UNSET_XYZ_1}").unwrap();
        assert_eq!(out, "key: ");
    }

    #[test]
    fn empty_expanded_key_normalized_to_none() {
        std::env::remove_var("AISH_UNSET_XYZ_2");
        let cfg = Config::from_yaml(
            "providers:\n  openai: { api_key: ${AISH_UNSET_XYZ_2} }\nmodels:\n  default: { provider: openai, model: m }\ncommit: { style: conventional, language: en, model: default }",
        )
        .unwrap();
        assert!(cfg.providers["openai"].api_key.is_none());
    }

    #[test]
    fn template_loads_even_when_provider_keys_unset() {
        // The P1 regression: template must load without every key being set.
        let cfg = Config::from_yaml(Config::template()).unwrap();
        assert_eq!(cfg.models["default"].provider, "anthropic");
        assert!(cfg.providers.contains_key("ollama"));
        assert!(!cfg.providers.contains_key("openai"));
        assert!(!cfg.providers.contains_key("google"));
        assert!(!cfg.providers.contains_key("kilo"));
        assert_eq!(cfg.commit.model, "default");
    }

    #[test]
    fn template_parses_as_valid_config_when_env_present() {
        std::env::set_var("ANTHROPIC_API_KEY", "a");
        std::env::set_var("OPENAI_API_KEY", "o");
        std::env::set_var("GOOGLE_API_KEY", "g");
        std::env::set_var("KILO_API_KEY", "k");
        let cfg = Config::from_yaml(Config::template()).unwrap();
        assert_eq!(cfg.commit.model, "default");
        assert!(cfg.providers.contains_key("ollama"));
        assert!(!cfg.providers.contains_key("openai"));
        assert_eq!(cfg.models["default"].provider, "anthropic");
    }
}
