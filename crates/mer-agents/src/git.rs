//! Git facts for the branch chip, read without starting a program wherever possible.
//!
//! The current branch is a read of `HEAD` (microseconds, and every prompt does it). Listing
//! every branch needs `git` (packed refs, ordering), so it runs only when the picker opens.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{command_input, shell_quote};

/// What `HEAD` points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    Branch(String),
    /// A checked-out commit rather than a branch: its short id.
    Detached(String),
}

impl Head {
    /// The text the chip shows.
    pub fn label(&self) -> &str {
        match self {
            Self::Branch(name) | Self::Detached(name) => name,
        }
    }
}

/// The git directory of the repository `start` is in: a `.git` folder, or the one a worktree's
/// `.git` file points to.
pub fn find_git_dir(start: &Path) -> Option<PathBuf> {
    start.ancestors().find_map(|dir| {
        let dot_git = dir.join(".git");
        if dot_git.is_dir() {
            return Some(dot_git);
        }
        let pointer = fs::read_to_string(&dot_git).ok()?;
        let target = Path::new(pointer.strip_prefix("gitdir:")?.trim());
        let resolved = if target.is_absolute() { target.to_owned() } else { dir.join(target) };
        resolved.is_dir().then_some(resolved)
    })
}

/// Parses the text of a `HEAD` file.
pub fn parse_head(text: &str) -> Option<Head> {
    let text = text.trim();
    if let Some(reference) = text.strip_prefix("ref:") {
        let reference = reference.trim();
        let name = reference.strip_prefix("refs/heads/").unwrap_or(reference);
        return (!name.is_empty()).then(|| Head::Branch(name.to_owned()));
    }
    let is_id = text.len() >= 7 && text.chars().all(|c| c.is_ascii_hexdigit());
    is_id.then(|| Head::Detached(text.chars().take(7).collect()))
}

/// Where `cwd` is on in its repository, or `None` outside one.
pub fn current_head(cwd: &Path) -> Option<Head> {
    parse_head(&fs::read_to_string(find_git_dir(cwd)?.join("HEAD")).ok()?)
}

/// Branch names from `git for-each-ref` output, one per line.
pub fn parse_branches(stdout: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && seen.insert(*line))
        .map(str::to_owned)
        .collect()
}

/// Local branches of the repository at `cwd`, most recently committed first. Empty when there
/// is no repository or no `git`. Starts a process: call it off the interface thread.
pub fn list_branches(cwd: &Path) -> Vec<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["for-each-ref", "--sort=-committerdate", "--format=%(refname:short)", "refs/heads"])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match output {
        Ok(out) if out.status.success() => parse_branches(&String::from_utf8_lossy(&out.stdout)),
        _ => Vec::new(),
    }
}

/// The keystrokes that switch to `branch` at a shell prompt.
pub fn switch_input(branch: &str) -> Option<String> {
    command_input(&format!("git switch {}", shell_quote(branch)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_names_a_branch_or_a_short_commit() {
        assert_eq!(parse_head("ref: refs/heads/develop\n"), Some(Head::Branch("develop".into())));
        assert_eq!(
            parse_head("ref: refs/heads/fix/30341\n"),
            Some(Head::Branch("fix/30341".into()))
        );
        assert_eq!(
            parse_head("4a4c8150f03ef3c5e0c4f837dc0263c137f5b0ce\n"),
            Some(Head::Detached("4a4c815".into()))
        );
        assert_eq!(parse_head("garbage"), None);
        assert_eq!(parse_head(""), None);
    }

    #[test]
    fn branch_lists_drop_blanks_and_repeats_and_keep_order() {
        assert_eq!(
            parse_branches("develop\n\nfix/1\ndevelop\n  master \n"),
            ["develop", "fix/1", "master"]
        );
        assert!(parse_branches("").is_empty());
    }

    #[test]
    fn a_repository_is_found_from_a_subfolder() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join(".git")).unwrap();
        fs::write(root.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let deep = root.path().join("a/b/c");
        fs::create_dir_all(&deep).unwrap();
        assert_eq!(current_head(&deep), Some(Head::Branch("main".into())));
    }

    #[test]
    fn a_worktree_follows_its_git_file() {
        let root = tempfile::tempdir().unwrap();
        let admin = root.path().join("main/.git/worktrees/feature");
        fs::create_dir_all(&admin).unwrap();
        fs::write(admin.join("HEAD"), "ref: refs/heads/feature\n").unwrap();
        let tree = root.path().join("feature");
        fs::create_dir_all(&tree).unwrap();
        fs::write(tree.join(".git"), format!("gitdir: {}\n", admin.display())).unwrap();
        assert_eq!(current_head(&tree), Some(Head::Branch("feature".into())));
    }

    #[test]
    fn outside_a_repository_there_is_no_head() {
        let dir = tempfile::tempdir().unwrap();
        // The temp folder could itself sit inside a repository; only assert when it does not.
        if find_git_dir(dir.path()).is_none() {
            assert_eq!(current_head(dir.path()), None);
        }
    }

    #[test]
    fn switching_quotes_the_branch() {
        assert_eq!(switch_input("develop").as_deref(), Some("\u{15}git switch develop\r"));
        assert_eq!(switch_input("it's odd").as_deref(), Some("\u{15}git switch 'it'\\''s odd'\r"));
        assert_eq!(switch_input("bad\nname"), None);
    }

    #[test]
    fn real_git_lists_branches_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
                .args(args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        };
        if !git(&["init", "-q", "-b", "main"]) {
            return; // no git on this machine
        }
        assert!(git(&["commit", "-q", "--allow-empty", "-m", "one"]));
        assert!(git(&["branch", "older"]));
        assert_eq!(list_branches(dir.path()).first().map(String::as_str), Some("main"));
        assert!(list_branches(dir.path()).contains(&"older".to_owned()));
        assert!(list_branches(&dir.path().join("missing")).is_empty());
    }
}
