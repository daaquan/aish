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

    // Normalized: with a trailing `/` or `/.`, as tab completion writes a
    // link to a dir, a path to a symlink makes even lstat and remove_dir_all
    // follow it, which would empty the tree it points to, then fail on it.
    let data: std::path::PathBuf = data_dir(&home).components().collect();
    // Validate BEFORE deleting anything, so a bad $AISH_HOME aborts the
    // whole uninstall instead of leaving a half-removed install behind.
    let mut link_target = None;
    if purge {
        validate_purge_path(&data, &home).map_err(|e| anyhow!(e))?;
        link_target = validate_resolved_purge_path(&data, &home)?;
    }

    let purged = purge.then_some((data.as_path(), link_target.as_deref()));
    if !yes && !confirm(&exe, purged)? {
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

    // The binary goes before the data dir, so a binary that stays, as it does
    // on Windows, leaves the install untouched instead of half-removed.
    std::fs::remove_file(&exe).map_err(|e| {
        let unpurged = (purge && data.exists()).then(|| data_label(&data, link_target.as_deref()));
        anyhow!(
            "cannot remove {}: {e}; {}",
            exe.display(),
            remove_exe_hint(cfg!(windows), unpurged.as_deref())
        )
    })?;

    let mut removed_data = false;
    if purge && data.exists() {
        // On a symlink this only unlinks it, so the tree it points to goes
        // next. The link first, as it may sit inside that tree.
        std::fs::remove_dir_all(&data)
            .with_context(|| format!("removing data dir {}", data.display()))?;
        if let Some(target) = &link_target {
            std::fs::remove_dir_all(target)
                .with_context(|| format!("removing data dir {}", target.display()))?;
        }
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
            if let Some(target) = &link_target {
                println!("removed {}", target.display());
            }
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
/// Both ends of a symlinked `dir` must be in home, as the purge deletes both.
/// `remove_dir_all` unlinks a symlink instead of following it, so the entry
/// it removes is `dir`'s name in its resolved parent, and the tree the link
/// points to has to be removed on its own: that tree is returned.
fn validate_resolved_purge_path(
    dir: &std::path::Path,
    home: &std::path::Path,
) -> Result<Option<std::path::PathBuf>> {
    if !dir.exists() {
        return Ok(None);
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
    // remove_dir_all fails on anything else, and only once the binary is gone.
    if !real.is_dir() {
        return Err(anyhow!(
            "refusing to purge '{}': not a directory",
            dir.display()
        ));
    }
    // `dir` has no trailing `/` (see run), or this would follow the link.
    let is_link = dir
        .symlink_metadata()
        .with_context(|| format!("reading data dir {}", dir.display()))?
        .file_type()
        .is_symlink();
    Ok(is_link.then_some(real))
}

/// Default-no prompt showing exactly what will be removed. EOF (piped
/// stdin) counts as "no" so scripts can't uninstall by accident. `purge` is
/// the data dir and, when that is a symlink, the tree it points to.
fn confirm(
    exe: &std::path::Path,
    purge: Option<(&std::path::Path, Option<&std::path::Path>)>,
) -> Result<bool> {
    println!("This will remove: {}", exe.display());
    if let Some((dir, target)) = purge {
        println!(
            "          and purge: {} ({})",
            data_label(dir, target),
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

/// The data dir as the prompt and the errors name it: with the tree it points
/// to when it is a symlink, since the purge deletes that too.
fn data_label(dir: &std::path::Path, target: Option<&std::path::Path>) -> String {
    match target {
        Some(target) => format!("{} -> {}", dir.display(), target.display()),
        None => dir.display().to_string(),
    }
}

/// What to do about a binary that could not be removed. `unpurged` is the
/// data dir `--purge` would have deleted next. Windows refuses to delete a
/// running program's file whatever the privileges, so `sudo` is no help
/// there: the file can only go after aish has exited, by hand.
fn remove_exe_hint(windows: bool, unpurged: Option<&str>) -> String {
    if !windows {
        return "try `sudo aish uninstall`".into();
    }
    let mut hint = String::from(
        "Windows does not let a running program delete itself: delete it after aish exits",
    );
    if let Some(dir) = unpurged {
        hint += &format!(", and {dir} too, which was not purged");
    }
    hint
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn remove_exe_hint_suggests_sudo_off_windows() {
        for unpurged in [None, Some("/home/u/.aish")] {
            assert_eq!(
                remove_exe_hint(false, unpurged),
                "try `sudo aish uninstall`"
            );
        }
    }

    #[test]
    fn remove_exe_hint_on_windows_says_to_delete_by_hand() {
        let hint = remove_exe_hint(true, None);
        assert!(hint.ends_with("delete it after aish exits"), "{hint}");
        assert!(!hint.contains("sudo"), "{hint}");
        // Nothing was purged either, and the hint says what is left.
        let hint = remove_exe_hint(true, Some(r"C:\Users\u\.aish"));
        assert!(
            hint.ends_with(r"after aish exits, and C:\Users\u\.aish too, which was not purged"),
            "{hint}"
        );
    }

    #[test]
    fn data_label_shows_where_a_symlinked_data_dir_points() {
        let dir = Path::new("/home/u/.aish");
        assert_eq!(data_label(dir, None), "/home/u/.aish");
        assert_eq!(
            data_label(dir, Some(Path::new("/home/u/dotfiles/aish"))),
            "/home/u/.aish -> /home/u/dotfiles/aish"
        );
    }
}
