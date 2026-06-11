// SPDX-License-Identifier: MIT
//! Core logic for `aish update`: version parsing/comparison, release asset
//! naming, URL construction, payload validation, and atomic binary
//! replacement. Pure functions where possible; the network and CLI glue
//! live in `commands::update`.

use std::path::Path;

/// GitHub repository the release assets come from.
pub const REPO: &str = "daaquan/aish";

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

/// Ensure a tag has the `v` prefix GitHub releases use (`0.5.0` → `v0.5.0`).
pub fn normalize_tag(tag: &str) -> String {
    if tag.starts_with('v') {
        tag.to_string()
    } else {
        format!("v{tag}")
    }
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

/// True if the executable lives under `~/.cargo` — installed via
/// `cargo install`, so self-update/uninstall should defer to cargo.
pub fn is_cargo_install(exe: &Path, home: &Path) -> bool {
    exe.starts_with(home.join(".cargo"))
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
        assert_eq!(normalize_tag("0.5.0"), "v0.5.0");
        assert_eq!(normalize_tag("v0.5.0"), "v0.5.0");
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

    #[test]
    fn detects_cargo_installed_binaries() {
        let home = PathBuf::from("/home/u");
        assert!(is_cargo_install(
            Path::new("/home/u/.cargo/bin/aish"),
            &home
        ));
        assert!(!is_cargo_install(Path::new("/usr/local/bin/aish"), &home));
        assert!(!is_cargo_install(
            Path::new("/home/u/.local/bin/aish"),
            &home
        ));
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
