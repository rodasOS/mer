//! Agent detection (process inspection), activity states and hook installation.
//!
//! Two layers: the process layer always works and says *which* agent is in the foreground
//! of a terminal; the hook layer (the agent calls `mer hook`) says what it is *doing*.

mod hooks;
mod install;
mod long_command;
mod process;
mod registry;
mod shell;
mod state;

pub use hooks::{
    HookError, parse_claude_payload, parse_codex_payload, patch_claude_settings, patch_codex_config,
};
pub use install::{backup_once, replace_file, shell_quote};
pub use long_command::{format_duration, is_interactive, should_notify};
pub use process::{
    COMMAND_MARKER, ForegroundProcess, StatInfo, command_from_title, foreground, parse_stat,
    program_label,
};
pub use registry::{AgentRegistry, AgentRule};
pub use shell::{ZSHRC_LINE, patch_zshrc, zsh_init_script};
pub use state::{AgentKey, AgentStatus, HookKind, reports_turn_start};
