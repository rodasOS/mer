use std::fs;

/// The fields of `/proc/<pid>/stat` we care about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatInfo {
    /// Process group of the process.
    pub pgrp: u32,
    /// Foreground process group of the terminal the process is attached to.
    pub tpgid: i64,
}

/// Parses a `/proc/<pid>/stat` line. The command name sits in parentheses and may itself
/// contain spaces or parentheses, so fields are counted after the *last* `)`.
pub fn parse_stat(stat: &str) -> Option<StatInfo> {
    let rest = &stat[stat.rfind(')')? + 1..];
    let mut fields = rest.split_whitespace();
    // state ppid pgrp session tty_nr tpgid
    let pgrp = fields.nth(2)?.parse().ok()?;
    let tpgid = fields.nth(2)?.parse().ok()?;
    Some(StatInfo { pgrp, tpgid })
}

/// A program running in the foreground of a terminal, other than its shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForegroundProcess {
    pub pid: u32,
    /// Kernel name of the process (`/proc/<pid>/comm`).
    pub comm: String,
    /// Full command line.
    pub cmdline: Vec<String>,
}

/// What is running in the foreground of the terminal whose shell is `shell_pid`.
/// `None` when the shell itself is in the foreground (nothing running) or on any read error.
pub fn foreground(shell_pid: u32) -> Option<ForegroundProcess> {
    let shell = parse_stat(&fs::read_to_string(format!("/proc/{shell_pid}/stat")).ok()?)?;
    let foreground_group = u32::try_from(shell.tpgid).ok()?;
    if foreground_group == shell.pgrp {
        return None;
    }
    // The group leader's pid equals the group id.
    let pid = foreground_group;
    let comm = fs::read_to_string(format!("/proc/{pid}/comm")).ok()?.trim().to_owned();
    let cmdline = fs::read(format!("/proc/{pid}/cmdline"))
        .ok()?
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect();
    Some(ForegroundProcess { pid, comm, cmdline })
}

/// Prefix of the title the shell integration sends just before running a command
/// (`ESC ] 2 ; mer:cmd:<line> BEL`). It is a private channel, never shown as a title.
pub const COMMAND_MARKER: &str = "mer:cmd:";

/// The command line carried by a shell-integration title, if this is one.
pub fn command_from_title(title: &str) -> Option<&str> {
    let line = title.strip_prefix(COMMAND_MARKER)?.trim();
    Some(line)
}

/// Prefix of the title the shell integration sends at every prompt
/// (`ESC ] 2 ; mer:ctx:node=<version>;nvm=<dir>;cwd=<folder> BEL`). Private, never shown.
pub const CONTEXT_MARKER: &str = "mer:ctx:";

/// What the shell reports at a prompt.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShellReport {
    /// Active nvm node version (`v24.15.0`); `None` without nvm.
    pub node: Option<String>,
    /// Where nvm keeps its versions.
    pub nvm_dir: Option<std::path::PathBuf>,
    /// Folder the shell is in.
    pub cwd: Option<std::path::PathBuf>,
}

/// The prompt report carried by a shell-integration title, if this is one. The folder comes
/// last because it is the only field that may contain `;`.
pub fn context_from_title(title: &str) -> Option<ShellReport> {
    let rest = title.strip_prefix(CONTEXT_MARKER)?;
    let (node, rest) = rest.strip_prefix("node=")?.split_once(";nvm=")?;
    let (nvm_dir, cwd) = rest.split_once(";cwd=")?;
    let text = |value: &str| Some(value.trim().to_owned()).filter(|value| !value.is_empty());
    Some(ShellReport {
        node: text(node).filter(|node| node.starts_with('v')),
        nvm_dir: text(nvm_dir).map(Into::into),
        cwd: text(cwd).map(Into::into),
    })
}

/// Programs that only host a script: the script is what the user thinks of as "the program".
fn is_interpreter(name: &str) -> bool {
    matches!(name, "node" | "nodejs" | "bun" | "deno" | "ruby" | "perl" | "php")
        || name
            .strip_prefix("python")
            .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

fn base_name(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or(path)
}

/// The longest label we keep, in characters.
const LABEL_MAX: usize = 40;

/// A short, human name for a foreground program, for tab titles: `npm run dev`, `vim notes.md`.
///
/// Interpreters are skipped in favor of their script (`node /x/bin/vite dev` → `vite dev`) and
/// `python -m http.server` keeps its module. Programs that rewrite their own command line to
/// one string (node's `process.title`) are split back into words. Falls back to `comm`.
pub fn program_label(comm: &str, cmdline: &[String]) -> String {
    let mut words: Vec<&str> =
        cmdline.iter().map(|arg| arg.trim()).filter(|a| !a.is_empty()).collect();
    if words.len() == 1 {
        words = words[0].split_whitespace().collect();
    }
    let Some((first, rest)) = words.split_first() else {
        return comm.to_owned();
    };
    let name = base_name(first);
    let mut parts: Vec<String> = Vec::new();
    if is_interpreter(name) {
        if let Some(position) = rest.iter().position(|arg| !arg.starts_with('-')) {
            let flags = &rest[..position];
            if flags.last() == Some(&"-m") {
                parts.push(name.to_owned());
                parts.push("-m".to_owned());
            }
            parts.push(base_name(rest[position]).to_owned());
            parts.extend(rest[position + 1..].iter().take(3).map(|arg| shorten_arg(arg)));
        } else {
            parts.push(name.to_owned());
        }
    } else {
        parts.push(name.to_owned());
        parts.extend(rest.iter().take(3).map(|arg| shorten_arg(arg)));
    }
    let label = parts.join(" ");
    if label.chars().count() > LABEL_MAX {
        let cut: String = label.chars().take(LABEL_MAX - 1).collect();
        format!("{cut}…")
    } else {
        label
    }
}

/// Absolute paths in arguments are noise in a tab title: keep the last component.
fn shorten_arg(arg: &str) -> String {
    if arg.starts_with('/') { base_name(arg).to_owned() } else { arg.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_typical_stat_line() {
        let stat = "1234 (zsh) S 1000 1234 1234 34816 1234 4194304 100 0 0 0";
        assert_eq!(parse_stat(stat), Some(StatInfo { pgrp: 1234, tpgid: 1234 }));
    }

    #[test]
    fn command_names_with_spaces_and_parentheses_do_not_break_parsing() {
        let stat = "77 (my (weird) app) R 1 500 77 34816 900 4194304";
        assert_eq!(parse_stat(stat), Some(StatInfo { pgrp: 500, tpgid: 900 }));
    }

    #[test]
    fn no_controlling_terminal_gives_negative_tpgid() {
        let stat = "5 (daemon) S 1 5 5 0 -1 4194304";
        assert_eq!(parse_stat(stat), Some(StatInfo { pgrp: 5, tpgid: -1 }));
    }

    #[test]
    fn garbage_is_rejected() {
        assert_eq!(parse_stat(""), None);
        assert_eq!(parse_stat("1 (x) S"), None);
    }

    #[test]
    fn a_missing_process_has_no_foreground() {
        assert_eq!(foreground(u32::MAX - 1), None);
    }
    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_owned()).collect()
    }

    #[test]
    fn plain_programs_keep_their_arguments() {
        assert_eq!(program_label("vim", &args(&["vim", "notes.md"])), "vim notes.md");
        assert_eq!(program_label("npm", &args(&["npm", "run", "dev"])), "npm run dev");
    }

    #[test]
    fn interpreters_give_way_to_their_script() {
        let cmd = args(&["node", "/home/u/proj/node_modules/.bin/vite", "dev"]);
        assert_eq!(program_label("node", &cmd), "vite dev");
        let cmd = args(&["/usr/bin/python3", "-u", "server.py", "--port", "80"]);
        assert_eq!(program_label("python3", &cmd), "server.py --port 80");
    }

    #[test]
    fn python_modules_keep_the_module() {
        let cmd = args(&["python3", "-m", "http.server", "8000"]);
        assert_eq!(program_label("python3", &cmd), "python3 -m http.server 8000");
    }

    #[test]
    fn rewritten_command_lines_are_split_back_into_words() {
        assert_eq!(program_label("ionic", &args(&["ionic      "])), "ionic");
        assert_eq!(program_label("npm", &args(&["npm run dev"])), "npm run dev");
    }

    #[test]
    fn empty_command_line_falls_back_to_comm() {
        assert_eq!(program_label("kworker", &[]), "kworker");
    }

    #[test]
    fn long_labels_are_cut() {
        let long = "a".repeat(80);
        let label = program_label("x", &args(&["x", &long]));
        assert_eq!(label.chars().count(), 40);
        assert!(label.ends_with('…'));
    }

    #[test]
    fn shell_integration_titles_are_recognised() {
        assert_eq!(command_from_title("mer:cmd:ionic serve "), Some("ionic serve"));
        assert_eq!(command_from_title("~/dev · main"), None);
    }

    #[test]
    fn a_prompt_report_is_read_back() {
        let report =
            context_from_title("mer:ctx:node=v24.15.0;nvm=/home/a/.nvm;cwd=/home/a/dev").unwrap();
        assert_eq!(report.node.as_deref(), Some("v24.15.0"));
        assert_eq!(report.nvm_dir, Some("/home/a/.nvm".into()));
        assert_eq!(report.cwd, Some("/home/a/dev".into()));
    }

    #[test]
    fn a_prompt_report_without_nvm_has_no_node() {
        let report = context_from_title("mer:ctx:node=;nvm=;cwd=/tmp").unwrap();
        assert_eq!(report, ShellReport { cwd: Some("/tmp".into()), ..ShellReport::default() });
    }

    #[test]
    fn a_folder_with_semicolons_survives() {
        let report = context_from_title("mer:ctx:node=;nvm=;cwd=/tmp/a;b;cwd=c").unwrap();
        assert_eq!(report.cwd, Some("/tmp/a;b;cwd=c".into()));
    }

    #[test]
    fn other_titles_are_not_reports() {
        assert_eq!(context_from_title("~/dev · main"), None);
        assert_eq!(context_from_title("mer:cmd:ls"), None);
        assert_eq!(context_from_title("mer:ctx:garbage"), None);
    }
}
