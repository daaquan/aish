// SPDX-License-Identifier: MIT
use crate::commands::emit_json;
use crate::config::resolve::resolve_model;
use crate::config::Config;
use crate::git;
use crate::tool::review::{build_messages, postprocess};
use anyhow::{anyhow, Result};

pub async fn run(
    branch: bool,
    base: Option<String>,
    model: Option<String>,
    lang: Option<String>,
    no_cache: bool,
    json: bool,
) -> Result<()> {
    let cfg = Config::load()?;
    let cwd = std::env::current_dir()?;

    let diff = if branch || base.is_some() {
        let base = match base {
            Some(b) => b,
            None => git::default_branch(&cwd)?,
        };
        let diff = git::branch_diff(&cwd, &base)?;
        if diff.trim().is_empty() {
            return Err(anyhow!("branch diff against `{base}` is empty"));
        }
        diff
    } else {
        let diff = git::staged_diff(&cwd)?;
        if diff.trim().is_empty() {
            return Err(anyhow!("nothing staged; run `git add` or use --branch"));
        }
        diff
    };

    let alias = model.unwrap_or_else(|| cfg.commit.model.clone());
    let resolved = resolve_model(&cfg, &alias)?;
    let lang = lang.unwrap_or_else(|| cfg.commit.language.clone());
    let messages = build_messages(&lang, &diff);

    let generated =
        crate::commands::generate::generate(&resolved, messages, no_cache, json).await?;
    let review = postprocess(&generated.raw).ok_or_else(|| {
        anyhow!(
            "model returned an empty/unusable review. raw: {:?}",
            generated.raw
        )
    })?;

    if json {
        emit_json(&serde_json::json!({
            "review": review,
            "cached": generated.cached,
            "provider": resolved.provider_name.clone(),
            "model": resolved.model.clone(),
            "prompt_tokens": generated.usage.prompt_tokens,
            "completion_tokens": generated.usage.completion_tokens,
        }));
    } else {
        println!("\n{review}\n");
    }

    let _ = crate::audit::record(&crate::audit::AuditEntry {
        tool: "git.diff.review".into(),
        provider: resolved.provider_name.clone(),
        model: resolved.model.clone(),
        prompt_tokens: generated.usage.prompt_tokens,
        completion_tokens: generated.usage.completion_tokens,
        decision: "reviewed".into(),
    });
    Ok(())
}
