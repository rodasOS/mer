//! Every user-facing command. Keyboard actions and the command palette both end here.

/// Something the user can ask the workspace to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    NewTab,
    CloseTab,
    ClosePane,
    SplitRight,
    SplitDown,
    NewSpace,
    NewTerminal,
    CloseSpace,
    RenameSpace,
    RenameTab,
    NextTab,
    PrevTab,
    NextSpace,
    PrevSpace,
    FocusNext,
    FocusPrev,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    ResizeLeft,
    ResizeRight,
    ResizeUp,
    ResizeDown,
    ToggleSidebar,
    OpenPalette,
    SleepSpace,
    SleepOthers,
    OpenSettings,
    SleepTab,
    DeleteSpace,
    CycleSpaceColor,
    CycleTabColor,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    UiScaleUp,
    UiScaleDown,
    UiScaleReset,
    MoveTabLeft,
    MoveTabRight,
    MoveSpaceUp,
    MoveSpaceDown,
    SearchTerminal,
    ScrollToBottom,
    Quit,
}

/// A palette entry.
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub label: &'static str,
    pub shortcut: &'static str,
    pub cmd: Cmd,
}

const fn entry(label: &'static str, shortcut: &'static str, cmd: Cmd) -> Entry {
    Entry { label, shortcut, cmd }
}

/// Commands shown in the palette, in display order.
pub const PALETTE: &[Entry] = &[
    entry("New tab", "Ctrl+Shift+T", Cmd::NewTab),
    entry("Split right", "Ctrl+Shift+D", Cmd::SplitRight),
    entry("Split down", "Ctrl+Shift+E", Cmd::SplitDown),
    entry("Close pane", "Ctrl+Shift+W", Cmd::ClosePane),
    entry("Close tab", "", Cmd::CloseTab),
    entry("Sleep tab", "", Cmd::SleepTab),
    entry("Change tab color", "", Cmd::CycleTabColor),
    entry("Rename tab", "", Cmd::RenameTab),
    entry("Move tab left", "", Cmd::MoveTabLeft),
    entry("Move tab right", "", Cmd::MoveTabRight),
    entry("Next tab", "Ctrl+Tab", Cmd::NextTab),
    entry("Previous tab", "Ctrl+Shift+Tab", Cmd::PrevTab),
    entry("New terminal", "", Cmd::NewTerminal),
    entry("New space", "Ctrl+Shift+N", Cmd::NewSpace),
    entry("Rename space", "", Cmd::RenameSpace),
    entry("Close space", "", Cmd::CloseSpace),
    entry("Delete space…", "", Cmd::DeleteSpace),
    entry("Change space color", "", Cmd::CycleSpaceColor),
    entry("Sleep space", "Ctrl+Shift+Z", Cmd::SleepSpace),
    entry("Sleep other spaces", "", Cmd::SleepOthers),
    entry("Move space up", "", Cmd::MoveSpaceUp),
    entry("Move space down", "", Cmd::MoveSpaceDown),
    entry("Next space", "Ctrl+Shift+PageDown", Cmd::NextSpace),
    entry("Previous space", "Ctrl+Shift+PageUp", Cmd::PrevSpace),
    entry("Focus next pane", "", Cmd::FocusNext),
    entry("Focus previous pane", "", Cmd::FocusPrev),
    entry("Toggle sidebar", "Ctrl+Shift+B", Cmd::ToggleSidebar),
    entry("Zoom in", "Ctrl+=", Cmd::ZoomIn),
    entry("Zoom out", "Ctrl+-", Cmd::ZoomOut),
    entry("Reset zoom", "Ctrl+0", Cmd::ZoomReset),
    entry("Interface bigger", "Ctrl+Alt+=", Cmd::UiScaleUp),
    entry("Interface smaller", "Ctrl+Alt+-", Cmd::UiScaleDown),
    entry("Reset interface size", "Ctrl+Alt+0", Cmd::UiScaleReset),
    entry("Search in terminal", "Ctrl+Shift+F", Cmd::SearchTerminal),
    entry("Scroll to bottom", "Ctrl+Shift+End", Cmd::ScrollToBottom),
    entry("Settings", "Ctrl+,", Cmd::OpenSettings),
    entry("Quit mer", "Ctrl+Shift+Q", Cmd::Quit),
];

/// Entries whose label contains the query's letters in order (case-insensitive).
pub fn filter(query: &str) -> Vec<Entry> {
    PALETTE.iter().copied().filter(|entry| matches_query(entry.label, query)).collect()
}

fn matches_query(label: &str, query: &str) -> bool {
    let mut label = label.chars().flat_map(char::to_lowercase);
    query
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| !c.is_whitespace())
        .all(|wanted| label.any(|c| c == wanted))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_lists_everything() {
        assert_eq!(filter("").len(), PALETTE.len());
    }

    #[test]
    fn query_matches_letters_in_order_ignoring_case_and_spaces() {
        let labels: Vec<_> = filter("SPL ri").iter().map(|e| e.label).collect();
        assert_eq!(labels, vec!["Split right"]);
        assert!(filter("zzz").is_empty());
    }

    #[test]
    fn every_command_with_a_shortcut_has_a_unique_one() {
        let mut shortcuts: Vec<_> =
            PALETTE.iter().map(|e| e.shortcut).filter(|s| !s.is_empty()).collect();
        let total = shortcuts.len();
        shortcuts.sort_unstable();
        shortcuts.dedup();
        assert_eq!(shortcuts.len(), total);
    }
}
