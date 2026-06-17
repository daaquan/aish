// SPDX-License-Identifier: MIT
use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use tempfile::tempdir;

/// Write a minimal valid config and return its path plus the temp dir guard.
fn config() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.yaml");
    std::fs::write(
        &path,
        r#"
providers:
  openai: { api_key: sk-x }
models:
  default: { provider: openai, model: gpt-5-mini }
commit: { style: conventional, language: en, model: default }
"#,
    )
    .unwrap();
    (dir, path)
}

fn aish(cfg_dir: &std::path::Path, cfg_path: &std::path::Path, reply: &str) -> Command {
    let mut cmd = Command::cargo_bin("aish").unwrap();
    cmd.env("AISH_CONFIG", cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", reply)
        .env("HOME", cfg_dir); // keep audit log inside the temp dir
    cmd
}

#[test]
fn print_emits_command_without_running() {
    let (dir, cfg) = config();
    aish(dir.path(), &cfg, "echo hi")
        .args(["run", "--print", "say hi"])
        .assert()
        .success()
        .stdout(predicates::str::contains("echo hi"));
}

#[test]
fn aborts_on_no_and_runs_nothing() {
    let (dir, cfg) = config();
    // A command that would create a marker file; aborting must not run it.
    let marker = dir.path().join("ran.txt");
    let reply = format!("touch {}", marker.display());
    aish(dir.path(), &cfg, &reply)
        .args(["run", "make a marker"])
        .write_stdin("n\n")
        .assert()
        .success();
    assert!(!marker.exists(), "command must not run when aborted");
}

#[test]
fn yes_runs_and_propagates_exit_code() {
    let (dir, cfg) = config();
    aish(dir.path(), &cfg, "exit 42")
        .args(["run", "--yes", "fail with 42"])
        .assert()
        .code(42);
}

#[test]
fn yes_runs_the_command() {
    let (dir, cfg) = config();
    aish(dir.path(), &cfg, "echo ranagain")
        .args(["run", "--yes", "print something"])
        .assert()
        .success()
        .stdout(predicates::str::contains("ranagain"));
}

#[test]
fn empty_reply_fails_and_runs_nothing() {
    let (dir, cfg) = config();
    aish(dir.path(), &cfg, "   ")
        .args(["run", "--yes", "do nothing useful"])
        .assert()
        .failure()
        .stdout(predicates::str::contains("ranagain").not());
}

#[test]
fn json_mode_emits_command_envelope_and_runs() {
    let (dir, cfg) = config();
    let out = aish(dir.path(), &cfg, "exit 7")
        .args(["--json", "run", "fail with 7"])
        .assert()
        .code(7)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).expect("stdout is valid JSON");
    assert_eq!(v["command"], "exit 7");
    assert_eq!(v["ran"], true);
    assert_eq!(v["provider"], "openai");
}

#[test]
fn json_print_does_not_run() {
    let (dir, cfg) = config();
    let out = aish(dir.path(), &cfg, "exit 9")
        .args(["--json", "run", "--print", "fail with 9"])
        .assert()
        .success() // --print never executes, so exit 0 regardless of the command
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).expect("stdout is valid JSON");
    assert_eq!(v["decision"], "printed");
    assert_eq!(v["ran"], false);
}
