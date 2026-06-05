// SPDX-License-Identifier: MIT
use assert_cmd::Command;
use std::path::Path;
use std::process::Command as Std;
use tempfile::tempdir;

fn git(dir: &Path, args: &[&str]) {
    Std::new("git")
        .current_dir(dir)
        .args(args)
        .status()
        .unwrap();
}

/// Path to a local checkout of the plugins repo used as the registry.
/// Override with AISH_PLUGINS_DIR; defaults to /opt/aish-plugins.
fn plugins_registry() -> String {
    std::env::var("AISH_PLUGINS_DIR").unwrap_or_else(|_| "/opt/aish-plugins".into())
}

#[test]
fn install_then_commit_end_to_end() {
    let registry = plugins_registry();
    if !Path::new(&registry).join("commit/Cargo.toml").exists() {
        eprintln!("skipping: plugins registry not present at {registry}");
        return;
    }

    let home = tempdir().unwrap(); // AISH_HOME
    let cfgdir = tempdir().unwrap();
    let cfg_path = cfgdir.path().join("config.yaml");
    std::fs::write(
        &cfg_path,
        "providers:\n  openai: { api_key: sk-x }\nmodels:\n  default: { provider: openai, model: gpt-5-mini }\ncommit: { style: conventional, language: en, model: default }\n",
    )
    .unwrap();

    // Install the commit plugin (build-on-install from the local registry).
    Command::cargo_bin("aish")
        .unwrap()
        .env("AISH_HOME", home.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_REGISTRY", &registry)
        .args(["plugin", "install", "commit", "--yes"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Installed `commit`"));

    // It shows up enabled.
    Command::cargo_bin("aish")
        .unwrap()
        .env("AISH_HOME", home.path())
        .env("AISH_CONFIG", &cfg_path)
        .args(["plugin", "list"])
        .assert()
        .success()
        .stdout(predicates::str::contains("commit"));

    // Updating an installed plugin rebuilds + reinstalls in place and succeeds.
    Command::cargo_bin("aish")
        .unwrap()
        .env("AISH_HOME", home.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_REGISTRY", &registry)
        .args(["plugin", "update", "commit"])
        .assert()
        .success()
        .stdout(predicates::str::contains("commit"));

    // Run `aish commit --apply` in a temp repo via the mock provider.
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
        .env("AISH_HOME", home.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", "feat: add greeting file")
        .args(["commit", "--apply"])
        .assert()
        .success();

    let log = Std::new("git")
        .current_dir(repo.path())
        .args(["log", "--oneline"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&log.stdout).contains("feat: add greeting file"));
}

#[test]
fn unknown_subcommand_without_plugin_errors() {
    let home = tempdir().unwrap();
    let cfgdir = tempdir().unwrap();
    let cfg_path = cfgdir.path().join("config.yaml");
    std::fs::write(&cfg_path, "providers: {}\nmodels: {}\ncommit: { style: conventional, language: en, model: default }\n").unwrap();

    Command::cargo_bin("aish")
        .unwrap()
        .env("AISH_HOME", home.path())
        .env("AISH_CONFIG", &cfg_path)
        .args(["commit"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "no enabled plugin provides `commit`",
        ));
}
