// SPDX-License-Identifier: MIT
use assert_cmd::Command;
use tempfile::tempdir;

fn run_repair(cfg_path: &std::path::Path) -> assert_cmd::assert::Assert {
    Command::cargo_bin("aish")
        .unwrap()
        .env("AISH_CONFIG", cfg_path)
        .args(["setup", "--repair"])
        .assert()
}

#[test]
fn repair_writes_template_when_absent() {
    let dir = tempdir().unwrap();
    let cfg = dir.path().join("config.yaml");

    run_repair(&cfg).success();

    let written = std::fs::read_to_string(&cfg).unwrap();
    assert!(written.contains("providers:"));
    assert!(written.contains("anthropic"));
    // The corrected kilo endpoint ships in the template.
    assert!(written.contains("https://api.kilo.ai/api/gateway"));
    // No backup is made when there was no prior file.
    assert!(!cfg.with_extension("yaml.bak").exists());
}

#[test]
fn repair_backs_up_existing_then_restores_template() {
    let dir = tempdir().unwrap();
    let cfg = dir.path().join("config.yaml");
    std::fs::write(&cfg, "garbage: not valid aish config").unwrap();

    run_repair(&cfg).success();

    // Original content is preserved alongside the restored template.
    let mut bak = cfg.clone().into_os_string();
    bak.push(".bak");
    let bak = std::path::PathBuf::from(bak);
    assert_eq!(
        std::fs::read_to_string(&bak).unwrap(),
        "garbage: not valid aish config"
    );
    let restored = std::fs::read_to_string(&cfg).unwrap();
    assert!(restored.contains("models:"));
}

#[test]
fn wizard_without_tty_errors() {
    let dir = tempdir().unwrap();
    let cfg = dir.path().join("config.yaml");
    // No --repair, stdin is not a terminal under the test harness.
    Command::cargo_bin("aish")
        .unwrap()
        .env("AISH_CONFIG", &cfg)
        .arg("setup")
        .assert()
        .failure()
        .stderr(predicates::str::contains("interactive terminal"));
}
