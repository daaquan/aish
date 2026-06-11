// SPDX-License-Identifier: MIT
use assert_cmd::Command;

#[test]
fn completions_emit_scripts_for_supported_shells() {
    for shell in ["bash", "zsh", "fish"] {
        let out = Command::cargo_bin("aish")
            .unwrap()
            .args(["completions", shell])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let script = String::from_utf8(out).unwrap();
        assert!(
            script.contains("aish") && script.len() > 100,
            "{shell} script should be a real completion script, got {} bytes",
            script.len()
        );
    }
}

#[test]
fn completions_reject_unknown_shell_listing_supported_ones() {
    Command::cargo_bin("aish")
        .unwrap()
        .args(["completions", "tcsh"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("bash"))
        .stderr(predicates::str::contains("zsh"))
        .stderr(predicates::str::contains("fish"));
}
