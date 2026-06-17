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

fn aish(cfg_dir: &std::path::Path, cfg_path: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("aish").unwrap();
    cmd.env("AISH_CONFIG", cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", "Clone `cfg` before the move.")
        .env("HOME", cfg_dir); // keep audit log inside the temp dir
    cmd
}

#[test]
fn diagnoses_a_failing_command_and_propagates_its_exit_code() {
    let (dir, cfg) = config();
    aish(dir.path(), &cfg)
        .args(["fix", "sh", "-c", "echo boom >&2; exit 3"])
        .assert()
        .code(3)
        .stderr(predicates::str::contains("boom")) // command output passed through
        .stdout(predicates::str::contains("Clone `cfg` before the move.")); // diagnosis
}

#[test]
fn passes_through_success_without_diagnosing() {
    let (dir, cfg) = config();
    aish(dir.path(), &cfg)
        .args(["fix", "sh", "-c", "exit 0"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Clone `cfg` before the move.").not());
}

#[test]
fn always_flag_diagnoses_even_on_success() {
    let (dir, cfg) = config();
    aish(dir.path(), &cfg)
        .args(["fix", "--always", "sh", "-c", "exit 0"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Clone `cfg` before the move."));
}

#[test]
fn propagates_arbitrary_exit_code() {
    let (dir, cfg) = config();
    aish(dir.path(), &cfg)
        .args(["fix", "sh", "-c", "exit 42"])
        .assert()
        .code(42);
}

#[test]
fn json_mode_emits_diagnosis_envelope() {
    let (dir, cfg) = config();
    let out = aish(dir.path(), &cfg)
        .args(["--json", "fix", "sh", "-c", "exit 1"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).expect("stdout is valid JSON");
    assert_eq!(v["exit_code"], 1);
    assert_eq!(v["diagnosis"], "Clone `cfg` before the move.");
    assert_eq!(v["provider"], "openai");
}
