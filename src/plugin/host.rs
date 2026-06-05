// SPDX-License-Identifier: MIT
use crate::config::Config;
use crate::plugin::install::verify_sha256;
use crate::plugin::manifest::{Manifest, PluginEntry};
use crate::plugin::protocol::{Frame, ProtoError, ABI_MAJOR, MAX_FRAME_BYTES};
use crate::plugin::services::{available_services, handle, scoped_config};
use anyhow::{anyhow, Context, Result};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

/// Largest stderr tail the host retains for crash diagnostics. A plugin spewing
/// more than this only keeps its most recent bytes — the buffer never grows
/// without bound.
const MAX_STDERR_BYTES: usize = 64 * 1024;

/// Deadlines the host enforces on a plugin. Production uses [`Timeouts::default`];
/// tests inject short values to exercise each path quickly.
#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    /// Waiting for the plugin's first frame after `invoke`.
    pub startup: Duration,
    /// Waiting for each subsequent frame from the plugin.
    pub request: Duration,
    /// Host-side handling of one plugin Request (e.g. a provider call).
    pub service: Duration,
    /// Reaping the child after a Result frame or EOF before we SIGKILL it.
    pub wait: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            startup: Duration::from_secs(30),
            request: Duration::from_secs(180),
            service: Duration::from_secs(120),
            wait: Duration::from_secs(5),
        }
    }
}

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
    run_plugin_with(
        entry,
        manifest,
        subcommand,
        args,
        cwd,
        cfg,
        Timeouts::default(),
    )
    .await
}

/// Like [`run_plugin`] but with explicit [`Timeouts`] (used by tests).
#[allow(clippy::too_many_arguments)]
pub async fn run_plugin_with(
    entry: &PluginEntry,
    manifest: &Manifest,
    subcommand: &str,
    args: &[String],
    cwd: &Path,
    cfg: &Config,
    timeouts: Timeouts,
) -> Result<i32> {
    if manifest.abi_major() != Some(ABI_MAJOR) {
        return Err(anyhow!(
            "plugin `{}` speaks ABI {} but this aish supports major {}",
            manifest.name,
            manifest.abi,
            ABI_MAJOR
        ));
    }

    verify_sha256(&entry.path, &entry.binary_sha256)?;

    let mut child = Command::new(&entry.path)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Ensure a timed-out / errored-out plugin is reaped instead of orphaned:
        // any early return drops `child`, which then SIGKILLs the process.
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("spawning plugin `{}`", entry.path.display()))?;

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // Always drain stderr so a chatty plugin cannot deadlock the pipe, but keep
    // only the most recent MAX_STDERR_BYTES so a flood cannot grow the buffer
    // without bound.
    let stderr_handle = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match reader.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if buf.len() > MAX_STDERR_BYTES {
                        let excess = buf.len() - MAX_STDERR_BYTES;
                        buf.drain(0..excess);
                    }
                }
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    });

    let invoke = Frame::Invoke {
        id: 1,
        subcommand: subcommand.to_string(),
        args: args.to_vec(),
        cwd: cwd.display().to_string(),
        config: scoped_config(cfg, &manifest.name),
        services: available_services(manifest),
    };
    write_frame(&mut stdin, &invoke).await?;

    let mut first = true;
    loop {
        let frame_timeout = if first {
            timeouts.startup
        } else {
            timeouts.request
        };
        let maybe_line =
            tokio::time::timeout(frame_timeout, read_frame_line(&mut reader, MAX_FRAME_BYTES))
                .await
                .map_err(|_| anyhow!("plugin `{}` timed out", manifest.name))??;
        first = false;

        let Some(line) = maybe_line else {
            // EOF before a result frame => crash.
            let stderr_tail = tail(&stderr_handle.await.unwrap_or_default(), 2000);
            let status = reap(&mut child, timeouts.wait).await;
            return Err(anyhow!(
                "plugin `{}` exited before sending a result ({status}).\n{stderr_tail}",
                manifest.name
            ));
        };

        match Frame::from_line(line.trim_end()) {
            Ok(Frame::Request { id, op, payload }) => {
                // Bound host-side service work so a stalled provider call cannot
                // hang the host indefinitely.
                let handled =
                    tokio::time::timeout(timeouts.service, handle(&op, payload, manifest, cfg))
                        .await;
                let resp = match handled {
                    Err(_) => {
                        let _ = child.kill().await;
                        // Killing the child closes its stderr, so the drain task
                        // finishes; surface its tail for diagnostics.
                        let stderr_tail = tail(&stderr_handle.await.unwrap_or_default(), 2000);
                        return Err(anyhow!(
                            "plugin `{}` service call `{op}` timed out.\n{stderr_tail}",
                            manifest.name
                        ));
                    }
                    Ok(Ok(payload)) => Frame::Response {
                        id,
                        ok: true,
                        payload: Some(payload),
                        error: None,
                    },
                    Ok(Err(ProtoError { code, message })) => Frame::Response {
                        id,
                        ok: false,
                        payload: None,
                        error: Some(ProtoError { code, message }),
                    },
                };
                write_frame(&mut stdin, &resp).await?;
            }
            Ok(Frame::Result { ok, payload, .. }) => {
                let _ = reap(&mut child, timeouts.wait).await;
                if ok {
                    return Ok(payload.get("exit").and_then(|v| v.as_i64()).unwrap_or(0) as i32);
                }
                return Err(anyhow!(
                    "plugin `{}` reported failure: {payload}",
                    manifest.name
                ));
            }
            Ok(other) => {
                let _ = child.kill().await;
                return Err(anyhow!(
                    "protocol error: unexpected frame from plugin: {other:?}"
                ));
            }
            Err(e) => {
                let _ = child.kill().await;
                return Err(anyhow!("protocol error: malformed frame: {e}"));
            }
        }
    }
}

/// Wait for the child to exit within `dur`; if it overstays its welcome, SIGKILL
/// and reap it. Returns a human-readable status for diagnostics.
async fn reap(child: &mut Child, dur: Duration) -> String {
    match tokio::time::timeout(dur, child.wait()).await {
        Ok(Ok(status)) => format!("status {status}"),
        Ok(Err(e)) => format!("wait failed: {e}"),
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            "killed after exceeding wait timeout".to_string()
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

/// Read one newline-delimited frame, enforcing the size cap *during* the read so
/// a plugin that never emits a newline cannot make us buffer without bound.
/// `Ok(None)` on EOF.
async fn read_frame_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max: usize,
) -> Result<Option<String>> {
    let mut buf: Vec<u8> = Vec::new();
    // `take` bounds the read itself: at most `max + 1` bytes are consumed
    // regardless of whether a newline ever arrives. The `+ 1` headroom lets us
    // tell a complete `max`-byte frame apart from one that overruns the cap.
    let n = {
        let mut limited = (&mut *reader).take(max as u64 + 1);
        limited.read_until(b'\n', &mut buf).await?
    };
    if n == 0 {
        return Ok(None);
    }
    // The cap applies to frame content, not the newline terminator.
    let content_len = if buf.last() == Some(&b'\n') {
        buf.len() - 1
    } else {
        buf.len()
    };
    if content_len > max {
        return Err(anyhow!("protocol error: frame exceeds {max} bytes"));
    }
    let line =
        String::from_utf8(buf).map_err(|_| anyhow!("protocol error: frame is not valid UTF-8"))?;
    Ok(Some(line))
}

fn tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let start = s.len() - max;
    let start = (start..s.len())
        .find(|i| s.is_char_boundary(*i))
        .unwrap_or(s.len());
    format!("…{}", &s[start..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn read_frame_line_returns_line_within_cap() {
        let data = b"hello\n".to_vec();
        let mut reader = BufReader::new(&data[..]);
        let line = read_frame_line(&mut reader, 64).await.unwrap();
        assert_eq!(line.as_deref(), Some("hello\n"));
    }

    #[tokio::test]
    async fn read_frame_line_eof_returns_none() {
        let data: &[u8] = b"";
        let mut reader = BufReader::new(data);
        assert_eq!(read_frame_line(&mut reader, 64).await.unwrap(), None);
    }

    #[tokio::test]
    async fn read_frame_line_unterminated_final_line_within_cap() {
        let data = b"tail-no-newline".to_vec();
        let mut reader = BufReader::new(&data[..]);
        let line = read_frame_line(&mut reader, 64).await.unwrap();
        assert_eq!(line.as_deref(), Some("tail-no-newline"));
    }

    #[tokio::test]
    async fn read_frame_line_accepts_unterminated_line_exactly_at_cap() {
        // A final frame of exactly `max` bytes with no trailing newline is valid:
        // the cap is on content, and EOF at the cap is not an overflow.
        let data = [b'a'; 4];
        let mut reader = BufReader::new(&data[..]);
        let line = read_frame_line(&mut reader, 4).await.unwrap();
        assert_eq!(line.as_deref(), Some("aaaa"));
    }

    #[tokio::test]
    async fn read_frame_line_caps_unbounded_input() {
        // Endless stream with no newline: must error at the cap, never hang or
        // buffer without bound. Original read_line would loop here forever.
        let src = tokio::io::repeat(b'a');
        let mut reader = BufReader::new(src);
        let err = read_frame_line(&mut reader, 64).await.unwrap_err();
        assert!(err.to_string().contains("exceeds"));
    }
}
