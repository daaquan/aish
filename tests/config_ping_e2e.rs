// SPDX-License-Identifier: MIT
use assert_cmd::Command;
use tempfile::tempdir;

const CONFIG: &str = r#"
providers:
  openai: { api_key: sk-x }
  anthropic: { api_key: sk-y }
models:
  default: { provider: openai, model: gpt-5-mini }
  claude: { provider: anthropic, model: claude-sonnet-4-6 }
commit: { style: conventional, language: en, model: default }
"#;

#[test]
fn ping_reports_ok_per_provider_and_exits_zero() {
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    let out = Command::cargo_bin("aish")
        .unwrap()
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("HOME", cfg.path())
        .args(["config", "check", "--ping", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).expect("stdout is valid JSON");
    assert_eq!(v["ok"], true);
    let ping = v["ping"].as_array().unwrap();
    assert_eq!(ping.len(), 2);
    assert!(ping.iter().all(|p| p["status"] == "ok"));
    assert!(ping
        .iter()
        .any(|p| p["provider"] == "openai" && p["model"] == "gpt-5-mini"));
}

#[test]
fn ping_reports_failure_with_reason_and_exits_nonzero() {
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    let out = Command::cargo_bin("aish")
        .unwrap()
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_FAIL", "anthropic")
        .env("HOME", cfg.path())
        .args(["config", "check", "--ping", "--json"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).expect("stdout is valid JSON");
    assert_eq!(v["ok"], false);
    let ping = v["ping"].as_array().unwrap();
    let anthropic = ping.iter().find(|p| p["provider"] == "anthropic").unwrap();
    assert_eq!(anthropic["status"], "fail");
    assert!(anthropic["error"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("auth"));
    let openai = ping.iter().find(|p| p["provider"] == "openai").unwrap();
    assert_eq!(openai["status"], "ok");
}

#[test]
fn ping_human_output_lists_provider_status() {
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    Command::cargo_bin("aish")
        .unwrap()
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("HOME", cfg.path())
        .args(["config", "check", "--ping"])
        .assert()
        .success()
        .stdout(predicates::str::contains("openai"))
        .stdout(predicates::str::contains("anthropic"))
        .stdout(predicates::str::contains("OK"));
}

#[test]
fn check_without_ping_stays_offline_and_unchanged() {
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    // AISH_MOCK_FAIL would fail any ping; without --ping it must not matter.
    let out = Command::cargo_bin("aish")
        .unwrap()
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_FAIL", "openai")
        .env("HOME", cfg.path())
        .args(["config", "check", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["ok"], true);
    assert!(v.get("ping").is_none() || v["ping"].is_null());
}
