// SPDX-License-Identifier: MIT
//! `aish run <prompt>`: turn a natural-language description into a single shell
//! command, show it behind a confirm/edit gate, and run it via `sh -c`. The
//! wrapped command's exit code is propagated; aborting or `--print` exits 0.

use crate::commands::emit_json;
use crate::commands::generate::Generated;
use crate::config::resolve::{resolve_model, Resolved};
use crate::config::Config;
use crate::tool::run::{build_messages, postprocess};
use anyhow::{anyhow, Result};
use std::io::Write;
use std::process::Command;

pub async fn run(
    prompt: Vec<String>,
    yes: bool,
    print: bool,
    model: Option<String>,
    lang: Option<String>,
    no_cache: bool,
    json: bool,
) -> Result<()> {
    let cfg = Config::load()?;
    let alias = model.unwrap_or_else(|| cfg.commit.model.clone());
    let resolved = resolve_model(&cfg, &alias)?;
    let lang = lang.unwrap_or_else(|| cfg.commit.language.clone());

    let os = std::env::consts::OS;
    let shell = current_shell();
    let messages = build_messages(&lang, os, &shell, &prompt.join(" "));

    let generated =
        crate::commands::generate::generate(&resolved, messages, no_cache, json).await?;
    let command = postprocess(&generated.raw).ok_or_else(|| {
        anyhow!(
            "model returned an empty/unusable command. raw: {:?}",
            generated.raw
        )
    })?;

    // --print takes precedence: never executes, so the no-prompt gate is moot.
    let decision: &'static str = if print {
        if json {
            emit_json(&serde_json::json!({
                "command": command,
                "decision": "printed",
                "ran": false,
                "cached": generated.cached,
                "provider": resolved.provider_name.clone(),
                "model": resolved.model.clone(),
                "prompt_tokens": generated.usage.prompt_tokens,
                "completion_tokens": generated.usage.completion_tokens,
            }));
        } else {
            println!("{command}");
        }
        "printed"
    } else {
        run_with_gate(command.clone(), yes, json, &resolved, &generated)
    };

    let _ = crate::audit::record(&crate::audit::AuditEntry {
        tool: "command.generate".into(),
        provider: resolved.provider_name.clone(),
        model: resolved.model.clone(),
        prompt_tokens: generated.usage.prompt_tokens,
        completion_tokens: generated.usage.completion_tokens,
        decision: decision.into(),
    });

    // On "ran"/"edited" the gate already called process::exit; reaching here
    // means "printed" or "aborted", both a clean exit 0.
    Ok(())
}

/// Confirm/edit gate (skipped under `--yes` and `--json`), then execute.
/// Returns the audit decision; calls `std::process::exit` after running so the
/// wrapped command's exit code is propagated.
fn run_with_gate(
    mut command: String,
    yes: bool,
    json: bool,
    resolved: &Resolved<'_>,
    generated: &Generated,
) -> &'static str {
    let mut edited = false;

    if !yes && !json {
        match confirm_loop(&command) {
            Ok(Some((cmd, was_edited))) => {
                command = cmd;
                edited = was_edited;
            }
            Ok(None) => {
                println!("Aborted.");
                return "aborted";
            }
            Err(e) => {
                eprintln!("aish: {e}");
                return "aborted";
            }
        }
    }

    if json {
        emit_json(&serde_json::json!({
            "command": command,
            "decision": if edited { "edited" } else { "ran" },
            "ran": true,
            "cached": generated.cached,
            "provider": resolved.provider_name.clone(),
            "model": resolved.model.clone(),
            "prompt_tokens": generated.usage.prompt_tokens,
            "completion_tokens": generated.usage.completion_tokens,
        }));
    }

    let code = exec(&command);
    std::process::exit(code);
}

/// Show the command and prompt `[Y/n/e(dit)]`. Returns the (possibly edited)
/// command to run, or None to abort.
fn confirm_loop(initial: &str) -> Result<Option<(String, bool)>> {
    let mut command = initial.to_string();
    let mut edited = false;
    loop {
        println!("\n─── aish ───────────────────────────────────────────────");
        println!("{command}");
        println!("─────────────────────────────────────────────────────────");
        print!("Run? [Y/n/e(dit)] ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        let n = std::io::stdin().read_line(&mut input)?;
        if n == 0 {
            // EOF / non-interactive (e.g. </dev/null): do not run.
            return Ok(None);
        }
        match input.trim().to_lowercase().as_str() {
            "" | "y" | "yes" => return Ok(Some((command, edited))),
            "e" | "edit" => {
                command = crate::editor::edit(&command).map_err(|e| anyhow!(e))?;
                command = command.trim().to_string();
                if command.is_empty() {
                    return Ok(None);
                }
                edited = true;
            }
            _ => return Ok(None),
        }
    }
}

/// Run the command via `sh -c`, inheriting stdio so it behaves as if typed.
/// Returns its exit code (127 if it could not start).
fn exec(command: &str) -> i32 {
    match Command::new("sh").arg("-c").arg(command).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("aish: failed to run command: {e}");
            127
        }
    }
}

/// Basename of `$SHELL`, falling back to `sh`, for prompt grounding only.
fn current_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .as_deref()
        .and_then(|p| p.rsplit('/').next())
        .filter(|s| !s.is_empty())
        .unwrap_or("sh")
        .to_string()
}
