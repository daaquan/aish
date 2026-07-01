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

/// Repo with an initial commit on `main` and one extra commit on `feature`.
fn repo_with_feature_branch() -> tempfile::TempDir {
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
    std::fs::write(p.join("b.txt"), "new feature file").unwrap();
    git(p, &["add", "b.txt"]);
    git(p, &["commit", "-q", "-m", "feat: add b"]);
    repo
}

const MOCK_REPLY: &str =
    "feat: add pr command\n\nAdds the pr subcommand.\n\n- generates title and body";

#[test]
fn pr_prints_generated_title_and_body() {
    let repo = repo_with_feature_branch();
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    Command::cargo_bin("aish")
        .unwrap()
        .current_dir(repo.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", MOCK_REPLY)
        .env("HOME", cfg.path())
        .args(["pr"])
        .write_stdin("") // EOF: do not create the PR
        .assert()
        .success()
        .stdout(predicates::str::contains("feat: add pr command"))
        .stdout(predicates::str::contains("Adds the pr subcommand."));
}

#[test]
fn pr_apply_invokes_gh_with_title_and_body() {
    let repo = repo_with_feature_branch();
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    // Fake `gh` on PATH records its argv so we can assert the invocation.
    let bin = tempdir().unwrap();
    let log_path = bin.path().join("gh_args.log");
    let gh = bin.path().join("gh");
    std::fs::write(
        &gh,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\n",
            log_path.display()
        ),
    )
    .unwrap();
    let mut perm = std::fs::metadata(&gh).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    perm.set_mode(0o755);
    std::fs::set_permissions(&gh, perm).unwrap();
    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap()
    );

    Command::cargo_bin("aish")
        .unwrap()
        .current_dir(repo.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", MOCK_REPLY)
        .env("HOME", cfg.path())
        .env("PATH", &path)
        .args(["pr", "--apply"])
        .assert()
        .success()
        .stdout(predicates::str::contains("PR created"));

    let logged = std::fs::read_to_string(&log_path).expect("gh must have been invoked");
    assert!(logged.contains("pr"));
    assert!(logged.contains("create"));
    assert!(logged.contains("feat: add pr command"));
    assert!(logged.contains("Adds the pr subcommand."));
}

#[test]
fn pr_draft_flag_passes_through_to_gh() {
    let repo = repo_with_feature_branch();
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    let bin = tempdir().unwrap();
    let log_path = bin.path().join("gh_args.log");
    let gh = bin.path().join("gh");
    std::fs::write(
        &gh,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\n",
            log_path.display()
        ),
    )
    .unwrap();
    let mut perm = std::fs::metadata(&gh).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    perm.set_mode(0o755);
    std::fs::set_permissions(&gh, perm).unwrap();
    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap()
    );

    Command::cargo_bin("aish")
        .unwrap()
        .current_dir(repo.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", MOCK_REPLY)
        .env("HOME", cfg.path())
        .env("PATH", &path)
        .args(["pr", "--apply", "--draft"])
        .assert()
        .success()
        .stdout(predicates::str::contains("PR created"));

    let logged = std::fs::read_to_string(&log_path).expect("gh must have been invoked");
    assert!(logged.contains("--draft"));
}

#[test]
fn pr_json_without_apply_suggests_without_creating() {
    let repo = repo_with_feature_branch();
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    let out = Command::cargo_bin("aish")
        .unwrap()
        .current_dir(repo.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", MOCK_REPLY)
        .env("HOME", cfg.path())
        .args(["pr", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).expect("stdout is valid JSON");
    assert_eq!(v["decision"], "suggested");
    assert_eq!(v["created"], false);
    assert_eq!(v["title"], "feat: add pr command");
    assert!(v["body"]
        .as_str()
        .unwrap()
        .contains("Adds the pr subcommand."));
    assert_eq!(v["provider"], "openai");
}

#[test]
fn pr_base_flag_overrides_default_branch_detection() {
    // Repo whose default branch is named `trunk`: auto-detection (origin/HEAD,
    // main, master) finds nothing and must fail; `--base trunk` must work.
    let repo = tempdir().unwrap();
    let p = repo.path();
    git(p, &["init", "-q"]);
    git(p, &["config", "user.email", "t@e.st"]);
    git(p, &["config", "user.name", "t"]);
    git(p, &["config", "commit.gpgsign", "false"]);
    std::fs::write(p.join("a.txt"), "hello").unwrap();
    git(p, &["add", "a.txt"]);
    git(p, &["commit", "-q", "-m", "init"]);
    git(p, &["branch", "-M", "trunk"]);
    git(p, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(p.join("b.txt"), "new").unwrap();
    git(p, &["add", "b.txt"]);
    git(p, &["commit", "-q", "-m", "feat: add b"]);

    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    let run = |args: &[&str]| {
        let mut full = vec!["pr"];
        full.extend_from_slice(args);
        Command::cargo_bin("aish")
            .unwrap()
            .current_dir(p)
            .env("AISH_CONFIG", &cfg_path)
            .env("AISH_PROVIDER", "mock")
            .env("AISH_MOCK_REPLY", MOCK_REPLY)
            .env("HOME", cfg.path())
            .args(full)
            .write_stdin("")
            .assert()
    };

    run(&[]).failure().stderr(predicates::str::contains(
        "cannot determine the default branch",
    ));
    run(&["--base", "trunk"])
        .success()
        .stdout(predicates::str::contains("feat: add pr command"));
}

#[test]
fn pr_fails_on_default_branch() {
    let repo = repo_with_feature_branch();
    git(repo.path(), &["checkout", "-q", "main"]);
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    Command::cargo_bin("aish")
        .unwrap()
        .current_dir(repo.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", MOCK_REPLY)
        .env("HOME", cfg.path())
        .args(["pr"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("default branch"));
}

#[test]
fn pr_fails_with_no_commits_ahead() {
    let repo = repo_with_feature_branch();
    // A branch pointing at main's tip: zero commits ahead.
    git(repo.path(), &["checkout", "-q", "main"]);
    git(repo.path(), &["checkout", "-q", "-b", "empty-branch"]);
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    Command::cargo_bin("aish")
        .unwrap()
        .current_dir(repo.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", MOCK_REPLY)
        .env("HOME", cfg.path())
        .args(["pr"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("no commits"));
}
