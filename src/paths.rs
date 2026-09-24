// SPDX-License-Identifier: MIT
//! Where aish keeps its files, and how. The config, the response cache and the
//! audit log all live in one data dir, resolved here and nowhere else, so that
//! `aish uninstall --purge` removes exactly what the other commands wrote. It
//! holds API keys and provider responses, so the helpers here create the dir,
//! and the files aish writes to it, owner-only on unix.

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

/// Create `dir` and any missing parents, owner-only (`0700`) on unix: the data
/// dir holds API keys and provider responses, which can contain the user's
/// code. Only dirs created here get that mode. One that already exists is
/// left as it is, since a `$AISH_HOME` may be shared on purpose.
pub fn create_dir_owner_only(dir: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
}

/// `OpenOptions` for writing to a file that is created owner-only (`0600`) on
/// unix, never at the caller's umask; the caller picks how it is created and
/// written (truncate, append, `create_new`). The mode only applies on
/// creation: an existing file keeps its own.
pub fn owner_only_options() -> std::fs::OpenOptions {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts
}

/// Write `contents` to `path`, restricting it to the owner (`0600`) on unix
/// since the file may hold plaintext API keys (or, for a cache entry, a
/// provider response). Missing parent dirs are created owner-only too (see
/// [`create_dir_owner_only`]).
///
/// The file is owner-only *before* anything is written to it (see
/// [`open_owner_only`]): writing first and tightening second leaves a window
/// where the key is readable by other local users.
pub fn write_owner_only(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        create_dir_owner_only(parent)?;
    }
    open_owner_only(path)?.write_all(contents.as_ref())
}

/// Open `path` truncated for writing, already restricted to `0600` on unix.
///
/// A new file is 0600 from the start ([`owner_only_options`]). That only
/// applies on creation, so an existing, looser file is tightened through the
/// handle before the caller writes anything; if that fails (e.g. the file
/// belongs to another user), nothing secret has been written. A descriptor
/// another user opened while the old file was still readable keeps working —
/// a mode change never revokes an open descriptor.
fn open_owner_only(path: &Path) -> std::io::Result<std::fs::File> {
    let f = owner_only_options()
        .create(true)
        .truncate(true)
        .open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(f)
}

/// Write `contents` to a file that must not exist yet, owner-only (`0600`) on
/// unix like [`write_owner_only`]. Fails with `AlreadyExists` rather than touch
/// a file (or follow a symlink) already at `path`.
///
/// `create_new` checks and creates in one step, so a file that appears between
/// a caller's own existence check and the write is never clobbered. The file
/// is always new, so [`owner_only_options`] alone keeps it owner-only from the
/// start. Parent directories are not created.
pub fn write_new_owner_only(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    use std::io::Write;
    owner_only_options()
        .create_new(true)
        .open(path)?
        .write_all(contents.as_ref())
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

    /// Checks group/other bits only, so the result does not depend on the
    /// umask the tests run under.
    #[cfg(unix)]
    #[test]
    fn creates_missing_dirs_owner_only_and_leaves_existing_ones_alone() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let root = tempfile::tempdir().unwrap();
        let shared = root.path().join("shared");
        std::fs::create_dir(&shared).unwrap();
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o755)).unwrap();

        let data = shared.join("data");
        let cache = data.join("cache");
        create_dir_owner_only(&cache).unwrap();

        for dir in [&data, &cache] {
            let m = mode(dir);
            assert_eq!(m & 0o077, 0, "{} created at {m:o}", dir.display());
        }
        assert_eq!(mode(&shared), 0o755, "existing dir must keep its mode");
        // Already there: nothing to do, and still no chmod.
        create_dir_owner_only(&shared).unwrap();
        assert_eq!(mode(&shared), 0o755);
    }

    /// A config file may hold a plaintext API key, so it must never be
    /// readable by anyone but the owner — not even for the instant between
    /// opening it and writing the key into it.
    #[cfg(unix)]
    #[test]
    fn write_owner_only_never_exposes_contents_to_other_users() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let dir = tempfile::tempdir().unwrap();

        // Fresh file: 0600, in a fresh dir with no group/other bits.
        let fresh = dir.path().join("nested").join("config.yaml");
        write_owner_only(&fresh, "providers:\n  openai: { api_key: sk-secret }\n").unwrap();
        let m = mode(&fresh);
        assert_eq!(m, 0o600, "fresh config left at {m:o}");
        let m = mode(fresh.parent().unwrap());
        assert_eq!(m & 0o077, 0, "fresh data dir created at {m:o}");

        // Pre-existing world-readable file (chmod, not umask-dependent): it is
        // already 0600 once the handle the key is written through exists,
        // i.e. before any byte of the new contents lands in it.
        let existing = dir.path().join("loose.yaml");
        std::fs::write(&existing, "old").unwrap();
        std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(mode(&existing), 0o644, "fixture must start world-readable");
        drop(open_owner_only(&existing).unwrap());
        let m = mode(&existing);
        assert_eq!(m, 0o600, "existing config at {m:o} before the write");

        let body = "providers: {}\n";
        write_owner_only(&existing, body).unwrap();
        let m = mode(&existing);
        assert_eq!(m, 0o600, "existing config left at {m:o}");
        assert_eq!(std::fs::read_to_string(&existing).unwrap(), body);
    }
}
