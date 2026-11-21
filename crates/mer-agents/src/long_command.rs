//! When is a command that finished worth telling the user about?
//!
//! A long build or test run ends while you are in another window or tab: that is news. A
//! `vim` session that lasted an hour is not, and neither is an agent (agents have their own
//! alerts). The decision is pure so it can be tested without a window.

use std::time::Duration;

/// Programs that are the user's own interactive session: their end is never news.
const INTERACTIVE: &[&str] = &[
    // shells
    "bash", "zsh", "fish", "sh", "dash", "nu", "pwsh", "xonsh", // editors and pagers
    "vim", "nvim", "vi", "nano", "emacs", "micro", "hx", "helix", "less", "more", "man", "most",
    // monitors, file managers and other full-screen tools
    "top", "htop", "btop", "glances", "nvtop", "mc", "ranger", "lf", "yazi", "ncdu", "tig",
    "lazygit", "gitui", "k9s", "fzf", "watch", // remote sessions and multiplexers
    "ssh", "mosh", "telnet", "tmux", "screen", "zellij",
    // agents (they report on their own)
    "claude", "codex", "gemini", "opencode",
];

/// Is this program an interactive session, by its name or the command line that started it?
/// Only the first word counts, without its folder: `/usr/bin/vim notes.md` is `vim`.
pub fn is_interactive(name: &str) -> bool {
    let first = name.split_whitespace().next().unwrap_or_default();
    let program = first.rsplit('/').next().unwrap_or(first);
    INTERACTIVE.contains(&program)
}

/// Should a finished command raise an alert? `threshold_secs` is 0 when the feature is off.
pub fn should_notify(
    elapsed: Duration,
    threshold_secs: u32,
    on_screen: bool,
    is_agent: bool,
    name: &str,
) -> bool {
    threshold_secs > 0
        && elapsed >= Duration::from_secs(u64::from(threshold_secs))
        && !on_screen
        && !is_agent
        && !is_interactive(name)
}

/// A short duration for the alert: `45 s`, `2 min 13 s`, `1 h 5 min`.
pub fn format_duration(duration: Duration) -> String {
    let total = duration.as_secs();
    let (hours, minutes, seconds) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours} h {minutes} min")
    } else if minutes > 0 {
        format!("{minutes} min {seconds} s")
    } else {
        format!("{seconds} s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG: Duration = Duration::from_secs(90);

    #[test]
    fn a_long_command_out_of_sight_is_news() {
        assert!(should_notify(LONG, 30, false, false, "cargo build --release"));
    }

    #[test]
    fn short_commands_and_a_turned_off_feature_are_not() {
        assert!(!should_notify(Duration::from_secs(5), 30, false, false, "ls"));
        assert!(!should_notify(LONG, 0, false, false, "cargo test"));
        // Exactly at the threshold counts.
        assert!(should_notify(Duration::from_secs(30), 30, false, false, "make"));
    }

    #[test]
    fn what_you_are_looking_at_and_agents_are_not_news() {
        assert!(!should_notify(LONG, 30, true, false, "cargo test"));
        assert!(!should_notify(LONG, 30, false, true, "cargo test"));
    }

    #[test]
    fn interactive_sessions_are_never_news() {
        for name in ["vim notes.md", "/usr/bin/nvim", "htop", "ssh server", "less log.txt"] {
            assert!(is_interactive(name), "{name}");
            assert!(!should_notify(LONG, 30, false, false, name), "{name}");
        }
        for name in ["cargo build", "npm run dev", "make -j8", "python train.py", ""] {
            assert!(!is_interactive(name), "{name}");
        }
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(format_duration(Duration::from_secs(45)), "45 s");
        assert_eq!(format_duration(Duration::from_secs(133)), "2 min 13 s");
        assert_eq!(format_duration(Duration::from_mins(65)), "1 h 5 min");
        assert_eq!(format_duration(Duration::from_secs(0)), "0 s");
    }
}
