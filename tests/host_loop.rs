// SPDX-License-Identifier: MIT
use aish::config::Config;
use aish::plugin::host::{run_plugin, run_plugin_with, Timeouts};
use aish::plugin::manifest::{Manifest, Permissions, PluginEntry};
use serial_test::serial;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

fn build_fake() -> PathBuf {
    let status = Command::new(env!("CARGO"))
        .args([
            "build",
            "--manifest-path",
            "tests/fixtures/fake-plugin/Cargo.toml",
        ])
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
        permissions: Permissions {
            model: true,
            audit: true,
        },
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
#[serial]
async fn plugin_ok_returns_exit_zero() {
    let bin = build_fake();
    std::env::set_var("FAKE_MODE", "ok");
    let code = run_plugin(
        &entry(bin),
        &manifest(),
        "fake",
        &[],
        &std::env::current_dir().unwrap(),
        &cfg(),
    )
    .await
    .unwrap();
    assert_eq!(code, 0);
}

#[tokio::test]
#[serial]
async fn plugin_model_chat_roundtrips_through_host() {
    let bin = build_fake();
    std::env::set_var("FAKE_MODE", "echo_model");
    std::env::set_var("AISH_PROVIDER", "mock");
    std::env::set_var("AISH_MOCK_REPLY", "feat: from host");
    let code = run_plugin(
        &entry(bin),
        &manifest(),
        "fake",
        &[],
        &std::env::current_dir().unwrap(),
        &cfg(),
    )
    .await
    .unwrap();
    assert_eq!(code, 0);
    std::env::remove_var("AISH_PROVIDER");
}

#[tokio::test]
#[serial]
async fn plugin_crash_before_result_is_an_error() {
    let bin = build_fake();
    std::env::set_var("FAKE_MODE", "crash");
    let err = run_plugin(
        &entry(bin),
        &manifest(),
        "fake",
        &[],
        &std::env::current_dir().unwrap(),
        &cfg(),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("before sending a result"));
}

/// A plugin that sends its result then never exits must not hang the host:
/// the bounded `child.wait()` kills it and we still return the reported code.
#[tokio::test]
#[serial]
async fn hang_after_result_is_reaped() {
    let bin = build_fake();
    std::env::set_var("FAKE_MODE", "hang_after_result");
    let (e, m, c, cwd) = (
        entry(bin),
        manifest(),
        cfg(),
        std::env::current_dir().unwrap(),
    );
    let timeouts = Timeouts {
        wait: Duration::from_millis(200),
        ..Timeouts::default()
    };
    let fut = run_plugin_with(&e, &m, "fake", &[], &cwd, &c, timeouts);
    // Bound the whole call so a regression hangs the test harness for ~3s, not forever.
    let code = tokio::time::timeout(Duration::from_secs(3), fut)
        .await
        .expect("run_plugin_with hung past child.wait timeout")
        .unwrap();
    assert_eq!(code, 0);
}

/// A host-side service call (provider request) that stalls must be timed out
/// instead of hanging the host indefinitely.
#[tokio::test]
#[serial]
async fn slow_service_call_times_out() {
    let bin = build_fake();
    std::env::set_var("FAKE_MODE", "slow_service");
    std::env::set_var("AISH_PROVIDER", "mock");
    std::env::set_var("AISH_MOCK_REPLY", "too slow");
    std::env::set_var("AISH_MOCK_DELAY_MS", "5000");
    let (e, m, c, cwd) = (
        entry(bin),
        manifest(),
        cfg(),
        std::env::current_dir().unwrap(),
    );
    let timeouts = Timeouts {
        service: Duration::from_millis(150),
        ..Timeouts::default()
    };
    let fut = run_plugin_with(&e, &m, "fake", &[], &cwd, &c, timeouts);
    let err = tokio::time::timeout(Duration::from_secs(3), fut)
        .await
        .expect("service call was not timed out")
        .unwrap_err();
    std::env::remove_var("AISH_PROVIDER");
    std::env::remove_var("AISH_MOCK_DELAY_MS");
    assert!(
        err.to_string().contains("timed out"),
        "unexpected error: {err}"
    );
}

/// A plugin flooding stderr then crashing must surface the crash error promptly
/// without the host buffering all of stderr.
#[tokio::test]
#[serial]
async fn stderr_flood_then_crash_is_an_error() {
    let bin = build_fake();
    std::env::set_var("FAKE_MODE", "stderr_flood");
    let (e, m, c, cwd) = (
        entry(bin),
        manifest(),
        cfg(),
        std::env::current_dir().unwrap(),
    );
    let fut = run_plugin(&e, &m, "fake", &[], &cwd, &c);
    let err = tokio::time::timeout(Duration::from_secs(5), fut)
        .await
        .expect("stderr flood deadlocked the host")
        .unwrap_err();
    assert!(
        err.to_string().contains("before sending a result"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
#[serial]
async fn abi_major_mismatch_is_rejected() {
    let bin = build_fake();
    let mut m = manifest();
    m.abi = "2".into();
    let err = run_plugin(
        &entry(bin),
        &m,
        "fake",
        &[],
        &std::env::current_dir().unwrap(),
        &cfg(),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("ABI"));
}
