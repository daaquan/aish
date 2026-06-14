// SPDX-License-Identifier: MIT
//! `aish fix <cmd>`: run a command, stream its output, and on failure append
//! a model-generated diagnosis plus a suggested fix. The wrapped command's
//! exit code is always propagated; the diagnosis is best-effort and never
//! masks that code.

use crate::commands::emit_json;
use crate::config::resolve::resolve_model;
use crate::config::Config;
use crate::tool::fix::build_messages;
use crate::tool::review::postprocess;
use anyhow::{anyhow, Result};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

pub async fn run(
    cmd: Vec<String>,
    shell: bool,
    always: bool,
    model: Option<String>,
    lang: Option<String>,
    no_cache: bool,
    json: bool,
) -> Result<()> {
    // In JSON mode the command's own output would corrupt the envelope, so we
    // capture it silently; in human mode we tee it live to the terminal.
    let tee = !json;
    let (exit_code, output) = run_command(&cmd, shell, tee);

    // Success without --always is a transparent pass-through: no model call.
    if exit_code == 0 && !always {
        if json {
            emit_json(&serde_json::json!({ "exit_code": 0, "diagnosed": false }));
        }
        std::process::exit(0);
    }

    // Best-effort diagnosis. Any failure here degrades to a warning so the
    // real exit code is never hidden behind an aish error.
    if let Err(e) = diagnose(&cmd, exit_code, &output, model, lang, no_cache, json).await {
        if json {
            emit_json(&serde_json::json!({
                "exit_code": exit_code,
                "diagnosed": false,
                "error": e.to_string(),
            }));
        } else {
            eprintln!("aish: could not diagnose: {e}");
        }
    }

    std::process::exit(exit_code);
}

/// Render the command as a single string for display and the prompt.
fn command_string(cmd: &[String]) -> String {
    cmd.join(" ")
}

/// Spawn the command, capturing combined stdout+stderr while optionally teeing
/// it live to the terminal. Returns the exit code (127 if it could not start).
fn run_command(cmd: &[String], shell: bool, tee: bool) -> (i32, String) {
    let mut command = if shell {
        let mut c = Command::new("sh");
        c.arg("-c").arg(cmd.join(" "));
        c
    } else {
        let mut c = Command::new(&cmd[0]);
        c.args(&cmd[1..]);
        c
    };
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("aish: failed to run `{}`: {e}", command_string(cmd));
            return (127, String::new());
        }
    };

    let buf = Arc::new(Mutex::new(String::new()));
    let out = child.stdout.take().expect("piped stdout");
    let err = child.stderr.take().expect("piped stderr");

    // Two threads append to a shared buffer as lines arrive, so stdout/stderr
    // interleave roughly in chronological order. Each also tees to its own
    // standard stream when `tee` is set.
    let out_t = pump(out, buf.clone(), tee, false);
    let err_t = pump(err, buf.clone(), tee, true);
    let _ = out_t.join();
    let _ = err_t.join();

    let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(1);
    let captured = Arc::try_unwrap(buf)
        .map(|m| m.into_inner().unwrap())
        .unwrap_or_default();
    (code, captured)
}

/// Read `reader` line by line, append each line to `buf`, and tee to stdout or
/// stderr when requested.
fn pump<R: std::io::Read + Send + 'static>(
    reader: R,
    buf: Arc<Mutex<String>>,
    tee: bool,
    is_err: bool,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            let Ok(line) = line else { break };
            if tee {
                if is_err {
                    let mut h = std::io::stderr();
                    let _ = writeln!(h, "{line}");
                } else {
                    let mut h = std::io::stdout();
                    let _ = writeln!(h, "{line}");
                }
            }
            if let Ok(mut b) = buf.lock() {
                b.push_str(&line);
                b.push('\n');
            }
        }
    })
}

#[allow(clippy::too_many_arguments)]
async fn diagnose(
    cmd: &[String],
    exit_code: i32,
    output: &str,
    model: Option<String>,
    lang: Option<String>,
    no_cache: bool,
    json: bool,
) -> Result<()> {
    let cfg = Config::load()?;
    let alias = model.unwrap_or_else(|| cfg.commit.model.clone());
    let resolved = resolve_model(&cfg, &alias)?;
    let lang = lang.unwrap_or_else(|| cfg.commit.language.clone());
    let messages = build_messages(&lang, &command_string(cmd), exit_code, output);

    let generated =
        crate::commands::generate::generate(&resolved, messages, no_cache, json).await?;
    let diagnosis = postprocess(&generated.raw).ok_or_else(|| {
        anyhow!(
            "model returned an empty/unusable diagnosis. raw: {:?}",
            generated.raw
        )
    })?;

    if json {
        emit_json(&serde_json::json!({
            "exit_code": exit_code,
            "diagnosed": true,
            "diagnosis": diagnosis,
            "cached": generated.cached,
            "provider": resolved.provider_name.clone(),
            "model": resolved.model.clone(),
            "prompt_tokens": generated.usage.prompt_tokens,
            "completion_tokens": generated.usage.completion_tokens,
        }));
    } else {
        println!("\n─── aish ───────────────────────────────────────────────");
        println!("{diagnosis}");
        println!("─────────────────────────────────────────────────────────");
    }

    let _ = crate::audit::record(&crate::audit::AuditEntry {
        tool: "command.diagnose".into(),
        provider: resolved.provider_name.clone(),
        model: resolved.model.clone(),
        prompt_tokens: generated.usage.prompt_tokens,
        completion_tokens: generated.usage.completion_tokens,
        decision: "diagnosed".into(),
    });
    Ok(())
}
