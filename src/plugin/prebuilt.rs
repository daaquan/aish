// SPDX-License-Identifier: MIT
use crate::plugin::aish_home;
use crate::plugin::install::RegistrySource;
use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const HOST_TARGET: &str = env!("AISH_TARGET");

pub fn release_repo(source: &RegistrySource, registry_dir: &Path) -> Option<String> {
    match source {
        RegistrySource::Git { url } => parse_github_repo(url),
        RegistrySource::Local(_) => {
            let out = Command::new("git")
                .current_dir(registry_dir)
                .args(["remote", "get-url", "origin"])
                .output()
                .ok()?;
            if !out.status.success() {
                return None;
            }
            parse_github_repo(String::from_utf8_lossy(&out.stdout).trim())
        }
    }
}

pub async fn fetch_prebuilt(
    repo: &str,
    name: &str,
    version: &str,
    target: &str,
) -> Result<Option<PathBuf>> {
    let asset = format!("{name}-{target}");
    let base = format!("https://github.com/{repo}/releases/download/{name}-v{version}");
    fetch_prebuilt_from(&base, &asset, "SHA256SUMS").await
}

async fn fetch_prebuilt_from(
    base_url: &str,
    asset: &str,
    sums_asset: &str,
) -> Result<Option<PathBuf>> {
    let client = reqwest::Client::new();
    let asset_url = format!("{}/{asset}", base_url.trim_end_matches('/'));
    let response = client
        .get(&asset_url)
        .send()
        .await
        .with_context(|| format!("fetching prebuilt asset {asset_url}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(anyhow!(
            "fetching prebuilt asset {asset_url} failed with {}",
            response.status()
        ));
    }
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("reading prebuilt asset {asset_url}"))?;

    let sums_url = format!("{}/{sums_asset}", base_url.trim_end_matches('/'));
    let sums_response = client
        .get(&sums_url)
        .send()
        .await
        .with_context(|| format!("fetching checksum asset {sums_url}"))?;
    if !sums_response.status().is_success() {
        return Err(anyhow!(
            "fetching checksum asset {sums_url} failed with {}",
            sums_response.status()
        ));
    }
    let sums = sums_response
        .text()
        .await
        .with_context(|| format!("reading checksum asset {sums_url}"))?;
    let expected = checksum_for_asset(&sums, asset)
        .ok_or_else(|| anyhow!("no checksum for {asset} in {sums_asset}"))?;
    let actual = hex_sha256(&bytes);
    if !actual.eq_ignore_ascii_case(&expected) {
        return Err(anyhow!(
            "checksum mismatch for {asset}: expected {expected}, got {actual}"
        ));
    }

    let build_dir = aish_home().join("build");
    std::fs::create_dir_all(&build_dir)
        .with_context(|| format!("creating {}", build_dir.display()))?;
    let path = build_dir.join(format!("{asset}.download"));
    std::fs::write(&path, &bytes).with_context(|| format!("writing {}", path.display()))?;
    make_executable(&path)?;
    Ok(Some(path))
}

fn parse_github_repo(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let repo = if let Some(rest) = trimmed.strip_prefix("git@github.com:") {
        rest
    } else {
        trimmed.strip_prefix("https://github.com/")?
    };
    let repo = repo
        .trim_end_matches('/')
        .strip_suffix(".git")
        .unwrap_or(repo);
    let mut parts = repo.split('/');
    let owner = parts.next()?;
    let name = parts.next()?;
    if owner.is_empty() || name.is_empty() || parts.next().is_some() {
        return None;
    }
    Some(format!("{owner}/{name}"))
}

fn checksum_for_asset(sums: &str, asset: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?;
        if name == asset {
            Some(hash.to_string())
        } else {
            None
        }
    })
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn parses_github_repo_urls() {
        assert_eq!(
            parse_github_repo("git@github.com:daaquan/aish-plugins.git"),
            Some("daaquan/aish-plugins".to_string())
        );
        assert_eq!(
            parse_github_repo("https://github.com/daaquan/aish-plugins"),
            Some("daaquan/aish-plugins".to_string())
        );
        assert_eq!(
            parse_github_repo("https://github.com/daaquan/aish-plugins.git"),
            Some("daaquan/aish-plugins".to_string())
        );
        assert_eq!(parse_github_repo("https://gitlab.com/x/y"), None);
    }

    #[tokio::test]
    #[serial]
    async fn fetch_prebuilt_downloads_verified_executable_binary() {
        let home = tempdir().unwrap();
        std::env::set_var("AISH_HOME", home.path());
        let server = MockServer::start().await;
        let bytes = b"#!/bin/sh\necho ok\n";
        let sha = hex_sha256(bytes);
        Mock::given(method("GET"))
            .and(path("/releases/download/demo-v0.1.0/demo-x86_64-test"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/releases/download/demo-v0.1.0/SHA256SUMS"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(format!("{sha}  demo-x86_64-test\n")),
            )
            .mount(&server)
            .await;

        let path = fetch_prebuilt_from(
            &format!("{}/releases/download/demo-v0.1.0", server.uri()),
            "demo-x86_64-test",
            "SHA256SUMS",
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o111,
            0o111
        );
        std::env::remove_var("AISH_HOME");
    }

    #[tokio::test]
    async fn fetch_prebuilt_returns_none_when_binary_is_missing() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/releases/download/demo-v0.1.0/demo-x86_64-test"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let result = fetch_prebuilt_from(
            &format!("{}/releases/download/demo-v0.1.0", server.uri()),
            "demo-x86_64-test",
            "SHA256SUMS",
        )
        .await
        .unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    #[serial]
    async fn fetch_prebuilt_errors_on_sha_mismatch() {
        let home = tempdir().unwrap();
        std::env::set_var("AISH_HOME", home.path());
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/releases/download/demo-v0.1.0/demo-x86_64-test"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"actual"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/releases/download/demo-v0.1.0/SHA256SUMS"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(format!("{}  demo-x86_64-test\n", hex_sha256(b"expected"))),
            )
            .mount(&server)
            .await;

        let err = fetch_prebuilt_from(
            &format!("{}/releases/download/demo-v0.1.0", server.uri()),
            "demo-x86_64-test",
            "SHA256SUMS",
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("checksum mismatch"));
        std::env::remove_var("AISH_HOME");
    }

    #[tokio::test]
    async fn fetch_prebuilt_errors_when_checksum_line_is_missing() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/releases/download/demo-v0.1.0/demo-x86_64-test"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"actual"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/releases/download/demo-v0.1.0/SHA256SUMS"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(format!("{}  other-asset\n", hex_sha256(b"actual"))),
            )
            .mount(&server)
            .await;

        let err = fetch_prebuilt_from(
            &format!("{}/releases/download/demo-v0.1.0", server.uri()),
            "demo-x86_64-test",
            "SHA256SUMS",
        )
        .await
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("no checksum for demo-x86_64-test in SHA256SUMS"));
    }

    fn hex_sha256(bytes: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(bytes);
        format!("{:x}", h.finalize())
    }
}
