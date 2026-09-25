// SPDX-License-Identifier: MIT
//! E2E for `aish update` / `aish uninstall`. The real binary is copied into
//! a temp dir and the copy is executed, so `current_exe()` resolves inside
//! the sandbox and the build artifact is never touched. Releases are served
//! by wiremock via the `AISH_UPDATE_*_BASE` overrides — no network. Those
//! overrides are honoured in debug builds only (see `update::endpoint_base`),
//! so the `update_*` cases below are ignored under `--release`.
//!
//! Unix only: these tests place `~/.aish` and `~/.cargo` through `$HOME`,
//! which `dirs` ignores on Windows, so there `uninstall --purge` would delete
//! the developer's real data dir.
#![cfg(unix)]

use aish::update::asset_name;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// cargo's `.crates2.json` for a `cargo install --root` of aish.
const CRATES2_AISH: &str =
    r#"{"installs":{"aish 0.9.0 (git+https://github.com/daaquan/aish#1)":{"bins":["aish"]}}}"#;

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
    // rustup sets CARGO_HOME for everything cargo runs, tests included; a
    // temp dir under it would make aish treat the copy as a cargo install.
    cmd.env_remove("AISH_HOME").env_remove("CARGO_HOME");
    if let Some(uri) = server {
        // The release server is plain http on 127.0.0.1: reach it directly,
        // whatever proxy the caller's environment names for http URLs.
        for var in ["ALL_PROXY", "all_proxy", "HTTP_PROXY", "http_proxy"] {
            cmd.env_remove(var);
        }
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

#[cfg_attr(not(debug_assertions), ignore = "AISH_UPDATE_* hooks are debug-only")]
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

#[cfg_attr(not(debug_assertions), ignore = "AISH_UPDATE_* hooks are debug-only")]
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

#[cfg_attr(not(debug_assertions), ignore = "AISH_UPDATE_* hooks are debug-only")]
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

#[cfg_attr(not(debug_assertions), ignore = "AISH_UPDATE_* hooks are debug-only")]
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

#[cfg_attr(not(debug_assertions), ignore = "AISH_UPDATE_* hooks are debug-only")]
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

#[cfg_attr(not(debug_assertions), ignore = "AISH_UPDATE_* hooks are debug-only")]
#[tokio::test(flavor = "multi_thread")]
async fn update_refuses_cargo_root_install() {
    let home = tempdir().unwrap();
    let bin = copy_bin(home.path(), "tools/bin");
    let root = home.path().join("tools");
    std::fs::write(root.join(".crates2.json"), CRATES2_AISH).unwrap();
    let before = std::fs::read(&bin).unwrap();
    let server = MockServer::start().await;
    mock_release(&server, "v99.0.0", b"\x7fELF x").await;

    let out = run(&bin, home.path(), Some(&server.uri()), &["update"]);

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(&format!("cargo install --root {}", root.display())),
        "stderr should name the install root: {stderr}"
    );
    assert_eq!(
        std::fs::read(&bin).unwrap(),
        before,
        "binary must be untouched"
    );
}

#[cfg_attr(not(debug_assertions), ignore = "AISH_UPDATE_* hooks are debug-only")]
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
fn uninstall_purge_removes_config_written_under_aish_home() {
    let home = tempdir().unwrap();
    let bin = copy_bin(home.path(), "bin");
    let data = home.path().join("custom-data");
    let aish = |args: &[&str]| {
        let out = Command::new(&bin)
            .env("HOME", home.path())
            .env("AISH_HOME", &data)
            .env_remove("AISH_CONFIG")
            .env_remove("CARGO_HOME")
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "aish {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };

    // The first run lays down the template config, where API keys go.
    aish(&["models", "list"]);
    assert!(
        data.join("config.yaml").exists(),
        "config not in $AISH_HOME"
    );

    aish(&["uninstall", "--yes", "--purge"]);

    assert!(!data.exists(), "$AISH_HOME should be purged");
    assert!(
        !home.path().join(".aish").exists(),
        "nothing may be left behind in ~/.aish"
    );
}

/// The other e2e suites all set `$AISH_HOME`, so this is where the writers'
/// default location is covered: with neither it nor `$AISH_CONFIG` set,
/// config, cache and audit log must land in the `~/.aish` that `--purge`
/// deletes.
#[test]
fn uninstall_purge_removes_files_written_to_default_data_dir() {
    let home = tempdir().unwrap();
    let bin = copy_bin(home.path(), "bin");
    let data = home.path().join(".aish");

    // One mock `run --print` lays down the template config, caches the reply
    // and audits the decision, without running anything.
    let out = Command::new(&bin)
        .env("HOME", home.path())
        .env_remove("AISH_HOME")
        .env_remove("AISH_CONFIG")
        .env_remove("CARGO_HOME")
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", "echo hi")
        .args(["--json", "run", "--print", "say hi"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    for name in ["config.yaml", "cache", "audit.log"] {
        assert!(data.join(name).exists(), "{name} not in ~/.aish");
    }

    let out = run(&bin, home.path(), None, &["uninstall", "--yes", "--purge"]);

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!data.exists(), "~/.aish should be purged");
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
        .env_remove("CARGO_HOME")
        .args(["uninstall", "--yes", "--purge"])
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert!(!out.status.success());
    assert!(bin.exists(), "nothing may be deleted when the guard fires");
    assert!(outside.path().join("precious").exists());
}

/// `<home>/link/data` is inside home as written, but `link` points out of it.
#[test]
fn uninstall_purge_refuses_aish_home_behind_a_symlink_out_of_home() {
    let home = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let target = outside.path().join("data");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("precious"), "data").unwrap();
    std::os::unix::fs::symlink(outside.path(), home.path().join("link")).unwrap();
    let bin = copy_bin(home.path(), "bin");

    let out = Command::new(&bin)
        .env("HOME", home.path())
        .env("AISH_HOME", home.path().join("link/data"))
        .env_remove("CARGO_HOME")
        .args(["uninstall", "--yes", "--purge"])
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("outside home directory"),
        "stderr should say why: {stderr}"
    );
    assert!(bin.exists(), "nothing may be deleted when the guard fires");
    assert!(target.join("precious").exists());
}

/// `<home>/out/link` resolves back into home, but `out` leads out of it, so
/// the entry `remove_dir_all` would unlink, `link` itself, is outside home.
#[test]
fn uninstall_purge_refuses_a_link_out_of_home_that_points_back_in() {
    let home = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let stuff = home.path().join("stuff");
    std::fs::create_dir(&stuff).unwrap();
    std::os::unix::fs::symlink(outside.path(), home.path().join("out")).unwrap();
    let link = outside.path().join("link");
    std::os::unix::fs::symlink(&stuff, &link).unwrap();
    let bin = copy_bin(home.path(), "bin");

    let out = Command::new(&bin)
        .env("HOME", home.path())
        .env("AISH_HOME", home.path().join("out/link"))
        .env_remove("CARGO_HOME")
        .args(["uninstall", "--yes", "--purge"])
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("outside home directory"),
        "stderr should say why: {stderr}"
    );
    assert!(bin.exists(), "nothing may be deleted when the guard fires");
    assert!(
        link.symlink_metadata().is_ok(),
        "the link outside home must survive"
    );
}

/// A home behind a symlink (FreeBSD's `/home -> /usr/home`) still purges:
/// the resolved data dir is compared against the resolved home.
#[test]
fn uninstall_purge_works_when_home_is_behind_a_symlink() {
    let root = tempdir().unwrap();
    let real = root.path().join("real");
    let data = real.join(".aish");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("config.yaml"), "x: 1").unwrap();
    let home = root.path().join("home");
    std::os::unix::fs::symlink(&real, &home).unwrap();
    let bin = copy_bin(&home, "bin");

    let out = run(&bin, &home, None, &["uninstall", "--yes", "--purge"]);

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!bin.exists());
    assert!(!data.exists(), "data dir should be purged");
}

/// Dotdirs on a symlinked disk are common; a link that stays in home is fine.
#[test]
fn uninstall_purge_follows_a_symlink_that_stays_in_home() {
    let home = tempdir().unwrap();
    let data = home.path().join("disk/aish");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("config.yaml"), "x: 1").unwrap();
    std::os::unix::fs::symlink(home.path().join("disk"), home.path().join("link")).unwrap();
    let bin = copy_bin(home.path(), "bin");

    let out = Command::new(&bin)
        .env("HOME", home.path())
        .env("AISH_HOME", home.path().join("link/aish"))
        .env_remove("CARGO_HOME")
        .args(["uninstall", "--yes", "--purge"])
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!bin.exists());
    assert!(!data.exists(), "data dir should be purged");
}

/// Purge a stow-style `<home>/.aish -> dotfiles/aish`, the default data dir
/// or, given `aish_home`, `$AISH_HOME=<home>/<aish_home>`: the prompt must
/// name the target, and the link and what it points to must go, alone.
fn assert_purges_symlinked_data_dir(aish_home: Option<&str>) {
    let home = tempdir().unwrap();
    let dotfiles = home.path().join("dotfiles");
    let target = dotfiles.join("aish");
    std::fs::create_dir_all(target.join("cache")).unwrap();
    std::fs::write(target.join("config.yaml"), "x: 1").unwrap();
    std::fs::write(target.join("audit.log"), "{}").unwrap();
    std::fs::write(dotfiles.join("bashrc"), "keep").unwrap();
    let data = home.path().join(".aish");
    std::os::unix::fs::symlink("dotfiles/aish", &data).unwrap();
    let bin = copy_bin(home.path(), "bin");
    let uninstall = |args: &[&str]| {
        let mut cmd = Command::new(&bin);
        cmd.env("HOME", home.path()).env_remove("CARGO_HOME");
        match aish_home {
            // `join` keeps a trailing `/` or `/.` as written.
            Some(dir) => cmd.env("AISH_HOME", home.path().join(dir)),
            None => cmd.env_remove("AISH_HOME"),
        };
        cmd.args(args).stdin(Stdio::null()).output().unwrap()
    };

    // stdin is null, so the prompt aborts after showing what it would purge.
    let out = uninstall(&["uninstall", "--purge"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let shown = format!("-> {}", target.canonicalize().unwrap().display());
    assert!(stdout.contains(&shown), "{aish_home:?}: prompt: {stdout}");
    assert!(bin.exists(), "{aish_home:?}: the prompt should abort");

    let out = uninstall(&["uninstall", "--yes", "--purge", "--json"]);

    assert!(
        out.status.success(),
        "{aish_home:?}: stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["removed_data"], json!(true), "{aish_home:?}");
    assert!(!bin.exists(), "{aish_home:?}");
    let gone = |p: &Path| p.symlink_metadata().is_err();
    assert!(gone(&data), "{aish_home:?}: the link should be gone");
    assert!(gone(&target), "{aish_home:?}: its target should be purged");
    assert!(dotfiles.join("bashrc").exists(), "only the data dir may go");
}

/// A data dir that is itself a link, as GNU stow makes them: `remove_dir_all`
/// only unlinks it, so the purge has to remove what it points to as well.
#[test]
fn uninstall_purge_removes_what_a_symlinked_data_dir_points_to() {
    assert_purges_symlinked_data_dir(None);
}

/// Tab completion ends a link to a dir with `/`. Spelled so, or with `/.`,
/// the path makes the OS follow the link where the purge must remove it.
#[test]
fn uninstall_purge_removes_a_symlinked_data_dir_named_with_a_trailing_slash() {
    assert_purges_symlinked_data_dir(Some(".aish/"));
    assert_purges_symlinked_data_dir(Some(".aish/."));
}

/// `~/.aish` itself links out of home: its target must survive, the link too.
#[test]
fn uninstall_purge_refuses_a_symlinked_data_dir_pointing_out_of_home() {
    let home = tempdir().unwrap();
    let outside = tempdir().unwrap();
    std::fs::write(outside.path().join("precious"), "data").unwrap();
    let data = home.path().join(".aish");
    std::os::unix::fs::symlink(outside.path(), &data).unwrap();
    let bin = copy_bin(home.path(), "bin");

    let out = run(&bin, home.path(), None, &["uninstall", "--yes", "--purge"]);

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("outside home directory"),
        "stderr should say why: {stderr}"
    );
    assert!(bin.exists(), "nothing may be deleted when the guard fires");
    assert!(data.symlink_metadata().is_ok(), "the link must survive");
    assert!(outside.path().join("precious").exists());
}

/// A link to a file has no tree to purge; the uninstall must stop before the
/// binary goes, not fail after it.
#[test]
fn uninstall_purge_refuses_a_data_dir_that_is_not_a_directory() {
    let home = tempdir().unwrap();
    std::fs::write(home.path().join("aish.yaml"), "x: 1").unwrap();
    let data = home.path().join(".aish");
    std::os::unix::fs::symlink("aish.yaml", &data).unwrap();
    let bin = copy_bin(home.path(), "bin");

    let out = run(&bin, home.path(), None, &["uninstall", "--yes", "--purge"]);

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not a directory"),
        "stderr should say why: {stderr}"
    );
    assert!(bin.exists(), "nothing may be deleted when the guard fires");
    assert!(data.symlink_metadata().is_ok(), "the link must survive");
    assert!(home.path().join("aish.yaml").exists());
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

#[test]
fn uninstall_refuses_cargo_root_install() {
    let home = tempdir().unwrap();
    let bin = copy_bin(home.path(), "tools/bin");
    let root = home.path().join("tools");
    std::fs::write(root.join(".crates2.json"), CRATES2_AISH).unwrap();

    let out = run(&bin, home.path(), None, &["uninstall", "--yes"]);

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(&format!("cargo uninstall --root {} aish", root.display())),
        "stderr should name the install root: {stderr}"
    );
    assert!(bin.exists());
}

#[test]
fn uninstall_refuses_binary_under_cargo_home() {
    let home = tempdir().unwrap();
    let cargo_home = tempdir().unwrap();
    let bin = copy_bin(cargo_home.path(), "bin");

    let out = Command::new(&bin)
        .env("HOME", home.path())
        .env_remove("AISH_HOME")
        .env("CARGO_HOME", cargo_home.path())
        .args(["uninstall", "--yes"])
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("cargo uninstall aish"),
        "stderr should hint at cargo: {stderr}"
    );
    assert!(bin.exists());
}
