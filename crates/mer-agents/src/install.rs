//! Safe editing of the user's own files (`~/.claude/settings.json`, `~/.codex/config.toml`,
//! `~/.zshrc`) and quoting of the command that goes into them.

use std::fs::{self, OpenOptions, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Quotes `text` so a POSIX shell reads it back as exactly one word, whatever it contains
/// (spaces, quotes, `$`, backticks, `;`). Plain paths are returned unchanged.
#[must_use]
pub fn shell_quote(text: &str) -> String {
    let plain = !text.is_empty()
        && text.chars().all(|c| c.is_ascii_alphanumeric() || "_-./+:@%=,".contains(c));
    if plain { text.to_owned() } else { format!("'{}'", text.replace('\'', r"'\''")) }
}

/// The file `path` really is: a symlink (dotfile managers use them) is followed, so the link
/// stays a link and the file it points to is the one that changes.
fn real_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

/// Copies `path` to `<path>.bak-mer` unless that backup already exists, so the first backup
/// (the file as the user wrote it) is never overwritten by a later install.
/// Returns whether a backup was made. The copy keeps the original's permissions.
///
/// # Errors
/// Any i/o error from reading or copying.
pub fn backup_once(path: &Path) -> io::Result<bool> {
    let target = real_path(path);
    if !target.exists() {
        return Ok(false);
    }
    let backup = PathBuf::from(format!("{}.bak-mer", target.display()));
    if backup.exists() {
        return Ok(false);
    }
    fs::copy(&target, &backup)?;
    Ok(true)
}

/// Replaces `path` with `text` atomically (temporary file in the same directory, then rename).
/// An existing file keeps its permissions (a private `0600` file stays private); a new one is
/// created with `new_file_mode`. A symlink is followed, not replaced.
///
/// # Errors
/// Any i/o error; the original file is untouched if one happens before the rename.
pub fn replace_file(path: &Path, text: &str, new_file_mode: u32) -> io::Result<()> {
    let target = real_path(path);
    let mode =
        fs::metadata(&target).map_or(new_file_mode, |meta| meta.permissions().mode() & 0o7777);
    let temporary = PathBuf::from(format!("{}.tmp-mer", target.display()));
    let _ = fs::remove_file(&temporary);
    let written = (|| {
        let mut file =
            OpenOptions::new().write(true).create_new(true).mode(mode).open(&temporary)?;
        // `mode()` is filtered by the umask on creation; set the exact bits.
        file.set_permissions(Permissions::from_mode(mode))?;
        file.write_all(text.as_bytes())?;
        file.sync_all()
    })();
    if let Err(error) = written.and_then(|()| fs::rename(&temporary, &target)) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).expect("exists").permissions().mode() & 0o777
    }

    #[test]
    fn plain_paths_are_not_quoted() {
        assert_eq!(shell_quote("/home/u/.local/bin/mer"), "/home/u/.local/bin/mer");
        assert_eq!(shell_quote("mer"), "mer");
    }

    #[test]
    fn a_quoted_path_reads_back_as_one_word_in_a_real_shell() {
        for hostile in [
            "/opt/my apps/mer",
            "/tmp/it's/mer",
            "/tmp/$(touch pwned)/mer",
            "/tmp/`id`/mer",
            "/tmp/a;b&c|d/mer",
            "/tmp/\"quoted\"/mer",
            "",
        ] {
            let output = Command::new("sh")
                .arg("-c")
                .arg(format!("printf %s {}", shell_quote(hostile)))
                .output()
                .expect("runs sh");
            assert_eq!(String::from_utf8_lossy(&output.stdout), hostile, "for {hostile:?}");
        }
    }

    #[test]
    fn an_existing_private_file_stays_private_and_a_new_one_gets_the_given_mode() {
        let dir = tempfile::tempdir().expect("temp dir");
        let existing = dir.path().join("settings.json");
        fs::write(&existing, "old").expect("writes");
        fs::set_permissions(&existing, Permissions::from_mode(0o600)).expect("chmod");
        replace_file(&existing, "new", 0o644).expect("replaces");
        assert_eq!(fs::read_to_string(&existing).expect("reads"), "new");
        assert_eq!(mode(&existing), 0o600);

        let created = dir.path().join("fresh.toml");
        replace_file(&created, "x", 0o600).expect("creates");
        assert_eq!(mode(&created), 0o600);
        assert!(!dir.path().join("fresh.toml.tmp-mer").exists(), "no temporary file is left");
    }

    #[test]
    fn a_symlinked_file_stays_a_symlink_and_its_target_changes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let real = dir.path().join("dotfiles-zshrc");
        let link = dir.path().join(".zshrc");
        fs::write(&real, "export A=1\n").expect("writes");
        std::os::unix::fs::symlink(&real, &link).expect("links");

        replace_file(&link, "export A=2\n", 0o644).expect("replaces");
        assert!(fs::symlink_metadata(&link).expect("exists").file_type().is_symlink());
        assert_eq!(fs::read_to_string(&real).expect("reads"), "export A=2\n");
    }

    #[test]
    fn the_first_backup_is_kept_and_keeps_its_permissions() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("config.toml");
        assert!(!backup_once(&file).expect("nothing to back up"));

        fs::write(&file, "original").expect("writes");
        fs::set_permissions(&file, Permissions::from_mode(0o600)).expect("chmod");
        assert!(backup_once(&file).expect("backs up"));
        let backup = dir.path().join("config.toml.bak-mer");
        assert_eq!(fs::read_to_string(&backup).expect("reads"), "original");
        assert_eq!(mode(&backup), 0o600);

        fs::write(&file, "patched by mer").expect("writes");
        assert!(!backup_once(&file).expect("keeps the first one"));
        assert_eq!(fs::read_to_string(&backup).expect("reads"), "original");
    }
}
