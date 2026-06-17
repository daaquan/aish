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

/// Populate $HOME/.aish/cache by running one mock commit suggestion.
fn populate_cache(home: &std::path::Path, cfg_path: &std::path::Path) {
    let repo = tempdir().unwrap();
    git(repo.path(), &["init", "-q"]);
    git(repo.path(), &["config", "user.email", "t@e.st"]);
    git(repo.path(), &["config", "user.name", "t"]);
    git(repo.path(), &["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.path().join("a.txt"), "hello").unwrap();
    git(repo.path(), &["add", "a.txt"]);
    Command::cargo_bin("aish")
        .unwrap()
        .current_dir(repo.path())
        .env("AISH_CONFIG", cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", "feat: cached entry")
        .env("HOME", home)
        .args(["commit"])
        .write_stdin("")
        .assert()
        .success();
}

#[test]
fn cache_stats_reports_entries_then_clear_empties() {
    let home = tempdir().unwrap();
    let cfg_path = home.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();
    populate_cache(home.path(), &cfg_path);

    let stats = |args: &[&str]| {
        let mut full = vec!["cache"];
        full.extend_from_slice(args);
        let out = Command::cargo_bin("aish")
            .unwrap()
            .env("AISH_CONFIG", &cfg_path)
            .env("HOME", home.path())
            .args(full)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        serde_json::from_slice::<serde_json::Value>(&out).expect("stdout is valid JSON")
    };

    let v = stats(&["stats", "--json"]);
    assert_eq!(v["entries"], 1);
    assert!(v["bytes"].as_u64().unwrap() > 0);

    Command::cargo_bin("aish")
        .unwrap()
        .env("AISH_CONFIG", &cfg_path)
        .env("HOME", home.path())
        .args(["cache", "clear", "--yes"])
        .assert()
        .success()
        .stdout(predicates::str::contains("1"));

    let v = stats(&["stats", "--json"]);
    assert_eq!(v["entries"], 0);
    assert_eq!(v["bytes"], 0);
}

#[test]
fn cache_clear_without_yes_aborts_on_eof() {
    let home = tempdir().unwrap();
    let cfg_path = home.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();
    populate_cache(home.path(), &cfg_path);

    Command::cargo_bin("aish")
        .unwrap()
        .env("AISH_CONFIG", &cfg_path)
        .env("HOME", home.path())
        .args(["cache", "clear"])
        .write_stdin("") // EOF: must not delete
        .assert()
        .success()
        .stdout(predicates::str::contains("Aborted"));

    let out = Command::cargo_bin("aish")
        .unwrap()
        .env("AISH_CONFIG", &cfg_path)
        .env("HOME", home.path())
        .args(["cache", "stats", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["entries"], 1, "entry must survive an aborted clear");
}

#[test]
fn cache_stats_on_fresh_home_is_empty() {
    let home = tempdir().unwrap();
    let cfg_path = home.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    Command::cargo_bin("aish")
        .unwrap()
        .env("AISH_CONFIG", &cfg_path)
        .env("HOME", home.path())
        .args(["cache", "stats"])
        .assert()
        .success()
        .stdout(predicates::str::contains("0"));
}
