// SPDX-License-Identifier: MIT
//! `aish uninstall` — remove the binary, optionally purge the data dir.
//! The lexical path-safety guard lives in [`crate::uninstall`]; this module
//! re-runs it on the symlink-resolved path before purging, and owns the
//! confirmation prompt and CLI output.

use crate::commands::emit_json;
use crate::paths::data_dir;
use crate::uninstall::{dir_size, human_size, validate_purge_path};
use crate::update::cargo_install;
use anyhow::{anyhow, Context, Result};
use std::io::Write;

pub fn run(purge: bool, yes: bool, json: bool) -> Result<()> {
    let exe = std::env::current_exe().context("resolving current executable")?;
    let home = dirs::home_dir().ok_or_else(|| anyhow!("cannot determine home directory"))?;

    if let Some(install) = cargo_install(&exe, &home) {
        return Err(anyhow!(
            "{} was installed via cargo; run `cargo uninstall{} aish` instead",
            exe.display(),
            install.root_arg()
        ));
    }

    let data = data_dir(&home);
    // Validate BEFORE deleting anything, so a bad $AISH_HOME aborts the
    // whole uninstall instead of leaving a half-removed install behind.
    if purge {
        validate_purge_path(&data, &home).map_err(|e| anyhow!(e))?;
        validate_resolved_purge_path(&data, &home)?;
    }

    if !yes && !confirm(&exe, purge.then_some(data.as_path()))? {
        if json {
            emit_json(&serde_json::json!({
                "removed_binary": serde_json::Value::Null,
                "removed_data": false,
                "aborted": true,
            }));
        } else {
            println!("Aborted.");
        }
        return Ok(());
    }

    std::fs::remove_file(&exe).map_err(|e| {
        anyhow!(
            "cannot remove {}: {e}; try `sudo aish uninstall`",
            exe.display()
        )
    })?;

    let mut removed_data = false;
    if purge && data.exists() {
        std::fs::remove_dir_all(&data)
            .with_context(|| format!("removing data dir {}", data.display()))?;
        removed_data = true;
    }

    if json {
        emit_json(&serde_json::json!({
            "removed_binary": exe.display().to_string(),
            "removed_data": removed_data,
        }));
    } else {
        println!("removed {}", exe.display());
        if removed_data {
            println!("removed {}", data.display());
        } else if data.exists() {
            println!(
                "kept data dir {} ({}) — remove it with `rm -r` or rerun with --purge",
                data.display(),
                human_size(dir_size(&data))
            );
        }
    }
    Ok(())
}

/// [`validate_purge_path`] again, on where `dir` really is. That check is
/// lexical, but the OS follows a symlink in any component of a path: with
/// `~/link -> /etc`, `$AISH_HOME=~/link/aish` passes it and names
/// `/etc/aish`. `home` is resolved too, as it may itself sit behind a symlink
/// (`/var -> /private/var` on macOS). A missing dir has nothing to delete.
///
/// Both ends of a symlinked `dir` must be in home. `remove_dir_all` unlinks a
/// symlink instead of following it, so the entry it removes is `dir`'s name
/// in its resolved parent; and a target outside home would be left in place
/// while the purge reported it deleted.
fn validate_resolved_purge_path(dir: &std::path::Path, home: &std::path::Path) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let real_home = home
        .canonicalize()
        .with_context(|| format!("resolving home directory {}", home.display()))?;
    // The lexical check already refused `/` and `..`; fail closed regardless.
    let (Some(parent), Some(name)) = (dir.parent(), dir.file_name()) else {
        return Err(anyhow!("refusing to purge '{}'", dir.display()));
    };
    let entry = parent
        .canonicalize()
        .with_context(|| format!("resolving {}", parent.display()))?
        .join(name);
    validate_purge_path(&entry, &real_home)
        .map_err(|e| anyhow!("{} is at {}: {e}", dir.display(), entry.display()))?;
    let real = dir
        .canonicalize()
        .with_context(|| format!("resolving data dir {}", dir.display()))?;
    validate_purge_path(&real, &real_home)
        .map_err(|e| anyhow!("{} resolves to {}: {e}", dir.display(), real.display()))?;
    Ok(())
}

/// Default-no prompt showing exactly what will be removed. EOF (piped
/// stdin) counts as "no" so scripts can't uninstall by accident.
fn confirm(exe: &std::path::Path, purge_dir: Option<&std::path::Path>) -> Result<bool> {
    println!("This will remove: {}", exe.display());
    if let Some(dir) = purge_dir {
        println!(
            "          and purge: {} ({})",
            dir.display(),
            human_size(dir_size(dir))
        );
    }
    print!("Continue? [y/N] ");
    std::io::stdout().flush()?;
    let mut input = String::new();
    let n = std::io::stdin().read_line(&mut input)?;
    if n == 0 {
        return Ok(false);
    }
    Ok(matches!(input.trim().to_lowercase().as_str(), "y" | "yes"))
}
