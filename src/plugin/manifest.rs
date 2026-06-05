// SPDX-License-Identifier: MIT
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub abi: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub subcommands: Vec<String>,
    #[serde(default)]
    pub permissions: Permissions,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Permissions {
    #[serde(default)]
    pub model: bool,
    #[serde(default)]
    pub audit: bool,
}

impl Manifest {
    pub fn from_toml(s: &str) -> Result<Manifest, toml::de::Error> {
        toml::from_str(s)
    }
    /// Major component of the declared `abi` string ("1" or "1.2" -> 1).
    pub fn abi_major(&self) -> Option<u32> {
        self.abi.split('.').next()?.parse().ok()
    }
}

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One installed plugin's recorded state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginEntry {
    pub version: String,
    pub abi: String,
    pub enabled: bool,
    pub path: PathBuf,
    #[serde(default)]
    pub subcommands: Vec<String>,
    pub source: String,
    pub revision: String,
    pub binary_sha256: String,
}

/// The whole `~/.aish/plugins.toml`: name -> entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstalledRegistry {
    #[serde(flatten, default)]
    pub plugins: BTreeMap<String, PluginEntry>,
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("io error on {0}: {1}")]
    Io(PathBuf, String),
    #[error("invalid plugins.toml: {0}")]
    Parse(String),
    #[error("serialize plugins.toml: {0}")]
    Serialize(String),
    #[error("subcommand `{sub}` already provided by enabled plugin `{owner}`")]
    SubcommandConflict { sub: String, owner: String },
}

impl InstalledRegistry {
    pub fn load(path: &Path) -> Result<Self, RegistryError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path)
            .map_err(|e| RegistryError::Io(path.to_path_buf(), e.to_string()))?;
        toml::from_str(&raw).map_err(|e| RegistryError::Parse(e.to_string()))
    }

    pub fn save(&self, path: &Path) -> Result<(), RegistryError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| RegistryError::Io(parent.to_path_buf(), e.to_string()))?;
        }
        let s =
            toml::to_string_pretty(self).map_err(|e| RegistryError::Serialize(e.to_string()))?;
        std::fs::write(path, s).map_err(|e| RegistryError::Io(path.to_path_buf(), e.to_string()))
    }

    /// Find the enabled plugin (name, entry) that provides `subcommand`.
    pub fn find_by_subcommand(&self, subcommand: &str) -> Option<(&str, &PluginEntry)> {
        self.plugins
            .iter()
            .filter(|(_, e)| e.enabled)
            .find(|(_, e)| e.subcommands.iter().any(|s| s == subcommand))
            .map(|(n, e)| (n.as_str(), e))
    }

    /// Reject if inserting/enabling `name` with `subcommands` would make two
    /// *enabled* plugins claim the same subcommand.
    pub fn check_conflicts(&self, name: &str, subcommands: &[String]) -> Result<(), RegistryError> {
        for sub in subcommands {
            if let Some((owner, _)) = self.find_by_subcommand(sub) {
                if owner != name {
                    return Err(RegistryError::SubcommandConflict {
                        sub: sub.clone(),
                        owner: owner.to_string(),
                    });
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const SAMPLE: &str = r#"
name = "commit"
version = "0.1.0"
abi = "1"
description = "AI commit message"
subcommands = ["commit"]
[permissions]
model = true
audit = true
"#;

    #[test]
    fn parses_full_manifest() {
        let m = Manifest::from_toml(SAMPLE).unwrap();
        assert_eq!(m.name, "commit");
        assert_eq!(m.subcommands, vec!["commit".to_string()]);
        assert!(m.permissions.model);
        assert!(m.permissions.audit);
        assert_eq!(m.abi_major(), Some(1));
    }

    #[test]
    fn permissions_default_to_false() {
        let m = Manifest::from_toml("name=\"x\"\nversion=\"0.1.0\"\nabi=\"1\"\n").unwrap();
        assert!(!m.permissions.model);
        assert!(!m.permissions.audit);
        assert!(m.subcommands.is_empty());
    }

    #[test]
    fn abi_major_parses_dotted() {
        let m = Manifest::from_toml("name=\"x\"\nversion=\"0.1.0\"\nabi=\"2.5\"\n").unwrap();
        assert_eq!(m.abi_major(), Some(2));
    }

    fn entry(enabled: bool, subs: &[&str]) -> PluginEntry {
        PluginEntry {
            version: "0.1.0".into(),
            abi: "1".into(),
            enabled,
            path: PathBuf::from("/x/bin"),
            subcommands: subs.iter().map(|s| s.to_string()).collect(),
            source: "local".into(),
            revision: "0".into(),
            binary_sha256: "deadbeef".into(),
        }
    }

    #[test]
    fn registry_roundtrips_through_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins.toml");
        let mut reg = InstalledRegistry::default();
        reg.plugins
            .insert("commit".into(), entry(true, &["commit"]));
        reg.save(&path).unwrap();
        let loaded = InstalledRegistry::load(&path).unwrap();
        assert_eq!(
            loaded.plugins["commit"].subcommands,
            vec!["commit".to_string()]
        );
        assert!(loaded.plugins["commit"].enabled);
    }

    #[test]
    fn missing_file_loads_empty() {
        let dir = tempdir().unwrap();
        let reg = InstalledRegistry::load(&dir.path().join("nope.toml")).unwrap();
        assert!(reg.plugins.is_empty());
    }

    #[test]
    fn find_by_subcommand_ignores_disabled() {
        let mut reg = InstalledRegistry::default();
        reg.plugins
            .insert("commit".into(), entry(false, &["commit"]));
        assert!(reg.find_by_subcommand("commit").is_none());
        reg.plugins.get_mut("commit").unwrap().enabled = true;
        assert_eq!(reg.find_by_subcommand("commit").unwrap().0, "commit");
    }

    #[test]
    fn conflict_detected_between_enabled_plugins() {
        let mut reg = InstalledRegistry::default();
        reg.plugins
            .insert("commit".into(), entry(true, &["commit"]));
        let err = reg
            .check_conflicts("other", &["commit".to_string()])
            .unwrap_err();
        assert!(matches!(err, RegistryError::SubcommandConflict { .. }));
        // Re-checking the same plugin name is allowed (idempotent install).
        assert!(reg
            .check_conflicts("commit", &["commit".to_string()])
            .is_ok());
    }
}
