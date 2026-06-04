// SPDX-License-Identifier: AGPL-3.0-only
use assert_cmd::Command;
use std::process::Command as Std;
use tempfile::tempdir;

fn git(dir: &std::path::Path, args: &[&str]) {
    Std::new("git")
        .current_dir(dir)
        .args(args)
        .status()
        .unwrap();
}

#[test]
fn commit_apply_creates_commit_via_mock_provider() {
    let repo = tempdir().unwrap();
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(
        &cfg_path,
        r#"
providers:
  openai: { api_key: sk-x }
models:
  default: { provider: openai, model: gpt-5-mini }
commit: { style: conventional, language: en, model: default }
"#,
    )
    .unwrap();

    git(repo.path(), &["init", "-q"]);
    git(repo.path(), &["config", "user.email", "t@e.st"]);
    git(repo.path(), &["config", "user.name", "t"]);
    git(repo.path(), &["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.path().join("a.txt"), "hello").unwrap();
    git(repo.path(), &["add", "a.txt"]);

    Command::cargo_bin("aish")
        .unwrap()
        .current_dir(repo.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", "feat: add greeting file")
        .env("HOME", cfg.path()) // keep audit log inside temp
        .args(["commit", "--apply"])
        .assert()
        .success()
        .stdout(predicates::str::contains("feat: add greeting file"));

    let log = Std::new("git")
        .current_dir(repo.path())
        .args(["log", "--oneline"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&log.stdout).contains("feat: add greeting file"));
}

#[test]
fn commit_reports_nothing_staged() {
    let repo = tempdir().unwrap();
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(
        &cfg_path,
        r#"
providers:
  openai: { api_key: sk-x }
models:
  default: { provider: openai, model: gpt-5-mini }
commit: { style: conventional, language: en, model: default }
"#,
    )
    .unwrap();
    git(repo.path(), &["init", "-q"]);

    Command::cargo_bin("aish")
        .unwrap()
        .current_dir(repo.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .args(["commit"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Nothing staged"));
}
