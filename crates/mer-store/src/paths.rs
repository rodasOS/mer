use std::path::{Path, PathBuf};

/// Where `mer` keeps its state: `$XDG_STATE_HOME/mer` (default `~/.local/state/mer`).
#[derive(Debug, Clone)]
pub struct StateDir {
    root: PathBuf,
}

impl StateDir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn default_location() -> Option<Self> {
        let base = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
            })?;
        Some(Self::new(base.join("mer")))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn database(&self) -> PathBuf {
        self.root.join("state.db")
    }

    pub fn scrollback_dir(&self) -> PathBuf {
        self.root.join("scrollback")
    }
}
