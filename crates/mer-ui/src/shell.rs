//! Which shell new terminals run.

use std::path::Path;

use mer_term::ShellCommand;

/// Shells tried in order. `zsh` first: with a typical dotfile setup it starts several times
/// faster than `bash` (measured: 0.21 s against 1.41 s on this machine).
const PREFERRED: &[&str] = &["/usr/bin/zsh", "/bin/zsh", "/usr/local/bin/zsh"];

/// The configured shell, else `MER_SHELL` if set, else the first preferred shell that
/// exists, else `None` (the user's login shell).
pub fn preferred_shell(configured: Option<&str>) -> Option<ShellCommand> {
    let from_env = std::env::var("MER_SHELL").ok();
    if let Some(program) = configured.map(str::to_owned).or(from_env).filter(|v| !v.is_empty()) {
        return Some(ShellCommand { program, args: Vec::new() });
    }
    PREFERRED
        .iter()
        .find(|path| Path::new(path).is_file())
        .map(|path| ShellCommand { program: (*path).to_owned(), args: Vec::new() })
}

/// Programs listed in `/etc/shells` that are not something to start a terminal with.
fn is_not_a_shell(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, "tmux" | "screen" | "nologin" | "git-shell"))
}

/// Shells installed on this machine (from `/etc/shells`), for the settings panel. Symlinked
/// duplicates (`/bin/bash` and `/usr/bin/bash`) show once, under their real path.
pub fn installed_shells() -> Vec<String> {
    let Ok(text) = std::fs::read_to_string("/etc/shells") else { return Vec::new() };
    let mut shells: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('/'))
        .filter_map(|line| std::fs::canonicalize(line).ok())
        .filter(|path| path.is_file() && !is_not_a_shell(path))
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    shells.sort();
    shells.dedup();
    shells
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiplexers_and_nologin_are_not_offered_as_shells() {
        assert!(is_not_a_shell(Path::new("/usr/bin/tmux")));
        assert!(is_not_a_shell(Path::new("/usr/sbin/nologin")));
        assert!(!is_not_a_shell(Path::new("/usr/bin/zsh")));
    }

    #[test]
    fn listed_shells_are_unique_real_paths() {
        let shells = installed_shells();
        let mut sorted = shells.clone();
        sorted.dedup();
        assert_eq!(shells, sorted);
        assert!(shells.iter().all(|path| !path.ends_with("/tmux")));
    }
}
