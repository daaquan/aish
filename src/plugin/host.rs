// SPDX-License-Identifier: MIT
use crate::config::Config;
use crate::plugin::manifest::{Manifest, PluginEntry};
use crate::plugin::protocol::{Frame, ProtoError, ABI_MAJOR, MAX_FRAME_BYTES};
use crate::plugin::services::{available_services, handle, scoped_config};
use anyhow::{anyhow, Context, Result};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);

/// Spawn an installed plugin and drive one invocation to completion.
/// Returns the plugin's reported exit code.
pub async fn run_plugin(
    entry: &PluginEntry,
    manifest: &Manifest,
    subcommand: &str,
    args: &[String],
    cwd: &Path,
    cfg: &Config,
) -> Result<i32> {
    if manifest.abi_major() != Some(ABI_MAJOR) {
        return Err(anyhow!(
            "plugin `{}` speaks ABI {} but this aish supports major {}",
            manifest.name,
            manifest.abi,
            ABI_MAJOR
        ));
    }

    let mut child = Command::new(&entry.path)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawning plugin `{}`", entry.path.display()))?;

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // Always drain stderr so a chatty plugin cannot deadlock the pipe.
    let stderr_handle = tokio::spawn(async move {
        let mut buf = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut buf).await;
        buf
    });

    let invoke = Frame::Invoke {
        id: 1,
        subcommand: subcommand.to_string(),
        args: args.to_vec(),
        cwd: cwd.display().to_string(),
        config: scoped_config(cfg),
        services: available_services(manifest),
    };
    write_frame(&mut stdin, &invoke).await?;

    let mut first = true;
    loop {
        let timeout = if first { STARTUP_TIMEOUT } else { REQUEST_TIMEOUT };
        let maybe_line = tokio::time::timeout(timeout, read_frame_line(&mut reader))
            .await
            .map_err(|_| anyhow!("plugin `{}` timed out", manifest.name))??;
        first = false;

        let Some(line) = maybe_line else {
            // EOF before a result frame => crash.
            let stderr_tail = tail(&stderr_handle.await.unwrap_or_default(), 2000);
            let status = child.wait().await?;
            return Err(anyhow!(
                "plugin `{}` exited before sending a result (status {status}).\n{stderr_tail}",
                manifest.name
            ));
        };

        match Frame::from_line(line.trim_end()) {
            Ok(Frame::Request { id, op, payload }) => {
                let resp = match handle(&op, payload, manifest, cfg).await {
                    Ok(payload) => Frame::Response { id, ok: true, payload: Some(payload), error: None },
                    Err(ProtoError { code, message }) => Frame::Response {
                        id,
                        ok: false,
                        payload: None,
                        error: Some(ProtoError { code, message }),
                    },
                };
                write_frame(&mut stdin, &resp).await?;
            }
            Ok(Frame::Result { ok, payload, .. }) => {
                let _ = child.wait().await?;
                if ok {
                    return Ok(payload.get("exit").and_then(|v| v.as_i64()).unwrap_or(0) as i32);
                }
                return Err(anyhow!("plugin `{}` reported failure: {payload}", manifest.name));
            }
            Ok(other) => {
                let _ = child.kill().await;
                return Err(anyhow!("protocol error: unexpected frame from plugin: {other:?}"));
            }
            Err(e) => {
                let _ = child.kill().await;
                return Err(anyhow!("protocol error: malformed frame: {e}"));
            }
        }
    }
}

async fn write_frame<W: AsyncWriteExt + Unpin>(w: &mut W, frame: &Frame) -> Result<()> {
    let line = frame.to_line()?;
    w.write_all(line.as_bytes()).await?;
    w.write_all(b"\n").await?;
    w.flush().await?;
    Ok(())
}

/// Read one newline-delimited frame, enforcing the size cap. `Ok(None)` on EOF.
async fn read_frame_line<R: AsyncBufRead + AsyncBufReadExt + Unpin>(
    reader: &mut R,
) -> Result<Option<String>> {
    let mut line = String::new();
    let n = reader.read_line(&mut line).await?;
    if n == 0 {
        return Ok(None);
    }
    if line.len() > MAX_FRAME_BYTES {
        return Err(anyhow!("protocol error: frame exceeds {MAX_FRAME_BYTES} bytes"));
    }
    Ok(Some(line))
}

fn tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let start = s.len() - max;
    let start = (start..s.len()).find(|i| s.is_char_boundary(*i)).unwrap_or(s.len());
    format!("…{}", &s[start..])
}
