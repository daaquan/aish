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
    #[error("git command failed: {0}")]
    Failed(String),
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
                GitError::Failed(e.to_string())
            }
        })
}

/// Return the staged diff (`git diff --cached`). Empty string if nothing staged.
pub fn staged_diff(dir: &Path) -> Result<String, GitError> {
    let check = run(dir, &["rev-parse", "--is-inside-work-tree"])?;
    if !check.status.success() {
        return Err(GitError::NotARepo);
    }
    let out = run(dir, &["diff", "--cached"])?;
    if !out.status.success() {
        return Err(GitError::Failed(
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Create a commit with the given message.
/// Pass `signoff: true` to append a DCO `Signed-off-by` trailer (`git commit -s`).
pub fn commit(dir: &Path, message: &str, signoff: bool) -> Result<(), GitError> {
    let mut args = vec!["commit", "-m", message];
    if signoff {
        args.push("-s");
    }
    let out = run(dir, &args)?;
    if !out.status.success() {
        return Err(GitError::Failed(
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ));
    }
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
