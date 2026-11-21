use rusqlite::{OptionalExtension, params};

use crate::{Store, StoreError};

/// How the window was last left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowMode {
    Windowed,
    Maximized,
    Fullscreen,
}

impl WindowMode {
    const fn name(self) -> &'static str {
        match self {
            Self::Windowed => "windowed",
            Self::Maximized => "maximized",
            Self::Fullscreen => "fullscreen",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        match name {
            "windowed" => Some(Self::Windowed),
            "maximized" => Some(Self::Maximized),
            "fullscreen" => Some(Self::Fullscreen),
            _ => None,
        }
    }
}

/// Size of the window, in logical pixels, and its mode. Wayland does not let an app read or
/// set the window position, so only the size is remembered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowState {
    pub width: u32,
    pub height: u32,
    pub mode: WindowMode,
}

impl WindowState {
    /// Windows smaller than this are never restored: they are almost surely a glitch.
    pub const MIN_WIDTH: u32 = 400;
    pub const MIN_HEIGHT: u32 = 240;

    /// The same state with a size that can be used.
    #[must_use]
    pub fn clamped(self) -> Self {
        Self {
            width: self.width.clamp(Self::MIN_WIDTH, 16_000),
            height: self.height.clamp(Self::MIN_HEIGHT, 16_000),
            mode: self.mode,
        }
    }
}

/// Interface state that is not part of the workspace itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UiState {
    pub sidebar_collapsed: bool,
    /// Width the user gave the sidebar, in unscaled pixels. `None` means the default.
    pub sidebar_width: Option<u16>,
    pub window: Option<WindowState>,
}

impl Store {
    pub fn save_ui(&mut self, state: &UiState) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        let put = |key: &str, value: Option<String>| -> Result<(), StoreError> {
            match value {
                Some(value) => tx.execute(
                    "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![key, value],
                )?,
                None => tx.execute("DELETE FROM meta WHERE key = ?1", [key])?,
            };
            Ok(())
        };
        put("ui.sidebar_collapsed", Some(u8::from(state.sidebar_collapsed).to_string()))?;
        put("ui.sidebar_width", state.sidebar_width.map(|w| w.to_string()))?;
        put("ui.window.width", state.window.map(|w| w.width.to_string()))?;
        put("ui.window.height", state.window.map(|w| w.height.to_string()))?;
        put("ui.window.mode", state.window.map(|w| w.mode.name().to_owned()))?;
        tx.commit()?;
        Ok(())
    }

    pub fn load_ui(&self) -> Result<UiState, StoreError> {
        let get = |key: &str| -> Result<Option<String>, StoreError> {
            Ok(self
                .conn
                .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| row.get(0))
                .optional()?)
        };
        let window = match (
            get("ui.window.width")?.and_then(|v| v.parse().ok()),
            get("ui.window.height")?.and_then(|v| v.parse().ok()),
            get("ui.window.mode")?.and_then(|v| WindowMode::parse(&v)),
        ) {
            (Some(width), Some(height), Some(mode)) => {
                Some(WindowState { width, height, mode }.clamped())
            }
            _ => None,
        };
        Ok(UiState {
            sidebar_collapsed: get("ui.sidebar_collapsed")?.as_deref() == Some("1"),
            sidebar_width: get("ui.sidebar_width")?.and_then(|v| v.parse().ok()),
            window,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_nothing_was_saved() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.load_ui().unwrap(), UiState::default());
    }

    #[test]
    fn ui_state_round_trips() {
        let mut store = Store::open_in_memory().unwrap();
        let state = UiState {
            sidebar_collapsed: true,
            sidebar_width: Some(300),
            window: Some(WindowState { width: 1280, height: 800, mode: WindowMode::Maximized }),
        };
        store.save_ui(&state).unwrap();
        assert_eq!(store.load_ui().unwrap(), state);

        store.save_ui(&UiState::default()).unwrap();
        assert_eq!(store.load_ui().unwrap(), UiState::default());
    }

    #[test]
    fn tiny_windows_are_clamped_when_loaded() {
        let mut store = Store::open_in_memory().unwrap();
        let tiny = UiState {
            sidebar_collapsed: false,
            sidebar_width: None,
            window: Some(WindowState { width: 3, height: 2, mode: WindowMode::Windowed }),
        };
        store.save_ui(&tiny).unwrap();
        let loaded = store.load_ui().unwrap().window.unwrap();
        assert_eq!(
            (loaded.width, loaded.height),
            (WindowState::MIN_WIDTH, WindowState::MIN_HEIGHT)
        );
    }
}
