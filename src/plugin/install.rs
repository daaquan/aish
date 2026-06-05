// SPDX-License-Identifier: MIT
use crate::plugin::aish_home;
use crate::plugin::manifest::{InstalledRegistry, Manifest, PluginEntry};
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
        .open(lock_path())?;
    lock.lock_exclusive()?;
    let result = (|| {
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
        std::fs::copy(built_binary, &staged_bin)
            .with_context(|| format!("copying {} -> {}", built_binary.display(), staged_bin.display()))?;
        make_executable(&staged_bin)?;
        std::fs::write(staging.join("aish-plugin.toml"), toml::to_string_pretty(manifest)?)?;

        let sha = sha256_file(&staged_bin)?;

        // Atomic-ish replace of the install dir.
        if final_dir.exists() {
            std::fs::remove_dir_all(&final_dir)?;
        }
        std::fs::rename(&staging, &final_dir)
            .with_context(|| format!("renaming staging into {}", final_dir.display()))?;

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
        return Err(anyhow!("plugin binary at {} failed integrity check", path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn manifest() -> Manifest {
        Manifest::from_toml("name=\"demo\"\nversion=\"0.1.0\"\nabi=\"1\"\nsubcommands=[\"demo\"]\n").unwrap()
    }

    #[test]
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

    #[test]
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
