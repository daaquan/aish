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

#[cfg(test)]
mod tests {
    use super::*;

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
}
