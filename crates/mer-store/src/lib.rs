//! Persistence: `SQLite` for the workspace (spaces, tabs, panes) and `zstd` files for scrollback.
//!
//! Writes go through [`Writer`], a background thread, so the UI never waits on the disk.

mod db;
mod paths;
mod private;
mod scrollback;
mod ui_state;
mod writer;

pub use db::{Store, StoreError};
pub use paths::StateDir;
pub use scrollback::Scrollback;
pub use ui_state::{UiState, WindowMode, WindowState};
pub use writer::Writer;
