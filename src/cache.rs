// SPDX-License-Identifier: MIT
//! Deterministic on-disk cache for AI chat responses.
//!
//! The cache key is a stable hash of the exact request (provider, model, and
//! every message). Identical requests — e.g. regenerating a commit message for
//! the same staged diff — reuse the stored response and skip the network call.
//!
//! FNV-1a is used instead of [`std::hash`] because its result must stay stable
//! across Rust versions and platforms; `DefaultHasher` makes no such promise.

use crate::provider::{Message, Role};
use std::path::{Path, PathBuf};

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x00000100000001b3;

/// FNV-1a 64-bit hash. Deterministic across runs, versions, and platforms.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn role_tag(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

/// Deterministic cache key (16 hex chars) for a chat request.
///
/// Fields are length-prefixed so no message content can be crafted to collide
/// with a different field layout.
pub fn request_key(provider: &str, model: &str, messages: &[Message]) -> String {
    let mut buf = String::new();
    buf.push_str("aish-cache-v1\n");
    for field in [provider, model] {
        buf.push_str(&field.len().to_string());
        buf.push('\n');
        buf.push_str(field);
        buf.push('\n');
    }
    for m in messages {
        buf.push_str(role_tag(m.role));
        buf.push('\n');
        buf.push_str(&m.content.len().to_string());
        buf.push('\n');
        buf.push_str(&m.content);
        buf.push('\n');
    }
    format!("{:016x}", fnv1a(buf.as_bytes()))
}

/// Default cache directory (`~/.aish/cache`), mirroring the audit log location.
pub fn cache_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".aish").join("cache")
}

fn entry_path(dir: &Path, key: &str) -> PathBuf {
    dir.join(format!("{key}.txt"))
}

/// Return the cached response for `key`, if present.
pub fn get(dir: &Path, key: &str) -> Option<String> {
    std::fs::read_to_string(entry_path(dir, key)).ok()
}

/// Store `value` for `key`, creating the cache directory if needed.
pub fn put(dir: &Path, key: &str, value: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(entry_path(dir, key), value)
}

/// Entry count and total size in bytes of the cache directory.
/// A missing directory counts as an empty cache.
pub fn stats(dir: &Path) -> std::io::Result<(usize, u64)> {
    let mut count = 0;
    let mut bytes = 0;
    for entry in entries(dir)? {
        count += 1;
        bytes += entry.metadata()?.len();
    }
    Ok((count, bytes))
}

/// Delete every cache entry. Returns the number of entries removed.
/// A missing directory counts as an empty cache.
pub fn clear(dir: &Path) -> std::io::Result<usize> {
    let mut removed = 0;
    for entry in entries(dir)? {
        std::fs::remove_file(entry.path())?;
        removed += 1;
    }
    Ok(removed)
}

/// Cache entry files in `dir`. A missing directory yields no entries.
fn entries(dir: &Path) -> std::io::Result<Vec<std::fs::DirEntry>> {
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for entry in read {
        let entry = entry?;
        if entry.path().extension().is_some_and(|e| e == "txt") {
            out.push(entry);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn msgs(diff: &str) -> Vec<Message> {
        vec![Message::system("sys"), Message::user(diff)]
    }

    #[test]
    fn key_is_deterministic_for_identical_requests() {
        let a = request_key("openai", "gpt-5-mini", &msgs("diff --git a/x"));
        let b = request_key("openai", "gpt-5-mini", &msgs("diff --git a/x"));
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn key_changes_with_diff_model_or_provider() {
        let base = request_key("openai", "gpt-5-mini", &msgs("diff A"));
        assert_ne!(base, request_key("openai", "gpt-5-mini", &msgs("diff B")));
        assert_ne!(base, request_key("openai", "gpt-5-nano", &msgs("diff A")));
        assert_ne!(
            base,
            request_key("anthropic", "gpt-5-mini", &msgs("diff A"))
        );
    }

    #[test]
    fn length_prefix_prevents_field_boundary_collisions() {
        // Without length prefixing, "ab" + "c" could collide with "a" + "bc".
        let one = request_key("ab", "c", &[]);
        let two = request_key("a", "bc", &[]);
        assert_ne!(one, two);
    }

    #[test]
    fn stats_empty_for_missing_or_empty_dir() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("nope");
        assert_eq!(stats(&missing).unwrap(), (0, 0));
        assert_eq!(stats(dir.path()).unwrap(), (0, 0));
    }

    #[test]
    fn stats_counts_entries_and_bytes() {
        let dir = tempdir().unwrap();
        put(dir.path(), "aaaa", "12345").unwrap();
        put(dir.path(), "bbbb", "123").unwrap();
        let (count, bytes) = stats(dir.path()).unwrap();
        assert_eq!(count, 2);
        assert_eq!(bytes, 8);
    }

    #[test]
    fn clear_removes_all_entries_and_reports_count() {
        let dir = tempdir().unwrap();
        put(dir.path(), "aaaa", "x").unwrap();
        put(dir.path(), "bbbb", "y").unwrap();
        assert_eq!(clear(dir.path()).unwrap(), 2);
        assert_eq!(stats(dir.path()).unwrap(), (0, 0));
        // Idempotent: clearing again (or a missing dir) removes nothing.
        assert_eq!(clear(dir.path()).unwrap(), 0);
        assert_eq!(clear(&dir.path().join("nope")).unwrap(), 0);
    }

    #[test]
    fn put_then_get_roundtrips() {
        let dir = tempdir().unwrap();
        let key = request_key("openai", "gpt-5-mini", &msgs("diff"));
        assert!(get(dir.path(), &key).is_none());
        put(dir.path(), &key, "feat: cached message").unwrap();
        assert_eq!(
            get(dir.path(), &key).as_deref(),
            Some("feat: cached message")
        );
    }
}
