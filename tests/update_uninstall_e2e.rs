// SPDX-License-Identifier: MIT
//! E2E for `aish update` / `aish uninstall`. The real binary is copied into
//! a temp dir and the copy is executed, so `current_exe()` resolves inside
//! the sandbox and the build artifact is never touched. Releases are served
//! by wiremock via the `AISH_UPDATE_*_BASE` overrides — no network.

use aish::update::asset_name;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// Copy the built `aish` into `<dir>/<sub>/aish` and return the copy's path.
///
/// Copies via a spawned `cp` instead of `std::fs::copy`: an in-process copy
/// holds a write fd on the destination that a concurrently forked test child
/// inherits until its exec, and exec-ing the destination inside that window
/// fails with ETXTBSY (#28). With `cp` the write fd never exists in this
/// process, so it cannot leak into forks.
fn copy_bin(dir: &Path, sub: &str) -> PathBuf {
    let dest_dir = dir.join(sub);
    std::fs::create_dir_all(&dest_dir).unwrap();
    let dest = dest_dir.join("aish");
    let status = Command::new("cp")
        .arg("-p")
        .arg(assert_cmd::cargo::cargo_bin("aish"))
        .arg(&dest)
        .status()
        .unwrap();
    assert!(status.success(), "cp failed copying test binary");
    dest
}

fn run(bin: &Path, home: &Path, server: Option<&str>, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(bin);
    cmd.env("HOME", home).args(args).stdin(Stdio::null());
    cmd.env_remove("AISH_HOME");
    if let Some(uri) = server {
        cmd.env("AISH_UPDATE_API_BASE", uri)
            .env("AISH_UPDATE_DOWNLOAD_BASE", uri);
    }
    cmd.output().unwrap()
}

async fn mock_release(server: &MockServer, tag: &str, body: &[u8]) {
    Mock::given(method("GET"))
        .and(path("/repos/daaquan/aish/releases/latest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "tag_name": tag })))
        .mount(server)
        .await;
    let asset = asset_name(std::env::consts::OS, std::env::consts::ARCH).unwrap();
    Mock::given(method("GET"))
        .and(path(format!(
            "/daaquan/aish/releases/download/{tag}/{asset}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
        .mount(server)
        .await;
}

// ---------------------------------------------------------------- update --

#[tokio::test(flavor = "multi_thread")]
async fn update_replaces_binary_when_newer_release_exists() {
    let home = tempdir().unwrap();
    let bin = copy_bin(home.path(), "bin");
    let fake = b"\x7fELF fake-new-release".to_vec();
    let server = MockServer::start().await;
    mock_release(&server, "v99.0.0", &fake).await;

    let out = run(&bin, home.path(), Some(&server.uri()), &["update"]);

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::read(&bin).unwrap(), fake, "binary not replaced");
}

#[tokio::test(flavor = "multi_thread")]
async fn update_is_noop_when_already_latest() {
    let home = tempdir().unwrap();
    let bin = copy_bin(home.path(), "bin");
    let before = std::fs::read(&bin).unwrap();
    let server = MockServer::start().await;
    mock_release(&server, &format!("v{CURRENT}"), b"unused").await;

    let out = run(
        &bin,
        home.path(),
        Some(&server.uri()),
        &["update", "--json"],
    );

    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["updated"], json!(false));
    assert_eq!(v["current"], json!(CURRENT));
    assert_eq!(
        std::fs::read(&bin).unwrap(),
        before,
        "binary must be untouched"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn update_check_reports_without_downloading() {
    let home = tempdir().unwrap();
    let bin = copy_bin(home.path(), "bin");
    let before = std::fs::read(&bin).unwrap();
    let server = MockServer::start().await;
    mock_release(&server, "v99.0.0", b"unused").await;

    let out = run(
        &bin,
        home.path(),
        Some(&server.uri()),
        &["update", "--check", "--json"],
    );

    // Outdated → nonzero exit (CI gate), but the binary stays untouched.
    assert!(!out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["latest"], json!("99.0.0"));
    assert_eq!(v["updated"], json!(false));
    assert_eq!(std::fs::read(&bin).unwrap(), before);

    // Up to date → exit 0.
    let server2 = MockServer::start().await;
    mock_release(&server2, &format!("v{CURRENT}"), b"unused").await;
    let out2 = run(
        &bin,
        home.path(),
        Some(&server2.uri()),
        &["update", "--check"],
    );
    assert!(out2.status.success());
}

#[tokio::test(flavor = "multi_thread")]
async fn update_rejects_non_binary_payload_and_keeps_old_binary() {
    let home = tempdir().unwrap();
    let bin = copy_bin(home.path(), "bin");
    let before = std::fs::read(&bin).unwrap();
    let server = MockServer::start().await;
    mock_release(&server, "v99.0.0", b"<html><body>404</body></html>").await;

    let out = run(&bin, home.path(), Some(&server.uri()), &["update"]);

    assert!(!out.status.success());
    assert_eq!(
        std::fs::read(&bin).unwrap(),
        before,
        "binary must survive bad payload"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn update_refuses_cargo_installed_binary() {
    let home = tempdir().unwrap();
    let bin = copy_bin(home.path(), ".cargo/bin");
    let server = MockServer::start().await;
    mock_release(&server, "v99.0.0", b"\x7fELF x").await;

    let out = run(&bin, home.path(), Some(&server.uri()), &["update"]);

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("cargo"),
        "stderr should hint at cargo: {stderr}"
    );
    assert!(bin.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn update_version_flag_pins_a_specific_tag() {
    let home = tempdir().unwrap();
    let bin = copy_bin(home.path(), "bin");
    let fake = b"\x7fELF pinned".to_vec();
    let server = MockServer::start().await;
    // Only the pinned download endpoint exists — no /releases/latest call.
    let asset = asset_name(std::env::consts::OS, std::env::consts::ARCH).unwrap();
    Mock::given(method("GET"))
        .and(path(format!(
            "/daaquan/aish/releases/download/v98.0.0/{asset}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(fake.clone()))
        .mount(&server)
        .await;

    let out = run(
        &bin,
        home.path(),
        Some(&server.uri()),
        &["update", "--version", "98.0.0"],
    );

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::read(&bin).unwrap(), fake);
}

// ------------------------------------------------------------- uninstall --

#[test]
fn uninstall_yes_removes_binary_but_keeps_data() {
    let home = tempdir().unwrap();
    let bin = copy_bin(home.path(), "bin");
    let data = home.path().join(".aish");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("config.yaml"), "x: 1").unwrap();

    let out = run(&bin, home.path(), None, &["uninstall", "--yes"]);

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!bin.exists(), "binary should be removed");
    assert!(
        data.join("config.yaml").exists(),
        "data must be kept without --purge"
    );
}

#[test]
fn uninstall_purge_removes_data_dir_too() {
    let home = tempdir().unwrap();
    let bin = copy_bin(home.path(), "bin");
    let data = home.path().join(".aish");
    std::fs::create_dir_all(data.join("cache")).unwrap();
    std::fs::write(data.join("audit.log"), "{}").unwrap();

    let out = run(
        &bin,
        home.path(),
        None,
        &["uninstall", "--yes", "--purge", "--json"],
    );

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["removed_data"], json!(true));
    assert!(!bin.exists());
    assert!(!data.exists(), "data dir should be purged");
}

#[test]
fn uninstall_without_yes_aborts_on_eof() {
    let home = tempdir().unwrap();
    let bin = copy_bin(home.path(), "bin");

    // stdin is null → EOF → default-no prompt aborts.
    let out = run(&bin, home.path(), None, &["uninstall"]);

    assert!(out.status.success(), "abort is not an error");
    assert!(bin.exists(), "binary must survive an aborted uninstall");
}

#[test]
fn uninstall_purge_refuses_aish_home_outside_home() {
    let home = tempdir().unwrap();
    let outside = tempdir().unwrap();
    std::fs::write(outside.path().join("precious"), "data").unwrap();
    let bin = copy_bin(home.path(), "bin");

    let mut cmd = Command::new(&bin);
    let out = cmd
        .env("HOME", home.path())
        .env("AISH_HOME", outside.path())
        .args(["uninstall", "--yes", "--purge"])
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert!(!out.status.success());
    assert!(bin.exists(), "nothing may be deleted when the guard fires");
    assert!(outside.path().join("precious").exists());
}

#[test]
fn uninstall_refuses_cargo_installed_binary() {
    let home = tempdir().unwrap();
    let bin = copy_bin(home.path(), ".cargo/bin");

    let out = run(&bin, home.path(), None, &["uninstall", "--yes"]);

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("cargo"),
        "stderr should hint at cargo: {stderr}"
    );
    assert!(bin.exists());
}
