// SPDX-License-Identifier: MIT
//! Core logic for `aish update`: version parsing/comparison, release asset
//! naming, URL construction, payload validation, and atomic binary
//! replacement. Pure functions where possible; the network and CLI glue
//! live in `commands::update`.

use std::path::{Path, PathBuf};

/// GitHub repository the release assets come from.
pub const REPO: &str = "daaquan/aish";

/// Resolve a release endpoint base URL.
///
/// `AISH_UPDATE_API_BASE` / `AISH_UPDATE_DOWNLOAD_BASE` exist so the e2e tests
/// can point the update flow at a local mock server. They are honoured in debug
/// builds only: in a shipped binary, anything able to set an environment
/// variable could otherwise redirect a self-update download — over plain HTTP —
/// to a host of its choosing, and the payload is only checked for executable
/// magic bytes. Release builds always use the compiled-in default.
///
/// Note for anyone running the e2e suite under `--release`: the hooks are off
/// there, so those tests are debug-profile only.
pub fn endpoint_base(var: &str, default: &str) -> String {
    endpoint_base_with(cfg!(debug_assertions), var, default)
}

/// [`endpoint_base`] with the build-profile switch as a parameter, so the
/// release behaviour (`honour_env == false`) is unit-tested in every profile.
fn endpoint_base_with(honour_env: bool, var: &str, default: &str) -> String {
    if honour_env {
        if let Some(v) = std::env::var(var).ok().filter(|v| !v.is_empty()) {
            return v;
        }
    }
    default.to_string()
}

/// Parse `"0.5.0"` or `"v0.5.0"` into `(major, minor, patch)`.
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.strip_prefix('v').unwrap_or(s);
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// True if `candidate` is a strictly newer version than `current`.
/// Unparseable input is never newer — fail safe, no surprise installs.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(c), Some(cur)) => c > cur,
        _ => false,
    }
}

/// Release asset name for a platform, mirroring the install script:
/// `aish-$(uname -s)-$(uname -m)`. Takes `std::env::consts::{OS, ARCH}`
/// values (`"linux"`/`"macos"`, `"x86_64"`/`"aarch64"`).
pub fn asset_name(os: &str, arch: &str) -> Option<String> {
    let (uname_os, uname_arch) = match (os, arch) {
        ("linux", "x86_64") => ("Linux", "x86_64"),
        ("linux", "aarch64") => ("Linux", "aarch64"),
        ("macos", "x86_64") => ("Darwin", "x86_64"),
        // macOS reports arm64 via `uname -m`, not aarch64.
        ("macos", "aarch64") => ("Darwin", "arm64"),
        _ => return None,
    };
    Some(format!("aish-{uname_os}-{uname_arch}"))
}

/// GitHub API URL returning the latest release as JSON (`tag_name` field).
pub fn api_latest_url(api_base: &str) -> String {
    format!(
        "{}/repos/{REPO}/releases/latest",
        api_base.trim_end_matches('/')
    )
}

/// Download URL for one release asset.
pub fn download_url(download_base: &str, tag: &str, asset: &str) -> String {
    format!(
        "{}/{REPO}/releases/download/{tag}/{asset}",
        download_base.trim_end_matches('/')
    )
}

/// Canonical release tag for a pinned version: `0.5.0` / `v0.5.0` → `v0.5.0`.
///
/// `None` unless `tag` is a plain `X.Y.Z` version. The tag is spliced into the
/// download URL path, so anything else — e.g. `..` segments that would resolve
/// to another repository's release asset — must never reach it.
pub fn normalize_tag(tag: &str) -> Option<String> {
    let (major, minor, patch) = parse_version(tag)?;
    Some(format!("v{major}.{minor}.{patch}"))
}

/// Positive magic-byte check: ELF or Mach-O executable. Rejects empty
/// bodies, HTML error pages, and JSON error payloads.
pub fn looks_like_binary(bytes: &[u8]) -> bool {
    const MAGICS: [[u8; 4]; 6] = [
        [0x7f, b'E', b'L', b'F'], // ELF
        [0xfe, 0xed, 0xfa, 0xce], // Mach-O 32 BE
        [0xfe, 0xed, 0xfa, 0xcf], // Mach-O 64 BE
        [0xce, 0xfa, 0xed, 0xfe], // Mach-O 32 LE
        [0xcf, 0xfa, 0xed, 0xfe], // Mach-O 64 LE
        [0xca, 0xfe, 0xba, 0xbe], // Mach-O universal (fat)
    ];
    bytes.len() >= 4 && MAGICS.iter().any(|m| bytes.starts_with(m))
}

/// How the running executable was found to be managed by `cargo install`.
#[derive(Debug, PartialEq, Eq)]
pub enum CargoInstall {
    /// Under cargo's home (`$CARGO_HOME`, or `~/.cargo` while that is unset),
    /// the root cargo uses when given none.
    Home,
    /// In `<root>/bin` of an explicit `cargo install --root <root>` (or
    /// `$CARGO_INSTALL_ROOT`), found via cargo's install metadata there; or
    /// under `~/.cargo` while `$CARGO_HOME` points elsewhere.
    Root(PathBuf),
}

impl CargoInstall {
    /// ` --root <root>` for a [`CargoInstall::Root`] install, else empty:
    /// cargo only finds the install again when pointed at the same root.
    pub fn root_arg(&self) -> String {
        match self {
            CargoInstall::Home => String::new(),
            CargoInstall::Root(root) => {
                let root = root.display().to_string();
                // Double-quote a root a shell would split or unescape (spaces,
                // common on Windows; backslashes under Git Bash): sh, cmd and
                // PowerShell all read "<root>" as one argument.
                let plain = |c: char| c.is_alphanumeric() || "/._-+:~".contains(c);
                if root.chars().all(plain) {
                    format!(" --root {root}")
                } else {
                    format!(" --root \"{root}\"")
                }
            }
        }
    }
}

/// `Some` if the executable was installed via `cargo install`, so
/// self-update/uninstall should defer to cargo: replacing or deleting it
/// would leave cargo's install metadata describing a binary that is gone.
pub fn cargo_install(exe: &Path, home: &Path) -> Option<CargoInstall> {
    let cargo_home = std::env::var_os("CARGO_HOME");
    cargo_install_with(exe, home, cargo_home.as_deref().map(Path::new))
}

/// [`cargo_install`] with `$CARGO_HOME` as a parameter, so every case is
/// unit-tested without touching the environment.
fn cargo_install_with(exe: &Path, home: &Path, cargo_home: Option<&Path>) -> Option<CargoInstall> {
    // cargo treats an empty $CARGO_HOME as unset; as a prefix it would match
    // every path.
    let cargo_home = cargo_home.filter(|h| !h.as_os_str().is_empty());
    let home_cargo = home.join(".cargo");
    if cargo_home.map_or(exe.starts_with(&home_cargo), |h| exe.starts_with(h)) {
        return Some(CargoInstall::Home);
    }
    // Still cargo's, but with $CARGO_HOME pointing elsewhere a plain
    // `cargo uninstall aish` looks there and finds nothing.
    if exe.starts_with(&home_cargo) {
        return Some(CargoInstall::Root(home_cargo));
    }
    // `cargo install --root <root>` puts binaries in `<root>/bin` and records
    // them in `<root>`. Checking that the record names this binary keeps an
    // install.sh copy in /usr/local/bin apart from cargo tools under /usr/local.
    let bin = exe.file_name()?.to_str()?;
    let bin_dir = exe.parent()?;
    if bin_dir.file_name()? != "bin" {
        return None;
    }
    let root = bin_dir.parent()?;
    // A missing or unreadable file reads as empty, which lists nothing.
    let read = |name| std::fs::read_to_string(root.join(name)).unwrap_or_default();
    (crates2_json_lists(&read(".crates2.json"), bin)
        || crates_toml_lists(&read(".crates.toml"), bin))
    .then(|| CargoInstall::Root(root.to_path_buf()))
}

/// True if cargo's `.crates2.json` records `bin` for any installed package:
/// `{"installs": {"<pkg id>": {"bins": ["aish"], ...}}}`. Garbled JSON lists
/// nothing.
fn crates2_json_lists(text: &str, bin: &str) -> bool {
    #[derive(serde::Deserialize)]
    struct Crates2 {
        installs: std::collections::BTreeMap<String, Install>,
    }
    #[derive(serde::Deserialize)]
    struct Install {
        #[serde(default)]
        bins: Vec<String>,
    }
    serde_json::from_str::<Crates2>(text)
        .is_ok_and(|c| c.installs.values().any(|i| i.bins.iter().any(|b| b == bin)))
}

/// True if cargo's older `.crates.toml` records `bin` in its `[v1]` table:
/// `"<pkg id>" = ["aish"]`. A textual check rather than a TOML dependency:
/// package ids always contain spaces (`name version (source)`), so the only
/// quoted string in the table that can equal a bin name is a listed bin.
fn crates_toml_lists(text: &str, bin: &str) -> bool {
    let mut in_v1 = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_v1 = line == "[v1]";
        } else if in_v1 && line.split('"').skip(1).step_by(2).any(|s| s == bin) {
            return true;
        }
    }
    false
}

/// Atomically replace `target` with `bytes`: write a sibling temp file,
/// set it executable, then rename over the target. Never truncates the
/// live binary in place.
pub fn replace_binary(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = target.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "target has no parent directory",
        )
    })?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("aish"),
        std::process::id()
    ));
    let write = (|| {
        std::fs::write(&tmp, bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        }
        std::fs::rename(&tmp, target)
    })();
    if write.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    write
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The `AISH_UPDATE_*` variables are test hooks. A shipped binary must
    /// ignore them, or anything that can set an environment variable can point
    /// a self-update download at a host of its choosing.
    #[test]
    fn endpoint_base_honours_the_override_only_in_debug_builds() {
        const VAR: &str = "AISH_TEST_ENDPOINT_BASE";
        const DEFAULT: &str = "https://api.github.com";
        const HOOK: &str = "http://127.0.0.1:9999";

        std::env::remove_var(VAR);
        let unset = endpoint_base_with(true, VAR, DEFAULT);
        std::env::set_var(VAR, "");
        let empty = endpoint_base_with(true, VAR, DEFAULT);
        std::env::set_var(VAR, HOOK);
        let release = endpoint_base_with(false, VAR, DEFAULT);
        let debug = endpoint_base_with(true, VAR, DEFAULT);
        let this_build = endpoint_base(VAR, DEFAULT);
        std::env::remove_var(VAR);

        assert_eq!(unset, DEFAULT);
        assert_eq!(empty, DEFAULT);
        assert_eq!(release, DEFAULT, "release build honoured the env hook");
        assert_eq!(debug, HOOK, "test hook must work in debug builds");
        let expected = if cfg!(debug_assertions) {
            HOOK
        } else {
            DEFAULT
        };
        assert_eq!(this_build, expected);
    }

    #[test]
    fn parses_plain_and_v_prefixed_versions() {
        assert_eq!(parse_version("0.4.0"), Some((0, 4, 0)));
        assert_eq!(parse_version("v0.5.1"), Some((0, 5, 1)));
        assert_eq!(parse_version("10.20.30"), Some((10, 20, 30)));
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("not-a-version"), None);
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
    }

    #[test]
    fn is_newer_compares_numerically_not_lexically() {
        assert!(is_newer("0.5.0", "0.4.0"));
        assert!(is_newer("0.10.0", "0.9.0")); // lexical compare would fail
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.4.0", "0.4.0"));
        assert!(!is_newer("0.3.9", "0.4.0"));
        // Unparseable input is never "newer" — fail safe, no surprise installs.
        assert!(!is_newer("garbage", "0.4.0"));
        assert!(!is_newer("0.5.0", "garbage"));
    }

    #[test]
    fn asset_names_match_install_script_convention() {
        assert_eq!(asset_name("linux", "x86_64").unwrap(), "aish-Linux-x86_64");
        assert_eq!(
            asset_name("linux", "aarch64").unwrap(),
            "aish-Linux-aarch64"
        );
        assert_eq!(asset_name("macos", "x86_64").unwrap(), "aish-Darwin-x86_64");
        // macOS ARM is arm64, NOT aarch64 — raw `uname -m` output.
        assert_eq!(asset_name("macos", "aarch64").unwrap(), "aish-Darwin-arm64");
        assert_eq!(asset_name("windows", "x86_64"), None);
        assert_eq!(asset_name("linux", "riscv64"), None);
    }

    #[test]
    fn urls_target_the_aish_repo() {
        assert_eq!(
            api_latest_url("https://api.github.com"),
            "https://api.github.com/repos/daaquan/aish/releases/latest"
        );
        // Trailing slash on the base must not produce a double slash.
        assert_eq!(
            api_latest_url("http://127.0.0.1:9999/"),
            "http://127.0.0.1:9999/repos/daaquan/aish/releases/latest"
        );
        assert_eq!(
            download_url("https://github.com", "v0.5.0", "aish-Linux-x86_64"),
            "https://github.com/daaquan/aish/releases/download/v0.5.0/aish-Linux-x86_64"
        );
    }

    #[test]
    fn normalize_tag_adds_v_prefix_once() {
        assert_eq!(normalize_tag("0.5.0").as_deref(), Some("v0.5.0"));
        assert_eq!(normalize_tag("v0.5.0").as_deref(), Some("v0.5.0"));
    }

    #[test]
    fn normalize_tag_rejects_anything_but_a_plain_version() {
        // Spliced into the download URL, `..` would resolve to another
        // repository's release asset on github.com.
        assert_eq!(
            normalize_tag("v1/../../../../../evil/aish/releases/download/v1"),
            None
        );
        assert_eq!(normalize_tag("1.2.3/../x"), None);
        assert_eq!(normalize_tag("latest"), None);
        assert_eq!(normalize_tag(""), None);
    }

    #[test]
    fn looks_like_binary_accepts_elf_and_macho_only() {
        assert!(looks_like_binary(b"\x7fELF\x02\x01\x01rest"));
        assert!(looks_like_binary(&[0xcf, 0xfa, 0xed, 0xfe, 0x00])); // Mach-O 64 LE
        assert!(looks_like_binary(&[0xfe, 0xed, 0xfa, 0xcf, 0x00])); // Mach-O 64 BE
        assert!(looks_like_binary(&[0xca, 0xfe, 0xba, 0xbe, 0x00])); // fat binary
        assert!(!looks_like_binary(b""));
        assert!(!looks_like_binary(b"<html><body>404</body></html>"));
        assert!(!looks_like_binary(b"{\"message\":\"Not Found\"}"));
        assert!(!looks_like_binary(b"Not Found"));
    }

    const CRATES2_AISH: &str = r#"{"installs":{"aish 0.9.0 (git+https://github.com/daaquan/aish#298f3704)":{"version_req":null,"bins":["aish"],"features":[],"all_features":false,"no_default_features":false,"profile":"release","target":"x86_64-unknown-linux-gnu","rustc":"rustc 1.81.0"}}}"#;
    const CRATES_TOML_AISH: &str = "[v1]\n\
        \"aish 0.9.0 (git+https://github.com/daaquan/aish#298f3704)\" = [\"aish\"]\n\
        \"ripgrep 14.1.0 (registry+https://github.com/rust-lang/crates.io-index)\" = [\"rg\"]\n";

    /// A `<root>/bin` dir holding cargo metadata `files` in `<root>`, and the
    /// path of `bin` inside it (the binary itself need not exist).
    fn install_root(bin: &str, files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("bin")).unwrap();
        for (name, body) in files {
            std::fs::write(root.path().join(name), body).unwrap();
        }
        let exe = root.path().join("bin").join(bin);
        (root, exe)
    }

    #[test]
    fn detects_binaries_under_home_cargo() {
        let home = Path::new("/home/u");
        let exe = Path::new("/home/u/.cargo/bin/aish");
        assert_eq!(
            cargo_install_with(exe, home, None),
            Some(CargoInstall::Home)
        );
        assert_eq!(
            cargo_install_with(exe, home, Some(Path::new("/home/u/.cargo"))),
            Some(CargoInstall::Home)
        );
        // ~/.cargo still counts when $CARGO_HOME points elsewhere, but cargo
        // then only finds it via `--root ~/.cargo`.
        assert_eq!(
            cargo_install_with(exe, home, Some(Path::new("/usr/local/cargo"))),
            Some(CargoInstall::Root(PathBuf::from("/home/u/.cargo")))
        );
    }

    #[test]
    fn empty_cargo_home_counts_as_unset() {
        let home = Path::new("/home/u");
        assert_eq!(
            cargo_install_with(
                Path::new("/home/u/.cargo/bin/aish"),
                home,
                Some(Path::new(""))
            ),
            Some(CargoInstall::Home)
        );
        let (_root, exe) = install_root("aish", &[]);
        assert_eq!(cargo_install_with(&exe, home, Some(Path::new(""))), None);
    }

    #[test]
    fn detects_binaries_under_cargo_home_env() {
        // The official rust Docker image sets CARGO_HOME=/usr/local/cargo.
        assert_eq!(
            cargo_install_with(
                Path::new("/usr/local/cargo/bin/aish"),
                Path::new("/root"),
                Some(Path::new("/usr/local/cargo"))
            ),
            Some(CargoInstall::Home)
        );
    }

    #[test]
    fn unrelated_cargo_home_is_not_a_cargo_install() {
        let (_root, exe) = install_root("aish", &[]);
        assert_eq!(
            cargo_install_with(&exe, Path::new("/home/u"), Some(Path::new("/opt/cargo"))),
            None
        );
    }

    #[test]
    fn detects_root_installs_via_crates2_json() {
        let (root, exe) = install_root("aish", &[(".crates2.json", CRATES2_AISH)]);
        assert_eq!(
            cargo_install_with(&exe, Path::new("/home/u"), None),
            Some(CargoInstall::Root(root.path().to_path_buf()))
        );
    }

    #[test]
    fn detects_root_installs_via_crates_toml() {
        let (root, exe) = install_root("aish", &[(".crates.toml", CRATES_TOML_AISH)]);
        assert_eq!(
            cargo_install_with(&exe, Path::new("/home/u"), None),
            Some(CargoInstall::Root(root.path().to_path_buf()))
        );
        // A garbled .crates2.json does not hide a valid .crates.toml.
        let (root, exe) = install_root(
            "aish",
            &[
                (".crates2.json", "{\"installs\":"),
                (".crates.toml", CRATES_TOML_AISH),
            ],
        );
        assert_eq!(
            cargo_install_with(&exe, Path::new("/home/u"), None),
            Some(CargoInstall::Root(root.path().to_path_buf()))
        );
    }

    #[test]
    fn root_installs_match_the_exe_file_name() {
        // Windows records the bin with its extension.
        let json = CRATES2_AISH.replace("[\"aish\"]", "[\"aish.exe\"]");
        let (root, exe) = install_root("aish.exe", &[(".crates2.json", &json)]);
        assert_eq!(
            cargo_install_with(&exe, Path::new("/home/u"), None),
            Some(CargoInstall::Root(root.path().to_path_buf()))
        );
        let (_root, exe) = install_root("aish.exe", &[(".crates2.json", CRATES2_AISH)]);
        assert_eq!(cargo_install_with(&exe, Path::new("/home/u"), None), None);
    }

    #[test]
    fn root_metadata_that_does_not_list_aish_is_not_a_cargo_install() {
        // install.sh put aish in /usr/local/bin, next to cargo tools installed
        // with `--root /usr/local`. The package id mentions aish; no bin is it.
        let json = r#"{"installs":{"aish-helper 1.0.0 (git+https://github.com/daaquan/aish#1)":{"bins":["aish-helper"]},"ripgrep 14.1.0 (registry+https://github.com/rust-lang/crates.io-index)":{"bins":["rg"]}}}"#;
        let toml = "[v1]\n\
            \"aish-helper 1.0.0 (git+https://github.com/daaquan/aish#1)\" = [\"aish-helper\"]\n\
            \"ripgrep 14.1.0 (registry+https://github.com/rust-lang/crates.io-index)\" = [\"rg\"]\n";
        let (_root, exe) = install_root("aish", &[(".crates2.json", json), (".crates.toml", toml)]);
        assert_eq!(cargo_install_with(&exe, Path::new("/home/u"), None), None);
    }

    #[test]
    fn missing_or_garbled_root_metadata_is_not_a_cargo_install() {
        let home = Path::new("/home/u");
        let (_root, exe) = install_root("aish", &[]);
        assert_eq!(cargo_install_with(&exe, home, None), None);

        let (_root, exe) = install_root(
            "aish",
            &[
                (".crates2.json", "{\"installs\": [\"aish\"]}"),
                // `aish` outside the [v1] table is not an install record.
                (".crates.toml", "aish\n[v2]\n\"x 1.0.0 (y)\" = [\"aish\"]\n"),
            ],
        );
        assert_eq!(cargo_install_with(&exe, home, None), None);

        // Not UTF-8: unreadable, so it lists nothing even though it names aish.
        let (root, exe) = install_root("aish", &[]);
        let mut bytes = b"\xff\xfe".to_vec();
        bytes.extend_from_slice(CRATES_TOML_AISH.as_bytes());
        std::fs::write(root.path().join(".crates.toml"), bytes).unwrap();
        assert_eq!(cargo_install_with(&exe, home, None), None);
    }

    #[test]
    fn root_installs_sit_directly_in_root_bin() {
        let (root, _exe) = install_root("aish", &[(".crates2.json", CRATES2_AISH)]);
        let exe = root.path().join("libexec").join("aish");
        assert_eq!(cargo_install_with(&exe, Path::new("/home/u"), None), None);
    }

    #[test]
    fn root_arg_names_the_root_only_for_root_installs() {
        assert_eq!(CargoInstall::Home.root_arg(), "");
        assert_eq!(
            CargoInstall::Root(PathBuf::from("/opt/tools")).root_arg(),
            " --root /opt/tools"
        );
        // Quoted where a shell would split or unescape it.
        assert_eq!(
            CargoInstall::Root(PathBuf::from("/opt/my tools")).root_arg(),
            " --root \"/opt/my tools\""
        );
        assert_eq!(
            CargoInstall::Root(PathBuf::from(r"C:\Users\me\tools")).root_arg(),
            r#" --root "C:\Users\me\tools""#
        );
    }

    #[test]
    fn replace_binary_swaps_content_and_sets_exec_bit() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("aish");
        std::fs::write(&target, b"old").unwrap();

        replace_binary(&target, b"\x7fELF new contents").unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"\x7fELF new contents");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "exec bits not set: {mode:o}");
        }
        // No temp file left behind.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, vec![std::ffi::OsString::from("aish")]);
    }

    #[test]
    fn replace_binary_fails_on_unwritable_dir_without_touching_target() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dir = tempfile::tempdir().unwrap();
            let target = dir.path().join("aish");
            std::fs::write(&target, b"old").unwrap();
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();

            let err = replace_binary(&target, b"new");
            // Restore so tempdir can clean up.
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

            assert!(err.is_err());
            assert_eq!(std::fs::read(&target).unwrap(), b"old");
        }
    }
}
