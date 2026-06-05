// SPDX-License-Identifier: MIT
pub mod host;
pub mod install;
pub mod manifest;
pub mod protocol;
pub mod services;

use std::path::PathBuf;

/// Root for installed plugins + state. `~/.aish`, overridable via `$AISH_HOME`.
pub fn aish_home() -> PathBuf {
    if let Ok(p) = std::env::var("AISH_HOME") {
        return PathBuf::from(p);
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".aish")
}
