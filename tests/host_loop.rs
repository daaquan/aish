// SPDX-License-Identifier: MIT
use aish::config::Config;
use aish::plugin::host::run_plugin;
use aish::plugin::manifest::{Manifest, Permissions, PluginEntry};
use std::path::PathBuf;
use std::process::Command;

fn build_fake() -> PathBuf {
    let status = Command::new(env!("CARGO"))
        .args(["build", "--manifest-path", "tests/fixtures/fake-plugin/Cargo.toml"])
        .status()
        .unwrap();
    assert!(status.success(), "fake-plugin build failed");
    PathBuf::from("tests/fixtures/fake-plugin/target/debug/fake-plugin")
}

fn manifest() -> Manifest {
    Manifest {
        name: "fake".into(),
        version: "0.0.0".into(),
        abi: "1".into(),
        description: None,
        subcommands: vec!["fake".into()],
        permissions: Permissions { model: true, audit: true },
    }
}

fn entry(bin: PathBuf) -> PluginEntry {
    PluginEntry {
        version: "0.0.0".into(),
        abi: "1".into(),
        enabled: true,
        path: bin,
        subcommands: vec!["fake".into()],
        source: "local".into(),
        revision: "0".into(),
        binary_sha256: String::new(),
    }
}

fn cfg() -> Config {
    Config::from_yaml("providers:\n  openai: { api_key: sk-x }\nmodels:\n  default: { provider: openai, model: gpt-5-mini }\ncommit: { style: conventional, language: en, model: default }\n").unwrap()
}

#[tokio::test]
async fn plugin_ok_returns_exit_zero() {
    let bin = build_fake();
    std::env::set_var("FAKE_MODE", "ok");
    let code = run_plugin(&entry(bin), &manifest(), "fake", &[], &std::env::current_dir().unwrap(), &cfg()).await.unwrap();
    assert_eq!(code, 0);
}

#[tokio::test]
async fn plugin_model_chat_roundtrips_through_host() {
    let bin = build_fake();
    std::env::set_var("FAKE_MODE", "echo_model");
    std::env::set_var("AISH_PROVIDER", "mock");
    std::env::set_var("AISH_MOCK_REPLY", "feat: from host");
    let code = run_plugin(&entry(bin), &manifest(), "fake", &[], &std::env::current_dir().unwrap(), &cfg()).await.unwrap();
    assert_eq!(code, 0);
    std::env::remove_var("AISH_PROVIDER");
}

#[tokio::test]
async fn plugin_crash_before_result_is_an_error() {
    let bin = build_fake();
    std::env::set_var("FAKE_MODE", "crash");
    let err = run_plugin(&entry(bin), &manifest(), "fake", &[], &std::env::current_dir().unwrap(), &cfg()).await.unwrap_err();
    assert!(err.to_string().contains("before sending a result"));
}

#[tokio::test]
async fn abi_major_mismatch_is_rejected() {
    let bin = build_fake();
    let mut m = manifest();
    m.abi = "2".into();
    let err = run_plugin(&entry(bin), &m, "fake", &[], &std::env::current_dir().unwrap(), &cfg()).await.unwrap_err();
    assert!(err.to_string().contains("ABI"));
}
