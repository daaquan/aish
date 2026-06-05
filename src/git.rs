// SPDX-License-Identifier: AGPL-3.0-only
use std::path::Path;
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GitError {
    #[error("`git` executable not found on PATH")]
    NotInstalled,
    #[error("not a git repository (or any parent)")]
    NotARepo,
    #[error("failed to run `git`: {0}")]
    Spawn(String),
    #[error("`git {command}` failed (exit {code})\n{detail}")]
    Failed {
        /// The git subcommand that was invoked (e.g. `commit`).
        command: String,
        /// Exit code, or "signal" when terminated by a signal.
        code: String,
        /// Captured stderr, falling back to stdout when stderr is empty.
        detail: String,
    },
}

fn run(dir: &Path, args: &[&str]) -> Result<std::process::Output, GitError> {
    Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                GitError::NotInstalled
            } else {
                GitError::Spawn(e.to_string())
            }
        })
}

/// Run a git command and fail with rich context (subcommand, exit code, and
/// stderr — or stdout when stderr is empty) if it exits non-zero.
fn run_checked(dir: &Path, args: &[&str]) -> Result<std::process::Output, GitError> {
    let out = run(dir, args)?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let detail = if stderr.trim().is_empty() {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        return Err(GitError::Failed {
            command: args.first().copied().unwrap_or_default().to_string(),
            code: out
                .status
                .code()
                .map_or_else(|| "signal".to_string(), |c| c.to_string()),
            detail,
        });
    }
    Ok(out)
}

/// Return the staged diff (`git diff --cached`). Empty string if nothing staged.
pub fn staged_diff(dir: &Path) -> Result<String, GitError> {
    let check = run(dir, &["rev-parse", "--is-inside-work-tree"])?;
    if !check.status.success() {
        return Err(GitError::NotARepo);
    }
    let out = run_checked(dir, &["diff", "--cached"])?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Create a commit with the given message.
/// Pass `signoff: true` to append a DCO `Signed-off-by` trailer (`git commit -s`).
pub fn commit(dir: &Path, message: &str, signoff: bool) -> Result<(), GitError> {
    let mut args = vec!["commit", "-m", message];
    if signoff {
        args.push("-s");
    }
    run_checked(dir, &args)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::tempdir;

    fn init_repo() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let p = dir.path();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "t@e.st"],
            vec!["config", "user.name", "t"],
        ] {
            Command::new("git")
                .current_dir(p)
                .args(args)
                .status()
                .unwrap();
        }
        dir
    }

    #[test]
    fn reads_staged_diff() {
        let dir = init_repo();
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "a.txt"])
            .status()
            .unwrap();
        let diff = staged_diff(dir.path()).unwrap();
        assert!(diff.contains("a.txt"));
        assert!(diff.contains("hello"));
    }

    #[test]
    fn empty_when_nothing_staged() {
        let dir = init_repo();
        let diff = staged_diff(dir.path()).unwrap();
        assert!(diff.trim().is_empty());
    }

    #[test]
    fn commit_creates_revision() {
        let dir = init_repo();
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "a.txt"])
            .status()
            .unwrap();
        commit(dir.path(), "feat: add a", false).unwrap();
        let log = Command::new("git")
            .current_dir(dir.path())
            .args(["log", "--oneline"])
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&log.stdout).contains("feat: add a"));
    }

    #[test]
    fn failed_command_reports_subcommand_and_exit_code() {
        // `git commit` in a non-repo exits non-zero; the error must name the
        // subcommand, the exit code, and carry git's own diagnostic text.
        let dir = tempdir().unwrap();
        let err = commit(dir.path(), "feat: x", false).unwrap_err();
        match &err {
            GitError::Failed {
                command,
                code,
                detail,
            } => {
                assert_eq!(command, "commit");
                assert_ne!(code, "0");
                assert!(!detail.is_empty(), "detail should carry git's stderr");
            }
            other => panic!("expected GitError::Failed, got {other:?}"),
        }
        let rendered = err.to_string();
        assert!(rendered.contains("`git commit` failed"));
        assert!(rendered.contains("(exit "));
    }

    #[test]
    fn failed_command_falls_back_to_stdout_when_stderr_empty() {
        // "nothing to commit" is written to stdout (not stderr) with exit 1.
        let dir = init_repo();
        let err = commit(dir.path(), "feat: nothing", false).unwrap_err();
        match err {
            GitError::Failed { detail, .. } => {
                assert!(
                    detail.contains("nothing to commit"),
                    "stdout fallback should surface git's message, got: {detail:?}"
                );
            }
            other => panic!("expected GitError::Failed, got {other:?}"),
        }
    }

    #[test]
    fn commit_with_signoff_adds_trailer() {
        let dir = init_repo();
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "a.txt"])
            .status()
            .unwrap();
        commit(dir.path(), "feat: add a", true).unwrap();
        let log = Command::new("git")
            .current_dir(dir.path())
            .args(["log", "-1", "--format=%B"])
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&log.stdout).contains("Signed-off-by:"));
    }
}
