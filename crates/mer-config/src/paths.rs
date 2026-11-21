//! Well-known configuration locations.

use std::path::PathBuf;

fn resolve(xdg: Option<std::ffi::OsString>, home: Option<std::ffi::OsString>) -> Option<PathBuf> {
    if let Some(x) = xdg {
        let p = PathBuf::from(x);
        if p.is_absolute() {
            return Some(p.join("mer"));
        }
    }
    let h = PathBuf::from(home?);
    if h.as_os_str().is_empty() {
        return None;
    }
    Some(h.join(".config").join("mer"))
}

/// `$XDG_CONFIG_HOME/mer` (when absolute), else `$HOME/.config/mer`.
pub fn config_dir() -> Option<PathBuf> {
    resolve(std::env::var_os("XDG_CONFIG_HOME"), std::env::var_os("HOME"))
}

/// Path of `config.toml`.
pub fn config_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config.toml"))
}

/// Directory holding user theme files.
pub fn themes_dir() -> Option<PathBuf> {
    config_dir().map(|d| d.join("themes"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn xdg_absolute_wins() {
        let r = resolve(Some(OsString::from("/x")), Some(OsString::from("/h")));
        assert_eq!(r, Some(PathBuf::from("/x/mer")));
    }

    #[test]
    fn relative_xdg_ignored() {
        let r = resolve(Some(OsString::from("rel")), Some(OsString::from("/h")));
        assert_eq!(r, Some(PathBuf::from("/h/.config/mer")));
    }

    #[test]
    fn no_home() {
        assert_eq!(resolve(None, None), None);
    }
}
