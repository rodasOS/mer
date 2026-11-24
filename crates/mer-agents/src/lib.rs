//! Agent detection (process inspection), activity states and hook installation.
//!
//! Two layers: the process layer always works and says *which* agent is in the foreground
//! of a terminal; the hook layer (the agent calls `mer hook`) says what it is *doing*.

mod git;
mod hooks;
mod install;
mod long_command;
mod node;
mod process;
mod registry;
mod shell;
mod state;

pub use git::{Head, current_head, find_git_dir, list_branches, switch_input};
pub use hooks::{
    HookError, parse_claude_payload, parse_codex_payload, patch_claude_settings, patch_codex_config,
};
pub use install::{backup_once, replace_file, shell_quote};
pub use long_command::{format_duration, is_interactive, should_notify};
pub use node::{
    NodeNeed, installed_versions, is_alias, nvm_dir, parse_version_file, required_version, resolve,
    use_input,
};
pub use process::{
    COMMAND_MARKER, CONTEXT_MARKER, ForegroundProcess, ShellReport, StatInfo, command_from_title,
    context_from_title, foreground, parse_stat, program_label,
};
pub use registry::{AgentRegistry, AgentRule};
pub use shell::{ZSHRC_LINE, command_input, patch_zshrc, zsh_init_script};
pub use state::{AgentKey, AgentStatus, HookKind, reports_turn_start};
