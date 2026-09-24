// SPDX-License-Identifier: MIT
//! Where aish keeps its files. The config, the response cache and the audit
//! log all live in one data dir, resolved here and nowhere else, so that
//! `aish uninstall --purge` removes exactly what the other commands wrote.

use std::path::{Path, PathBuf};

/// The data dir: `$AISH_HOME` if it is an absolute path, else `<home>/.aish`.
/// A relative value (blank, a quoted `~/...` the shell never expanded, a
/// stray leading space) is ignored rather than resolved against the working
/// directory, which is usually a git checkout: every command writes here, so
/// API keys and cached responses would land in it as untracked files, and
/// `uninstall --purge` refuses to delete a relative path anyway.
pub fn data_dir(home: &Path) -> PathBuf {
    match std::env::var_os("AISH_HOME").map(PathBuf::from) {
        Some(p) if p.is_absolute() => p,
        _ => home.join(".aish"),
    }
}

/// [`data_dir`] for the current user, under the working directory when there
/// is no home directory.
pub fn default_data_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    data_dir(&home)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial(aish_home)]
    fn data_dir_defaults_to_dot_aish_under_home() {
        std::env::remove_var("AISH_HOME");
        assert_eq!(
            data_dir(Path::new("/home/u")),
            PathBuf::from("/home/u/.aish")
        );
    }

    /// An absolute path on every platform (`/srv/...` has no drive letter on
    /// Windows). It is never created.
    fn absolute_data_dir() -> PathBuf {
        std::env::temp_dir().join("aish-data")
    }

    #[test]
    #[serial(aish_home)]
    fn data_dir_honors_aish_home_env() {
        let data = absolute_data_dir();
        std::env::set_var("AISH_HOME", &data);
        let got = data_dir(Path::new("/home/u"));
        std::env::remove_var("AISH_HOME");
        assert_eq!(got, data);
    }

    #[test]
    #[serial(aish_home)]
    fn data_dir_ignores_blank_or_relative_aish_home() {
        let padded = format!(" {}", absolute_data_dir().display());
        for value in ["", "  ", "~/.config/aish", "rel/dir", padded.as_str()] {
            std::env::set_var("AISH_HOME", value);
            let got = data_dir(Path::new("/home/u"));
            std::env::remove_var("AISH_HOME");
            assert_eq!(got, PathBuf::from("/home/u/.aish"), "AISH_HOME={value:?}");
        }
    }

    /// Everything aish writes must land in the dir `uninstall --purge`
    /// deletes; `$AISH_CONFIG` only moves the config file.
    #[test]
    #[serial(aish_home)]
    fn config_cache_and_audit_log_live_in_the_data_dir() {
        let data = absolute_data_dir();
        std::env::set_var("AISH_HOME", &data);
        std::env::remove_var("AISH_CONFIG");
        let config = crate::config::Config::default_path();
        let cache = crate::cache::cache_dir();
        let audit = crate::audit::log_path();
        std::env::set_var("AISH_CONFIG", "/elsewhere/config.yaml");
        let overridden = crate::config::Config::default_path();
        std::env::remove_var("AISH_CONFIG");
        std::env::remove_var("AISH_HOME");

        assert_eq!(config, data.join("config.yaml"));
        assert_eq!(cache, data.join("cache"));
        assert_eq!(audit, data.join("audit.log"));
        assert_eq!(overridden, PathBuf::from("/elsewhere/config.yaml"));
    }
}
