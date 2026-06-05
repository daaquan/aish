// SPDX-License-Identifier: MIT
use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
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
fn commit_edit_opens_editor_and_commits_edited_message() {
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

    // Non-interactive "editor": overwrite the message file with edited text.
    Command::cargo_bin("aish")
        .unwrap()
        .current_dir(repo.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", "feat: ai suggestion")
        .env("HOME", cfg.path())
        .env("EDITOR", "printf 'fix: hand-edited subject' >")
        .args(["commit"])
        .write_stdin("e\n")
        .assert()
        .success()
        .stdout(predicates::str::contains("Committed"));

    let log = Std::new("git")
        .current_dir(repo.path())
        .args(["log", "-1", "--format=%B"])
        .output()
        .unwrap();
    let body = String::from_utf8_lossy(&log.stdout);
    assert!(body.contains("fix: hand-edited subject"), "got: {body:?}");
    assert!(
        !body.contains("ai suggestion"),
        "must commit edited text, not the suggestion"
    );
}

#[test]
fn commit_edit_aborts_when_message_emptied() {
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

    // Editor blanks the file -> empty message -> must not commit.
    Command::cargo_bin("aish")
        .unwrap()
        .current_dir(repo.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", "feat: ai suggestion")
        .env("HOME", cfg.path())
        .env("EDITOR", "printf '' >")
        .args(["commit"])
        .write_stdin("e\n")
        .assert()
        .success()
        .stdout(predicates::str::contains("Aborted (empty message)"));

    let log = Std::new("git")
        .current_dir(repo.path())
        .args(["log", "--oneline"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&log.stdout).trim().is_empty(),
        "must not have committed on empty edited message"
    );
}

#[test]
fn commit_aborts_on_eof_without_apply() {
    let repo = tempdir().unwrap();
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, "providers:\n  openai: { api_key: sk-x }\nmodels:\n  default: { provider: openai, model: gpt-5-mini }\ncommit: { style: conventional, language: en, model: default }\n").unwrap();
    git(repo.path(), &["init", "-q"]);
    git(repo.path(), &["config", "user.email", "t@e.st"]);
    git(repo.path(), &["config", "user.name", "t"]);
    git(repo.path(), &["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.path().join("a.txt"), "hi").unwrap();
    git(repo.path(), &["add", "a.txt"]);

    Command::cargo_bin("aish")
        .unwrap()
        .current_dir(repo.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", "feat: should not commit")
        .env("HOME", cfg.path())
        .args(["commit"])
        .write_stdin("") // EOF immediately
        .assert()
        .success()
        .stdout(predicates::str::contains("Aborted"));

    let log = Std::new("git")
        .current_dir(repo.path())
        .args(["log", "--oneline"])
        .output()
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&log.stdout).contains("should not commit"),
        "must not have committed on EOF"
    );
}

#[test]
fn second_request_for_same_diff_hits_cache_and_skips_provider() {
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

    let run = |reply: &str| {
        Command::cargo_bin("aish")
            .unwrap()
            .current_dir(repo.path())
            .env("AISH_CONFIG", &cfg_path)
            .env("AISH_PROVIDER", "mock")
            .env("AISH_MOCK_REPLY", reply)
            .env("HOME", cfg.path()) // cache + audit log live inside temp HOME
            .args(["commit"])
            .write_stdin("") // EOF: reject so the staged diff persists for run 2
            .assert()
            .success()
    };

    // First run: cache miss, stores the model's reply.
    run("feat: first reply").stdout(predicates::str::contains("feat: first reply"));

    // Second run: same staged diff -> cache hit. The different mock reply must
    // be ignored, proving no fresh model request was made.
    run("feat: DIFFERENT reply")
        .stdout(predicates::str::contains("feat: first reply"))
        .stdout(predicates::str::contains("(cached"))
        .stdout(predicates::str::contains("DIFFERENT").not());
}

#[test]
fn no_cache_flag_forces_fresh_request() {
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

    let run = |reply: &str, extra: &[&str]| {
        let mut args = vec!["commit"];
        args.extend_from_slice(extra);
        Command::cargo_bin("aish")
            .unwrap()
            .current_dir(repo.path())
            .env("AISH_CONFIG", &cfg_path)
            .env("AISH_PROVIDER", "mock")
            .env("AISH_MOCK_REPLY", reply)
            .env("HOME", cfg.path())
            .args(args)
            .write_stdin("")
            .assert()
            .success()
    };

    run("feat: first reply", &[]).stdout(predicates::str::contains("feat: first reply"));

    // --no-cache bypasses the stored entry and uses the fresh reply.
    run("feat: fresh reply", &["--no-cache"])
        .stdout(predicates::str::contains("feat: fresh reply"))
        .stdout(predicates::str::contains("(cached").not());
}

#[test]
fn commit_apply_json_emits_machine_readable_result() {
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

    let out = Command::cargo_bin("aish")
        .unwrap()
        .current_dir(repo.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", "feat: add greeting file")
        .env("HOME", cfg.path())
        .args(["commit", "--apply", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    // Stdout must be pure, parseable JSON (no human prose mixed in).
    let v: serde_json::Value = serde_json::from_slice(&out).expect("stdout is valid JSON");
    assert_eq!(v["committed"], true);
    assert_eq!(v["decision"], "applied");
    assert_eq!(v["message"], "feat: add greeting file");
    assert_eq!(v["provider"], "openai");
    assert_eq!(v["model"], "gpt-5-mini");
}

#[test]
fn commit_json_without_apply_suggests_without_committing() {
    let repo = tempdir().unwrap();
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, "providers:\n  openai: { api_key: sk-x }\nmodels:\n  default: { provider: openai, model: gpt-5-mini }\ncommit: { style: conventional, language: en, model: default }\n").unwrap();
    git(repo.path(), &["init", "-q"]);
    git(repo.path(), &["config", "user.email", "t@e.st"]);
    git(repo.path(), &["config", "user.name", "t"]);
    git(repo.path(), &["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.path().join("a.txt"), "hi").unwrap();
    git(repo.path(), &["add", "a.txt"]);

    let out = Command::cargo_bin("aish")
        .unwrap()
        .current_dir(repo.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", "feat: suggested only")
        .env("HOME", cfg.path())
        .args(["commit", "--json"]) // no --apply, no stdin: must not block or commit
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["committed"], false);
    assert_eq!(v["decision"], "suggested");
    assert_eq!(v["message"], "feat: suggested only");

    let log = Std::new("git")
        .current_dir(repo.path())
        .args(["log", "--oneline"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&log.stdout).trim().is_empty());
}

#[test]
fn config_check_json_reports_errors_and_fails() {
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    // Alias points at an undeclared provider -> one error.
    std::fs::write(&cfg_path, "providers:\n  openai: { api_key: sk-x }\nmodels:\n  default: { provider: ghost, model: m }\ncommit: { style: conventional, language: en, model: default }\n").unwrap();

    let out = Command::cargo_bin("aish")
        .unwrap()
        .env("AISH_CONFIG", &cfg_path)
        .env("HOME", cfg.path())
        .args(["config", "check", "--json"])
        .assert()
        .failure() // nonzero exit so CI gates fail
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["ok"], false);
    assert!(v["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["level"] == "error" && i["message"].as_str().unwrap().contains("ghost")));
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
