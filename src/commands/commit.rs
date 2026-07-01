// SPDX-License-Identifier: MIT
use crate::commands::emit_json;
use crate::config::resolve::resolve_model;
use crate::config::Config;
use crate::git;
use crate::tool::commit::{build_messages, postprocess};
use anyhow::{anyhow, Result};
use std::io::Write;

#[allow(clippy::too_many_arguments)]
pub async fn run(
    apply: bool,
    all: bool,
    edit: bool,
    model: Option<String>,
    style: Option<String>,
    lang: Option<String>,
    signoff: bool,
    no_cache: bool,
    json: bool,
) -> Result<()> {
    let cfg = Config::load()?;
    let cwd = std::env::current_dir()?;

    if all {
        git::stage_tracked(&cwd)?;
    }

    let diff = git::staged_diff(&cwd)?;
    if diff.trim().is_empty() {
        if json {
            emit_json(&serde_json::json!({
                "committed": false,
                "message": serde_json::Value::Null,
                "note": "nothing staged",
            }));
        } else {
            println!("Nothing staged. Run `git add` first.");
        }
        return Ok(());
    }

    let alias = model.unwrap_or_else(|| cfg.commit.model.clone());
    let resolved = resolve_model(&cfg, &alias)?;

    let style = style.unwrap_or_else(|| cfg.commit.style.clone());
    let lang = lang.unwrap_or_else(|| cfg.commit.language.clone());
    let messages = build_messages(&style, &lang, cfg.commit.instructions.as_deref(), &diff);

    let generated =
        crate::commands::generate::generate(&resolved, messages, no_cache, json).await?;
    let (cached, usage) = (generated.cached, generated.usage);
    let message = postprocess(&generated.raw);
    if message.is_empty() {
        return Err(anyhow!(
            "model returned an empty/unusable message; not committing. raw: {:?}",
            generated.raw
        ));
    }

    if !json {
        println!("\nSuggested commit:\n\n{message}\n");
    }

    let decision = if apply {
        git::commit(&cwd, &message, signoff)?;
        if !json {
            println!("Committed.");
        }
        "applied"
    } else if json {
        // JSON mode is non-interactive: emit the suggestion without committing.
        // CI that wants to commit passes `--apply --json`.
        "suggested"
    } else if edit {
        // Open git's editor pre-filled with the message; save commits, an
        // emptied message aborts (git handles both).
        if git::commit_with_editor(&cwd, &message, signoff)? {
            println!("Committed.");
            "edited"
        } else {
            println!("Aborted.");
            "rejected"
        }
    } else {
        confirm_loop(&cwd, message.clone(), signoff)?
    };

    if json {
        emit_json(&serde_json::json!({
            "message": message,
            "decision": decision,
            "committed": decision == "applied" || decision == "edited",
            "cached": cached,
            "provider": resolved.provider_name.clone(),
            "model": resolved.model.clone(),
            "prompt_tokens": usage.prompt_tokens,
            "completion_tokens": usage.completion_tokens,
        }));
    }

    let _ = crate::audit::record(&crate::audit::AuditEntry {
        tool: "git.commit.message.generate".into(),
        provider: resolved.provider_name.clone(),
        model: resolved.model.clone(),
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        decision: decision.into(),
    });
    Ok(())
}

/// Interactive accept/edit/reject loop. Re-prompts after each edit so the user
/// confirms the edited message before it is committed.
fn confirm_loop(cwd: &std::path::Path, mut message: String, signoff: bool) -> Result<&'static str> {
    let mut edited = false;
    loop {
        print!("Accept? [Y/n/e(dit)] ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        let n = std::io::stdin().read_line(&mut input)?;
        if n == 0 {
            // EOF / non-interactive (e.g. </dev/null): do not commit.
            println!("Aborted (no input).");
            return Ok("rejected");
        }
        match input.trim().to_lowercase().as_str() {
            "" | "y" | "yes" => {
                git::commit(cwd, &message, signoff)?;
                println!("Committed.");
                return Ok(if edited { "edited" } else { "applied" });
            }
            "e" | "edit" => {
                message = crate::editor::edit(&message).map_err(|e| anyhow!(e))?;
                if message.trim().is_empty() {
                    println!("Aborted (empty message).");
                    return Ok("rejected");
                }
                edited = true;
                println!("\nEdited commit:\n\n{message}\n");
            }
            _ => {
                println!("Aborted.");
                return Ok("rejected");
            }
        }
    }
}
