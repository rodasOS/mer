//! Versioned unix-socket protocol between the window, `mer hook` and `mer ctl`.
//!
//! Wire format: one line of JSON per message, `{"v":1,"request":...}` from the client and
//! `{"v":1,"response":...}` back. A different `v` is answered with an error.

mod client;
mod protocol;
mod server;

pub use client::Client;
pub use protocol::{CtlCommand, IpcError, PROTOCOL_VERSION, Request, Response, SpaceInfo};
pub use server::{Incoming, Server};

use std::path::PathBuf;

/// Environment variable holding the pane id (a UUID) for processes started inside a pane.
pub const ENV_PANE_ID: &str = "MER_PANE_ID";
/// Environment variable holding the socket path of the running instance.
pub const ENV_SOCKET: &str = "MER_SOCKET";

/// `$XDG_RUNTIME_DIR/mer/mer.sock`; `None` when the runtime dir is unset or not absolute.
#[must_use]
pub fn socket_path() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?);
    dir.is_absolute().then(|| dir.join("mer").join("mer.sock"))
}

#[cfg(test)]
mod tests;
