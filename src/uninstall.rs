// SPDX-License-Identifier: MIT
//! Core logic for `aish uninstall`: lexical purge-path safety validation
//! and directory sizing. The data dir itself comes from [`crate::paths`];
//! `commands::uninstall` re-validates the symlink-resolved path and owns the
//! confirmation prompt and CLI glue.

use std::path::{Component, Path};

/// Guard before recursive delete: reject empty, root, home itself, `..`, or
/// any path that is not strictly inside `home`. Returns the validated path.
///
/// The checks are lexical. `starts_with` compares components without
/// resolving them, so `/home/u/../../etc` would count as inside `/home/u`:
/// hence no `..` at all. `.` needs no check, since `Path` skips it when
/// comparing (`/home/u/.` is home itself). Symlinks are left to the caller,
/// which has to look at the filesystem to resolve them.
pub fn validate_purge_path<'a>(dir: &'a Path, home: &Path) -> Result<&'a Path, String> {
    if dir.as_os_str().is_empty() {
        return Err("refusing to purge: empty path".into());
    }
    if !dir.is_absolute() {
        return Err(format!(
            "refusing to purge relative path '{}'",
            dir.display()
        ));
    }
    if dir.components().any(|c| c == Component::ParentDir) {
        return Err(format!(
            "refusing to purge '{}': path contains '..'",
            dir.display()
        ));
    }
    if dir == Path::new("/") || dir == home {
        return Err(format!(
            "refusing to purge '{}': not a dedicated data dir",
            dir.display()
        ));
    }
    if !dir.starts_with(home) {
        return Err(format!(
            "refusing to purge '{}': outside home directory '{}'",
            dir.display(),
            home.display()
        ));
    }
    Ok(dir)
}

/// Total size in bytes of all files under `dir` (0 if it doesn't exist).
pub fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| {
            let path = e.path();
            match e.metadata() {
                Ok(m) if m.is_dir() => dir_size(&path),
                Ok(m) => m.len(),
                Err(_) => 0,
            }
        })
        .sum()
}

/// Human-readable size: `512 B`, `1.5 KiB`, `3.4 MiB`.
pub fn human_size(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KIB {
        format!("{bytes} B")
    } else if b < KIB * KIB {
        format!("{:.1} KiB", b / KIB)
    } else {
        format!("{:.1} MiB", b / (KIB * KIB))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn purge_rejects_dangerous_paths() {
        let home = Path::new("/home/u");
        assert!(validate_purge_path(Path::new(""), home).is_err());
        assert!(validate_purge_path(Path::new("/"), home).is_err());
        assert!(validate_purge_path(home, home).is_err());
        // Outside home: a typo'd $AISH_HOME must not nuke system dirs.
        assert!(validate_purge_path(Path::new("/etc"), home).is_err());
        assert!(validate_purge_path(Path::new("/srv/aish-data"), home).is_err());
        // Relative paths are ambiguous — reject.
        assert!(validate_purge_path(Path::new(".aish"), home).is_err());
    }

    /// An absolute home on every platform (`/home/u` has no drive letter on
    /// Windows, so it would be refused as relative). It is never created.
    fn absolute_home() -> PathBuf {
        std::env::temp_dir().join("u")
    }

    #[test]
    fn purge_rejects_parent_dir_components() {
        let home = absolute_home();
        let refused = |dir: PathBuf| {
            let err = validate_purge_path(&dir, &home).unwrap_err();
            assert!(
                err.contains("path contains '..'"),
                "{}: {err}",
                dir.display()
            );
        };
        // Lexically inside home, really outside it.
        refused(home.join("../../etc"));
        refused(home.join(".."));
        refused(home.join(".aish/.."));
        // Even where it would resolve inside home: no guessing.
        refused(home.join("../u/.aish"));
    }

    #[test]
    fn purge_sees_through_cur_dir_components() {
        let home = absolute_home();
        // `.` cannot pass home itself off as a subdir...
        for dir in [home.join("."), home.parent().unwrap().join("./u/")] {
            let err = validate_purge_path(&dir, &home).unwrap_err();
            assert!(
                err.contains("not a dedicated data dir"),
                "{}: {err}",
                dir.display()
            );
        }
        // ...and names the same dir it would without it.
        assert!(validate_purge_path(&home.join("./.aish"), &home).is_ok());
    }

    #[test]
    fn purge_accepts_dirs_strictly_inside_home() {
        let home = Path::new("/home/u");
        assert!(validate_purge_path(Path::new("/home/u/.aish"), home).is_ok());
        assert!(validate_purge_path(Path::new("/home/u/custom/aish"), home).is_ok());
    }

    #[test]
    fn dir_size_sums_files_recursively_and_handles_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a"), [0u8; 100]).unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/b"), [0u8; 50]).unwrap();
        assert_eq!(dir_size(dir.path()), 150);
        assert_eq!(dir_size(Path::new("/nonexistent/nowhere")), 0);
    }

    #[test]
    fn human_size_picks_sensible_units() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.5 KiB");
        assert_eq!(human_size(3 * 1024 * 1024 + 400 * 1024), "3.4 MiB");
    }
}
