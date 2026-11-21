/// What an agent reports through its hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HookKind {
    /// A session began or was resumed.
    SessionStart,
    /// The user sent a prompt; the agent is working.
    Working,
    /// The agent needs the user (permission or a question).
    Waiting,
    /// The agent finished its turn.
    Done,
}

/// What an agent in a terminal is doing right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentStatus {
    /// Detected in the foreground, no hook has said anything yet (or it is between turns).
    #[default]
    Idle,
    Working,
    Waiting,
    Done,
}

impl AgentStatus {
    /// The status after a hook event.
    #[must_use]
    pub fn after(self, hook: HookKind) -> Self {
        match hook {
            HookKind::SessionStart => Self::Idle,
            HookKind::Working => Self::Working,
            HookKind::Waiting => Self::Waiting,
            HookKind::Done => Self::Done,
        }
    }

    /// Needs the user's attention.
    pub fn wants_attention(self) -> bool {
        matches!(self, Self::Waiting | Self::Done)
    }

    /// The status after the user interrupted the agent (Esc or Ctrl+C). Claude Code sends no
    /// hook when a turn is interrupted, so without this it would look busy forever.
    #[must_use]
    pub fn after_interrupt(self) -> Self {
        match self {
            Self::Working | Self::Waiting => Self::Idle,
            other => other,
        }
    }

    /// The status after the user pressed Enter in the agent's terminal. Only agents whose hooks
    /// say nothing at the start of a turn (Codex) need this guess; others report it themselves.
    #[must_use]
    pub fn after_submit(self, reports_turn_start: bool) -> Self {
        match self {
            Self::Idle | Self::Done if !reports_turn_start => Self::Working,
            other => other,
        }
    }
}

/// Does this agent's hook say when the user starts a turn?
#[must_use]
pub fn reports_turn_start(agent: &str) -> bool {
    agent != "codex"
}

/// A key press in an agent's terminal that may change what the agent is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKey {
    /// Esc or Ctrl+C: the user is stopping the agent.
    Interrupt,
    /// Plain Enter: the user submits a prompt or answers a question.
    Submit,
}

impl AgentKey {
    /// Classifies a key press. `key` is the key name (`escape`, `enter`, `c`, ...).
    #[must_use]
    pub fn classify(key: &str, ctrl: bool, alt: bool, shift: bool) -> Option<Self> {
        match key {
            "escape" if !ctrl && !alt => Some(Self::Interrupt),
            "c" if ctrl && !alt && !shift => Some(Self::Interrupt),
            "enter" if !ctrl && !alt && !shift => Some(Self::Submit),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_normal_turn_goes_idle_working_waiting_working_done() {
        let mut status = AgentStatus::default();
        for (hook, expected) in [
            (HookKind::SessionStart, AgentStatus::Idle),
            (HookKind::Working, AgentStatus::Working),
            (HookKind::Waiting, AgentStatus::Waiting),
            (HookKind::Working, AgentStatus::Working),
            (HookKind::Done, AgentStatus::Done),
        ] {
            status = status.after(hook);
            assert_eq!(status, expected);
        }
    }

    #[test]
    fn interrupting_stops_a_busy_or_waiting_agent_only() {
        assert_eq!(AgentStatus::Working.after_interrupt(), AgentStatus::Idle);
        assert_eq!(AgentStatus::Waiting.after_interrupt(), AgentStatus::Idle);
        assert_eq!(AgentStatus::Done.after_interrupt(), AgentStatus::Done);
        assert_eq!(AgentStatus::Idle.after_interrupt(), AgentStatus::Idle);
    }

    #[test]
    fn enter_starts_a_turn_only_for_agents_that_do_not_report_it() {
        assert_eq!(AgentStatus::Idle.after_submit(false), AgentStatus::Working);
        assert_eq!(AgentStatus::Done.after_submit(false), AgentStatus::Working);
        assert_eq!(AgentStatus::Waiting.after_submit(false), AgentStatus::Waiting);
        assert_eq!(AgentStatus::Idle.after_submit(true), AgentStatus::Idle);
        assert!(reports_turn_start("claude"));
        assert!(!reports_turn_start("codex"));
    }

    #[test]
    fn keys_are_classified() {
        assert_eq!(AgentKey::classify("escape", false, false, false), Some(AgentKey::Interrupt));
        assert_eq!(AgentKey::classify("c", true, false, false), Some(AgentKey::Interrupt));
        assert_eq!(AgentKey::classify("c", false, false, false), None);
        assert_eq!(AgentKey::classify("enter", false, false, false), Some(AgentKey::Submit));
        assert_eq!(AgentKey::classify("enter", false, false, true), None);
        assert_eq!(AgentKey::classify("a", false, false, false), None);
    }

    #[test]
    fn only_waiting_and_done_want_attention() {
        assert!(AgentStatus::Waiting.wants_attention());
        assert!(AgentStatus::Done.wants_attention());
        assert!(!AgentStatus::Working.wants_attention());
        assert!(!AgentStatus::Idle.wants_attention());
    }
}
