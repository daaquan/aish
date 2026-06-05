// SPDX-License-Identifier: AGPL-3.0-only
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

/// Disambiguates temp files between concurrent `edit` calls in one process.
static EDIT_SEQ: AtomicU64 = AtomicU64::new(0);

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
    // Unique temp path; PID + per-call sequence avoids collisions between
    // concurrent processes and concurrent calls within one process.
    let seq = EDIT_SEQ.fetch_add(1, Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("aish-COMMIT_EDITMSG-{}-{seq}", std::process::id()));
    std::fs::write(&path, message).map_err(EditorError::Write)?;

    let result = launch(editor, &path);
    let edited = std::fs::read_to_string(&path).map_err(EditorError::Read);
    let _ = std::fs::remove_file(&path);
    result?;

    Ok(edited?.trim_end().to_string())
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
}
