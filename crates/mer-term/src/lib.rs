//! Terminal session: PTY, VT emulation (`alacritty_terminal`), keyboard encoding.
//!
//! This crate knows nothing about the UI. The UI reads the grid through
//! [`Session::term`] and feeds input through [`Session::write`].

pub mod keys;
pub mod mouse;
pub mod paste;
pub mod search;
mod session;
pub mod snapshot;

pub use alacritty_terminal;
pub use session::{EventProxy, GridSize, Session, ShellCommand, SpawnOptions, TermError};
