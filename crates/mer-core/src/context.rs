//! What a pane's shell says about where it is: folder, git branch, node version.
//!
//! Only exists while the pane is alive and its shell has the mer integration. Nothing here
//! reads the disk or runs a program; the UI fills it in from the shell's reports.

use std::path::PathBuf;

/// The node version a project asks for, already looked up against what is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeNeed {
    /// What the file says: `v18.20.4`, `18`, `lts/*`…
    pub spec: String,
    /// The file that said it (`.nvmrc`).
    pub file: String,
    /// The installed version it means. `None` for an alias or a version that is not installed.
    pub version: Option<String>,
    /// `spec` is an alias (`lts/*`, `node`…) that only nvm can resolve: it is shown, never judged.
    pub alias: bool,
}

/// How the active node version compares with what the project asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeFit {
    /// Nothing to compare: no file, an alias, or no active version.
    Unknown,
    Matches,
    Differs,
    /// The project asks for a version that is not installed.
    NotInstalled,
}

/// The live context of one pane, shown as chips under the terminal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaneContext {
    /// Folder of the shell, as it reported at its last prompt.
    pub cwd: Option<PathBuf>,
    /// Current git branch (or a short commit id when detached). `None` outside a repository.
    pub branch: Option<String>,
    /// Active node version, when it comes from nvm.
    pub node: Option<String>,
    /// Where nvm keeps its versions, as the shell knows it.
    pub nvm_dir: Option<PathBuf>,
    /// What the project asks for, read lazily: only when the folder or branch changes.
    pub required_node: Option<NodeNeed>,
    /// The `(folder, branch)` `required_node` was read for, so it is not read again for nothing.
    pub need_read_for: Option<(PathBuf, Option<String>)>,
    /// The shell is waiting at its prompt: the last thing it reported was a prompt, not a command.
    pub at_prompt: bool,
}

impl PaneContext {
    /// There is something to show.
    pub fn has_chips(&self) -> bool {
        self.branch.is_some() || self.node.is_some()
    }

    /// The active version against the one the project asks for.
    pub fn node_fit(&self) -> NodeFit {
        let (Some(active), Some(need)) = (self.node.as_deref(), self.required_node.as_ref()) else {
            return NodeFit::Unknown;
        };
        match (&need.version, need.alias) {
            (_, true) => NodeFit::Unknown,
            (Some(wanted), false) if wanted == active => NodeFit::Matches,
            (Some(_), false) => NodeFit::Differs,
            (None, false) => NodeFit::NotInstalled,
        }
    }

    /// Is the project's node version worth reading again for this folder and branch?
    pub fn needs_node_lookup(&self) -> bool {
        let here = self.cwd.clone().map(|cwd| (cwd, self.branch.clone()));
        here.is_some() && self.need_read_for != here
    }

    /// May mer type a command into this pane? Only when the shell sits at its prompt **and**
    /// nothing else owns the terminal. Typing into a running program (an agent, an editor, a dev
    /// server) would feed it keys it never asked for, so mer never does.
    pub fn accepts_commands(&self, foreground_busy: bool) -> bool {
        self.at_prompt && !foreground_busy
    }
}

/// The items that contain `query` (ignoring case), those that start with it first and each
/// group in its original order. An empty query keeps everything.
pub fn filter_items<'a>(items: &'a [String], query: &str) -> Vec<&'a str> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return items.iter().map(String::as_str).collect();
    }
    let (mut starts, mut contains) = (Vec::new(), Vec::new());
    for item in items {
        let lower = item.to_lowercase();
        if lower.starts_with(&query) {
            starts.push(item.as_str());
        } else if lower.contains(&query) {
            contains.push(item.as_str());
        }
    }
    starts.extend(contains);
    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn an_empty_query_keeps_everything_in_order() {
        let items = names(&["develop", "fix/1", "master"]);
        assert_eq!(filter_items(&items, "  "), ["develop", "fix/1", "master"]);
    }

    #[test]
    fn prefix_matches_come_before_other_matches() {
        let items = names(&["hotfix/30484", "fix/30341", "feature/fix-it", "fix/30202", "master"]);
        assert_eq!(
            filter_items(&items, "FIX"),
            ["fix/30341", "fix/30202", "hotfix/30484", "feature/fix-it"]
        );
        assert!(filter_items(&items, "zzz").is_empty());
    }

    #[test]
    fn commands_need_an_idle_prompt() {
        let at_prompt = PaneContext { at_prompt: true, ..PaneContext::default() };
        assert!(at_prompt.accepts_commands(false));
        assert!(!at_prompt.accepts_commands(true), "a program owns the terminal");
        let running = PaneContext { at_prompt: false, ..PaneContext::default() };
        assert!(!running.accepts_commands(false), "the shell is running a command");
        assert!(!running.accepts_commands(true));
    }

    fn need(spec: &str, version: Option<&str>, alias: bool) -> NodeNeed {
        NodeNeed {
            spec: spec.into(),
            file: ".nvmrc".into(),
            version: version.map(Into::into),
            alias,
        }
    }

    fn with(node: Option<&str>, need: Option<NodeNeed>) -> PaneContext {
        PaneContext { node: node.map(Into::into), required_node: need, ..PaneContext::default() }
    }

    #[test]
    fn the_active_version_is_judged_against_the_project() {
        let wants_18 = need("18", Some("v18.20.4"), false);
        assert_eq!(with(Some("v18.20.4"), Some(wants_18.clone())).node_fit(), NodeFit::Matches);
        assert_eq!(with(Some("v24.15.0"), Some(wants_18)).node_fit(), NodeFit::Differs);
        let missing = need("22", None, false);
        assert_eq!(with(Some("v24.15.0"), Some(missing)).node_fit(), NodeFit::NotInstalled);
    }

    #[test]
    fn nothing_is_judged_without_a_file_an_alias_or_an_active_version() {
        assert_eq!(with(Some("v24.15.0"), None).node_fit(), NodeFit::Unknown);
        let alias = need("lts/*", None, true);
        assert_eq!(with(Some("v24.15.0"), Some(alias)).node_fit(), NodeFit::Unknown);
        assert_eq!(
            with(None, Some(need("18", Some("v18.20.4"), false))).node_fit(),
            NodeFit::Unknown
        );
    }

    #[test]
    fn the_project_is_read_again_only_when_the_folder_or_branch_changes() {
        let mut context = PaneContext { cwd: Some("/a".into()), ..PaneContext::default() };
        assert!(context.needs_node_lookup());
        context.need_read_for = Some(("/a".into(), None));
        assert!(!context.needs_node_lookup());
        context.branch = Some("develop".into());
        assert!(context.needs_node_lookup(), "another branch may carry another .nvmrc");
        context.need_read_for = Some(("/a".into(), Some("develop".into())));
        context.cwd = Some("/b".into());
        assert!(context.needs_node_lookup());
        assert!(!PaneContext::default().needs_node_lookup(), "no folder reported yet");
    }

    #[test]
    fn chips_show_only_with_something_to_say() {
        assert!(!PaneContext::default().has_chips());
        let branch = PaneContext { branch: Some("main".into()), ..PaneContext::default() };
        assert!(branch.has_chips());
        let node = PaneContext { node: Some("v24.15.0".into()), ..PaneContext::default() };
        assert!(node.has_chips());
    }
}
