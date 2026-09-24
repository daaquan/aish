// SPDX-License-Identifier: MIT
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize, Deserialize)]
pub struct AuditEntry {
    pub tool: String,
    pub provider: String,
    pub model: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub decision: String,
}

/// Path to the default audit log: `audit.log` in the data dir (`$AISH_HOME`,
/// default `~/.aish`).
pub fn log_path() -> PathBuf {
    crate::paths::default_data_dir().join("audit.log")
}

/// Append one JSONL record to the default audit log ([`log_path`]).
pub fn record(entry: &AuditEntry) -> std::io::Result<()> {
    record_to(&log_path(), entry)
}

pub fn record_to(path: &Path, entry: &AuditEntry) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        crate::paths::create_dir_owner_only(parent)?;
    }
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut value = serde_json::to_value(entry).unwrap();
    value["ts"] = serde_json::json!(ts);
    // The whole line, newline included, goes out in one write. The file is
    // unbuffered and serde_json emits a record in many small pieces, so
    // formatting straight into it let concurrent aish processes (xargs -P, CI
    // matrices) interleave fragments of their lines, which `aish usage` then
    // skipped. In append mode each single write lands whole at the end.
    let mut line = value.to_string();
    line.push('\n');
    // Owner-only from creation, like everything else in the data dir. An
    // existing log keeps its mode: it holds metadata only, never a secret.
    let mut f = crate::paths::owner_only_options()
        .create(true)
        .append(true)
        .open(path)?;
    f.write_all(line.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn appends_jsonl_line_without_secrets() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("audit.log");
        let entry = AuditEntry {
            tool: "git.commit.message.generate".into(),
            provider: "openai".into(),
            model: "gpt-5-mini".into(),
            prompt_tokens: 10,
            completion_tokens: 4,
            decision: "applied".into(),
        };
        record_to(&path, &entry).unwrap();
        record_to(&path, &entry).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content.lines().count(), 2);
        let first: serde_json::Value =
            serde_json::from_str(content.lines().next().unwrap()).unwrap();
        assert_eq!(first["provider"], "openai");
        assert!(first.get("ts").is_some());
        assert!(!content.contains("api_key"));
    }

    /// Concurrent aish processes (xargs -P, CI matrices) append to one log,
    /// each through its own handle. A line that reaches the file in several
    /// writes can have another process's fragments inside it.
    #[test]
    fn concurrent_appends_keep_every_line_whole() {
        const WRITERS: usize = 8;
        const RECORDS: usize = 200;
        let dir = tempdir().unwrap();
        let path = dir.path().join("audit.log");
        std::thread::scope(|s| {
            for w in 0..WRITERS {
                let path = &path;
                s.spawn(move || {
                    let entry = AuditEntry {
                        tool: format!("writer-{w}"),
                        provider: "openai".into(),
                        model: "gpt-5-mini".into(),
                        prompt_tokens: 10,
                        completion_tokens: 4,
                        decision: "applied".into(),
                    };
                    for _ in 0..RECORDS {
                        record_to(path, &entry).unwrap();
                    }
                });
            }
        });

        let content = std::fs::read_to_string(&path).unwrap();
        let mut per_writer = vec![0; WRITERS];
        for line in content.lines() {
            let entry: AuditEntry = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("broken audit line {line:?}: {e}"));
            let w: usize = entry.tool["writer-".len()..].parse().unwrap();
            per_writer[w] += 1;
        }
        assert_eq!(per_writer, vec![RECORDS; WRITERS]);
    }

    /// Group/other bits only, so the result does not depend on the umask.
    #[cfg(unix)]
    #[test]
    fn creates_data_dir_and_log_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let root = tempdir().unwrap();
        let data = root.path().join("data");
        let path = data.join("audit.log");
        let entry = AuditEntry {
            tool: "git.commit.message.generate".into(),
            provider: "openai".into(),
            model: "gpt-5-mini".into(),
            prompt_tokens: 1,
            completion_tokens: 1,
            decision: "applied".into(),
        };
        record_to(&path, &entry).unwrap();
        for p in [&data, &path] {
            let m = mode(p);
            assert_eq!(m & 0o077, 0, "{} created at {m:o}", p.display());
        }
    }
}
