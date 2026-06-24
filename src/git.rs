// SPDX-License-Identifier: MIT
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

/// Name of the repository's default branch.
/// Prefers `origin/HEAD` when a remote is configured; falls back to a local
/// `main` or `master` branch.
pub fn default_branch(dir: &Path) -> Result<String, GitError> {
    let head = run(
        dir,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )?;
    if head.status.success() {
        let name = String::from_utf8_lossy(&head.stdout).trim().to_string();
        if let Some(short) = name.strip_prefix("origin/") {
            return Ok(short.to_string());
        }
    }
    for candidate in ["main", "master"] {
        let exists = run(dir, &["rev-parse", "--verify", "-q", candidate])?;
        if exists.status.success() {
            return Ok(candidate.to_string());
        }
    }
    Err(GitError::Failed {
        command: "symbolic-ref".into(),
        code: "1".into(),
        detail: "cannot determine the default branch (no origin/HEAD, main, or master)".into(),
    })
}

/// Short name of the currently checked-out branch (`git rev-parse --abbrev-ref HEAD`).
pub fn current_branch(dir: &Path) -> Result<String, GitError> {
    let out = run_checked(dir, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Diff between the merge-base of `base` and HEAD (`git diff base...HEAD`).
pub fn branch_diff(dir: &Path, base: &str) -> Result<String, GitError> {
    let range = format!("{base}...HEAD");
    let out = run_checked(dir, &["diff", &range])?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// One-line subjects of commits on HEAD that are not on `base` (`git log base..HEAD`).
pub fn branch_log(dir: &Path, base: &str) -> Result<String, GitError> {
    let range = format!("{base}..HEAD");
    let out = run_checked(dir, &["log", "--format=%s", &range])?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Most recent tag reachable from HEAD (`git describe --tags --abbrev=0`),
/// or None when the repository has no tags.
pub fn latest_tag(dir: &Path) -> Result<Option<String>, GitError> {
    let out = run(dir, &["describe", "--tags", "--abbrev=0"])?;
    if !out.status.success() {
        return Ok(None);
    }
    let tag = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok((!tag.is_empty()).then_some(tag))
}

/// One-line subjects of commits in `from..to`.
pub fn log_range(dir: &Path, from: &str, to: &str) -> Result<String, GitError> {
    let range = format!("{from}..{to}");
    let out = run_checked(dir, &["log", "--format=%s", &range])?;
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

/// Commit by opening git's editor (`git commit -e`) seeded with `message` and
/// the standard commit template: saving commits, an emptied message aborts.
/// Inherits our stdio so the editor is interactive (unlike [`commit`], which
/// captures output), and treats git's "aborting commit due to empty message"
/// exit as a clean cancellation rather than an error.
pub fn commit_with_editor(dir: &Path, message: &str, signoff: bool) -> Result<bool, GitError> {
    let mut args = vec!["commit", "-e", "-m", message];
    if signoff {
        args.push("-s");
    }
    let status = Command::new("git")
        .current_dir(dir)
        .args(&args)
        .status()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                GitError::NotInstalled
            } else {
                GitError::Spawn(e.to_string())
            }
        })?;
    // git exits non-zero when the commit is aborted (e.g. emptied message);
    // that's a user cancellation, not a failure.
    Ok(status.success())
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

    fn stage_a(dir: &Path) {
        std::fs::write(dir.join("a.txt"), "hello").unwrap();
        Command::new("git")
            .current_dir(dir)
            .args(["add", "a.txt"])
            .status()
            .unwrap();
    }

    // GIT_EDITOR is process-global, so drive both branches in one test.
    #[test]
    fn commit_with_editor_commits_on_save_and_aborts_on_empty() {
        // Editor keeps the seeded message -> commit succeeds.
        let dir = init_repo();
        stage_a(dir.path());
        std::env::set_var("GIT_EDITOR", "true");
        let committed = commit_with_editor(dir.path(), "feat: via editor", false).unwrap();
        std::env::remove_var("GIT_EDITOR");
        assert!(committed);
        let log = Command::new("git")
            .current_dir(dir.path())
            .args(["log", "--oneline"])
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&log.stdout).contains("feat: via editor"));

        // Editor empties the message -> git aborts, we report cancellation.
        let dir2 = init_repo();
        stage_a(dir2.path());
        std::env::set_var("GIT_EDITOR", "truncate -s 0");
        let aborted = commit_with_editor(dir2.path(), "feat: discard me", false).unwrap();
        std::env::remove_var("GIT_EDITOR");
        assert!(!aborted);
        let log2 = Command::new("git")
            .current_dir(dir2.path())
            .args(["log", "--oneline"])
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&log2.stdout).trim().is_empty());
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
    fn default_branch_falls_back_to_local_main() {
        let dir = init_repo();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "a.txt"])
            .status()
            .unwrap();
        commit(dir.path(), "init", false).unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["branch", "-M", "main"])
            .status()
            .unwrap();
        assert_eq!(default_branch(dir.path()).unwrap(), "main");
    }

    #[test]
    fn default_branch_falls_back_to_local_master() {
        let dir = init_repo();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "a.txt"])
            .status()
            .unwrap();
        commit(dir.path(), "init", false).unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["branch", "-M", "master"])
            .status()
            .unwrap();
        assert_eq!(default_branch(dir.path()).unwrap(), "master");
    }

    #[test]
    fn branch_diff_and_log_cover_commits_ahead_of_base() {
        let dir = init_repo();
        let p = dir.path();
        std::fs::write(p.join("a.txt"), "x").unwrap();
        Command::new("git")
            .current_dir(p)
            .args(["add", "a.txt"])
            .status()
            .unwrap();
        commit(p, "init", false).unwrap();
        Command::new("git")
            .current_dir(p)
            .args(["branch", "-M", "main"])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(p)
            .args(["checkout", "-q", "-b", "feature"])
            .status()
            .unwrap();
        std::fs::write(p.join("b.txt"), "new file").unwrap();
        Command::new("git")
            .current_dir(p)
            .args(["add", "b.txt"])
            .status()
            .unwrap();
        commit(p, "feat: add b", false).unwrap();

        assert_eq!(current_branch(p).unwrap(), "feature");
        let diff = branch_diff(p, "main").unwrap();
        assert!(diff.contains("b.txt"));
        assert!(diff.contains("new file"));
        let log = branch_log(p, "main").unwrap();
        assert!(log.contains("feat: add b"));
        assert!(!log.contains("init"));
    }

    #[test]
    fn branch_diff_empty_when_no_commits_ahead() {
        let dir = init_repo();
        let p = dir.path();
        std::fs::write(p.join("a.txt"), "x").unwrap();
        Command::new("git")
            .current_dir(p)
            .args(["add", "a.txt"])
            .status()
            .unwrap();
        commit(p, "init", false).unwrap();
        Command::new("git")
            .current_dir(p)
            .args(["branch", "-M", "main"])
            .status()
            .unwrap();
        assert!(branch_log(p, "main").unwrap().trim().is_empty());
        assert!(branch_diff(p, "main").unwrap().trim().is_empty());
    }

    #[test]
    fn latest_tag_none_without_tags_then_some_after_tagging() {
        let dir = init_repo();
        let p = dir.path();
        std::fs::write(p.join("a.txt"), "x").unwrap();
        Command::new("git")
            .current_dir(p)
            .args(["add", "a.txt"])
            .status()
            .unwrap();
        commit(p, "init", false).unwrap();
        assert_eq!(latest_tag(p).unwrap(), None);
        Command::new("git")
            .current_dir(p)
            .args(["tag", "v0.1.0"])
            .status()
            .unwrap();
        assert_eq!(latest_tag(p).unwrap().as_deref(), Some("v0.1.0"));
    }

    #[test]
    fn log_range_lists_subjects_between_refs() {
        let dir = init_repo();
        let p = dir.path();
        std::fs::write(p.join("a.txt"), "x").unwrap();
        Command::new("git")
            .current_dir(p)
            .args(["add", "a.txt"])
            .status()
            .unwrap();
        commit(p, "init", false).unwrap();
        Command::new("git")
            .current_dir(p)
            .args(["tag", "v0.1.0"])
            .status()
            .unwrap();
        std::fs::write(p.join("b.txt"), "y").unwrap();
        Command::new("git")
            .current_dir(p)
            .args(["add", "b.txt"])
            .status()
            .unwrap();
        commit(p, "feat: add b", false).unwrap();

        let log = log_range(p, "v0.1.0", "HEAD").unwrap();
        assert!(log.contains("feat: add b"));
        assert!(!log.contains("init"));
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
