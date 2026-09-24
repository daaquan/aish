// SPDX-License-Identifier: MIT
pub mod resolve;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config file not found at {0} — run `aish setup`")]
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
    /// Extra free-form guidance appended to the commit-message prompt. Use it to
    /// shape style (gitmoji, longer subjects, mandatory body, …) without touching
    /// the output-format guardrails the tool relies on. `None` = no extra rules.
    #[serde(default)]
    pub instructions: Option<String>,
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
    /// Settings for the built-in `aish commit` command.
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
        instructions: None,
    }
}

impl Config {
    /// Default path: `config.yaml` in the data dir (`$AISH_HOME`, default
    /// `~/.aish`); `$AISH_CONFIG` overrides the file itself.
    pub fn default_path() -> PathBuf {
        if let Ok(p) = std::env::var("AISH_CONFIG") {
            return PathBuf::from(p);
        }
        crate::paths::default_data_dir().join("config.yaml")
    }

    pub fn load() -> Result<Self, ConfigError> {
        let path = Self::default_path();
        if !path.exists() {
            // First run with the default path: lay down the template so the
            // tool works out of the box. That includes a custom $AISH_HOME,
            // which names a dir for aish to keep its files in, like ~/.aish.
            // A custom $AISH_CONFIG pointed at a missing file is the user
            // naming a specific file — don't create a different one for them;
            // surface NotFound instead.
            if std::env::var_os("AISH_CONFIG").is_none()
                && Self::write_template(&path, false).is_ok()
            {
                // Point first-run users at the wizard; the template ships with
                // Anthropic + Ollama but no API keys, so commands fail until one
                // is configured. To stderr so `--json` stdout stays clean.
                eprintln!(
                    "Created a default config at {} — run `aish setup` to add provider API keys.",
                    path.display()
                );
            } else {
                return Err(ConfigError::NotFound(path));
            }
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

/// Severity of a config problem found by [`Config::validate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueLevel {
    /// Breaks functionality — the config will fail when used.
    Error,
    /// Suspicious but not fatal — the config may still work as intended.
    Warning,
}

/// A single problem found by [`Config::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub level: IssueLevel,
    pub message: String,
}

impl Config {
    /// Check the config for problems without making any network requests.
    /// Issues are returned in a stable order (errors discovered while walking
    /// models, then commit, then providers). An empty vec means the config is
    /// sound. This is the proactive counterpart to the lazy checks in
    /// [`resolve::resolve_model`], surfacing every problem up front rather than
    /// only the one alias a command happens to use.
    pub fn validate(&self) -> Vec<Issue> {
        let mut issues = Vec::new();
        // Every model alias must point at a declared provider.
        for (alias, m) in &self.models {
            if !self.providers.contains_key(&m.provider) {
                issues.push(Issue {
                    level: IssueLevel::Error,
                    message: format!(
                        "model alias `{alias}` references unknown provider `{}`",
                        m.provider
                    ),
                });
            }
        }
        // The default commit model must be a defined alias.
        if !self.models.contains_key(&self.commit.model) {
            issues.push(Issue {
                level: IssueLevel::Error,
                message: format!(
                    "commit.model `{}` is not a defined model alias",
                    self.commit.model
                ),
            });
        }
        // A provider with neither a key nor an endpoint cannot be reached.
        for (name, p) in &self.providers {
            if p.api_key.is_none() && p.base_url.is_none() {
                issues.push(Issue {
                    level: IssueLevel::Warning,
                    message: format!("provider `{name}` has neither api_key nor base_url set"),
                });
            }
        }
        // A pricing entry that matches no alias's model string is dead config:
        // `aish usage` can never apply it. Likely a typo or stale model name.
        if !self.pricing.is_empty() {
            let used: std::collections::BTreeSet<&str> =
                self.models.values().map(|m| m.model.as_str()).collect();
            for model in self.pricing.keys() {
                if !used.contains(model.as_str()) {
                    issues.push(Issue {
                        level: IssueLevel::Warning,
                        message: format!(
                            "pricing entry `{model}` matches no model used by any alias"
                        ),
                    });
                }
            }
        }
        issues
    }

    /// Commented YAML template written on first run and by `aish setup --repair`.
    pub fn template() -> &'static str {
        r#"# aish configuration: config.yaml in the data dir ($AISH_HOME, default
# ~/.aish), unless $AISH_CONFIG names another file.
#
# Only providers you leave uncommented are loaded. The default template keeps
# Anthropic and local Ollama available, while other example providers are
# commented so unset optional API keys never block config loading.
#
# Any provider other than `anthropic` and `google` is treated as
# OpenAI-compatible: set `base_url` to its endpoint and `api_key` to its key.
providers:
  anthropic: { api_key: ${ANTHROPIC_API_KEY} }
  ollama:    { base_url: http://localhost:11434/v1 }
  # openai:     { api_key: ${OPENAI_API_KEY} }
  # google:     { api_key: ${GOOGLE_API_KEY} }
  # openrouter: { api_key: ${OPENROUTER_API_KEY}, base_url: https://openrouter.ai/api/v1 }
  # deepseek:   { api_key: ${DEEPSEEK_API_KEY},   base_url: https://api.deepseek.com/v1 }
  # groq:       { api_key: ${GROQ_API_KEY},       base_url: https://api.groq.com/openai/v1 }
  # kilo:       { api_key: ${KILO_API_KEY},       base_url: https://api.kilo.ai/api/gateway }

models:
  default: { provider: anthropic, model: claude-opus-4-8 }
  local:   { provider: ollama,    model: qwen3-coder }
  # fast:   { provider: openai,   model: gpt-5-mini }

commit:
  style: conventional
  language: en
  model: default
  # Optional extra style guidance appended to the prompt. Free-form, multi-line.
  # instructions: |
  #   Prefix the subject with a gitmoji.
  #   Always add a one-line body explaining why.

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
        write_secure(path, Self::template())
    }
}

/// Write `contents` to `path`, restricting it to the owner (`0600`) on unix
/// since the file may hold plaintext API keys (or, for a cache entry, a
/// provider response). Missing parent dirs are created owner-only too (see
/// [`crate::paths::create_dir_owner_only`]).
///
/// The file is owner-only *before* anything is written to it (see
/// [`open_owner_only`]): writing first and tightening second leaves a window
/// where the key is readable by other local users.
pub fn write_secure(path: &std::path::Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        crate::paths::create_dir_owner_only(parent)?;
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        open_owner_only(path)?.write_all(contents.as_ref())?;
    }
    #[cfg(not(unix))]
    std::fs::write(path, contents)?;
    Ok(())
}

/// Open `path` truncated for writing, already restricted to `0600`.
///
/// `mode()` makes a new file 0600 from the start, never at the caller's umask.
/// It only applies on creation, so an existing, looser file is tightened
/// through the handle before the caller writes anything; if that fails
/// (e.g. the file belongs to another user), nothing secret has been written.
/// A descriptor another user opened while the old file was still readable
/// keeps working — a mode change never revokes an open descriptor.
#[cfg(unix)]
fn open_owner_only(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    Ok(f)
}

/// Write `contents` to a file that must not exist yet, owner-only (`0600`) on
/// unix like [`write_secure`]. Fails with `AlreadyExists` rather than touch a
/// file (or follow a symlink) already at `path`.
///
/// `create_new` checks and creates in one step, so a file that appears between
/// a caller's own existence check and the write is never clobbered. The file
/// is always new, so `mode()` alone keeps it owner-only from the start. Parent
/// directories are not created.
pub fn write_new_secure(path: &std::path::Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)?.write_all(contents.as_ref())
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

    /// A config file may hold a plaintext API key, so it must never be
    /// readable by anyone but the owner — not even for the instant between
    /// opening it and writing the key into it.
    #[cfg(unix)]
    #[test]
    fn write_secure_never_exposes_contents_to_other_users() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let dir = tempfile::tempdir().unwrap();

        // Fresh file: 0600, in a fresh dir with no group/other bits.
        let fresh = dir.path().join("nested").join("config.yaml");
        write_secure(&fresh, "providers:\n  openai: { api_key: sk-secret }\n").unwrap();
        let m = mode(&fresh);
        assert_eq!(m, 0o600, "fresh config left at {m:o}");
        let m = mode(fresh.parent().unwrap());
        assert_eq!(m & 0o077, 0, "fresh data dir created at {m:o}");

        // Pre-existing world-readable file (chmod, not umask-dependent): it is
        // already 0600 once the handle the key is written through exists,
        // i.e. before any byte of the new contents lands in it.
        let existing = dir.path().join("loose.yaml");
        std::fs::write(&existing, "old").unwrap();
        std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(mode(&existing), 0o644, "fixture must start world-readable");
        drop(open_owner_only(&existing).unwrap());
        let m = mode(&existing);
        assert_eq!(m, 0o600, "existing config at {m:o} before the write");

        let body = "providers: {}\n";
        write_secure(&existing, body).unwrap();
        let m = mode(&existing);
        assert_eq!(m, 0o600, "existing config left at {m:o}");
        assert_eq!(std::fs::read_to_string(&existing).unwrap(), body);
    }

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
    fn validate_accepts_sound_config() {
        let cfg = Config::from_yaml(
            "providers:\n  openai: { api_key: sk-x }\nmodels:\n  default: { provider: openai, model: m }\ncommit: { style: conventional, language: en, model: default }",
        )
        .unwrap();
        assert!(cfg.validate().is_empty());
    }

    #[test]
    fn validate_flags_alias_with_missing_provider() {
        let cfg = Config::from_yaml(
            "providers:\n  openai: { api_key: sk-x }\nmodels:\n  default: { provider: ghost, model: m }\ncommit: { style: conventional, language: en, model: default }",
        )
        .unwrap();
        let issues = cfg.validate();
        assert!(issues
            .iter()
            .any(|i| i.level == IssueLevel::Error && i.message.contains("ghost")));
    }

    #[test]
    fn validate_flags_commit_model_not_an_alias() {
        let cfg = Config::from_yaml(
            "providers:\n  openai: { api_key: sk-x }\nmodels:\n  default: { provider: openai, model: m }\ncommit: { style: conventional, language: en, model: nope }",
        )
        .unwrap();
        let issues = cfg.validate();
        assert!(issues
            .iter()
            .any(|i| i.level == IssueLevel::Error && i.message.contains("nope")));
    }

    #[test]
    fn validate_warns_on_pricing_for_unused_model() {
        let cfg = Config::from_yaml(
            "providers:\n  openai: { api_key: sk-x }\nmodels:\n  default: { provider: openai, model: m }\ncommit: { style: conventional, language: en, model: default }\npricing:\n  ghost-model: { input_per_mtok: 1.0, output_per_mtok: 2.0 }",
        )
        .unwrap();
        let issues = cfg.validate();
        assert!(issues
            .iter()
            .any(|i| i.level == IssueLevel::Warning && i.message.contains("ghost-model")));
    }

    #[test]
    fn validate_accepts_pricing_for_used_model() {
        let cfg = Config::from_yaml(
            "providers:\n  openai: { api_key: sk-x }\nmodels:\n  default: { provider: openai, model: m }\ncommit: { style: conventional, language: en, model: default }\npricing:\n  m: { input_per_mtok: 1.0, output_per_mtok: 2.0 }",
        )
        .unwrap();
        assert!(cfg.validate().is_empty());
    }

    #[test]
    fn validate_warns_on_unconfigured_provider() {
        std::env::remove_var("AISH_UNSET_VALIDATE_1");
        let cfg = Config::from_yaml(
            "providers:\n  openai: { api_key: ${AISH_UNSET_VALIDATE_1} }\nmodels:\n  default: { provider: openai, model: m }\ncommit: { style: conventional, language: en, model: default }",
        )
        .unwrap();
        let issues = cfg.validate();
        assert!(issues
            .iter()
            .any(|i| i.level == IssueLevel::Warning && i.message.contains("openai")));
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
