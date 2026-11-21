//! Files that hold terminal output may contain secrets, so they are private to the user:
//! directories are `0700` and files `0600`, whatever the umask or the home directory says.
//!
//! The mode is set when the file is created (no window where it is readable) and, for things
//! that already exist from an older version, tightened afterwards on a best-effort basis.

use std::fs::{self, DirBuilder, OpenOptions, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

/// Creates `dir` (and its parents) as `0700` and tightens it if it already existed.
pub(crate) fn create_dir(dir: &Path) -> io::Result<()> {
    if dir.as_os_str().is_empty() {
        return Ok(());
    }
    DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    // Best effort: a directory we do not own cannot be changed, and the files inside are
    // private on their own.
    let _ = fs::set_permissions(dir, Permissions::from_mode(0o700));
    Ok(())
}

/// Makes sure `path` exists as an (empty if new) `0600` file, so `SQLite` and the `-wal` and
/// `-shm` files it derives from it start private.
pub(crate) fn touch(path: &Path) -> io::Result<()> {
    let file =
        OpenOptions::new().write(true).create(true).truncate(false).mode(0o600).open(path)?;
    let _ = file.set_permissions(Permissions::from_mode(0o600));
    Ok(())
}

/// Writes `bytes` to `path` as a `0600` file, replacing what was there.
pub(crate) fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file =
        OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    let _ = file.set_permissions(Permissions::from_mode(0o600));
    file.write_all(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).expect("exists").permissions().mode() & 0o777
    }

    #[test]
    fn new_directories_and_files_are_private() {
        let root = tempfile::tempdir().expect("temp dir");
        let dir = root.path().join("a/b");
        create_dir(&dir).expect("creates");
        assert_eq!(mode(&dir), 0o700);

        let file = dir.join("x.bin");
        write(&file, b"secret").expect("writes");
        assert_eq!(mode(&file), 0o600);
        assert_eq!(fs::read(&file).expect("reads"), b"secret");
    }

    #[test]
    fn loose_permissions_from_an_older_version_are_tightened() {
        let root = tempfile::tempdir().expect("temp dir");
        let dir = root.path().join("state");
        fs::create_dir(&dir).expect("creates");
        fs::set_permissions(&dir, Permissions::from_mode(0o755)).expect("loosens");
        let file = dir.join("old.db");
        fs::write(&file, b"data").expect("writes");
        fs::set_permissions(&file, Permissions::from_mode(0o644)).expect("loosens");

        create_dir(&dir).expect("tightens");
        touch(&file).expect("tightens");
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&file), 0o600);
        assert_eq!(fs::read(&file).expect("reads"), b"data", "touch must not erase the file");
    }

    #[test]
    fn an_empty_path_is_ignored() {
        create_dir(Path::new("")).expect("a relative file name has an empty parent");
    }
}
