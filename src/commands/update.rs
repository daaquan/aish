// SPDX-License-Identifier: MIT
//! `aish update` — self-update from GitHub releases. Pure helpers live in
//! [`crate::update`]; this module owns the network calls and CLI output.
//!
//! Test hooks: `AISH_UPDATE_API_BASE` / `AISH_UPDATE_DOWNLOAD_BASE` redirect
//! the GitHub endpoints to a local mock server. Debug builds only — see
//! [`crate::update::endpoint_base`].

use crate::commands::{emit_json, Exit};
use crate::update::{
    asset_name, cargo_install, download_url, endpoint_base, is_newer, looks_like_binary,
    normalize_tag, parse_version, replace_binary,
};
use anyhow::{anyhow, Context, Result};

const DEFAULT_API_BASE: &str = "https://api.github.com";
const DEFAULT_DOWNLOAD_BASE: &str = "https://github.com";

/// `update --check` exit status when an update is available.
const CHECK_OUTDATED: i32 = 1;
/// `update --check` exit status when the check itself could not be answered:
/// not 1, which means "outdated" there, nor 2, clap's usage error.
const CHECK_FAILED: i32 = 3;

pub async fn run(check: bool, version: Option<String>, json: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    let api_base = endpoint_base("AISH_UPDATE_API_BASE", DEFAULT_API_BASE);
    let download_base = endpoint_base("AISH_UPDATE_DOWNLOAD_BASE", DEFAULT_DOWNLOAD_BASE);

    let client = reqwest::Client::new();
    let tag = match resolve_tag(&client, &api_base, version.as_deref()).await {
        Ok(tag) => tag,
        Err(e) if check => return Err(Exit::new(CHECK_FAILED, e).into()),
        Err(e) => return Err(e),
    };
    let latest = tag.trim_start_matches('v').to_string();

    // Pinned tag: install whenever it differs (downgrades allowed).
    // Latest: only a strictly newer version counts.
    let available = match &version {
        Some(_) => parse_version(&latest) != parse_version(current),
        None => is_newer(&latest, current),
    };

    if check {
        if json {
            emit_json(&serde_json::json!({
                "current": current,
                "latest": latest,
                "available": available,
                "updated": false,
            }));
        } else if available {
            println!("update available: v{latest} (current v{current})");
        } else {
            println!("aish v{current} is up to date");
        }
        if available {
            // Nonzero so `aish update --check` works as a CI gate, but not
            // an error: the check succeeded, so nothing goes to stderr.
            std::process::exit(CHECK_OUTDATED);
        }
        return Ok(());
    }

    if !available {
        if json {
            emit_json(&serde_json::json!({
                "current": current,
                "latest": latest,
                "updated": false,
            }));
        } else {
            println!("aish v{current} is up to date");
        }
        return Ok(());
    }

    let exe = std::env::current_exe().context("resolving current executable")?;
    let home = dirs::home_dir().ok_or_else(|| anyhow!("cannot determine home directory"))?;
    if let Some(install) = cargo_install(&exe, &home) {
        // Not `cargo install aish`: no `aish` crate is published on crates.io,
        // so that name would install whatever package claims it.
        return Err(anyhow!(
            "{} was installed via cargo; run `cargo install{} --git https://github.com/{}` to update instead",
            exe.display(),
            install.root_arg(),
            crate::update::REPO
        ));
    }

    let asset = asset_name(std::env::consts::OS, std::env::consts::ARCH).ok_or_else(|| {
        anyhow!(
            "no prebuilt binary for {}-{}; build from source: https://github.com/{}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            crate::update::REPO
        )
    })?;
    let url = download_url(&download_base, &tag, &asset);

    if !json {
        println!("downloading {asset} ({tag})");
    }
    let bytes = fetch_asset(&client, &url).await?;
    if !looks_like_binary(&bytes) {
        return Err(anyhow!(
            "downloaded payload is not an executable (asset '{asset}' missing for {tag}?)"
        ));
    }

    replace_binary(&exe, &bytes).map_err(|e| {
        anyhow!(
            "cannot replace {}: {e}; try `sudo aish update` or reinstall with AISH_INSTALL_DIR",
            exe.display()
        )
    })?;

    if json {
        emit_json(&serde_json::json!({
            "current": current,
            "latest": latest,
            "updated": true,
        }));
    } else {
        println!("updated aish v{current} -> v{latest} ({})", exe.display());
    }
    Ok(())
}

/// The release tag to check or install: the pinned `version` or the latest
/// release's, either way a plain `vX.Y.Z`. A latest tag that is no version
/// cannot be compared, so it is an error rather than "up to date".
async fn resolve_tag(
    client: &reqwest::Client,
    api_base: &str,
    version: Option<&str>,
) -> Result<String> {
    match version {
        Some(v) => normalize_tag(v)
            .ok_or_else(|| anyhow!("invalid version '{v}': expected X.Y.Z, e.g. 0.5.0")),
        None => {
            let tag = fetch_latest_tag(client, api_base).await?;
            normalize_tag(&tag)
                .ok_or_else(|| anyhow!("latest release tag '{tag}' is not an X.Y.Z version"))
        }
    }
}

async fn fetch_latest_tag(client: &reqwest::Client, api_base: &str) -> Result<String> {
    #[derive(serde::Deserialize)]
    struct Release {
        tag_name: String,
    }
    let url = crate::update::api_latest_url(api_base);
    let resp = client
        .get(&url)
        // GitHub's API rejects requests without a User-Agent.
        .header("user-agent", "aish-update")
        .send()
        .await
        .map_err(|e| anyhow!("release check failed: {}", e.without_url()))?;
    if !resp.status().is_success() {
        return Err(anyhow!("release check failed: HTTP {}", resp.status()));
    }
    let release: Release = resp
        .json()
        .await
        .context("decoding GitHub release response")?;
    Ok(release.tag_name)
}

async fn fetch_asset(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let resp = client
        .get(url)
        .header("user-agent", "aish-update")
        .send()
        .await
        .map_err(|e| anyhow!("download failed: {}", e.without_url()))?;
    if !resp.status().is_success() {
        return Err(anyhow!("download failed: HTTP {} for {url}", resp.status()));
    }
    Ok(resp
        .bytes()
        .await
        .context("reading release asset")?
        .to_vec())
}
