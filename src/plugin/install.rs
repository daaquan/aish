// SPDX-License-Identifier: MIT
use crate::plugin::aish_home;
use crate::plugin::manifest::{InstalledRegistry, Manifest, PluginEntry};
use crate::plugin::prebuilt;
use anyhow::{anyhow, Context, Result};
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub fn plugins_dir() -> PathBuf {
    aish_home().join("plugins")
}
pub fn plugins_toml() -> PathBuf {
    aish_home().join("plugins.toml")
}
fn lock_path() -> PathBuf {
    aish_home().join("plugins.lock")
}

/// Reject plugin names that are unsafe as a filesystem path component. Plugins
/// are trusted, but a stray `../` or separator in a registry manifest name must
/// never let an install escape `~/.aish/plugins`.
fn validate_plugin_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !ok {
        return Err(anyhow!(
            "invalid plugin name `{name}`: must be non-empty, match [A-Za-z0-9._-]+, and not be `.` or `..`"
        ));
    }
    Ok(())
}

fn format_cargo_build_error(name: &str, stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr);
    let mut msg = format!("cargo build failed for `{name}`: {stderr}");
    if stderr.contains("can't find crate for `std`")
        || stderr.contains("can't find crate for `core`")
    {
        msg.push_str(
            "\n\nRust target standard libraries are missing. Install the host target, then retry:\n  rustup target add x86_64-unknown-linux-gnu",
        );
    }
    msg
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Ok(format!("{:x}", h.finalize()))
}

/// Install an already-built plugin binary + manifest atomically and record it.
/// `source`/`revision` describe provenance for `plugins.toml`.
pub fn install_built(
    manifest: &Manifest,
    built_binary: &Path,
    source: &str,
    revision: &str,
) -> Result<PluginEntry> {
    let home = aish_home();
    std::fs::create_dir_all(&home)?;

    // Serialize concurrent installs.
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(lock_path())?;
    lock.lock_exclusive()?;
    let result = (|| {
        validate_plugin_name(&manifest.name)?;
        let mut reg = InstalledRegistry::load(&plugins_toml())?;
        reg.check_conflicts(&manifest.name, &manifest.subcommands)?;

        let final_dir = plugins_dir().join(&manifest.name);
        let staging = plugins_dir().join(format!(".{}.staging", manifest.name));
        if staging.exists() {
            std::fs::remove_dir_all(&staging)?;
        }
        std::fs::create_dir_all(&staging)?;

        let bin_name = manifest.name.clone();
        let staged_bin = staging.join(&bin_name);
        std::fs::copy(built_binary, &staged_bin).with_context(|| {
            format!(
                "copying {} -> {}",
                built_binary.display(),
                staged_bin.display()
            )
        })?;
        make_executable(&staged_bin)?;
        std::fs::write(
            staging.join("aish-plugin.toml"),
            toml::to_string_pretty(manifest)?,
        )?;

        let sha = sha256_file(&staged_bin)?;

        // Crash-safe replace: move any previous install aside as a backup, swap
        // the staging dir into place, then drop the backup. If the swap fails,
        // restore the backup so a failed install never destroys a working one.
        let backup = plugins_dir().join(format!(".{}.backup", manifest.name));
        if backup.exists() {
            std::fs::remove_dir_all(&backup)?;
        }
        let had_previous = final_dir.exists();
        if had_previous {
            std::fs::rename(&final_dir, &backup)?;
        }
        match std::fs::rename(&staging, &final_dir) {
            Ok(()) => {
                if had_previous {
                    let _ = std::fs::remove_dir_all(&backup);
                }
            }
            Err(e) => {
                if had_previous {
                    let _ = std::fs::rename(&backup, &final_dir);
                }
                return Err(anyhow::Error::from(e))
                    .with_context(|| format!("renaming staging into {}", final_dir.display()));
            }
        }

        let entry = PluginEntry {
            version: manifest.version.clone(),
            abi: manifest.abi.clone(),
            enabled: true,
            path: final_dir.join(&bin_name),
            subcommands: manifest.subcommands.clone(),
            source: source.to_string(),
            revision: revision.to_string(),
            binary_sha256: sha,
        };
        reg.plugins.insert(manifest.name.clone(), entry.clone());
        reg.save(&plugins_toml())?;
        Ok(entry)
    })();
    let _ = FileExt::unlock(&lock);
    result
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

/// Verify a plugin binary still matches its recorded hash (tamper check).
pub fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    if expected.is_empty() {
        return Ok(()); // unrecorded (e.g. test entries) — skip
    }
    let actual = sha256_file(path)?;
    if actual != expected {
        return Err(anyhow!(
            "plugin binary at {} failed integrity check",
            path.display()
        ));
    }
    Ok(())
}

use std::process::Command;

/// Where the registry lives. A local filesystem path (tests/dev) or a git URL.
#[derive(Debug, Clone)]
pub enum RegistrySource {
    Local(PathBuf),
    Git { url: String },
}

impl RegistrySource {
    /// Parse a config/CLI value: an existing path or a `file://` is Local,
    /// anything containing `://` or `git@` is Git.
    pub fn parse(value: &str) -> RegistrySource {
        let trimmed = value.trim();
        if let Some(rest) = trimmed.strip_prefix("file://") {
            return RegistrySource::Local(PathBuf::from(rest));
        }
        if trimmed.contains("://") || trimmed.starts_with("git@") {
            return RegistrySource::Git {
                url: trimmed.to_string(),
            };
        }
        RegistrySource::Local(PathBuf::from(trimmed))
    }
}

/// Ensure a local checkout of the registry exists, returning (dir, revision).
/// Local sources are used in place; git sources are cloned/updated under
/// `~/.aish/registry`.
pub fn ensure_registry(source: &RegistrySource) -> Result<(PathBuf, String)> {
    match source {
        RegistrySource::Local(dir) => {
            if !dir.exists() {
                return Err(anyhow!("registry path does not exist: {}", dir.display()));
            }
            Ok((dir.clone(), "local".to_string()))
        }
        RegistrySource::Git { url } => {
            let dir = aish_home().join("registry");
            if dir.join(".git").exists() {
                run_git(&dir, &["fetch", "--quiet", "origin"])?;
                run_git(&dir, &["reset", "--quiet", "--hard", "origin/HEAD"])?;
            } else {
                std::fs::create_dir_all(aish_home())?;
                run_git(
                    Path::new("."),
                    &["clone", "--quiet", url, &dir.display().to_string()],
                )?;
            }
            let rev = run_git(&dir, &["rev-parse", "HEAD"])?.trim().to_string();
            Ok((dir, rev))
        }
    }
}

fn run_git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .with_context(|| format!("running git {args:?}"))?;
    if !out.status.success() {
        return Err(anyhow!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Build the named plugin crate in the registry checkout and return the path to
/// the produced release binary. Uses an isolated CARGO_HOME under ~/.aish.
pub fn build_plugin(registry_dir: &Path, name: &str) -> Result<PathBuf> {
    let crate_dir = registry_dir.join(name);
    let manifest_path = crate_dir.join("Cargo.toml");
    if !manifest_path.exists() {
        return Err(anyhow!(
            "plugin `{name}` not found in registry ({})",
            manifest_path.display()
        ));
    }
    let cargo_home = aish_home().join("cargo-home");
    std::fs::create_dir_all(&cargo_home)?;
    // Pin the output location explicitly. The registry may be a cargo workspace,
    // in which case the artifact would otherwise land in the workspace-root
    // target dir (not `crate_dir/target`); forcing CARGO_TARGET_DIR makes the
    // built binary path deterministic regardless of the crate's layout.
    let target_dir = aish_home().join("build");
    std::fs::create_dir_all(&target_dir)?;
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .env("CARGO_HOME", &cargo_home)
        .env("CARGO_TARGET_DIR", &target_dir)
        .args(["build", "--release", "--locked", "--manifest-path"])
        .arg(&manifest_path)
        .output()
        .with_context(|| format!("building plugin `{name}`"))?;
    if !out.status.success() {
        return Err(anyhow!(format_cargo_build_error(name, &out.stderr)));
    }
    let bin = target_dir.join("release").join(name);
    if !bin.exists() {
        return Err(anyhow!("expected built binary at {}", bin.display()));
    }
    Ok(bin)
}

/// Read the plugin's manifest from the registry checkout.
pub fn read_manifest(registry_dir: &Path, name: &str) -> Result<Manifest> {
    let path = registry_dir.join(name).join("aish-plugin.toml");
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("reading manifest {}", path.display()))?;
    Manifest::from_toml(&raw).map_err(|e| anyhow!("invalid manifest {}: {e}", path.display()))
}

/// Full install pipeline: resolve registry -> read manifest -> build -> install.
pub async fn install_from_registry(source: &RegistrySource, name: &str) -> Result<PluginEntry> {
    let (dir, revision) = ensure_registry(source)?;
    let manifest = read_manifest(&dir, name)?;
    if manifest.name != name {
        return Err(anyhow!(
            "manifest name `{}` != requested `{name}`",
            manifest.name
        ));
    }
    let bin = match prebuilt::release_repo(source, &dir) {
        Some(release) => {
            match prebuilt::fetch_prebuilt(
                &release,
                &manifest.name,
                &manifest.version,
                prebuilt::HOST_TARGET,
            )
            .await
            {
                Ok(Some(path)) => path,
                Ok(None) => build_plugin(&dir, name)?,
                Err(e) => {
                    eprintln!("prebuilt fetch failed ({e}); falling back to cargo build");
                    build_plugin(&dir, name)?
                }
            }
        }
        None => build_plugin(&dir, name)?,
    };
    let source_str = match source {
        RegistrySource::Local(p) => p.display().to_string(),
        RegistrySource::Git { url } => url.clone(),
    };
    install_built(&manifest, &bin, &source_str, &revision)
}

/// Update an already-installed plugin: re-resolve the registry, rebuild, and
/// reinstall in place via the crash-safe swap. Errors if the plugin is not
/// currently installed. Returns `(old_entry, new_entry)` for diff reporting.
pub async fn update_from_registry(
    source: &RegistrySource,
    name: &str,
) -> Result<(PluginEntry, PluginEntry)> {
    let reg = InstalledRegistry::load(&plugins_toml())?;
    let old = reg
        .plugins
        .get(name)
        .ok_or_else(|| {
            anyhow!("plugin `{name}` is not installed — use `aish plugin install {name}`")
        })?
        .clone();
    let new = install_from_registry(source, name).await?;
    Ok((old, new))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    fn manifest() -> Manifest {
        Manifest::from_toml("name=\"demo\"\nversion=\"0.1.0\"\nabi=\"1\"\nsubcommands=[\"demo\"]\n")
            .unwrap()
    }

    #[test]
    #[serial]
    fn rejects_path_traversal_name() {
        let home = tempdir().unwrap();
        std::env::set_var("AISH_HOME", home.path());
        let m = Manifest::from_toml("name=\"../evil\"\nversion=\"0.1.0\"\nabi=\"1\"\n").unwrap();
        let src = tempdir().unwrap();
        let bin = src.path().join("b");
        std::fs::write(&bin, b"x").unwrap();
        let err = install_built(&m, &bin, "local", "r").unwrap_err();
        assert!(err.to_string().contains("invalid plugin name"));
        std::env::remove_var("AISH_HOME");
    }

    #[test]
    fn registry_source_parsing() {
        assert!(matches!(
            RegistrySource::parse("/opt/aish-plugins"),
            RegistrySource::Local(_)
        ));
        assert!(matches!(
            RegistrySource::parse("file:///tmp/x"),
            RegistrySource::Local(_)
        ));
        assert!(matches!(
            RegistrySource::parse("git@github.com:u/r.git"),
            RegistrySource::Git { .. }
        ));
        assert!(matches!(
            RegistrySource::parse("https://github.com/u/r"),
            RegistrySource::Git { .. }
        ));
    }

    #[test]
    fn ensure_local_registry_missing_errors() {
        let s = RegistrySource::Local(PathBuf::from("/no/such/registry/xyz"));
        assert!(ensure_registry(&s).is_err());
    }

    #[test]
    fn cargo_build_error_explains_missing_rust_target() {
        let err = format_cargo_build_error(
            "commit",
            b"error[E0463]: can't find crate for `std`\n= note: the `x86_64-unknown-linux-gnu` target may not be installed\n",
        );
        assert!(err.contains("rustup target add x86_64-unknown-linux-gnu"));
    }

    #[test]
    #[serial]
    fn install_built_records_and_hashes() {
        let home = tempdir().unwrap();
        std::env::set_var("AISH_HOME", home.path());
        let src = tempdir().unwrap();
        let bin = src.path().join("prebuilt");
        std::fs::write(&bin, b"#!/bin/sh\nexit 0\n").unwrap();

        let entry = install_built(&manifest(), &bin, "local", "rev1").unwrap();
        assert!(entry.path.exists());
        assert_eq!(entry.binary_sha256, sha256_file(&entry.path).unwrap());
        assert!(entry.enabled);

        let reg = InstalledRegistry::load(&plugins_toml()).unwrap();
        assert_eq!(reg.plugins["demo"].source, "local");
        verify_sha256(&entry.path, &entry.binary_sha256).unwrap();
        std::env::remove_var("AISH_HOME");
    }

    #[tokio::test]
    #[serial]
    async fn update_unknown_plugin_errors() {
        let home = tempdir().unwrap();
        std::env::set_var("AISH_HOME", home.path());
        let src = tempdir().unwrap();
        let err = update_from_registry(&RegistrySource::Local(src.path().into()), "nope")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not installed"));
        std::env::remove_var("AISH_HOME");
    }

    #[test]
    #[serial]
    fn tamper_check_fails_on_modified_binary() {
        let home = tempdir().unwrap();
        std::env::set_var("AISH_HOME", home.path());
        let src = tempdir().unwrap();
        let bin = src.path().join("prebuilt");
        std::fs::write(&bin, b"original").unwrap();
        let entry = install_built(&manifest(), &bin, "local", "rev1").unwrap();
        std::fs::write(&entry.path, b"tampered").unwrap();
        assert!(verify_sha256(&entry.path, &entry.binary_sha256).is_err());
        std::env::remove_var("AISH_HOME");
    }
}
