// SPDX-License-Identifier: AGPL-3.0-only
use crate::config::{Config, ProviderConfig};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("unknown model alias `{0}` — run `aish models list`")]
    UnknownAlias(String),
    #[error("model alias references unknown provider `{0}` — run `aish providers list`")]
    UnknownProvider(String),
}

#[derive(Debug)]
pub struct Resolved<'a> {
    pub provider_name: String,
    pub provider: &'a ProviderConfig,
    pub model: String,
}

pub fn resolve_model<'a>(cfg: &'a Config, alias: &str) -> Result<Resolved<'a>, ResolveError> {
    let m = cfg
        .models
        .get(alias)
        .ok_or_else(|| ResolveError::UnknownAlias(alias.to_string()))?;
    let provider = cfg
        .providers
        .get(&m.provider)
        .ok_or_else(|| ResolveError::UnknownProvider(m.provider.clone()))?;
    Ok(Resolved {
        provider_name: m.provider.clone(),
        provider,
        model: m.model.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn cfg() -> Config {
        Config::from_yaml(
            r#"
providers:
  openai: { api_key: sk-x }
models:
  default: { provider: openai, model: gpt-5-mini }
commit: { style: conventional, language: en, model: default }
"#,
        )
        .unwrap()
    }

    #[test]
    fn resolves_known_alias() {
        let config = cfg();
        let r = resolve_model(&config, "default").unwrap();
        assert_eq!(r.provider_name, "openai");
        assert_eq!(r.model, "gpt-5-mini");
    }

    #[test]
    fn unknown_alias_errors() {
        let err = resolve_model(&cfg(), "nope").unwrap_err().to_string();
        assert!(err.contains("nope"));
    }

    #[test]
    fn alias_pointing_at_missing_provider_errors() {
        let bad = Config::from_yaml(
            r#"
providers:
  openai: { api_key: sk-x }
models:
  default: { provider: ghost, model: m }
commit: { style: conventional, language: en, model: default }
"#,
        )
        .unwrap();
        let err = resolve_model(&bad, "default").unwrap_err().to_string();
        assert!(err.contains("ghost"));
    }
}
