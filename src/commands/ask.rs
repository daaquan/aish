// SPDX-License-Identifier: MIT
use crate::commands::emit_json;
use crate::config::resolve::resolve_model;
use crate::config::Config;
use crate::tool::ask::build_messages;
use anyhow::{anyhow, Result};
use std::io::{IsTerminal, Read};

pub async fn run(
    question: String,
    model: Option<String>,
    lang: Option<String>,
    no_cache: bool,
    json: bool,
) -> Result<()> {
    let cfg = Config::load()?;

    // Piped stdin (e.g. `cargo build 2>&1 | aish ask "explain"`) becomes
    // context; an interactive terminal contributes none.
    let context = if std::io::stdin().is_terminal() {
        None
    } else {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        (!buf.trim().is_empty()).then_some(buf)
    };

    let alias = model.unwrap_or_else(|| cfg.commit.model.clone());
    let resolved = resolve_model(&cfg, &alias)?;
    let lang = lang.unwrap_or_else(|| cfg.commit.language.clone());
    let messages = build_messages(&lang, &question, context.as_deref());

    let generated =
        crate::commands::generate::generate(&resolved, messages, no_cache, json).await?;
    let answer = crate::tool::review::postprocess(&generated.raw).ok_or_else(|| {
        anyhow!(
            "model returned an empty/unusable answer. raw: {:?}",
            generated.raw
        )
    })?;

    if json {
        emit_json(&serde_json::json!({
            "answer": answer,
            "cached": generated.cached,
            "provider": resolved.provider_name.clone(),
            "model": resolved.model.clone(),
            "prompt_tokens": generated.usage.prompt_tokens,
            "completion_tokens": generated.usage.completion_tokens,
        }));
    } else {
        println!("{answer}");
    }

    let _ = crate::audit::record(&crate::audit::AuditEntry {
        tool: "ask.answer".into(),
        provider: resolved.provider_name.clone(),
        model: resolved.model.clone(),
        prompt_tokens: generated.usage.prompt_tokens,
        completion_tokens: generated.usage.completion_tokens,
        decision: "answered".into(),
    });
    Ok(())
}
