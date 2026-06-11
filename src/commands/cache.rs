// SPDX-License-Identifier: MIT
use crate::commands::emit_json;
use anyhow::Result;
use std::io::Write;

pub fn stats(json: bool) -> Result<()> {
    let dir = crate::cache::cache_dir();
    let (entries, bytes) = crate::cache::stats(&dir)?;
    if json {
        emit_json(&serde_json::json!({
            "entries": entries,
            "bytes": bytes,
            "dir": dir.display().to_string(),
        }));
    } else {
        println!("{entries} entries, {bytes} bytes ({})", dir.display());
    }
    Ok(())
}

pub fn clear(yes: bool, json: bool) -> Result<()> {
    let dir = crate::cache::cache_dir();
    if !yes && !confirm(&dir)? {
        println!("Aborted.");
        return Ok(());
    }
    let removed = crate::cache::clear(&dir)?;
    if json {
        emit_json(&serde_json::json!({
            "removed": removed,
            "dir": dir.display().to_string(),
        }));
    } else {
        println!("Removed {removed} cache entries.");
    }
    Ok(())
}

/// Ask before deleting. EOF / non-interactive input counts as "no".
fn confirm(dir: &std::path::Path) -> Result<bool> {
    print!("Delete all cached responses in {}? [y/N] ", dir.display());
    std::io::stdout().flush()?;
    let mut input = String::new();
    let n = std::io::stdin().read_line(&mut input)?;
    Ok(n > 0 && matches!(input.trim().to_lowercase().as_str(), "y" | "yes"))
}
