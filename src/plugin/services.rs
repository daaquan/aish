// SPDX-License-Identifier: MIT
use crate::audit::{self, AuditEntry};
use crate::config::resolve::resolve_model;
use crate::config::Config;
use crate::plugin::manifest::Manifest;
use crate::plugin::protocol::{ProtoError, WireMessage};
use crate::provider::{build_provider, ChatRequest, Message, Provider, Role};
use serde::Deserialize;

/// Service ops a plugin may call given its declared permissions.
pub fn available_services(m: &Manifest) -> Vec<String> {
    let mut v = Vec::new();
    if m.permissions.model {
        v.push("model.chat".to_string());
    }
    if m.permissions.audit {
        v.push("audit.record".to_string());
    }
    v
}

/// The sanitized config slice forwarded to `plugin` in the `invoke` frame: its
/// own `[plugins.<name>]` table and nothing else. NEVER includes provider keys.
///
/// Back-compat: the deprecated top-level `commit:` block still feeds the commit
/// plugin, back-filling any key `[plugins.commit]` does not set.
pub fn scoped_config(cfg: &Config, plugin: &str) -> serde_json::Value {
    let mut out = cfg
        .plugins
        .get(plugin)
        .and_then(|v| serde_json::to_value(v).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();

    if plugin == "commit" {
        for (k, v) in [
            ("style", serde_json::json!(cfg.commit.style)),
            ("language", serde_json::json!(cfg.commit.language)),
            ("model", serde_json::json!(cfg.commit.model)),
        ] {
            out.entry(k.to_string()).or_insert(v);
        }
    }
    serde_json::Value::Object(out)
}

fn role_from_wire(r: &str) -> Role {
    match r {
        "system" => Role::System,
        "assistant" => Role::Assistant,
        _ => Role::User,
    }
}

fn err(code: &str, message: impl Into<String>) -> ProtoError {
    ProtoError {
        code: code.into(),
        message: message.into(),
    }
}

/// Dispatch one host service request. `Ok(payload)` -> Response ok:true,
/// `Err(proto)` -> Response ok:false with that error.
pub async fn handle(
    op: &str,
    payload: serde_json::Value,
    manifest: &Manifest,
    cfg: &Config,
) -> Result<serde_json::Value, ProtoError> {
    match op {
        "model.chat" => {
            if !manifest.permissions.model {
                return Err(err("permission_denied", "manifest does not grant `model`"));
            }
            model_chat(payload, cfg).await
        }
        "audit.record" => {
            if !manifest.permissions.audit {
                return Err(err("permission_denied", "manifest does not grant `audit`"));
            }
            audit_record(payload)
        }
        other => Err(err("unknown_op", format!("unknown service op `{other}`"))),
    }
}

async fn model_chat(
    payload: serde_json::Value,
    cfg: &Config,
) -> Result<serde_json::Value, ProtoError> {
    #[derive(Deserialize)]
    struct Req {
        model: String,
        messages: Vec<WireMessage>,
        #[serde(default)]
        temperature: Option<f32>,
    }
    let req: Req =
        serde_json::from_value(payload).map_err(|e| err("bad_request", e.to_string()))?;
    let resolved = resolve_model(cfg, &req.model).map_err(|e| err("resolve", e.to_string()))?;
    let messages: Vec<Message> = req
        .messages
        .into_iter()
        .map(|m| Message {
            role: role_from_wire(&m.role),
            content: m.content,
        })
        .collect();

    // Test hook: AISH_PROVIDER=mock returns canned text without network.
    let provider: Box<dyn Provider> = if std::env::var("AISH_PROVIDER").as_deref() == Ok("mock") {
        Box::new(crate::provider::mock::MockProvider::new(
            std::env::var("AISH_MOCK_REPLY").unwrap_or_else(|_| "feat: add thing".into()),
        ))
    } else {
        build_provider(&resolved.provider_name, &resolved)
            .map_err(|e| err("provider", e.to_string()))?
    };

    let resp = provider
        .chat(ChatRequest {
            model: resolved.model.clone(),
            messages,
            temperature: req.temperature,
        })
        .await
        .map_err(|e| err("provider", e.to_string()))?;
    let usage = resp.usage.unwrap_or_default();
    Ok(serde_json::json!({
        "content": resp.content,
        "usage": { "prompt_tokens": usage.prompt_tokens, "completion_tokens": usage.completion_tokens }
    }))
}

fn audit_record(payload: serde_json::Value) -> Result<serde_json::Value, ProtoError> {
    let entry: AuditEntry =
        serde_json::from_value(payload).map_err(|e| err("bad_request", e.to_string()))?;
    audit::record(&entry).map_err(|e| err("audit", e.to_string()))?;
    Ok(serde_json::json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::manifest::Permissions;

    fn manifest(model: bool, audit: bool) -> Manifest {
        Manifest {
            name: "commit".into(),
            version: "0.1.0".into(),
            abi: "1".into(),
            description: None,
            subcommands: vec!["commit".into()],
            permissions: Permissions { model, audit },
        }
    }

    fn cfg() -> Config {
        Config::from_yaml(
            "providers:\n  openai: { api_key: sk-x }\nmodels:\n  default: { provider: openai, model: gpt-5-mini }\ncommit: { style: conventional, language: en, model: default }\n",
        )
        .unwrap()
    }

    #[tokio::test]
    async fn model_chat_uses_mock_provider() {
        std::env::set_var("AISH_PROVIDER", "mock");
        std::env::set_var("AISH_MOCK_REPLY", "feat: hello");
        let payload = serde_json::json!({
            "model": "default",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let out = handle("model.chat", payload, &manifest(true, false), &cfg())
            .await
            .unwrap();
        assert_eq!(out["content"], "feat: hello");
        std::env::remove_var("AISH_PROVIDER");
    }

    #[tokio::test]
    async fn model_chat_denied_without_permission() {
        let payload = serde_json::json!({"model": "default", "messages": []});
        let e = handle("model.chat", payload, &manifest(false, false), &cfg())
            .await
            .unwrap_err();
        assert_eq!(e.code, "permission_denied");
    }

    #[tokio::test]
    async fn unknown_op_errors() {
        let e = handle(
            "bogus.op",
            serde_json::json!({}),
            &manifest(true, true),
            &cfg(),
        )
        .await
        .unwrap_err();
        assert_eq!(e.code, "unknown_op");
    }

    #[tokio::test]
    async fn scoped_config_excludes_secrets() {
        let v = scoped_config(&cfg(), "commit");
        assert_eq!(v["style"], "conventional");
        assert!(v.get("providers").is_none());
        assert!(!v.to_string().contains("sk-x"));
    }

    /// The deprecated top-level `commit:` block still reaches the commit plugin.
    #[tokio::test]
    async fn scoped_config_commit_backfills_from_top_level() {
        let v = scoped_config(&cfg(), "commit");
        assert_eq!(v["style"], "conventional");
        assert_eq!(v["language"], "en");
        assert_eq!(v["model"], "default");
    }

    /// A non-commit plugin must NOT receive commit settings — only its own table.
    #[tokio::test]
    async fn scoped_config_other_plugin_is_isolated() {
        let c = Config::from_yaml(
            "providers:\n  openai: { api_key: sk-x }\nmodels:\n  default: { provider: openai, model: m }\ncommit: { style: conventional, language: en, model: default }\nplugins:\n  jira: { project: ABC, url: https://x }\n",
        )
        .unwrap();
        let v = scoped_config(&c, "jira");
        assert_eq!(v["project"], "ABC");
        assert!(v.get("style").is_none(), "commit settings leaked to jira");
        let unknown = scoped_config(&c, "nope");
        assert_eq!(unknown, serde_json::json!({}));
    }

    /// An explicit `[plugins.commit]` key overrides the deprecated top-level one.
    #[tokio::test]
    async fn scoped_config_plugins_commit_overrides_top_level() {
        let c = Config::from_yaml(
            "providers:\n  openai: { api_key: sk-x }\nmodels:\n  default: { provider: openai, model: m }\ncommit: { style: conventional, language: en, model: default }\nplugins:\n  commit: { style: gitmoji }\n",
        )
        .unwrap();
        let v = scoped_config(&c, "commit");
        assert_eq!(v["style"], "gitmoji"); // override wins
        assert_eq!(v["language"], "en"); // unset key still back-fills
    }
}
