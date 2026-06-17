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

const MOCK_CHANGELOG: &str = "## Added\n\n- pr subcommand generating PR descriptions";

/// Repo with a v0.1.0 tag and one commit after it.
fn repo_with_tag_and_commit_after() -> tempfile::TempDir {
    let repo = tempdir().unwrap();
    let p = repo.path();
    git(p, &["init", "-q"]);
    git(p, &["config", "user.email", "t@e.st"]);
    git(p, &["config", "user.name", "t"]);
    git(p, &["config", "commit.gpgsign", "false"]);
    std::fs::write(p.join("a.txt"), "x").unwrap();
    git(p, &["add", "a.txt"]);
    git(p, &["commit", "-q", "-m", "init"]);
    git(p, &["tag", "v0.1.0"]);
    std::fs::write(p.join("b.txt"), "y").unwrap();
    git(p, &["add", "b.txt"]);
    git(p, &["commit", "-q", "-m", "feat: add pr subcommand"]);
    repo
}

#[test]
fn changelog_defaults_to_latest_tag_to_head() {
    let repo = repo_with_tag_and_commit_after();
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    Command::cargo_bin("aish")
        .unwrap()
        .current_dir(repo.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", MOCK_CHANGELOG)
        .env("HOME", cfg.path())
        .args(["changelog"])
        .assert()
        .success()
        .stdout(predicates::str::contains("## Added"))
        .stdout(predicates::str::contains("pr subcommand"));
}

#[test]
fn changelog_json_reports_range_and_text() {
    let repo = repo_with_tag_and_commit_after();
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    let out = Command::cargo_bin("aish")
        .unwrap()
        .current_dir(repo.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", MOCK_CHANGELOG)
        .env("HOME", cfg.path())
        .args(["changelog", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).expect("stdout is valid JSON");
    assert!(v["changelog"].as_str().unwrap().contains("## Added"));
    assert_eq!(v["from"], "v0.1.0");
    assert_eq!(v["to"], "HEAD");
    assert_eq!(v["provider"], "openai");
}

#[test]
fn changelog_from_flag_overrides_latest_tag() {
    let repo = repo_with_tag_and_commit_after();
    let p = repo.path();
    git(p, &["tag", "v0.2.0"]); // latest tag now at HEAD: default range would be empty
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    Command::cargo_bin("aish")
        .unwrap()
        .current_dir(p)
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", MOCK_CHANGELOG)
        .env("HOME", cfg.path())
        .args(["changelog", "--from", "v0.1.0"])
        .assert()
        .success()
        .stdout(predicates::str::contains("## Added"));
}

#[test]
fn changelog_fails_when_range_has_no_commits() {
    let repo = repo_with_tag_and_commit_after();
    let p = repo.path();
    git(p, &["tag", "v0.2.0"]); // latest tag at HEAD
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    Command::cargo_bin("aish")
        .unwrap()
        .current_dir(p)
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", MOCK_CHANGELOG)
        .env("HOME", cfg.path())
        .args(["changelog"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("no commits"));
}

#[test]
fn changelog_fails_without_tags_and_without_from() {
    let repo = tempdir().unwrap();
    let p = repo.path();
    git(p, &["init", "-q"]);
    git(p, &["config", "user.email", "t@e.st"]);
    git(p, &["config", "user.name", "t"]);
    git(p, &["config", "commit.gpgsign", "false"]);
    std::fs::write(p.join("a.txt"), "x").unwrap();
    git(p, &["add", "a.txt"]);
    git(p, &["commit", "-q", "-m", "init"]);

    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    Command::cargo_bin("aish")
        .unwrap()
        .current_dir(p)
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", MOCK_CHANGELOG)
        .env("HOME", cfg.path())
        .args(["changelog"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--from"));
}
