# 0009. Terminals without spaces, and a sidebar that hides completely

Status: accepted. Builds on [0005](0005-view-outlives-window.md) and the model of ADR 0004.

## Context
The sidebar only listed *spaces*, each holding tabs, each tab holding panes, and collapsing it left a
52 px rail of icons. Using a single terminal meant creating a space for it. Warp's vertical tabs
show the better shape: groups and plain terminals side by side, and a sidebar that, when hidden,
leaves the whole window to the terminals.

Three models were weighed (research in the planning session: the code of `mer-core`/`mer-store`,
the UI, and how Warp, Chrome, VS Code, WezTerm, tmux and Arc relate groups to loose items):

- **A. `Space` gets a `kind`** (`Space` or `Terminal`). A terminal is a space with exactly one tab.
- **B. A top-level enum** `Item::{Group(Space), Terminal(Tab)}`.
- **C. A flat list of tabs** with an optional group id (the Chrome model).

## Decision
**A.** Every rule the code already enforces keeps holding: there is always a space, a space never
has no tab, sleeping and auto-sleep work per entry, agent status rolls up per entry, IPC and drag and
drop address entries by index. Persistence is one column (`spaces.kind`, schema v3, default
`'space'`) and `SpaceData.kind` is `#[serde(default)]`, so old databases and old saved data read as
spaces. B would have touched about forty `active_space()` callers and needed `tabs.space_id` to
become nullable (a table rebuild in SQLite); C undoes the space/tab structure ADR 0004 and 0005
rely on.

Rules, all in `mer-core` with tests:
- A terminal has exactly one tab (which may be split into panes). Loading rejects anything else.
- `add_tab` inside a terminal adds a **terminal** beside it. A tab is never moved into a terminal.
- `adopt_terminal` moves a terminal into a space (its tab becomes the shown one) and the terminal
  entry disappears. `detach_tab` does the reverse, and `promote_to_space` turns a terminal into a space.
- Renaming a terminal renames its tab; its row shows what runs in it (`Tab::display_title`).
- Dropping a terminal on a space joins it; to reorder around a space use *Move up/down*.

UI: one ordered list of rows, with *+ Terminal* and *+ Space* below it. A terminal has no tabs, so
it gets **no tab bar** while the sidebar is visible (its name, folder and branch are in its row and
in the window title); most terminals do the same with a single tab (iTerm2, Kitty, Ghostty). A space
keeps its bar, whose *+* makes a tab. The **sidebar toggle** lives in the sidebar's header; with the
sidebar hidden it moves to the bar, or, for a terminal, to a slim 28 px strip that also names it, so
there is always a way back. Collapsing **hides the sidebar entirely**: the rail is gone, and dragging
the sidebar's edge below the fold width hides it too.

## Consequences
- Idle cost is unchanged: no polling, one `ALTER TABLE` the first time a database is opened.
- The v3 migration cannot be undone: an older `mer` refuses a database that is ahead of it. Back
  up `~/.local/state/mer` before running the new version for the first time if you may go back.
- If terminals ever need behavior that really differs from spaces, `kind` can grow into model B
  without losing data. Search over titles and groups, and nested folders, are not built yet.
- No hover-to-reveal for the hidden sidebar: Firefox dropped auto-collapse as annoying. It can
  come later as an option.
