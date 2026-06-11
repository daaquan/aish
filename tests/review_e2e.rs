// SPDX-License-Identifier: MIT
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

const CONFIG: &str = r#"
providers:
  openai: { api_key: sk-x }
models:
  default: { provider: openai, model: gpt-5-mini }
commit: { style: conventional, language: en, model: default }
"#;

const MOCK_REVIEW: &str = "## HIGH\n\n- src/a.txt: greeting is hardcoded; load it from config";

fn repo_with_staged_change() -> tempfile::TempDir {
    let repo = tempdir().unwrap();
    let p = repo.path();
    git(p, &["init", "-q"]);
    git(p, &["config", "user.email", "t@e.st"]);
    git(p, &["config", "user.name", "t"]);
    git(p, &["config", "commit.gpgsign", "false"]);
    std::fs::write(p.join("a.txt"), "hello").unwrap();
    git(p, &["add", "a.txt"]);
    repo
}

#[test]
fn review_prints_model_review_of_staged_diff() {
    let repo = repo_with_staged_change();
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    Command::cargo_bin("aish")
        .unwrap()
        .current_dir(repo.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", MOCK_REVIEW)
        .env("HOME", cfg.path())
        .args(["review"])
        .assert()
        .success()
        .stdout(predicates::str::contains("## HIGH"))
        .stdout(predicates::str::contains("greeting is hardcoded"));
}

#[test]
fn review_branch_uses_branch_diff_when_nothing_staged() {
    let repo = tempdir().unwrap();
    let p = repo.path();
    git(p, &["init", "-q"]);
    git(p, &["config", "user.email", "t@e.st"]);
    git(p, &["config", "user.name", "t"]);
    git(p, &["config", "commit.gpgsign", "false"]);
    std::fs::write(p.join("a.txt"), "hello").unwrap();
    git(p, &["add", "a.txt"]);
    git(p, &["commit", "-q", "-m", "init"]);
    git(p, &["branch", "-M", "main"]);
    git(p, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(p.join("b.txt"), "branch change").unwrap();
    git(p, &["add", "b.txt"]);
    git(p, &["commit", "-q", "-m", "feat: add b"]);

    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    // Nothing staged: plain `review` must fail, `review --branch` must work.
    Command::cargo_bin("aish")
        .unwrap()
        .current_dir(p)
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", MOCK_REVIEW)
        .env("HOME", cfg.path())
        .args(["review"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("nothing staged"));

    Command::cargo_bin("aish")
        .unwrap()
        .current_dir(p)
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", MOCK_REVIEW)
        .env("HOME", cfg.path())
        .args(["review", "--branch"])
        .assert()
        .success()
        .stdout(predicates::str::contains("## HIGH"));
}

#[test]
fn review_json_emits_machine_readable_envelope() {
    let repo = repo_with_staged_change();
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    let out = Command::cargo_bin("aish")
        .unwrap()
        .current_dir(repo.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", MOCK_REVIEW)
        .env("HOME", cfg.path())
        .args(["review", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).expect("stdout is valid JSON");
    assert!(v["review"].as_str().unwrap().contains("## HIGH"));
    assert_eq!(v["provider"], "openai");
    assert_eq!(v["model"], "gpt-5-mini");
    assert_eq!(v["cached"], false);
}

#[test]
fn review_branch_fails_with_no_commits_ahead() {
    let repo = tempdir().unwrap();
    let p = repo.path();
    git(p, &["init", "-q"]);
    git(p, &["config", "user.email", "t@e.st"]);
    git(p, &["config", "user.name", "t"]);
    git(p, &["config", "commit.gpgsign", "false"]);
    std::fs::write(p.join("a.txt"), "hello").unwrap();
    git(p, &["add", "a.txt"]);
    git(p, &["commit", "-q", "-m", "init"]);
    git(p, &["branch", "-M", "main"]);

    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    Command::cargo_bin("aish")
        .unwrap()
        .current_dir(p)
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", MOCK_REVIEW)
        .env("HOME", cfg.path())
        .args(["review", "--branch"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("empty"));
}
