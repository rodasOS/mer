//! Command line: opening the window, `mer hook` (called by agents) and `mer ctl`.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use mer_agents::HookKind;
use mer_ipc::{Client, CtlCommand, ENV_PANE_ID, ENV_SOCKET, Request, Response, SpaceInfo};
use tracing_subscriber::EnvFilter;

/// A terminal emulator for working with coding agents.
#[derive(Parser, Debug, PartialEq)]
#[command(name = "mer", version, about)]
pub struct Cli {
    /// Run a program (with its arguments) in the first terminal instead of the shell.
    #[arg(
        short = 'e',
        long = "exec",
        num_args = 1..,
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "PROGRAM"
    )]
    pub exec: Option<Vec<String>>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug, PartialEq)]
pub enum Command {
    /// Report an agent event to the running instance (called from agent hooks).
    Hook {
        /// Which agent is reporting.
        #[arg(long, value_enum)]
        agent: Agent,
        /// The event payload (codex passes it as an argument; claude sends it on stdin).
        payload: Option<String>,
    },
    /// Print the shell integration that lets mer name tabs after the command you run.
    ///
    /// Add `eval "$(mer shell-init zsh)"` to ~/.zshrc (Settings > Agents does it for you).
    ShellInit {
        /// The shell to print it for.
        #[arg(value_enum)]
        shell: ShellKind,
    },
    /// Close the running instance and everything running in it.
    Quit,
    /// Control the running instance.
    Ctl {
        #[command(subcommand)]
        command: Ctl,
    },
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellKind {
    Zsh,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Agent {
    Claude,
    Codex,
}

impl Agent {
    const fn name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

#[derive(Subcommand, Debug, PartialEq)]
pub enum Ctl {
    /// Manage spaces.
    Space {
        #[command(subcommand)]
        command: SpaceCmd,
    },
    /// Manage terminals on their own (outside any space).
    Terminal {
        #[command(subcommand)]
        command: TerminalCmd,
    },
    /// Manage the tabs of the active space.
    Tab {
        #[command(subcommand)]
        command: TabCmd,
    },
    /// Hide or show the sidebar (hidden, it leaves the terminals the whole window).
    Sidebar { state: SidebarState },
    /// Search the focused terminal for a text (leave it out to close the search bar).
    Search { query: Option<String> },
    /// Set the width of the sidebar, in pixels (160 to 480).
    SidebarWidth { width: u16 },
    /// Zoom the terminal text.
    Zoom { change: ZoomChange },
    /// Scale the interface: sidebar, tabs, menus and dialogs (not the terminal text).
    UiScale { change: ZoomChange },
    /// Show the window of the running instance (it may be running with no window).
    Window,
    /// Close the window, like its close button (mer may keep running without it).
    CloseWindow,
    /// Close the running instance and everything running in it.
    Quit,
    /// Open `settings` or a context menu (`space-menu`, `tab-menu`, `pane-menu`, `sidebar-menu`).
    Show { what: String },
    /// Open a directory in a new tab of the running instance.
    Open { dir: String },
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomChange {
    In,
    Out,
    Reset,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarState {
    Collapse,
    Expand,
}

#[derive(Subcommand, Debug, PartialEq)]
pub enum TabCmd {
    /// Put a tab to sleep.
    Sleep { index: usize },
    /// Color a tab; no color clears it.
    Color { index: usize, color: Option<String> },
    /// Move a tab of the active space to another position.
    Move { from: usize, to: usize },
    /// Move a tab of the active space into another space.
    ToSpace { tab: usize, space: usize },
}

#[derive(Subcommand, Debug, PartialEq)]
pub enum TerminalCmd {
    /// Create a terminal on its own.
    New {
        /// Working directory of the new terminal.
        #[arg(long)]
        cwd: Option<String>,
    },
}

#[derive(Subcommand, Debug, PartialEq)]
pub enum SpaceCmd {
    /// List the spaces.
    List,
    /// Create a space.
    New {
        /// Working directory of the new space.
        #[arg(long)]
        cwd: Option<String>,
    },
    /// Put a space to sleep (the active one by default).
    Sleep { index: Option<usize> },
    /// Wake a sleeping space.
    Wake { index: usize },
    /// Delete a space and its saved history.
    Delete { index: usize },
    /// Color a space; no color clears it.
    Color { index: usize, color: Option<String> },
    /// Move a space to another position.
    Move { from: usize, to: usize },
}

/// Entry point; returns the process exit code.
pub fn run() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            // A hook must never break the agent: stay silent and succeed.
            if std::env::args().nth(1).as_deref() == Some("hook") {
                return ExitCode::SUCCESS;
            }
            err.exit();
        }
    };
    match cli.command {
        Some(Command::Hook { agent, payload }) => {
            run_hook(agent, payload.as_deref());
            ExitCode::SUCCESS
        }
        Some(Command::ShellInit { shell: ShellKind::Zsh }) => {
            print!("{}", mer_agents::zsh_init_script());
            ExitCode::SUCCESS
        }
        Some(Command::Ctl { command }) => run_ctl(&command),
        Some(Command::Quit) => run_ctl(&Ctl::Quit),
        None => run_ui(cli.exec),
    }
}

fn socket() -> Option<PathBuf> {
    std::env::var_os(ENV_SOCKET).map(PathBuf::from).or_else(mer_ipc::socket_path)
}

fn run_ui(command: Option<Vec<String>>) -> ExitCode {
    // `-e` always starts a new instance: it asks for a specific program in a fresh window,
    // so it is never forwarded to a running one.
    if command.is_none()
        && let Some(path) = socket()
        && Client::is_running(&path)
    {
        // Running `mer` again just shows the window (it may be running in the background).
        // Open a folder in a new tab with `mer ctl open <dir>`.
        return send_ctl(&path, &ctl_request(&Ctl::Window));
    }
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_env("MER_LOG")).init();
    tracing::info!("mer starting");
    mer_ui::run(mer_ui::RunOptions { command });
    ExitCode::SUCCESS
}

fn run_hook(agent: Agent, argv_payload: Option<&str>) {
    let Ok(pane) = std::env::var(ENV_PANE_ID) else { return };
    let Some(path) = socket() else { return };
    let text = match (agent, argv_payload) {
        (Agent::Codex, payload) => payload.map(str::to_owned),
        (Agent::Claude, _) => read_stdin(),
    };
    let Some(request) = text.and_then(|t| hook_request(agent, &pane, &t)) else { return };
    let _ = Client::send(&path, &request);
}

fn read_stdin() -> Option<String> {
    let mut text = String::new();
    std::io::stdin().take(1 << 20).read_to_string(&mut text).ok()?;
    Some(text)
}

fn run_ctl(command: &Ctl) -> ExitCode {
    let Some(path) = socket() else {
        return fail("mer is not running (no runtime directory)");
    };
    send_ctl(&path, &ctl_request(command))
}

fn send_ctl(path: &Path, request: &Request) -> ExitCode {
    match Client::send(path, request) {
        Ok(response) => match format_response(&response) {
            Ok(text) => {
                if !text.is_empty() {
                    println!("{text}");
                }
                ExitCode::SUCCESS
            }
            Err(message) => fail(&message),
        },
        Err(_) if !Client::is_running(path) => fail("mer is not running"),
        Err(err) => fail(&err.to_string()),
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("mer: {message}");
    ExitCode::FAILURE
}

/// Builds the IPC request for a `mer hook` call; `None` when the payload is not understood.
fn hook_request(agent: Agent, pane: &str, payload: &str) -> Option<Request> {
    let (kind, session_id): (HookKind, Option<String>) = match agent {
        Agent::Claude => mer_agents::parse_claude_payload(payload)?,
        Agent::Codex => mer_agents::parse_codex_payload(payload)?,
    };
    Some(Request::Hook { pane: pane.to_owned(), agent: agent.name().to_owned(), kind, session_id })
}

/// Builds the IPC request for a `mer ctl` subcommand.
fn ctl_request(command: &Ctl) -> Request {
    Request::Ctl(match command {
        Ctl::Space { command: SpaceCmd::List } => CtlCommand::SpaceList,
        Ctl::Space { command: SpaceCmd::New { cwd } } => CtlCommand::SpaceNew { cwd: cwd.clone() },
        Ctl::Terminal { command: TerminalCmd::New { cwd } } => {
            CtlCommand::TerminalNew { cwd: cwd.clone() }
        }
        Ctl::Space { command: SpaceCmd::Sleep { index } } => {
            CtlCommand::SpaceSleep { index: *index }
        }
        Ctl::Space { command: SpaceCmd::Wake { index } } => CtlCommand::SpaceWake { index: *index },
        Ctl::Space { command: SpaceCmd::Delete { index } } => {
            CtlCommand::SpaceDelete { index: *index }
        }
        Ctl::Space { command: SpaceCmd::Move { from, to } } => {
            CtlCommand::SpaceMove { from: *from, to: *to }
        }
        Ctl::Tab { command: TabCmd::Move { from, to } } => {
            CtlCommand::TabMove { from: *from, to: *to }
        }
        Ctl::Tab { command: TabCmd::ToSpace { tab, space } } => {
            CtlCommand::TabToSpace { tab: *tab, space: *space }
        }
        Ctl::Space { command: SpaceCmd::Color { index, color } } => {
            CtlCommand::SpaceColor { index: *index, color: color.clone() }
        }
        Ctl::Tab { command: TabCmd::Sleep { index } } => CtlCommand::TabSleep { index: *index },
        Ctl::Tab { command: TabCmd::Color { index, color } } => {
            CtlCommand::TabColor { index: *index, color: color.clone() }
        }
        Ctl::Sidebar { state } => {
            CtlCommand::Sidebar { collapsed: *state == SidebarState::Collapse }
        }
        Ctl::Show { what } => CtlCommand::Show { what: what.clone() },
        Ctl::Window => CtlCommand::ShowWindow,
        Ctl::CloseWindow => CtlCommand::CloseWindow,
        Ctl::Quit => CtlCommand::Quit,
        Ctl::Search { query } => CtlCommand::Search { query: query.clone().unwrap_or_default() },
        Ctl::SidebarWidth { width } => CtlCommand::SidebarWidth { width: *width },
        Ctl::UiScale { change } => CtlCommand::UiScale { change: zoom_name(*change).into() },
        Ctl::Zoom { change } => CtlCommand::Zoom { change: zoom_name(*change).into() },
        Ctl::Open { dir } => CtlCommand::Open { dir: dir.clone() },
    })
}

fn zoom_name(change: ZoomChange) -> &'static str {
    match change {
        ZoomChange::In => "in",
        ZoomChange::Out => "out",
        ZoomChange::Reset => "reset",
    }
}

/// Text to print for a response, or the error message to report.
fn format_response(response: &Response) -> Result<String, String> {
    match response {
        Response::Ok | Response::Pong => Ok(String::new()),
        Response::Spaces(spaces) => Ok(format_spaces(spaces)),
        Response::Error(message) => Err(message.clone()),
    }
}

fn format_spaces(spaces: &[SpaceInfo]) -> String {
    if spaces.is_empty() {
        return "no spaces".to_owned();
    }
    let width = spaces.iter().map(|s| s.name.chars().count()).max().unwrap_or(0).max(4);
    let mut lines =
        vec![format!("  {:>3}  {:<8}  {:<width$}  {:<8}  PANES", "IDX", "KIND", "NAME", "STATE")];
    for space in spaces {
        let marker = if space.active { '*' } else { ' ' };
        // An older instance sends no kind: everything it lists is a space.
        let kind = if space.kind.is_empty() { "space" } else { space.kind.as_str() };
        lines.push(format!(
            "{marker} {:>3}  {kind:<8}  {:<width$}  {:<8}  {}",
            space.index, space.name, space.state, space.panes
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("mer").chain(args.iter().copied())).expect("parses")
    }

    #[test]
    fn no_arguments_opens_the_ui() {
        assert_eq!(parse(&[]), Cli { exec: None, command: None });
    }

    #[test]
    fn exec_takes_everything_after_it() {
        let cli = parse(&["-e", "htop", "-d", "5", "--flag"]);
        assert_eq!(cli.exec, Some(vec!["htop".into(), "-d".into(), "5".into(), "--flag".into()]));
        let cli = parse(&["--exec", "vim"]);
        assert_eq!(cli.exec, Some(vec!["vim".into()]));
    }

    #[test]
    fn hook_parses_agent_and_payload() {
        let cli = parse(&["hook", "--agent", "codex", "{\"type\":\"x\"}"]);
        assert_eq!(
            cli.command,
            Some(Command::Hook { agent: Agent::Codex, payload: Some("{\"type\":\"x\"}".into()) })
        );
        let cli = parse(&["hook", "--agent", "claude"]);
        assert_eq!(cli.command, Some(Command::Hook { agent: Agent::Claude, payload: None }));
        assert!(Cli::try_parse_from(["mer", "hook", "--agent", "other"]).is_err());
    }

    #[test]
    fn shell_init_parses_a_known_shell() {
        assert_eq!(
            parse(&["shell-init", "zsh"]).command,
            Some(Command::ShellInit { shell: ShellKind::Zsh })
        );
        assert!(Cli::try_parse_from(["mer", "shell-init", "fish"]).is_err());
    }

    #[test]
    fn ctl_commands_parse() {
        let space = |command| Some(Command::Ctl { command: Ctl::Space { command } });
        assert_eq!(parse(&["ctl", "space", "list"]).command, space(SpaceCmd::List));
        assert_eq!(
            parse(&["ctl", "space", "new", "--cwd", "/tmp"]).command,
            space(SpaceCmd::New { cwd: Some("/tmp".into()) })
        );
        assert_eq!(
            parse(&["ctl", "space", "sleep"]).command,
            space(SpaceCmd::Sleep { index: None })
        );
        assert_eq!(
            parse(&["ctl", "space", "sleep", "2"]).command,
            space(SpaceCmd::Sleep { index: Some(2) })
        );
        assert_eq!(
            parse(&["ctl", "space", "wake", "1"]).command,
            space(SpaceCmd::Wake { index: 1 })
        );
        assert_eq!(
            parse(&["ctl", "open", "/x"]).command,
            Some(Command::Ctl { command: Ctl::Open { dir: "/x".into() } })
        );
        assert!(Cli::try_parse_from(["mer", "ctl", "space", "wake"]).is_err());
    }

    #[test]
    fn appearance_commands_parse_and_build_requests() {
        let cli = parse(&["ctl", "space", "color", "1", "violet"]);
        assert_eq!(
            cli.command,
            Some(Command::Ctl {
                command: Ctl::Space {
                    command: SpaceCmd::Color { index: 1, color: Some("violet".into()) }
                }
            })
        );
        assert_eq!(
            ctl_request(&Ctl::Tab { command: TabCmd::Color { index: 0, color: None } }),
            Request::Ctl(CtlCommand::TabColor { index: 0, color: None })
        );
        assert_eq!(
            ctl_request(&Ctl::Sidebar { state: SidebarState::Collapse }),
            Request::Ctl(CtlCommand::Sidebar { collapsed: true })
        );
        assert_eq!(
            ctl_request(&Ctl::Show { what: "settings".into() }),
            Request::Ctl(CtlCommand::Show { what: "settings".into() })
        );
        assert!(Cli::try_parse_from(["mer", "ctl", "sidebar", "sideways"]).is_err());
        assert_eq!(
            ctl_request(&Ctl::Zoom { change: ZoomChange::Out }),
            Request::Ctl(CtlCommand::Zoom { change: "out".into() })
        );
        assert_eq!(ctl_request(&Ctl::Quit), Request::Ctl(CtlCommand::Quit));
        assert_eq!(ctl_request(&Ctl::Window), Request::Ctl(CtlCommand::ShowWindow));
        assert_eq!(parse(&["quit"]).command, Some(Command::Quit));
        assert!(Cli::try_parse_from(["mer", "ctl", "window"]).is_ok());
        assert!(Cli::try_parse_from(["mer", "ctl", "zoom", "in"]).is_ok());
        assert!(Cli::try_parse_from(["mer", "ctl", "zoom", "huge"]).is_err());
        assert_eq!(
            ctl_request(&Ctl::UiScale { change: ZoomChange::In }),
            Request::Ctl(CtlCommand::UiScale { change: "in".into() })
        );
        assert!(Cli::try_parse_from(["mer", "ctl", "ui-scale", "reset"]).is_ok());
        assert_eq!(
            ctl_request(&Ctl::Tab { command: TabCmd::Move { from: 0, to: 2 } }),
            Request::Ctl(CtlCommand::TabMove { from: 0, to: 2 })
        );
        assert_eq!(
            ctl_request(&Ctl::Tab { command: TabCmd::ToSpace { tab: 1, space: 0 } }),
            Request::Ctl(CtlCommand::TabToSpace { tab: 1, space: 0 })
        );
        assert_eq!(
            ctl_request(&Ctl::Space { command: SpaceCmd::Move { from: 2, to: 0 } }),
            Request::Ctl(CtlCommand::SpaceMove { from: 2, to: 0 })
        );
        assert!(Cli::try_parse_from(["mer", "ctl", "tab", "to-space", "1", "0"]).is_ok());
        assert_eq!(
            ctl_request(&Ctl::Search { query: Some("needle".into()) }),
            Request::Ctl(CtlCommand::Search { query: "needle".into() })
        );
        assert_eq!(
            ctl_request(&Ctl::Search { query: None }),
            Request::Ctl(CtlCommand::Search { query: String::new() })
        );
        assert_eq!(
            ctl_request(&Ctl::SidebarWidth { width: 300 }),
            Request::Ctl(CtlCommand::SidebarWidth { width: 300 })
        );
        assert!(Cli::try_parse_from(["mer", "ctl", "sidebar-width", "300"]).is_ok());
    }

    #[test]
    fn hook_requests_are_built_from_payloads() {
        let claude = r#"{"hook_event_name":"Stop","session_id":"abc"}"#;
        assert_eq!(
            hook_request(Agent::Claude, "pane-1", claude),
            Some(Request::Hook {
                pane: "pane-1".into(),
                agent: "claude".into(),
                kind: HookKind::Done,
                session_id: Some("abc".into()),
            })
        );
        let codex = r#"{"type":"agent-turn-complete","thread-id":"t1"}"#;
        assert_eq!(
            hook_request(Agent::Codex, "p", codex),
            Some(Request::Hook {
                pane: "p".into(),
                agent: "codex".into(),
                kind: HookKind::Done,
                session_id: Some("t1".into()),
            })
        );
        assert_eq!(hook_request(Agent::Claude, "p", "garbage"), None);
    }

    #[test]
    fn ctl_requests_are_built_from_arguments() {
        let request = ctl_request(&Ctl::Space { command: SpaceCmd::Wake { index: 3 } });
        assert_eq!(request, Request::Ctl(CtlCommand::SpaceWake { index: 3 }));
        let request = ctl_request(&Ctl::Open { dir: "/d".into() });
        assert_eq!(request, Request::Ctl(CtlCommand::Open { dir: "/d".into() }));
        let request = ctl_request(&Ctl::Terminal { command: TerminalCmd::New { cwd: None } });
        assert_eq!(request, Request::Ctl(CtlCommand::TerminalNew { cwd: None }));
    }

    #[test]
    fn a_terminal_is_created_from_the_command_line() {
        let cli = parse(&["ctl", "terminal", "new", "--cwd", "/tmp"]);
        assert_eq!(
            cli.command,
            Some(Command::Ctl {
                command: Ctl::Terminal { command: TerminalCmd::New { cwd: Some("/tmp".into()) } }
            })
        );
    }

    #[test]
    fn responses_are_formatted() {
        assert_eq!(format_response(&Response::Ok), Ok(String::new()));
        assert_eq!(format_response(&Response::Error("bad".into())), Err("bad".into()));
        assert_eq!(format_response(&Response::Spaces(vec![])), Ok("no spaces".into()));
        let spaces = vec![
            SpaceInfo {
                index: 0,
                name: "api".into(),
                kind: "space".into(),
                state: "awake".into(),
                panes: 2,
                active: true,
            },
            SpaceInfo {
                index: 1,
                name: "frontend".into(),
                kind: String::new(),
                state: "sleeping".into(),
                panes: 1,
                active: false,
            },
        ];
        let table = format_response(&Response::Spaces(spaces)).unwrap();
        let lines: Vec<&str> = table.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(
            lines[0].contains("NAME") && lines[0].contains("STATE") && lines[0].contains("KIND")
        );
        assert!(lines[1].contains("space") && lines[2].contains("space"), "no kind means a space");
        assert!(
            lines[1].starts_with('*') && lines[1].contains("api") && lines[1].contains("awake")
        );
        assert!(lines[2].starts_with(' ') && lines[2].contains("frontend"));
    }
}
