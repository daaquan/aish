// SPDX-License-Identifier: MIT
use crate::commands::emit_json;
use crate::config::resolve::resolve_model;
use crate::config::Config;
use crate::git;
use crate::tool::pr::{build_messages, parse_response, PrDescription};
use anyhow::{anyhow, Result};
use std::io::Write;
use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub async fn run(
    apply: bool,
    model: Option<String>,
    lang: Option<String>,
    base: Option<String>,
    draft: bool,
    no_cache: bool,
    json: bool,
) -> Result<()> {
    let cfg = Config::load()?;
    let cwd = std::env::current_dir()?;

    let base = match base {
        Some(b) => b,
        None => git::default_branch(&cwd)?,
    };
    let branch = git::current_branch(&cwd)?;
    if branch == base {
        return Err(anyhow!(
            "HEAD is on the default branch `{base}`; switch to a feature branch first"
        ));
    }
    let commits = git::branch_log(&cwd, &base)?;
    if commits.trim().is_empty() {
        return Err(anyhow!("no commits ahead of `{base}` on `{branch}`"));
    }
    let diff = git::branch_diff(&cwd, &base)?;

    let alias = model.unwrap_or_else(|| cfg.commit.model.clone());
    let resolved = resolve_model(&cfg, &alias)?;
    let lang = lang.unwrap_or_else(|| cfg.commit.language.clone());
    let messages = build_messages(&lang, &commits, &diff);

    let generated =
        crate::commands::generate::generate(&resolved, messages, no_cache, json).await?;
    let (cached, usage) = (generated.cached, generated.usage);

    let pr = parse_response(&generated.raw).ok_or_else(|| {
        anyhow!(
            "model returned an empty/unusable reply; not creating a PR. raw: {:?}",
            generated.raw
        )
    })?;

    if !json {
        println!("\nSuggested PR:\n\n{}\n\n{}\n", pr.title, pr.body);
    }

    let decision = if apply {
        create_pr(&cwd, &pr, draft)?;
        if !json {
            println!("PR created.");
        }
        "applied"
    } else if json {
        // JSON mode is non-interactive: emit the suggestion without creating.
        "suggested"
    } else {
        confirm_loop(&cwd, pr.clone(), draft)?
    };

    if json {
        emit_json(&serde_json::json!({
            "title": pr.title,
            "body": pr.body,
            "decision": decision,
            "created": decision == "applied" || decision == "edited",
            "cached": cached,
            "provider": resolved.provider_name.clone(),
            "model": resolved.model.clone(),
            "prompt_tokens": usage.prompt_tokens,
            "completion_tokens": usage.completion_tokens,
        }));
    }

    let _ = crate::audit::record(&crate::audit::AuditEntry {
        tool: "git.pr.description.generate".into(),
        provider: resolved.provider_name.clone(),
        model: resolved.model.clone(),
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        decision: decision.into(),
    });
    Ok(())
}

/// Create the PR via `gh pr create` with the generated title and body.
fn create_pr(cwd: &Path, pr: &PrDescription, draft: bool) -> Result<()> {
    let mut args = vec!["pr", "create", "--title", &pr.title, "--body", &pr.body];
    if draft {
        args.push("--draft");
    }
    let out = std::process::Command::new("gh")
        .current_dir(cwd)
        .args(&args)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow!("`gh` executable not found on PATH (https://cli.github.com)")
            } else {
                anyhow!("failed to run `gh`: {e}")
            }
        })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow!("`gh pr create` failed:\n{}", stderr.trim()));
    }
    let url = String::from_utf8_lossy(&out.stdout);
    if !url.trim().is_empty() {
        println!("{}", url.trim());
    }
    Ok(())
}

/// Interactive accept/edit/reject loop. The editor receives `title\n\nbody`;
/// after an edit the first line becomes the new title.
fn confirm_loop(cwd: &Path, mut pr: PrDescription, draft: bool) -> Result<&'static str> {
    let mut edited = false;
    loop {
        print!("Create PR? [Y/n/e(dit)] ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        let n = std::io::stdin().read_line(&mut input)?;
        if n == 0 {
            // EOF / non-interactive (e.g. </dev/null): do not create.
            println!("Aborted (no input).");
            return Ok("rejected");
        }
        match input.trim().to_lowercase().as_str() {
            "" | "y" | "yes" => {
                create_pr(cwd, &pr, draft)?;
                println!("PR created.");
                return Ok(if edited { "edited" } else { "applied" });
            }
            "e" | "edit" => {
                let text = format!("{}\n\n{}", pr.title, pr.body);
                let new = crate::editor::edit(&text).map_err(|e| anyhow!(e))?;
                match crate::tool::pr::parse_response(&new) {
                    Some(p) => pr = p,
                    None => {
                        println!("Aborted (empty description).");
                        return Ok("rejected");
                    }
                }
                edited = true;
                println!("\nEdited PR:\n\n{}\n\n{}\n", pr.title, pr.body);
            }
            _ => {
                println!("Aborted.");
                return Ok("rejected");
            }
        }
    }
}
