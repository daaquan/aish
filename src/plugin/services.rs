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

/// The sanitized config slice forwarded to a plugin in the `invoke` frame.
/// NEVER includes provider keys. v0.2 forwards the commit settings for all tools.
pub fn scoped_config(cfg: &Config) -> serde_json::Value {
    serde_json::json!({
        "style": cfg.commit.style,
        "language": cfg.commit.language,
        "model": cfg.commit.model,
    })
}

fn role_from_wire(r: &str) -> Role {
    match r {
        "system" => Role::System,
        "assistant" => Role::Assistant,
        _ => Role::User,
    }
}

fn err(code: &str, message: impl Into<String>) -> ProtoError {
    ProtoError { code: code.into(), message: message.into() }
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

async fn model_chat(payload: serde_json::Value, cfg: &Config) -> Result<serde_json::Value, ProtoError> {
    #[derive(Deserialize)]
    struct Req {
        model: String,
        messages: Vec<WireMessage>,
        #[serde(default)]
        temperature: Option<f32>,
    }
    let req: Req = serde_json::from_value(payload).map_err(|e| err("bad_request", e.to_string()))?;
    let resolved = resolve_model(cfg, &req.model).map_err(|e| err("resolve", e.to_string()))?;
    let messages: Vec<Message> = req
        .messages
        .into_iter()
        .map(|m| Message { role: role_from_wire(&m.role), content: m.content })
        .collect();

    // Test hook: AISH_PROVIDER=mock returns canned text without network.
    let provider: Box<dyn Provider> = if std::env::var("AISH_PROVIDER").as_deref() == Ok("mock") {
        Box::new(crate::provider::mock::MockProvider::new(
            std::env::var("AISH_MOCK_REPLY").unwrap_or_else(|_| "feat: add thing".into()),
        ))
    } else {
        build_provider(&resolved.provider_name, &resolved).map_err(|e| err("provider", e.to_string()))?
    };

    let resp = provider
        .chat(ChatRequest { model: resolved.model.clone(), messages, temperature: req.temperature })
        .await
        .map_err(|e| err("provider", e.to_string()))?;
    let usage = resp.usage.unwrap_or_default();
    Ok(serde_json::json!({
        "content": resp.content,
        "usage": { "prompt_tokens": usage.prompt_tokens, "completion_tokens": usage.completion_tokens }
    }))
}

fn audit_record(payload: serde_json::Value) -> Result<serde_json::Value, ProtoError> {
    let entry: AuditEntry = serde_json::from_value(payload).map_err(|e| err("bad_request", e.to_string()))?;
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
        let out = handle("model.chat", payload, &manifest(true, false), &cfg()).await.unwrap();
        assert_eq!(out["content"], "feat: hello");
        std::env::remove_var("AISH_PROVIDER");
    }

    #[tokio::test]
    async fn model_chat_denied_without_permission() {
        let payload = serde_json::json!({"model": "default", "messages": []});
        let e = handle("model.chat", payload, &manifest(false, false), &cfg()).await.unwrap_err();
        assert_eq!(e.code, "permission_denied");
    }

    #[tokio::test]
    async fn unknown_op_errors() {
        let e = handle("bogus.op", serde_json::json!({}), &manifest(true, true), &cfg()).await.unwrap_err();
        assert_eq!(e.code, "unknown_op");
    }

    #[tokio::test]
    async fn scoped_config_excludes_secrets() {
        let v = scoped_config(&cfg());
        assert_eq!(v["style"], "conventional");
        assert!(v.get("providers").is_none());
        assert!(!v.to_string().contains("sk-x"));
    }
}
