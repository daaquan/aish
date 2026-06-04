// SPDX-License-Identifier: AGPL-3.0-only
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
    #[error("environment variable {0} referenced in config is not set")]
    MissingEnv(String),
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
        serde_yaml::from_str(&expanded).map_err(|e| ConfigError::Parse(e.to_string()))
    }
}

/// Expand `${VAR}` occurrences. Missing variable → error naming the var (never its value).
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
        let val = std::env::var(var).map_err(|_| ConfigError::MissingEnv(var.to_string()))?;
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
    fn missing_env_var_errors_with_name_not_value() {
        let yaml = r#"
providers:
  openai: { api_key: ${AISH_DOES_NOT_EXIST} }
models:
  default: { provider: openai, model: gpt-5-mini }
commit: { style: conventional, language: en, model: default }
"#;
        let err = Config::from_yaml(yaml).unwrap_err().to_string();
        assert!(err.contains("AISH_DOES_NOT_EXIST"));
    }
}
