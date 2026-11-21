use mer_agents::HookKind;
use serde::{Deserialize, Serialize};

/// Current wire protocol version.
pub const PROTOCOL_VERSION: u32 = 1;

/// Errors of the IPC layer.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid message: {0}")]
    Json(#[from] serde_json::Error),
    #[error("another mer instance is already running")]
    AlreadyRunning,
    #[error("protocol error: {0}")]
    Protocol(String),
}

/// A message from a client to the running instance.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Request {
    Ping,
    /// Sent by `mer hook`. `pane` is the value of `MER_PANE_ID` (a UUID string).
    Hook {
        pane: String,
        agent: String,
        kind: HookKind,
        session_id: Option<String>,
    },
    Ctl(CtlCommand),
}

/// Commands of `mer ctl`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum CtlCommand {
    SpaceList,
    SpaceNew {
        cwd: Option<String>,
    },
    SpaceSleep {
        index: Option<usize>,
    },
    SpaceWake {
        index: usize,
    },
    /// Open the window if it is closed (the app keeps running without one).
    ShowWindow,
    /// Close the window, like its close button (mer may keep running without it).
    CloseWindow,
    /// Close mer and everything running in it.
    Quit,
    /// Delete a space and its saved history.
    SpaceDelete {
        index: usize,
    },
    /// Set (or clear, with `None`) the color of a space.
    SpaceColor {
        index: usize,
        color: Option<String>,
    },
    /// Put a tab of the active space to sleep.
    TabSleep {
        index: usize,
    },
    /// Set (or clear, with `None`) the color of a tab of the active space.
    TabColor {
        index: usize,
        color: Option<String>,
    },
    /// Zoom the terminal text: `in`, `out` or `reset`.
    Zoom {
        change: String,
    },
    /// Search the focused terminal's screen and history for a text; an empty text closes the
    /// search bar.
    Search {
        query: String,
    },
    /// Move a tab of the active space to another position.
    TabMove {
        from: usize,
        to: usize,
    },
    /// Move a tab of the active space into another space.
    TabToSpace {
        tab: usize,
        space: usize,
    },
    /// Move a space to another position.
    SpaceMove {
        from: usize,
        to: usize,
    },
    /// Scale the interface (not the terminal text): `in`, `out` or `reset`.
    UiScale {
        change: String,
    },
    /// Set the width of the sidebar, in pixels at the default interface size.
    SidebarWidth {
        width: u16,
    },
    /// Collapse or expand the sidebar.
    Sidebar {
        collapsed: bool,
    },
    /// Open part of the interface: `settings` or a context menu (`space-menu`, `tab-menu`,
    /// `pane-menu`, `sidebar-menu`).
    Show {
        what: String,
    },
    Open {
        dir: String,
    },
}

/// One row of the space list.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SpaceInfo {
    pub index: usize,
    pub name: String,
    /// `"awake"` or `"sleeping"`.
    pub state: String,
    pub panes: usize,
    pub active: bool,
}

/// The answer to a [`Request`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Response {
    Ok,
    Pong,
    Spaces(Vec<SpaceInfo>),
    Error(String),
}

#[derive(Serialize)]
struct RequestEnvelopeOut<'a> {
    v: u32,
    request: &'a Request,
}

#[derive(Serialize)]
struct ResponseEnvelopeOut<'a> {
    v: u32,
    response: &'a Response,
}

#[derive(Deserialize)]
struct Envelope {
    v: Option<u32>,
    request: Option<serde_json::Value>,
    response: Option<serde_json::Value>,
}

pub(crate) fn encode_request(request: &Request) -> Result<Vec<u8>, IpcError> {
    let mut line = serde_json::to_vec(&RequestEnvelopeOut { v: PROTOCOL_VERSION, request })?;
    line.push(b'\n');
    Ok(line)
}

pub(crate) fn encode_response(response: &Response) -> Vec<u8> {
    let mut line = serde_json::to_vec(&ResponseEnvelopeOut { v: PROTOCOL_VERSION, response })
        .unwrap_or_else(|_| br#"{"v":1,"response":{"Error":"encoding failed"}}"#.to_vec());
    line.push(b'\n');
    line
}

/// Parses a request line; the error is the response to send back.
pub(crate) fn decode_request(line: &str) -> Result<Request, Response> {
    let envelope: Envelope = serde_json::from_str(line)
        .map_err(|e| Response::Error(format!("malformed message: {e}")))?;
    if envelope.v != Some(PROTOCOL_VERSION) {
        return Err(Response::Error("unsupported protocol version".to_owned()));
    }
    let value = envelope.request.ok_or_else(|| Response::Error("missing request".to_owned()))?;
    serde_json::from_value(value).map_err(|e| Response::Error(format!("malformed request: {e}")))
}

pub(crate) fn decode_response(line: &str) -> Result<Response, IpcError> {
    let envelope: Envelope = serde_json::from_str(line)?;
    if envelope.v != Some(PROTOCOL_VERSION) {
        return Err(IpcError::Protocol("unsupported protocol version".to_owned()));
    }
    let value =
        envelope.response.ok_or_else(|| IpcError::Protocol("missing response".to_owned()))?;
    Ok(serde_json::from_value(value)?)
}
