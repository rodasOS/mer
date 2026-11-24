//! Pure domain model: spaces, tabs, panes and split layout. No UI, no IO.
//!
//! Terminals are *not* part of this model. The UI keeps one live terminal per
//! [`PaneId`] in the awake spaces and creates or drops them to match the model,
//! which is what lets a space sleep: its panes stay here, its processes do not.

mod accent;
mod context;
mod ids;
mod layout;
mod workspace;

pub use accent::Accent;
pub use context::{NodeFit, NodeNeed, PaneContext, filter_items};
pub use ids::{PaneId, SpaceId, TabId};
pub use layout::{Axis, Direction, Layout};
pub use workspace::{
    AgentMemo, CloseOutcome, InvalidData, Pane, Program, Space, SpaceData, SpaceKind, SpaceState,
    Tab, TabData, Workspace, WorkspaceData,
};
