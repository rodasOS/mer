use crate::ForegroundProcess;

/// Recognizes an agent: `name` is shown in the UI, `pattern` is matched (case-insensitively)
/// against the program names and the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRule {
    pub name: String,
    pub pattern: String,
}

impl AgentRule {
    pub fn new(name: &str, pattern: &str) -> Self {
        Self { name: name.to_owned(), pattern: pattern.to_lowercase() }
    }
}

/// The set of agents `mer` knows how to spot.
#[derive(Debug, Clone)]
pub struct AgentRegistry {
    rules: Vec<AgentRule>,
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::with_extra(Vec::new())
    }
}

impl AgentRegistry {
    /// Built-in agents plus the user's own rules (checked first).
    pub fn with_extra(extra: Vec<AgentRule>) -> Self {
        let mut rules = extra
            .into_iter()
            .map(|rule| AgentRule::new(&rule.name, &rule.pattern))
            .collect::<Vec<_>>();
        rules.extend([
            AgentRule::new("claude", "@anthropic-ai/claude-code"),
            AgentRule::new("claude", "claude"),
            AgentRule::new("codex", "@openai/codex"),
            AgentRule::new("codex", "codex"),
            AgentRule::new("gemini", "gemini"),
            AgentRule::new("opencode", "opencode"),
        ]);
        Self { rules }
    }

    /// The agent running as `process`, if any.
    ///
    /// A bare name must equal the program's file name (so `vim claude-notes.txt` is not an
    /// agent); a pattern containing `/` or `@` is searched in the whole command line.
    pub fn detect(&self, process: &ForegroundProcess) -> Option<&str> {
        let names = program_names(process);
        let joined = process.cmdline.join(" ").to_lowercase();
        self.rules
            .iter()
            .find(|rule| {
                if rule.pattern.contains('/') || rule.pattern.contains('@') {
                    joined.contains(&rule.pattern)
                } else {
                    names.contains(&rule.pattern)
                }
            })
            .map(|rule| rule.name.as_str())
    }
}

/// File names of the program and, for interpreters, of the script it runs.
fn program_names(process: &ForegroundProcess) -> Vec<String> {
    let base = |text: &str| text.rsplit('/').next().unwrap_or(text).to_lowercase();
    let mut names = vec![process.comm.to_lowercase()];
    names.extend(process.cmdline.iter().take(3).map(|arg| base(arg)));
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(comm: &str, cmdline: &[&str]) -> ForegroundProcess {
        ForegroundProcess {
            pid: 1,
            comm: comm.to_owned(),
            cmdline: cmdline.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn detects_the_known_agents() {
        let registry = AgentRegistry::default();
        assert_eq!(registry.detect(&process("claude", &["claude"])), Some("claude"));
        assert_eq!(
            registry.detect(&process("node", &["node", "/home/u/.nvm/bin/claude", "--resume"])),
            Some("claude")
        );
        assert_eq!(
            registry
                .detect(&process("node", &["node", "/x/node_modules/@openai/codex/bin/codex.js"])),
            Some("codex")
        );
        assert_eq!(registry.detect(&process("gemini", &["gemini"])), Some("gemini"));
        assert_eq!(registry.detect(&process("opencode", &["opencode"])), Some("opencode"));
    }

    #[test]
    fn ordinary_programs_are_not_agents() {
        let registry = AgentRegistry::default();
        assert_eq!(registry.detect(&process("vim", &["vim", "claude-notes.txt"])), None);
        assert_eq!(registry.detect(&process("cargo", &["cargo", "build"])), None);
        assert_eq!(registry.detect(&process("zsh", &["zsh"])), None);
    }

    #[test]
    fn user_rules_win_and_can_match_scripts() {
        let registry = AgentRegistry::with_extra(vec![AgentRule::new("aider", "aider")]);
        assert_eq!(
            registry.detect(&process("python3", &["python3", "/usr/bin/aider"])),
            Some("aider")
        );
    }
}
