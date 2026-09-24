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
        std::fs::create_dir_all(parent)?;
    }
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut value = serde_json::to_value(entry).unwrap();
    value["ts"] = serde_json::json!(ts);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{value}")?;
    Ok(())
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
}
