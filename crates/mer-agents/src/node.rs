//! Node versions installed with nvm, for the node chip.
//!
//! nvm keeps one folder per version under `<nvm dir>/versions/node`, so listing them is a
//! directory read: no `nvm` (a shell function that takes a while to load) and no `node -v`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::{command_input, shell_quote};

/// `v24.15.0` as numbers, for ordering. `None` for anything that is not a plain version.
fn version_key(name: &str) -> Option<(u64, u64, u64)> {
    let mut parts = name.strip_prefix('v')?.split('.');
    let key =
        (parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?);
    parts.next().is_none().then_some(key)
}

/// The node version a project asks for, as written in its `.nvmrc` or `.node-version`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeNeed {
    /// The text of the file: `v18.20.4`, `18`, `lts/*`…
    pub spec: String,
    /// Which file said it.
    pub file: &'static str,
}

const VERSION_FILES: [&str; 2] = [".nvmrc", ".node-version"];

/// The longest spec we keep, in characters: a real one is a few.
const SPEC_MAX: usize = 64;

/// The first meaningful line of a version file: no comments (`# …`), no blanks.
pub fn parse_version_file(text: &str) -> Option<String> {
    let line = text
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .find(|line| !line.is_empty())?;
    (line.chars().count() <= SPEC_MAX && !line.chars().any(char::is_control))
        .then(|| line.to_owned())
}

/// The version the project at `start` asks for: the nearest `.nvmrc` (else `.node-version`) in
/// `start` or any folder above it. A few `stat`s and one tiny read; call it off the interface thread.
pub fn required_version(start: &Path) -> Option<NodeNeed> {
    start.ancestors().find_map(|dir| {
        VERSION_FILES.into_iter().find_map(|file| {
            let spec = parse_version_file(&fs::read_to_string(dir.join(file)).ok()?)?;
            Some(NodeNeed { spec, file })
        })
    })
}

/// `18`, `v18.20`, `20.11.0` as the numbers they name (up to three). `None` for aliases such as
/// `lts/*`, `node` or `system`, which only nvm itself can resolve.
fn spec_numbers(spec: &str) -> Option<Vec<u64>> {
    let parts: Vec<u64> = spec
        .strip_prefix('v')
        .unwrap_or(spec)
        .split('.')
        .map(|part| part.parse().ok())
        .collect::<Option<_>>()?;
    (1..=3).contains(&parts.len()).then_some(parts)
}

/// Is `spec` something other than a version number (`lts/*`, `node`…)?
pub fn is_alias(spec: &str) -> bool {
    spec_numbers(spec).is_none()
}

/// The installed version a spec means: the exact one, or for a partial spec (`18`, `18.20`) the
/// newest installed one that starts with it. `None` for aliases and for versions not installed.
pub fn resolve(spec: &str, installed: &[String]) -> Option<String> {
    let wanted = spec_numbers(spec)?;
    installed
        .iter()
        .filter_map(|name| Some((version_key(name)?, name)))
        .filter(|((major, minor, patch), _)| [*major, *minor, *patch].starts_with(&wanted))
        .max_by_key(|(key, _)| *key)
        .map(|(_, name)| name.clone())
}

/// Keeps the plain versions and orders them newest first.
pub fn sort_versions(names: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut versions: Vec<_> =
        names.into_iter().filter_map(|name| Some((version_key(&name)?, name))).collect();
    versions.sort_by_key(|(key, _)| std::cmp::Reverse(*key));
    versions.into_iter().map(|(_, name)| name).collect()
}

/// Where nvm lives: what the shell reported, else `$NVM_DIR`, else `~/.nvm`.
pub fn nvm_dir(reported: Option<&Path>) -> Option<PathBuf> {
    reported
        .map(Path::to_owned)
        .or_else(|| std::env::var_os("NVM_DIR").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".nvm")))
}

/// Installed versions, newest first. Empty when there is no nvm.
pub fn installed_versions(nvm_dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(nvm_dir.join("versions/node")) else { return Vec::new() };
    sort_versions(entries.flatten().filter_map(|entry| entry.file_name().into_string().ok()))
}

/// The keystrokes that switch to `version` at a shell prompt.
pub fn use_input(version: &str) -> Option<String> {
    command_input(&format!("nvm use {}", shell_quote(version)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(names: &[&str]) -> Vec<String> {
        sort_versions(names.iter().map(|name| (*name).to_owned()))
    }

    #[test]
    fn versions_sort_by_number_not_by_text() {
        assert_eq!(
            sorted(&["v9.11.2", "v24.15.0", "v18.20.4", "v24.9.1"]),
            ["v24.15.0", "v24.9.1", "v18.20.4", "v9.11.2"]
        );
    }

    #[test]
    fn anything_that_is_not_a_version_is_dropped() {
        assert_eq!(
            sorted(&["v20.1.0", "current", ".DS_Store", "v20", "v20.1.0.1", "20.1.0"]),
            ["v20.1.0"]
        );
    }

    #[test]
    fn versions_are_read_from_the_nvm_folder() {
        let root = tempfile::tempdir().unwrap();
        for name in ["v18.20.4", "v24.15.0", "notes"] {
            fs::create_dir_all(root.path().join("versions/node").join(name)).unwrap();
        }
        assert_eq!(installed_versions(root.path()), ["v24.15.0", "v18.20.4"]);
        assert!(installed_versions(&root.path().join("missing")).is_empty());
    }

    #[test]
    fn a_version_file_gives_its_first_meaningful_line() {
        assert_eq!(parse_version_file("v18.20.4\n").as_deref(), Some("v18.20.4"));
        assert_eq!(parse_version_file("# pinned\n\n  20 # LTS\n22").as_deref(), Some("20"));
        assert_eq!(parse_version_file("lts/*").as_deref(), Some("lts/*"));
        assert_eq!(parse_version_file("  \n# nothing\n"), None);
        assert_eq!(parse_version_file(&"9".repeat(200)), None);
    }

    #[test]
    fn the_nearest_version_file_wins_and_nvmrc_goes_first() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(".node-version"), "16\n").unwrap();
        let app = root.path().join("apps/web");
        fs::create_dir_all(&app).unwrap();
        // Only the outer file exists: it applies to the subfolder.
        assert_eq!(
            required_version(&app),
            Some(NodeNeed { spec: "16".into(), file: ".node-version" })
        );
        // A closer file beats it, and `.nvmrc` beats `.node-version` in the same folder.
        fs::write(app.join(".nvmrc"), "v18.20.4\n").unwrap();
        fs::write(app.join(".node-version"), "17\n").unwrap();
        assert_eq!(
            required_version(&app),
            Some(NodeNeed { spec: "v18.20.4".into(), file: ".nvmrc" })
        );
        // A folder with no file anywhere above it asks for nothing.
        let bare = tempfile::tempdir().unwrap();
        assert_eq!(required_version(bare.path()), None);
    }

    fn installed() -> Vec<String> {
        sorted(&["v18.20.4", "v18.19.0", "v20.11.0", "v24.15.0"])
    }

    #[test]
    fn exact_and_partial_specs_resolve_to_installed_versions() {
        assert_eq!(resolve("v18.20.4", &installed()).as_deref(), Some("v18.20.4"));
        assert_eq!(resolve("20.11.0", &installed()).as_deref(), Some("v20.11.0"));
        assert_eq!(resolve("18", &installed()).as_deref(), Some("v18.20.4"), "newest 18.x");
        assert_eq!(resolve("v18.19", &installed()).as_deref(), Some("v18.19.0"));
        assert_eq!(resolve("1", &installed()), None, "1 is not 18");
    }

    #[test]
    fn missing_versions_and_aliases_do_not_resolve() {
        assert_eq!(resolve("22", &installed()), None);
        assert_eq!(resolve("18.5.0", &installed()), None);
        for alias in ["lts/*", "lts/iron", "node", "stable", "system", "v", ""] {
            assert_eq!(resolve(alias, &installed()), None, "{alias}");
            assert!(is_alias(alias), "{alias}");
        }
        assert!(!is_alias("22"));
        assert!(!is_alias("v1.2.3"));
        assert!(is_alias("1.2.3.4"));
    }

    #[test]
    fn using_a_version_types_nvm_use() {
        assert_eq!(use_input("v24.15.0").as_deref(), Some("\u{15}nvm use v24.15.0\r"));
    }

    #[test]
    fn the_reported_nvm_dir_wins() {
        assert_eq!(nvm_dir(Some(Path::new("/opt/nvm"))), Some(PathBuf::from("/opt/nvm")));
    }
}
