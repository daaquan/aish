// SPDX-License-Identifier: MIT
use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use tempfile::tempdir;

const CONFIG: &str = r#"
providers:
  openai: { api_key: sk-x }
models:
  default: { provider: openai, model: gpt-5-mini }
commit: { style: conventional, language: en, model: default }
"#;

#[test]
fn ask_prints_plain_answer() {
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    Command::cargo_bin("aish")
        .unwrap()
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", "EXDEV means cross-device link.")
        .env("HOME", cfg.path())
        .args(["ask", "what does EXDEV mean?"])
        .write_stdin("")
        .assert()
        .success()
        .stdout(predicates::str::contains("EXDEV means cross-device link."));
}

#[test]
fn ask_includes_piped_stdin_as_context() {
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    // Two runs differing only in piped stdin must produce different cache
    // keys: the second run must NOT reuse the first reply, proving stdin
    // reached the prompt.
    let run = |stdin: &str, reply: &str| {
        Command::cargo_bin("aish")
            .unwrap()
            .env("AISH_CONFIG", &cfg_path)
            .env("AISH_PROVIDER", "mock")
            .env("AISH_MOCK_REPLY", reply)
            .env("HOME", cfg.path())
            .args(["ask", "explain this error"])
            .write_stdin(stdin)
            .assert()
            .success()
    };

    run("error[E0382]: borrow of moved value", "first answer")
        .stdout(predicates::str::contains("first answer"));
    run("error[E0499]: cannot borrow twice", "second answer")
        .stdout(predicates::str::contains("second answer"))
        .stdout(predicates::str::contains("(cached").not());
}

#[test]
fn ask_caches_identical_question_and_context() {
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    let run = |reply: &str| {
        Command::cargo_bin("aish")
            .unwrap()
            .env("AISH_CONFIG", &cfg_path)
            .env("AISH_PROVIDER", "mock")
            .env("AISH_MOCK_REPLY", reply)
            .env("HOME", cfg.path())
            .args(["ask", "same question"])
            .write_stdin("same context")
            .assert()
            .success()
    };

    run("the answer").stdout(predicates::str::contains("the answer"));
    // Identical request: served from cache, fresh mock reply ignored.
    run("DIFFERENT")
        .stdout(predicates::str::contains("the answer"))
        .stdout(predicates::str::contains("(cached"))
        .stdout(predicates::str::contains("DIFFERENT").not());
}

#[test]
fn ask_json_emits_answer_envelope() {
    let cfg = tempdir().unwrap();
    let cfg_path = cfg.path().join("config.yaml");
    std::fs::write(&cfg_path, CONFIG).unwrap();

    let out = Command::cargo_bin("aish")
        .unwrap()
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", "plain answer")
        .env("HOME", cfg.path())
        .args(["ask", "--json", "question?"])
        .write_stdin("")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).expect("stdout is valid JSON");
    assert_eq!(v["answer"], "plain answer");
    assert_eq!(v["provider"], "openai");
    assert_eq!(v["cached"], false);
}
