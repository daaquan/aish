// SPDX-License-Identifier: MIT
//! `aish uninstall` — remove the binary, optionally purge the data dir.
//! Path-safety guards live in [`crate::uninstall`]; this module owns the
//! confirmation prompt and CLI output.

use crate::commands::emit_json;
use crate::uninstall::{data_dir, dir_size, human_size, validate_purge_path};
use crate::update::is_cargo_install;
use anyhow::{anyhow, Context, Result};
use std::io::Write;

pub fn run(purge: bool, yes: bool, json: bool) -> Result<()> {
    let exe = std::env::current_exe().context("resolving current executable")?;
    let home = dirs::home_dir().ok_or_else(|| anyhow!("cannot determine home directory"))?;

    if is_cargo_install(&exe, &home) {
        return Err(anyhow!(
            "{} was installed via cargo; run `cargo uninstall aish` instead",
            exe.display()
        ));
    }

    let data = data_dir(&home);
    // Validate BEFORE deleting anything, so a bad $AISH_HOME aborts the
    // whole uninstall instead of leaving a half-removed install behind.
    if purge {
        validate_purge_path(&data, &home).map_err(|e| anyhow!(e))?;
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
