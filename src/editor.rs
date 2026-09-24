// SPDX-License-Identifier: MIT
use crate::paths::write_new_owner_only;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

/// Disambiguates temp files between concurrent `edit` calls in one process.
static EDIT_SEQ: AtomicU64 = AtomicU64::new(0);

/// Names [`create_buffer`] tries before giving up. Every name carries a random
/// token, so one that is already taken is a freak coincidence and a retry or
/// two is plenty; the bound only keeps a temp dir that reports every name as
/// taken from spinning forever.
const CREATE_ATTEMPTS: u32 = 16;

#[derive(Debug, Error)]
pub enum EditorError {
    #[error("failed to launch editor `{editor}`: {source}")]
    Spawn {
        editor: String,
        source: std::io::Error,
    },
    #[error("editor `{editor}` exited with a non-zero status")]
    Failed { editor: String },
    #[error("failed to read edited message: {0}")]
    Read(std::io::Error),
    #[error("failed to write temp message file: {0}")]
    Write(std::io::Error),
}

/// Resolve the user's preferred editor: `$VISUAL`, then `$EDITOR`, then `vi`.
pub fn resolve_editor() -> String {
    std::env::var("VISUAL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            std::env::var("EDITOR")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })
        .unwrap_or_else(|| "vi".to_string())
}

/// Open the user's editor on `message` and return the edited text (trailing
/// whitespace trimmed). The editor is invoked through `sh -c` exactly like git,
/// so values carrying arguments (e.g. `code --wait`) work.
pub fn edit(message: &str) -> Result<String, EditorError> {
    edit_with(&resolve_editor(), message)
}

/// Core of [`edit`] with an explicit editor command (keeps the global-env
/// resolution out of the file/launch logic so it is testable without env races).
fn edit_with(editor: &str, message: &str) -> Result<String, EditorError> {
    let path = create_buffer(&std::env::temp_dir(), message, || {
        buffer_name(EDIT_SEQ.fetch_add(1, Ordering::Relaxed))
    })
    .map_err(EditorError::Write)?;

    let result = launch(editor, &path);
    let edited = std::fs::read_to_string(&path).map_err(EditorError::Read);
    let _ = std::fs::remove_file(&path);
    result?;

    Ok(edited?.trim_end().to_string())
}

/// File name for this process's `seq`-th edit buffer. PID + sequence keep
/// aish's own buffers apart (concurrent processes, concurrent calls in one
/// process). They are easy to guess, though, and another user who plants
/// every name [`create_buffer`] would try makes the edit fail; the random
/// token keeps the names unknown in advance. It comes from std's OS-seeded
/// hasher keys, which are best effort, so the exclusive create stays what
/// guarantees nothing is written through a planted file.
fn buffer_name(seq: u64) -> String {
    let pid = std::process::id();
    let token = RandomState::new().build_hasher().finish();
    format!("aish-COMMIT_EDITMSG-{pid}-{token:016x}-{seq}")
}

/// Write `message` to a new file in `dir`, owner-only (`0600`) on unix, and
/// return its path. The temp dir is usually shared (`/tmp`), so the name is
/// claimed with an exclusive create ([`write_new_owner_only`]) rather than
/// assumed free: a file or symlink someone else put there first is never
/// written through, and the next name from `next_name` is tried instead, up
/// to [`CREATE_ATTEMPTS`] names in all.
fn create_buffer(
    dir: &Path,
    message: &str,
    mut next_name: impl FnMut() -> String,
) -> io::Result<PathBuf> {
    let mut attempt = 1;
    loop {
        let path = dir.join(next_name());
        match write_new_owner_only(&path, message) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && attempt < CREATE_ATTEMPTS => {
                attempt += 1
            }
            Err(e) => return Err(e),
        }
    }
}

/// Run `sh -c '<editor> "$@"' aish <file>` so the path reaches the editor as a
/// single safely-quoted argument regardless of spaces or shell metacharacters.
fn launch(editor: &str, path: &Path) -> Result<(), EditorError> {
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} \"$@\""))
        .arg("aish")
        .arg(path)
        .status()
        .map_err(|source| EditorError::Spawn {
            editor: editor.to_string(),
            source,
        })?;
    if !status.success() {
        return Err(EditorError::Failed {
            editor: editor.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_prefers_visual_over_editor() {
        // Guarded by a serial lock would be ideal; these vars are process-global.
        std::env::set_var("VISUAL", "vis");
        std::env::set_var("EDITOR", "ed");
        assert_eq!(resolve_editor(), "vis");
        std::env::remove_var("VISUAL");
        assert_eq!(resolve_editor(), "ed");
        std::env::remove_var("EDITOR");
        assert_eq!(resolve_editor(), "vi");
    }

    #[test]
    fn edit_returns_editor_modified_content() {
        // A non-interactive "editor" that overwrites the file with new text.
        let out = edit_with("printf 'fix: edited subject' >", "feat: original").unwrap();
        assert_eq!(out, "fix: edited subject");
    }

    #[test]
    fn edit_trims_trailing_whitespace() {
        let out = edit_with("printf 'feat: x\\n\\n' >", "seed").unwrap();
        assert_eq!(out, "feat: x");
    }

    #[test]
    fn edit_surfaces_failure_when_editor_exits_nonzero() {
        let err = edit_with("false", "seed").unwrap_err();
        assert!(matches!(err, EditorError::Failed { .. }));
    }

    /// If the name followed from PID + sequence alone, another user could
    /// plant all [`CREATE_ATTEMPTS`] of them and block the edit.
    #[test]
    fn buffer_name_is_not_derived_from_pid_and_seq_alone() {
        let (a, b) = (buffer_name(0), buffer_name(0));
        assert_ne!(a, b, "same name twice for the same PID and sequence");
        let prefix = format!("aish-COMMIT_EDITMSG-{}-", std::process::id());
        assert!(a.starts_with(&prefix) && a.ends_with("-0"), "{a}");
    }

    /// Names `buf-0`, `buf-1`, ... so a test knows which one comes first.
    fn numbered() -> impl FnMut() -> String {
        let mut n = 0..;
        move || format!("buf-{}", n.next().unwrap())
    }

    /// The buffer holds the draft commit message or `aish run` command, and
    /// the temp dir is usually shared, so no one else may read it.
    #[cfg(unix)]
    #[test]
    fn buffer_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = create_buffer(dir.path(), "feat: draft", numbered()).unwrap();
        let m = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(m & 0o077, 0, "edit buffer created at {m:o}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "feat: draft");
    }

    #[test]
    fn buffer_skips_a_name_that_is_already_taken() {
        let dir = tempfile::tempdir().unwrap();
        let taken = dir.path().join("buf-0");
        std::fs::write(&taken, "someone else's").unwrap();

        let path = create_buffer(dir.path(), "feat: mine", numbered()).unwrap();
        assert_eq!(path, dir.path().join("buf-1"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "feat: mine");
        assert_eq!(std::fs::read_to_string(&taken).unwrap(), "someone else's");
    }

    /// A symlink planted at the predictable name must not redirect the write
    /// into a file of the planter's choosing.
    #[cfg(unix)]
    #[test]
    fn buffer_never_writes_through_a_planted_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::write(&target, "untouched").unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join("buf-0")).unwrap();
        // A dangling one too: following it would create the file it names.
        let dangling = dir.path().join("created-through-link");
        std::os::unix::fs::symlink(&dangling, dir.path().join("buf-1")).unwrap();

        let path = create_buffer(dir.path(), "feat: mine", numbered()).unwrap();
        assert_eq!(path, dir.path().join("buf-2"));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "untouched");
        assert!(!dangling.exists(), "wrote through a dangling symlink");
    }

    #[test]
    fn buffer_gives_up_after_bounded_attempts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("taken"), "").unwrap();
        let mut calls = 0;
        let err = create_buffer(dir.path(), "seed", || {
            calls += 1;
            "taken".to_string()
        })
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(calls, CREATE_ATTEMPTS);
    }
}
