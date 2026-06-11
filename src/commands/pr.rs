// SPDX-License-Identifier: MIT
use crate::commands::emit_json;
use crate::config::resolve::resolve_model;
use crate::config::Config;
use crate::git;
use crate::provider::{build_provider, ChatRequest};
use crate::tool::pr::{build_messages, parse_response, PrDescription};
use anyhow::{anyhow, Result};
use std::io::Write;
use std::path::Path;

pub async fn run(
    apply: bool,
    model: Option<String>,
    lang: Option<String>,
    base: Option<String>,
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

    let cache_dir = crate::cache::cache_dir();
    let cache_key = crate::cache::request_key(&resolved.provider_name, &resolved.model, &messages);

    let mut cached = false;
    let (raw, usage) = match (!no_cache)
        .then(|| crate::cache::get(&cache_dir, &cache_key))
        .flatten()
    {
        Some(hit) => {
            cached = true;
            if !json {
                println!("(cached — no model request made)");
            }
            (hit, crate::provider::Usage::default())
        }
        None => {
            // Test hook: AISH_PROVIDER=mock returns a canned reply without network.
            let provider: Box<dyn crate::provider::Provider> =
                if std::env::var("AISH_PROVIDER").as_deref() == Ok("mock") {
                    Box::new(crate::provider::mock::MockProvider::new(
                        std::env::var("AISH_MOCK_REPLY")
                            .unwrap_or_else(|_| "feat: add thing\n\nBody.".into()),
                    ))
                } else {
                    build_provider(&resolved.provider_name, &resolved).map_err(|e| anyhow!(e))?
                };

            let resp = provider
                .chat(ChatRequest {
                    model: resolved.model.clone(),
                    messages,
                    temperature: Some(0.2),
                })
                .await
                .map_err(|e| anyhow!(e))?;

            if !no_cache {
                let _ = crate::cache::put(&cache_dir, &cache_key, &resp.content);
            }
            (resp.content, resp.usage.unwrap_or_default())
        }
    };

    let pr = parse_response(&raw).ok_or_else(|| {
        anyhow!("model returned an empty/unusable reply; not creating a PR. raw: {raw:?}")
    })?;

    if !json {
        println!("\nSuggested PR:\n\n{}\n\n{}\n", pr.title, pr.body);
    }

    let decision = if apply {
        create_pr(&cwd, &pr)?;
        if !json {
            println!("PR created.");
        }
        "applied"
    } else if json {
        // JSON mode is non-interactive: emit the suggestion without creating.
        "suggested"
    } else {
        confirm_loop(&cwd, pr.clone())?
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
fn create_pr(cwd: &Path, pr: &PrDescription) -> Result<()> {
    let out = std::process::Command::new("gh")
        .current_dir(cwd)
        .args(["pr", "create", "--title", &pr.title, "--body", &pr.body])
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
fn confirm_loop(cwd: &Path, mut pr: PrDescription) -> Result<&'static str> {
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
                create_pr(cwd, &pr)?;
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
