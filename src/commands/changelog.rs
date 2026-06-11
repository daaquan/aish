// SPDX-License-Identifier: MIT
use crate::commands::emit_json;
use crate::config::resolve::resolve_model;
use crate::config::Config;
use crate::git;
use crate::tool::changelog::build_messages;
use anyhow::{anyhow, Result};

pub async fn run(
    from: Option<String>,
    to: Option<String>,
    model: Option<String>,
    lang: Option<String>,
    no_cache: bool,
    json: bool,
) -> Result<()> {
    let cfg = Config::load()?;
    let cwd = std::env::current_dir()?;

    let from = match from {
        Some(f) => f,
        None => git::latest_tag(&cwd)?
            .ok_or_else(|| anyhow!("no tags found; pass --from <ref> to set the range start"))?,
    };
    let to = to.unwrap_or_else(|| "HEAD".into());
    let commits = git::log_range(&cwd, &from, &to)?;
    if commits.trim().is_empty() {
        return Err(anyhow!("no commits in `{from}..{to}`"));
    }
    let range = format!("{from}..{to}");

    let alias = model.unwrap_or_else(|| cfg.commit.model.clone());
    let resolved = resolve_model(&cfg, &alias)?;
    let lang = lang.unwrap_or_else(|| cfg.commit.language.clone());
    let messages = build_messages(&lang, &range, &commits);

    let generated =
        crate::commands::generate::generate(&resolved, messages, no_cache, json).await?;
    let text = crate::tool::review::postprocess(&generated.raw).ok_or_else(|| {
        anyhow!(
            "model returned an empty/unusable changelog. raw: {:?}",
            generated.raw
        )
    })?;

    if json {
        emit_json(&serde_json::json!({
            "changelog": text,
            "from": from,
            "to": to,
            "cached": generated.cached,
            "provider": resolved.provider_name.clone(),
            "model": resolved.model.clone(),
            "prompt_tokens": generated.usage.prompt_tokens,
            "completion_tokens": generated.usage.completion_tokens,
        }));
    } else {
        println!("\n{text}\n");
    }

    let _ = crate::audit::record(&crate::audit::AuditEntry {
        tool: "git.changelog.generate".into(),
        provider: resolved.provider_name.clone(),
        model: resolved.model.clone(),
        prompt_tokens: generated.usage.prompt_tokens,
        completion_tokens: generated.usage.completion_tokens,
        decision: "generated".into(),
    });
    Ok(())
}
