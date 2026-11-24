use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Accent, Axis, Direction, Layout, PaneContext, PaneId, SpaceId, TabId};

/// What was running in a pane, kept so it can be resumed when the space wakes up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMemo {
    /// Agent name, e.g. `claude`.
    pub kind: String,
    /// Session to resume, when the agent reported one.
    pub session_id: Option<String>,
}

/// Metadata about one terminal pane. The terminal itself lives in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pane {
    pub id: PaneId,
    /// Where the shell should start (or where it was when the space went to sleep).
    pub cwd: Option<PathBuf>,
    /// Title reported by the program running in the pane.
    pub title: String,
    /// The agent last seen in this pane, if any.
    pub agent: Option<AgentMemo>,
    /// What runs in the foreground right now. Only exists while the pane is alive.
    #[serde(skip)]
    pub program: Option<Program>,
    /// Folder, branch and node version the shell reports. Only exists while the pane is alive.
    #[serde(skip)]
    pub context: PaneContext,
}

impl Pane {
    /// The folder the pane is in: where its shell says it is, else the last one saved.
    pub fn folder_path(&self) -> Option<&Path> {
        self.context.cwd.as_deref().or(self.cwd.as_deref())
    }

    /// The name of that folder, as a tab title: its last component, `~` for the home folder.
    pub fn folder_name(&self) -> Option<&str> {
        folder_label(self.folder_path()?, home_dir())
    }

    fn new(cwd: Option<PathBuf>) -> Self {
        Self {
            id: PaneId::new(),
            cwd,
            title: String::new(),
            agent: None,
            program: None,
            context: PaneContext::default(),
        }
    }
}

/// A program running in the foreground of a pane, other than its shell.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Program {
    /// Short name read from the process (`vite dev`). Empty until the process is seen.
    pub label: String,
    /// The exact line the user typed, when the shell reports it.
    pub command: Option<String>,
    /// The title the program set for itself.
    pub title: Option<String>,
    /// Long enough in the foreground to be worth naming the tab after. A command that ends in a
    /// blink (`ls`, `clear`) never gets here, so it never flashes in the title.
    pub visible: bool,
}

impl Program {
    /// The best name for the program: its own title, the typed command or the process label.
    pub fn name(&self) -> Option<&str> {
        [self.title.as_deref(), self.command.as_deref(), Some(self.label.as_str())]
            .into_iter()
            .flatten()
            .find(|name| !name.is_empty())
    }
}

#[derive(Debug, Clone)]
pub struct Tab {
    pub id: TabId,
    /// Name chosen by the user; falls back to the focused pane's title.
    pub title: Option<String>,
    /// Color chosen by the user.
    pub color: Option<Accent>,
    /// A sleeping tab has no running terminals; it wakes when selected.
    pub sleeping: bool,
    layout: Layout,
    focused: PaneId,
    panes: HashMap<PaneId, Pane>,
}

impl Tab {
    fn new(cwd: Option<PathBuf>) -> Self {
        let pane = Pane::new(cwd);
        let (id, focused) = (TabId::new(), pane.id);
        Self {
            id,
            title: None,
            color: None,
            sleeping: false,
            layout: Layout::Leaf(pane.id),
            focused,
            panes: HashMap::from([(pane.id, pane)]),
        }
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn focused(&self) -> PaneId {
        self.focused
    }

    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.get(&id)
    }

    pub fn pane_count(&self) -> usize {
        self.panes.len()
    }

    /// Panes in visual order.
    pub fn pane_ids(&self) -> Vec<PaneId> {
        self.layout.panes()
    }

    pub fn contains(&self, id: PaneId) -> bool {
        self.panes.contains_key(&id)
    }

    /// Text for the tab strip, first match wins:
    /// 1. the name the user gave it, which nothing else replaces;
    /// 2. the program that has been in the foreground long enough (see [`Program::visible`]);
    /// 3. at an idle prompt, the name of the folder, the same for every tab;
    /// 4. the title the shell set, when the folder is not known;
    /// 5. `shell`.
    pub fn display_title(&self) -> &str {
        if let Some(title) = self.title.as_deref().filter(|t| !t.is_empty()) {
            return title;
        }
        let pane = self.panes.get(&self.focused);
        pane.and_then(|pane| pane.program.as_ref().filter(|program| program.visible)?.name())
            .or_else(|| pane.and_then(Pane::folder_name))
            .or_else(|| pane.map(|pane| pane.title.as_str()).filter(|title| !title.is_empty()))
            .unwrap_or("shell")
    }

    /// The folder to show dimmed beside the title. Only while a program names the tab: at an
    /// idle prompt the title already is the folder.
    pub fn display_place(&self) -> Option<&str> {
        let pane = self.panes.get(&self.focused)?;
        pane.program.as_ref().filter(|program| program.visible)?;
        pane.folder_name()
    }

    /// Returns `true` when the tab has no panes left.
    fn remove_pane(&mut self, id: PaneId) -> bool {
        let order = self.layout.panes();
        let Some(position) = order.iter().position(|p| *p == id) else {
            return false;
        };
        let layout = std::mem::replace(&mut self.layout, Layout::Leaf(id));
        let Some(rest) = layout.without(id) else {
            self.panes.remove(&id);
            return true;
        };
        self.layout = rest;
        self.panes.remove(&id);
        if self.focused == id {
            // Prefer the pane before the closed one, otherwise the one after it.
            let neighbor = match position.checked_sub(1) {
                Some(previous) => order[previous],
                None => order[position + 1],
            };
            self.focused = neighbor;
        }
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpaceState {
    /// Its terminals are running.
    Awake,
    /// Only the layout and directories are kept; no processes.
    Sleeping,
}

/// What a sidebar entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpaceKind {
    /// A named group of tabs.
    #[default]
    Space,
    /// A single terminal on its own. It is a space with **exactly one tab** (which may still be
    /// split into panes), so sleeping, agents, saving and the rest work as they do for spaces.
    Terminal,
}

#[derive(Debug, Clone)]
pub struct Space {
    pub id: SpaceId,
    /// The space's name. Unused by a terminal, which is named after its program (see
    /// [`Space::display_name`]).
    pub name: String,
    pub state: SpaceState,
    /// Color chosen by the user, or given automatically when the space was created.
    pub color: Option<Accent>,
    kind: SpaceKind,
    tabs: Vec<Tab>,
    active: usize,
}

impl Space {
    fn new(name: String, cwd: Option<PathBuf>) -> Self {
        Self::with_tab(SpaceKind::Space, name, Tab::new(cwd))
    }

    fn with_tab(kind: SpaceKind, name: String, tab: Tab) -> Self {
        Self {
            id: SpaceId::new(),
            name,
            state: SpaceState::Awake,
            color: None,
            kind,
            tabs: vec![tab],
            active: 0,
        }
    }

    pub fn kind(&self) -> SpaceKind {
        self.kind
    }

    pub fn is_terminal(&self) -> bool {
        self.kind == SpaceKind::Terminal
    }

    /// What the rename box starts with: a space's name, or the name the user gave a terminal
    /// (empty while it follows its program).
    pub fn editable_name(&self) -> String {
        match self.kind {
            SpaceKind::Space => self.name.clone(),
            SpaceKind::Terminal => self.tabs[0].title.clone().unwrap_or_default(),
        }
    }

    /// The text of the sidebar row: a space's name, or what a terminal is running.
    pub fn display_name(&self) -> &str {
        match self.kind {
            SpaceKind::Space => &self.name,
            SpaceKind::Terminal => self.tabs[0].display_title(),
        }
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn active_tab_index(&self) -> usize {
        self.active
    }

    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    fn active_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    /// Number of terminals in this space, across all its tabs.
    pub fn pane_count(&self) -> usize {
        self.tabs.iter().map(Tab::pane_count).sum()
    }

    fn locate(&self, pane: PaneId) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.contains(pane))
    }
}

/// What disappeared when a pane was closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseOutcome {
    NotFound,
    PaneClosed,
    TabClosed,
    SpaceClosed,
    /// That was the last pane of the last space: the app should exit. Nothing was removed.
    WorkspaceEmpty,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    spaces: Vec<Space>,
    active: usize,
    created: usize,
}

impl Workspace {
    pub fn new(cwd: Option<PathBuf>) -> Self {
        let mut space = Space::new("Space 1".to_owned(), cwd);
        space.color = Some(Accent::rotation(0));
        Self { spaces: vec![space], active: 0, created: 1 }
    }

    pub fn spaces(&self) -> &[Space] {
        &self.spaces
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn active_space(&self) -> &Space {
        &self.spaces[self.active]
    }

    fn active_space_mut(&mut self) -> &mut Space {
        &mut self.spaces[self.active]
    }

    pub fn active_tab(&self) -> &Tab {
        self.active_space().active_tab()
    }

    pub fn focused_pane(&self) -> PaneId {
        self.active_tab().focused()
    }

    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.spaces.iter().flat_map(|s| &s.tabs).find_map(|t| t.pane(id))
    }

    /// Every pane that should have a running terminal: those in awake spaces.
    pub fn awake_panes(&self) -> Vec<&Pane> {
        self.spaces
            .iter()
            .filter(|space| space.state == SpaceState::Awake)
            .flat_map(|space| space.tabs.iter().filter(|tab| !tab.sleeping))
            .flat_map(|tab| tab.panes.values())
            .collect()
    }

    pub fn add_space(&mut self, cwd: Option<PathBuf>) -> PaneId {
        self.created += 1;
        let mut space = Space::new(format!("Space {}", self.created), cwd);
        space.color = Some(Accent::rotation(self.created - 1));
        let pane = space.tabs[0].focused;
        self.spaces.push(space);
        self.active = self.spaces.len() - 1;
        pane
    }

    /// Adds a terminal on its own, after the existing entries, and makes it the active one.
    pub fn add_terminal(&mut self, cwd: Option<PathBuf>) -> PaneId {
        let tab = Tab::new(cwd);
        let pane = tab.focused;
        self.spaces.push(Space::with_tab(SpaceKind::Terminal, String::new(), tab));
        self.active = self.spaces.len() - 1;
        pane
    }

    /// Turns a terminal into a space (so it can have tabs). Returns whether anything changed.
    pub fn promote_to_space(&mut self, index: usize) -> bool {
        let Some(space) = self.spaces.get_mut(index).filter(|space| space.is_terminal()) else {
            return false;
        };
        self.created += 1;
        space.kind = SpaceKind::Space;
        space.name = format!("Space {}", self.created);
        space.color = Some(Accent::rotation(self.created - 1));
        true
    }

    /// Moves a terminal into a space, as one more tab (the last one, and the one shown). The
    /// terminal entry disappears and the space becomes the active one. Fails (returns `false`)
    /// unless `terminal` is a terminal and `space` is a space.
    pub fn adopt_terminal(&mut self, terminal: usize, space: usize) -> bool {
        let valid = self.spaces.get(terminal).is_some_and(Space::is_terminal)
            && self.spaces.get(space).is_some_and(|target| !target.is_terminal());
        if !valid {
            return false;
        }
        let mut moved = self.spaces.remove(terminal);
        let target = if space > terminal { space - 1 } else { space };
        let tab = moved.tabs.remove(0);
        let destination = &mut self.spaces[target];
        destination.tabs.push(tab);
        destination.active = destination.tabs.len() - 1;
        self.select_space(target);
        true
    }

    /// Takes a tab out of the active space and leaves it as a terminal on its own, right after
    /// that space, which becomes the active entry. Fails (returns `false`) when the space would be
    /// left without tabs, or when the active entry is itself a terminal.
    pub fn detach_tab(&mut self, tab: usize) -> bool {
        let source = self.active;
        let space = &mut self.spaces[source];
        if space.is_terminal() || tab >= space.tabs.len() || space.tabs.len() < 2 {
            return false;
        }
        let mut moved = space.tabs.remove(tab);
        space.active = shift_after_removal(space.active, tab, space.tabs.len());
        space.tabs[space.active].sleeping = false;
        // What is shown runs.
        moved.sleeping = false;
        let terminal = Space::with_tab(SpaceKind::Terminal, String::new(), moved);
        self.spaces.insert(source + 1, terminal);
        self.select_space(source + 1);
        true
    }

    /// Makes a space the active one. A sleeping space wakes up.
    pub fn select_space(&mut self, index: usize) {
        if let Some(space) = self.spaces.get_mut(index) {
            space.state = SpaceState::Awake;
            space.tabs[space.active].sleeping = false;
            self.active = index;
        }
    }

    /// Moves a space to another position. The active space stays the same space. Returns
    /// whether the order changed. `to` past the end means the end.
    pub fn move_space(&mut self, from: usize, to: usize) -> bool {
        if from >= self.spaces.len() {
            return false;
        }
        let to = to.min(self.spaces.len() - 1);
        if from == to {
            return false;
        }
        let space = self.spaces.remove(from);
        self.spaces.insert(to, space);
        self.active = active_after_move(self.active, from, to);
        true
    }

    pub fn cycle_space(&mut self, forward: bool) {
        let next = cycle(self.active, self.spaces.len(), forward);
        self.select_space(next);
    }

    /// Puts a space to sleep. Fails if it already sleeps or if it is the only awake space.
    /// Sleeping the active space activates the next awake one.
    pub fn sleep_space(&mut self, index: usize) -> bool {
        let awake = self.spaces.iter().filter(|s| s.state == SpaceState::Awake).count();
        let Some(space) = self.spaces.get(index) else { return false };
        if space.state == SpaceState::Sleeping || awake <= 1 {
            return false;
        }
        self.spaces[index].state = SpaceState::Sleeping;
        if self.active == index {
            let len = self.spaces.len();
            if let Some(next) = (1..len)
                .map(|step| (index + step) % len)
                .find(|i| self.spaces[*i].state == SpaceState::Awake)
            {
                self.active = next;
            }
        }
        true
    }

    /// Sleeps every space except the active one (what a fresh start does after loading).
    pub fn sleep_all_but_active(&mut self) {
        for (index, space) in self.spaces.iter_mut().enumerate() {
            space.state =
                if index == self.active { SpaceState::Awake } else { SpaceState::Sleeping };
            if index == self.active {
                space.tabs[space.active].sleeping = false;
            }
        }
    }

    /// Panes of one space, in tab then visual order.
    pub fn space_panes(&self, index: usize) -> Vec<&Pane> {
        let Some(space) = self.spaces.get(index) else { return Vec::new() };
        space
            .tabs
            .iter()
            .flat_map(|tab| tab.pane_ids().into_iter().filter_map(|id| tab.pane(id)))
            .collect()
    }

    pub fn to_data(&self) -> WorkspaceData {
        WorkspaceData {
            spaces: self
                .spaces
                .iter()
                .map(|space| SpaceData {
                    id: space.id,
                    name: space.name.clone(),
                    state: space.state,
                    color: space.color,
                    kind: space.kind,
                    active_tab: space.active,
                    tabs: space
                        .tabs
                        .iter()
                        .map(|tab| TabData {
                            id: tab.id,
                            title: tab.title.clone(),
                            color: tab.color,
                            sleeping: tab.sleeping,
                            layout: tab.layout.clone(),
                            focused: tab.focused,
                            panes: tab
                                .pane_ids()
                                .into_iter()
                                .filter_map(|id| tab.pane(id).cloned())
                                .collect(),
                        })
                        .collect(),
                })
                .collect(),
            active: self.active,
            created: self.created,
        }
    }

    /// Rebuilds a workspace from saved data, rejecting data that breaks the model's rules.
    pub fn from_data(data: WorkspaceData) -> Result<Self, InvalidData> {
        if data.spaces.is_empty() {
            return Err(InvalidData("no spaces"));
        }
        if data.active >= data.spaces.len() {
            return Err(InvalidData("active space out of range"));
        }
        let mut seen = HashSet::new();
        let mut spaces = Vec::with_capacity(data.spaces.len());
        for space in data.spaces {
            if space.tabs.is_empty() {
                return Err(InvalidData("space without tabs"));
            }
            if space.active_tab >= space.tabs.len() {
                return Err(InvalidData("active tab out of range"));
            }
            if space.kind == SpaceKind::Terminal && space.tabs.len() != 1 {
                return Err(InvalidData("terminal with more than one tab"));
            }
            let mut tabs = Vec::with_capacity(space.tabs.len());
            for tab in space.tabs {
                let layout_panes = tab.layout.panes();
                let pane_ids: HashSet<PaneId> = tab.panes.iter().map(|p| p.id).collect();
                let layout_ids: HashSet<PaneId> = layout_panes.iter().copied().collect();
                if layout_ids != pane_ids || layout_ids.len() != layout_panes.len() {
                    return Err(InvalidData("layout and panes disagree"));
                }
                if !layout_ids.contains(&tab.focused) {
                    return Err(InvalidData("focused pane not in layout"));
                }
                if !layout_ids.iter().all(|id| seen.insert(*id)) {
                    return Err(InvalidData("duplicate pane id"));
                }
                tabs.push(Tab {
                    id: tab.id,
                    title: tab.title,
                    color: tab.color,
                    // The tab being shown always runs.
                    sleeping: tab.sleeping
                        && !(space.state == SpaceState::Awake && tabs.len() == space.active_tab),
                    layout: tab.layout,
                    focused: tab.focused,
                    panes: tab.panes.into_iter().map(|p| (p.id, p)).collect(),
                });
            }
            spaces.push(Space {
                id: space.id,
                name: space.name,
                state: space.state,
                color: space.color,
                kind: space.kind,
                tabs,
                active: space.active_tab,
            });
        }
        Ok(Self { spaces, active: data.active, created: data.created.max(1) })
    }

    pub fn set_pane_agent(&mut self, id: PaneId, agent: Option<AgentMemo>) {
        if let Some(pane) = self.pane_mut(id) {
            pane.agent = agent;
        }
    }

    /// Closes a space. The last remaining space cannot be closed.
    pub fn close_space(&mut self, index: usize) -> bool {
        if self.spaces.len() <= 1 || index >= self.spaces.len() {
            return false;
        }
        self.spaces.remove(index);
        self.active = shift_after_removal(self.active, index, self.spaces.len());
        true
    }

    /// Deletes a space for good. Deleting the last one leaves a fresh empty space instead,
    /// because a workspace always has at least one. Returns the panes that were removed.
    pub fn delete_space(&mut self, index: usize, cwd: Option<PathBuf>) -> Vec<PaneId> {
        if index >= self.spaces.len() {
            return Vec::new();
        }
        let removed: Vec<PaneId> = self.space_panes(index).iter().map(|pane| pane.id).collect();
        if self.spaces.len() == 1 {
            self.add_space(cwd);
        }
        self.close_space(index);
        removed
    }

    pub fn set_space_color(&mut self, index: usize, color: Option<Accent>) {
        if let Some(space) = self.spaces.get_mut(index) {
            space.color = color;
        }
    }

    /// Colors a tab of the active space.
    pub fn set_tab_color(&mut self, index: usize, color: Option<Accent>) {
        if let Some(tab) = self.active_space_mut().tabs.get_mut(index) {
            tab.color = color;
        }
    }

    /// Puts a tab of the active space to sleep. Fails if it already sleeps or if it is the only
    /// awake tab (sleep the whole space instead). Sleeping the shown tab shows the next awake one.
    pub fn sleep_tab(&mut self, index: usize) -> bool {
        let space = self.active_space_mut();
        let awake = space.tabs.iter().filter(|tab| !tab.sleeping).count();
        let Some(tab) = space.tabs.get(index) else { return false };
        if tab.sleeping || awake <= 1 {
            return false;
        }
        space.tabs[index].sleeping = true;
        if space.active == index {
            let len = space.tabs.len();
            if let Some(next) =
                (1..len).map(|step| (index + step) % len).find(|i| !space.tabs[*i].sleeping)
            {
                space.active = next;
            }
        }
        true
    }

    /// Panes of a tab of the active space, in visual order.
    pub fn tab_panes(&self, index: usize) -> Vec<&Pane> {
        let Some(tab) = self.active_space().tabs.get(index) else { return Vec::new() };
        tab.pane_ids().into_iter().filter_map(|id| tab.pane(id)).collect()
    }

    /// Renames a space. A terminal is named by its single tab: an empty name makes it follow
    /// its program again.
    pub fn rename_space(&mut self, index: usize, name: String) {
        if let Some(space) = self.spaces.get_mut(index) {
            if space.is_terminal() {
                space.tabs[0].title = Some(name).filter(|title| !title.is_empty());
            } else {
                space.name = name;
            }
        }
    }

    pub fn rename_active_tab(&mut self, title: String) {
        self.active_space_mut().active_tab_mut().title = Some(title).filter(|t| !t.is_empty());
    }

    /// Renames a tab of the active space. An empty name makes it follow the program again.
    pub fn rename_tab(&mut self, index: usize, title: String) {
        if let Some(tab) = self.active_space_mut().tabs.get_mut(index) {
            tab.title = Some(title).filter(|t| !t.is_empty());
        }
    }

    /// Adds a tab to the active space. A terminal has no tabs to add to, so it gets a new
    /// terminal beside it instead.
    pub fn add_tab(&mut self, cwd: Option<PathBuf>) -> PaneId {
        if self.active_space().is_terminal() {
            return self.add_terminal(cwd);
        }
        let tab = Tab::new(cwd);
        let pane = tab.focused;
        let space = self.active_space_mut();
        space.tabs.push(tab);
        space.active = space.tabs.len() - 1;
        pane
    }

    pub fn select_tab(&mut self, index: usize) {
        let space = self.active_space_mut();
        if index < space.tabs.len() {
            space.active = index;
            space.tabs[index].sleeping = false;
        }
    }

    /// Moves a tab of the active space to another position. The active tab stays the same tab.
    /// Returns whether the order changed. `to` past the end means the end.
    pub fn move_tab(&mut self, from: usize, to: usize) -> bool {
        let space = self.active_space_mut();
        if from >= space.tabs.len() {
            return false;
        }
        let to = to.min(space.tabs.len() - 1);
        if from == to {
            return false;
        }
        let tab = space.tabs.remove(from);
        space.tabs.insert(to, tab);
        space.active = active_after_move(space.active, from, to);
        true
    }

    /// Moves a tab of the active space to the end of another space, keeping its terminals.
    /// Fails (returns `false`) when the space would be left with no tab, for the same space, or
    /// when the destination is a terminal (a terminal has exactly one tab).
    /// The active tab of the source stays the same tab; the destination's active tab does not
    /// change. The caller must stop the tab's live terminals first when the destination sleeps.
    pub fn move_tab_to_space(&mut self, tab: usize, space: usize) -> bool {
        let source = self.active;
        if space == source
            || space >= self.spaces.len()
            || self.spaces[space].is_terminal()
            || tab >= self.spaces[source].tabs.len()
            || self.spaces[source].tabs.len() < 2
        {
            return false;
        }
        let from = &mut self.spaces[source];
        let moved = from.tabs.remove(tab);
        from.active = shift_after_removal(from.active, tab, from.tabs.len());
        from.tabs[from.active].sleeping = false;
        self.spaces[space].tabs.push(moved);
        true
    }

    pub fn cycle_tab(&mut self, forward: bool) {
        let space = self.active_space_mut();
        space.active = cycle(space.active, space.tabs.len(), forward);
        space.tabs[space.active].sleeping = false;
    }

    pub fn split_focused(&mut self, axis: Axis, cwd: Option<PathBuf>) -> PaneId {
        let new = Pane::new(cwd);
        let id = new.id;
        let tab = self.active_space_mut().active_tab_mut();
        let focused = tab.focused;
        tab.layout.split(focused, axis, id);
        tab.panes.insert(id, new);
        tab.focused = id;
        id
    }

    /// Closes a pane, and the tab or space that becomes empty because of it.
    ///
    /// Closing the very last pane returns [`CloseOutcome::WorkspaceEmpty`] and leaves the
    /// model untouched, so it stays valid while the app shuts down.
    pub fn close_pane(&mut self, id: PaneId) -> CloseOutcome {
        let Some(space_index) = self.spaces.iter().position(|s| s.locate(id).is_some()) else {
            return CloseOutcome::NotFound;
        };
        let only_space = self.spaces.len() == 1;
        let space = &mut self.spaces[space_index];
        let Some(tab_index) = space.locate(id) else {
            return CloseOutcome::NotFound;
        };
        if only_space && space.tabs.len() == 1 && space.tabs[tab_index].pane_count() == 1 {
            return CloseOutcome::WorkspaceEmpty;
        }
        if !space.tabs[tab_index].remove_pane(id) {
            return CloseOutcome::PaneClosed;
        }

        space.tabs.remove(tab_index);
        if !space.tabs.is_empty() {
            space.active = shift_after_removal(space.active, tab_index, space.tabs.len());
            space.tabs[space.active].sleeping = false;
            return CloseOutcome::TabClosed;
        }

        self.spaces.remove(space_index);
        self.active = shift_after_removal(self.active, space_index, self.spaces.len());
        CloseOutcome::SpaceClosed
    }

    /// Closes a tab of the active space with all its panes.
    pub fn close_tab(&mut self, index: usize) -> CloseOutcome {
        let Some(tab) = self.active_space().tabs.get(index) else {
            return CloseOutcome::NotFound;
        };
        let mut outcome = CloseOutcome::NotFound;
        for id in tab.pane_ids() {
            outcome = self.close_pane(id);
        }
        outcome
    }

    /// Focuses a pane, switching to its space and tab if needed.
    pub fn focus_pane(&mut self, id: PaneId) {
        for (space_index, space) in self.spaces.iter_mut().enumerate() {
            if let Some(tab_index) = space.locate(id) {
                space.active = tab_index;
                space.tabs[tab_index].sleeping = false;
                space.tabs[tab_index].focused = id;
                self.active = space_index;
                return;
            }
        }
    }

    pub fn focus_direction(&mut self, direction: Direction) {
        let tab = self.active_space_mut().active_tab_mut();
        if let Some(next) = tab.layout.neighbor(tab.focused, direction) {
            tab.focused = next;
        }
    }

    pub fn focus_cycle(&mut self, forward: bool) {
        let tab = self.active_space_mut().active_tab_mut();
        let order = tab.layout.panes();
        if let Some(position) = order.iter().position(|p| *p == tab.focused) {
            tab.focused = order[cycle(position, order.len(), forward)];
        }
    }

    /// Moves the divider next to the focused pane. Positive `delta` grows the first child.
    pub fn resize_focused(&mut self, axis: Axis, delta: f32) {
        let tab = self.active_space_mut().active_tab_mut();
        let focused = tab.focused;
        tab.layout.adjust_ratio(focused, axis, delta);
    }

    pub fn set_pane_title(&mut self, id: PaneId, title: String) {
        if let Some(pane) = self.pane_mut(id) {
            pane.title = title;
        }
    }

    /// Records what runs in a pane's foreground. Returns whether anything changed. A program
    /// that was already named in the title stays named while it only changes its own details.
    pub fn set_pane_program(&mut self, id: PaneId, mut program: Option<Program>) -> bool {
        match self.pane_mut(id) {
            Some(pane) => {
                if let (Some(before), Some(next)) = (&pane.program, &mut program) {
                    next.visible = before.visible;
                }
                if pane.program == program {
                    return false;
                }
                pane.program = program;
                true
            }
            None => false,
        }
    }

    /// The program of a pane has run long enough to name the tab. Returns whether it changed.
    pub fn mark_program_visible(&mut self, id: PaneId) -> bool {
        match self.pane_mut(id).and_then(|pane| pane.program.as_mut()) {
            Some(program) if !program.visible => {
                program.visible = true;
                true
            }
            _ => false,
        }
    }

    /// Records what a pane's shell reports about itself. Returns whether anything changed.
    pub fn set_pane_context(&mut self, id: PaneId, context: PaneContext) -> bool {
        match self.pane_mut(id) {
            Some(pane) if pane.context != context => {
                pane.context = context;
                true
            }
            _ => false,
        }
    }

    pub fn set_pane_cwd(&mut self, id: PaneId, cwd: Option<PathBuf>) {
        if let Some(pane) = self.pane_mut(id) {
            pane.cwd = cwd;
        }
    }

    fn pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        self.spaces.iter_mut().flat_map(|s| &mut s.tabs).find_map(|t| t.panes.get_mut(&id))
    }
}

/// Plain, serializable copy of the whole workspace, used for persistence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceData {
    pub spaces: Vec<SpaceData>,
    pub active: usize,
    pub created: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpaceData {
    pub id: SpaceId,
    pub name: String,
    pub state: SpaceState,
    #[serde(default)]
    pub color: Option<Accent>,
    /// Saved before terminals existed: absent means a space.
    #[serde(default)]
    pub kind: SpaceKind,
    pub active_tab: usize,
    pub tabs: Vec<TabData>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabData {
    pub id: TabId,
    pub title: Option<String>,
    #[serde(default)]
    pub color: Option<Accent>,
    #[serde(default)]
    pub sleeping: bool,
    pub layout: Layout,
    pub focused: PaneId,
    pub panes: Vec<Pane>,
}

/// Why saved data cannot be turned back into a workspace.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid saved workspace: {0}")]
pub struct InvalidData(pub &'static str);

/// The home folder, read once.
fn home_dir() -> Option<&'static Path> {
    static HOME: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    HOME.get_or_init(|| std::env::var_os("HOME").map(PathBuf::from).filter(|p| p.is_absolute()))
        .as_deref()
}

/// A folder as a short name: its last component, `~` for `home`, `/` for the root.
fn folder_label<'a>(path: &'a Path, home: Option<&Path>) -> Option<&'a str> {
    if home.is_some_and(|home| home == path) {
        return Some("~");
    }
    match path.file_name() {
        Some(name) => name.to_str(),
        None => path.to_str().filter(|text| *text == "/"),
    }
}

fn cycle(index: usize, len: usize, forward: bool) -> usize {
    if len == 0 {
        0
    } else if forward {
        (index + 1) % len
    } else {
        (index + len - 1) % len
    }
}

/// The index of the active item after the item at `from` moved to `to`.
fn active_after_move(active: usize, from: usize, to: usize) -> usize {
    if active == from {
        to
    } else if from < active && to >= active {
        active - 1
    } else if from > active && to <= active {
        active + 1
    } else {
        active
    }
}

/// New active index after removing `removed` from a list that now has `len` items.
fn shift_after_removal(active: usize, removed: usize, len: usize) -> usize {
    let adjusted = match active.cmp(&removed) {
        std::cmp::Ordering::Greater => active - 1,
        std::cmp::Ordering::Equal => removed.saturating_sub(1),
        std::cmp::Ordering::Less => active,
    };
    adjusted.min(len.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> Workspace {
        Workspace::new(Some(PathBuf::from("/tmp")))
    }

    /// A workspace whose active space has `count` tabs, each with one pane; returns the pane ids
    /// in tab order.
    fn with_tabs(count: usize) -> (Workspace, Vec<PaneId>) {
        let mut ws = workspace();
        let mut ids = vec![ws.focused_pane()];
        for _ in 1..count {
            ids.push(ws.add_tab(None));
        }
        (ws, ids)
    }

    fn tab_order(ws: &Workspace) -> Vec<PaneId> {
        ws.active_space().tabs().iter().map(Tab::focused).collect()
    }

    #[test]
    fn moving_a_tab_keeps_the_active_tab_the_same_tab() {
        for active in 0..4 {
            for from in 0..4 {
                for to in 0..4 {
                    let (mut ws, ids) = with_tabs(4);
                    ws.select_tab(active);
                    ws.move_tab(from, to);
                    let mut expected = ids.clone();
                    let moved = expected.remove(from);
                    expected.insert(to, moved);
                    assert_eq!(tab_order(&ws), expected, "order for {from}->{to}");
                    assert_eq!(
                        ws.active_tab().focused(),
                        ids[active],
                        "active tab after {from}->{to} with {active} active"
                    );
                }
            }
        }
    }

    #[test]
    fn moving_a_tab_to_where_it_is_or_out_of_range_changes_nothing() {
        let (mut ws, ids) = with_tabs(3);
        assert!(!ws.move_tab(1, 1));
        assert!(!ws.move_tab(7, 0));
        assert_eq!(tab_order(&ws), ids);
        // Past the end means the end.
        assert!(ws.move_tab(0, 99));
        assert_eq!(tab_order(&ws), vec![ids[1], ids[2], ids[0]]);
    }

    #[test]
    fn moving_a_space_keeps_the_active_space_the_same_space() {
        for active in 0..4 {
            for from in 0..4 {
                for to in 0..4 {
                    let mut ws = workspace();
                    for _ in 1..4 {
                        ws.add_space(None);
                    }
                    let ids: Vec<_> = ws.spaces().iter().map(|s| s.id).collect();
                    ws.select_space(active);
                    ws.move_space(from, to);
                    let mut expected = ids.clone();
                    let moved = expected.remove(from);
                    expected.insert(to, moved);
                    let order: Vec<_> = ws.spaces().iter().map(|s| s.id).collect();
                    assert_eq!(order, expected, "order for {from}->{to}");
                    assert_eq!(ws.active_space().id, ids[active], "{from}->{to}, {active} active");
                }
            }
        }
    }

    #[test]
    fn a_tab_moves_to_another_space_with_its_panes() {
        let (mut ws, ids) = with_tabs(3);
        ws.add_space(None);
        let other = ws.active_space().id;
        ws.select_space(0);
        ws.select_tab(1);
        assert!(ws.move_tab_to_space(1, 1));
        // The source keeps its other tabs and a valid active tab.
        assert_eq!(tab_order(&ws), vec![ids[0], ids[2]]);
        assert!(ws.active_space().active_tab_index() < 2);
        assert!(!ws.active_tab().sleeping);
        // The destination got the tab at its end, and its panes are still findable.
        let destination = &ws.spaces()[1];
        assert_eq!(destination.id, other);
        assert_eq!(destination.tabs().last().map(Tab::focused), Some(ids[1]));
        assert!(ws.pane(ids[1]).is_some());
        assert_eq!(ws.spaces()[1].tabs().len(), 2);
    }

    #[test]
    fn a_space_never_gives_away_its_only_tab_nor_a_tab_to_itself() {
        let mut ws = workspace();
        ws.add_space(None);
        ws.select_space(0);
        assert!(!ws.move_tab_to_space(0, 1), "the only tab stays");
        ws.add_tab(None);
        assert!(!ws.move_tab_to_space(0, 0), "same space");
        assert!(!ws.move_tab_to_space(5, 1), "no such tab");
        assert!(!ws.move_tab_to_space(0, 9), "no such space");
    }

    #[test]
    fn moves_survive_saving_and_loading() {
        let (mut ws, ids) = with_tabs(3);
        ws.add_space(None);
        ws.select_space(0);
        ws.move_tab(0, 2);
        ws.move_tab_to_space(0, 1);
        ws.move_space(0, 1);
        let loaded = Workspace::from_data(ws.to_data()).expect("valid data");
        let order = |w: &Workspace| -> Vec<Vec<PaneId>> {
            w.spaces().iter().map(|s| s.tabs().iter().map(Tab::focused).collect()).collect()
        };
        assert_eq!(order(&loaded), order(&ws));
        assert_eq!(loaded.active_index(), ws.active_index());
        let moved = ids[1];
        assert!(loaded.pane(moved).is_some());
    }

    #[test]
    fn starts_with_one_space_one_tab_one_pane() {
        let ws = workspace();
        assert_eq!(ws.spaces().len(), 1);
        assert_eq!(ws.active_space().tabs().len(), 1);
        assert_eq!(ws.active_tab().pane_count(), 1);
        assert_eq!(ws.awake_panes().len(), 1);
    }

    #[test]
    fn splitting_focuses_the_new_pane_and_keeps_the_directory() {
        let mut ws = workspace();
        let first = ws.focused_pane();
        let second = ws.split_focused(Axis::Row, Some(PathBuf::from("/var")));
        assert_ne!(first, second);
        assert_eq!(ws.focused_pane(), second);
        assert_eq!(ws.pane(second).and_then(|p| p.cwd.clone()), Some(PathBuf::from("/var")));
        assert_eq!(ws.active_tab().pane_ids(), vec![first, second]);
    }

    #[test]
    fn closing_one_of_two_panes_refocuses_the_other() {
        let mut ws = workspace();
        let first = ws.focused_pane();
        let second = ws.split_focused(Axis::Column, None);
        assert_eq!(ws.close_pane(second), CloseOutcome::PaneClosed);
        assert_eq!(ws.focused_pane(), first);
        assert_eq!(ws.active_tab().pane_count(), 1);
    }

    #[test]
    fn closing_the_last_pane_closes_the_tab_then_the_space_then_the_app() {
        let mut ws = workspace();
        let tab_pane = ws.add_tab(None);
        let space_pane = ws.add_space(None);
        assert_eq!(ws.spaces().len(), 2);

        assert_eq!(ws.close_pane(space_pane), CloseOutcome::SpaceClosed);
        assert_eq!(ws.spaces().len(), 1);
        assert_eq!(ws.close_pane(tab_pane), CloseOutcome::TabClosed);
        let last = ws.focused_pane();
        assert_eq!(ws.close_pane(last), CloseOutcome::WorkspaceEmpty);
        assert_eq!(ws.awake_panes().len(), 1, "the model stays valid");
        assert_eq!(ws.focused_pane(), last);
    }

    #[test]
    fn closing_an_unknown_pane_changes_nothing() {
        let mut ws = workspace();
        assert_eq!(ws.close_pane(PaneId::new()), CloseOutcome::NotFound);
        assert_eq!(ws.awake_panes().len(), 1);
    }

    #[test]
    fn focus_pane_switches_space_and_tab() {
        let mut ws = workspace();
        let first = ws.focused_pane();
        ws.add_tab(None);
        ws.add_space(None);
        assert_eq!(ws.active_index(), 1);
        ws.focus_pane(first);
        assert_eq!(ws.active_index(), 0);
        assert_eq!(ws.active_space().active_tab_index(), 0);
        assert_eq!(ws.focused_pane(), first);
    }

    #[test]
    fn closing_a_tab_removes_all_its_panes() {
        let mut ws = workspace();
        ws.add_tab(None);
        ws.split_focused(Axis::Row, None);
        assert_eq!(ws.active_tab().pane_count(), 2);
        assert_eq!(ws.awake_panes().len(), 3);

        assert_eq!(ws.close_tab(1), CloseOutcome::TabClosed);
        assert_eq!(ws.awake_panes().len(), 1);
        assert_eq!(ws.active_space().active_tab_index(), 0);
        assert_eq!(ws.close_tab(5), CloseOutcome::NotFound);
        assert_eq!(ws.close_tab(0), CloseOutcome::WorkspaceEmpty);
    }

    #[test]
    fn tabs_and_spaces_cycle_around() {
        let mut ws = workspace();
        ws.add_tab(None);
        ws.add_tab(None);
        assert_eq!(ws.active_space().active_tab_index(), 2);
        ws.cycle_tab(true);
        assert_eq!(ws.active_space().active_tab_index(), 0);
        ws.cycle_tab(false);
        assert_eq!(ws.active_space().active_tab_index(), 2);

        ws.add_space(None);
        ws.cycle_space(true);
        assert_eq!(ws.active_index(), 0);
    }

    #[test]
    fn the_last_space_cannot_be_closed_and_active_follows_removals() {
        let mut ws = workspace();
        assert!(!ws.close_space(0));
        ws.add_space(None);
        ws.add_space(None);
        assert_eq!(ws.active_index(), 2);
        assert!(ws.close_space(0));
        assert_eq!(ws.active_index(), 1);
        assert!(ws.close_space(1));
        assert_eq!(ws.active_index(), 0);
    }

    /// A program as it is once it has been in the foreground long enough to name the tab.
    fn shown(program: Program) -> Program {
        Program { visible: true, ..program }
    }

    #[test]
    fn an_idle_tab_is_named_after_its_folder_then_the_shell_title() {
        let mut ws = Workspace::new(None);
        let pane = ws.focused_pane();
        assert_eq!(ws.active_tab().display_title(), "shell", "nothing is known yet");
        ws.set_pane_title(pane, "vim".to_owned());
        assert_eq!(ws.active_tab().display_title(), "vim", "no folder: the shell's title");
        ws.set_pane_cwd(pane, Some(PathBuf::from("/home/u/proj")));
        assert_eq!(ws.active_tab().display_title(), "proj", "the folder beats the shell's title");
        ws.rename_active_tab("editor".to_owned());
        assert_eq!(ws.active_tab().display_title(), "editor", "the user's name beats everything");
        ws.rename_active_tab(String::new());
        assert_eq!(ws.active_tab().display_title(), "proj");
    }

    #[test]
    fn the_live_folder_beats_the_saved_one() {
        let mut ws = workspace();
        let pane = ws.focused_pane();
        ws.set_pane_cwd(pane, Some(PathBuf::from("/old/place")));
        assert_eq!(ws.active_tab().display_title(), "place");
        let context =
            PaneContext { cwd: Some(PathBuf::from("/now/here")), ..PaneContext::default() };
        ws.set_pane_context(pane, context);
        assert_eq!(ws.active_tab().display_title(), "here");
    }

    #[test]
    fn a_running_program_names_the_tab_only_once_it_is_visible() {
        let mut ws = workspace();
        let pane = ws.focused_pane();
        ws.set_pane_cwd(pane, Some(PathBuf::from("/home/u/mer")));
        let program = Program { label: "ionic".to_owned(), ..Program::default() };
        assert!(ws.set_pane_program(pane, Some(program.clone())));
        assert!(!ws.set_pane_program(pane, Some(program)));
        assert_eq!(ws.active_tab().display_title(), "mer", "a blink of a command is not shown");
        assert_eq!(ws.active_tab().display_place(), None);

        assert!(ws.mark_program_visible(pane));
        assert!(!ws.mark_program_visible(pane), "already visible");
        assert_eq!(ws.active_tab().display_title(), "ionic");
        assert_eq!(ws.active_tab().display_place(), Some("mer"), "the folder, beside it");

        // The exact command beats the process label; the program's own title beats both.
        let with_command = shown(Program {
            label: "ionic".to_owned(),
            command: Some("ionic serve".to_owned()),
            ..Program::default()
        });
        ws.set_pane_program(pane, Some(with_command.clone()));
        assert_eq!(ws.active_tab().display_title(), "ionic serve");
        let titled = Program { title: Some("✳ Claude Code".to_owned()), ..with_command };
        ws.set_pane_program(pane, Some(titled));
        assert_eq!(ws.active_tab().display_title(), "✳ Claude Code");

        // A name chosen by the user always wins; with nothing running the folder returns.
        ws.rename_active_tab("front".to_owned());
        assert_eq!(ws.active_tab().display_title(), "front");
        ws.rename_active_tab(String::new());
        ws.set_pane_program(pane, None);
        assert_eq!(ws.active_tab().display_title(), "mer");
        assert_eq!(ws.active_tab().display_place(), None, "the title already is the folder");
    }

    #[test]
    fn a_visible_program_stays_visible_while_its_details_change() {
        let mut ws = workspace();
        let pane = ws.focused_pane();
        let first = Program { command: Some("npm run dev".to_owned()), ..Program::default() };
        ws.set_pane_program(pane, Some(first));
        ws.mark_program_visible(pane);
        // The process is seen a moment later and fills in its label: still the same program.
        let seen = Program {
            label: "node".to_owned(),
            command: Some("npm run dev".to_owned()),
            ..Program::default()
        };
        assert!(ws.set_pane_program(pane, Some(seen)));
        assert!(ws.pane(pane).and_then(|pane| pane.program.as_ref()).is_some_and(|p| p.visible));
        assert_eq!(ws.active_tab().display_title(), "npm run dev");
        // A different command starts hidden again.
        ws.set_pane_program(pane, None);
        ws.set_pane_program(pane, Some(Program { label: "ls".to_owned(), ..Program::default() }));
        assert!(!ws.pane(pane).and_then(|pane| pane.program.as_ref()).is_some_and(|p| p.visible));
    }

    #[test]
    fn a_folder_is_named_by_its_last_component() {
        let home = Path::new("/home/u");
        assert_eq!(folder_label(Path::new("/home/u/dev/mer"), Some(home)), Some("mer"));
        assert_eq!(folder_label(Path::new("/home/u/dev/mer/"), Some(home)), Some("mer"));
        assert_eq!(folder_label(Path::new("/home/u"), Some(home)), Some("~"));
        assert_eq!(folder_label(Path::new("/home/u"), None), Some("u"));
        assert_eq!(folder_label(Path::new("/"), Some(home)), Some("/"));
        assert_eq!(folder_label(Path::new("relative"), None), Some("relative"));
        assert_eq!(folder_label(Path::new(""), None), None);
    }

    #[test]
    fn sleeping_moves_focus_and_selecting_wakes() {
        let mut ws = workspace();
        let first_pane = ws.focused_pane();
        ws.add_space(None);
        ws.add_space(None);
        assert_eq!(ws.active_index(), 2);

        assert!(ws.sleep_space(2));
        assert_eq!(ws.active_index(), 0);
        assert_eq!(ws.spaces()[2].state, SpaceState::Sleeping);
        assert_eq!(ws.awake_panes().len(), 2);
        assert!(!ws.sleep_space(2), "already sleeping");

        ws.select_space(2);
        assert_eq!(ws.spaces()[2].state, SpaceState::Awake);
        assert_eq!(ws.awake_panes().len(), 3);
        assert!(ws.pane(first_pane).is_some());
    }

    #[test]
    fn the_last_awake_space_cannot_sleep() {
        let mut ws = workspace();
        assert!(!ws.sleep_space(0));
        ws.add_space(None);
        assert!(ws.sleep_space(0));
        assert!(!ws.sleep_space(1));
    }

    #[test]
    fn cycling_onto_a_sleeping_space_wakes_it() {
        let mut ws = workspace();
        ws.add_space(None);
        ws.sleep_space(0);
        ws.cycle_space(true);
        assert_eq!(ws.active_index(), 0);
        assert_eq!(ws.spaces()[0].state, SpaceState::Awake);
    }

    fn rich_workspace() -> Workspace {
        let mut ws = workspace();
        ws.split_focused(Axis::Row, Some(PathBuf::from("/var")));
        let focused = ws.focused_pane();
        ws.set_pane_title(focused, "vim".to_owned());
        ws.set_pane_agent(
            focused,
            Some(AgentMemo { kind: "claude".to_owned(), session_id: Some("abc".to_owned()) }),
        );
        ws.add_tab(None);
        ws.rename_active_tab("logs".to_owned());
        ws.add_space(Some(PathBuf::from("/opt")));
        ws.rename_space(1, "api".to_owned());
        ws.sleep_space(0);
        ws
    }

    #[test]
    fn data_round_trips_through_the_model() {
        let ws = rich_workspace();
        let data = ws.to_data();

        let back = Workspace::from_data(data.clone()).expect("valid data");
        assert_eq!(back.to_data(), data);
        assert_eq!(back.spaces()[0].state, SpaceState::Sleeping);
        assert_eq!(back.spaces()[1].name, "api");
    }

    #[test]
    fn from_data_rejects_broken_data() {
        let good = rich_workspace().to_data();

        let mut data = good.clone();
        data.spaces.clear();
        assert!(Workspace::from_data(data).is_err());

        let mut data = good.clone();
        data.active = 99;
        assert!(Workspace::from_data(data).is_err());

        let mut data = good.clone();
        data.spaces[0].tabs[0].focused = PaneId::new();
        assert!(Workspace::from_data(data).is_err());

        let mut data = good.clone();
        data.spaces[0].tabs[0].panes.pop();
        assert!(Workspace::from_data(data).is_err());

        let mut data = good;
        data.spaces[1].tabs.clear();
        assert!(Workspace::from_data(data).is_err());
    }

    #[test]
    fn sleep_all_but_active_keeps_one_awake() {
        let mut ws = rich_workspace();
        ws.select_space(0);
        ws.sleep_all_but_active();
        assert_eq!(ws.spaces()[0].state, SpaceState::Awake);
        assert_eq!(ws.spaces()[1].state, SpaceState::Sleeping);
        assert_eq!(ws.space_panes(0).len(), 3);
    }

    #[test]
    fn sleeping_spaces_have_no_awake_panes() {
        let mut ws = workspace();
        ws.add_space(None);
        ws.spaces[0].state = SpaceState::Sleeping;
        assert_eq!(ws.awake_panes().len(), 1);
        assert_eq!(ws.spaces()[0].pane_count(), 1);
    }

    #[test]
    fn new_spaces_get_different_colors() {
        let mut ws = workspace();
        ws.add_space(None);
        ws.add_space(None);
        let colors: Vec<_> = ws.spaces().iter().map(|s| s.color).collect();
        assert!(colors.iter().all(Option::is_some));
        assert_ne!(colors[0], colors[1]);
        assert_ne!(colors[1], colors[2]);
        ws.set_space_color(1, None);
        assert_eq!(ws.spaces()[1].color, None);
        ws.set_tab_color(0, Some(Accent::Pink));
        assert_eq!(ws.active_space().tabs()[0].color, Some(Accent::Pink));
    }

    #[test]
    fn sleeping_a_tab_drops_its_panes_and_selecting_wakes_it() {
        let mut ws = workspace();
        ws.add_tab(None);
        assert_eq!(ws.awake_panes().len(), 2);
        assert!(ws.sleep_tab(1), "the shown tab can sleep when another is awake");
        assert_eq!(ws.active_space().active_tab_index(), 0);
        assert_eq!(ws.awake_panes().len(), 1);
        assert!(!ws.sleep_tab(1), "already asleep");
        assert!(!ws.sleep_tab(0), "the only awake tab stays");
        ws.select_tab(1);
        assert!(!ws.active_space().tabs()[1].sleeping);
        assert_eq!(ws.awake_panes().len(), 2);
    }

    #[test]
    fn cycling_onto_a_sleeping_tab_wakes_it() {
        let mut ws = workspace();
        ws.add_tab(None);
        assert!(ws.sleep_tab(1));
        ws.cycle_tab(true);
        assert_eq!(ws.active_space().active_tab_index(), 1);
        assert!(!ws.active_tab().sleeping);
    }

    #[test]
    fn color_and_sleeping_survive_a_data_round_trip() {
        let mut ws = workspace();
        ws.add_tab(None);
        ws.set_tab_color(0, Some(Accent::Cyan));
        ws.sleep_tab(1);
        let restored = Workspace::from_data(ws.to_data()).unwrap();
        assert_eq!(restored.active_space().tabs()[0].color, Some(Accent::Cyan));
        assert!(restored.active_space().tabs()[1].sleeping);
        assert_eq!(restored.spaces()[0].color, ws.spaces()[0].color);
    }

    #[test]
    fn a_saved_shown_tab_never_comes_back_asleep() {
        let mut ws = workspace();
        ws.add_tab(None);
        let mut data = ws.to_data();
        data.spaces[0].tabs[1].sleeping = true;
        let restored = Workspace::from_data(data).unwrap();
        assert!(!restored.active_tab().sleeping);
    }

    #[test]
    fn deleting_a_space_reports_its_panes_and_never_leaves_none() {
        let mut ws = workspace();
        ws.add_space(None);
        ws.split_focused(Axis::Row, None);
        let doomed: Vec<_> = ws.space_panes(1).iter().map(|p| p.id).collect();
        assert_eq!(doomed.len(), 2);
        assert_eq!(ws.delete_space(1, None), doomed);
        assert_eq!(ws.spaces().len(), 1);

        let last = ws.focused_pane();
        let removed = ws.delete_space(0, None);
        assert_eq!(removed, vec![last]);
        assert_eq!(ws.spaces().len(), 1);
        assert_ne!(ws.focused_pane(), last);
        assert!(ws.delete_space(9, None).is_empty());
    }
    // ----- terminals on their own -----

    fn kinds(ws: &Workspace) -> Vec<SpaceKind> {
        ws.spaces().iter().map(Space::kind).collect()
    }

    #[test]
    fn a_terminal_is_a_space_with_exactly_one_tab() {
        let mut ws = workspace();
        let pane = ws.add_terminal(Some(PathBuf::from("/tmp")));
        assert_eq!(kinds(&ws), [SpaceKind::Space, SpaceKind::Terminal]);
        assert_eq!(ws.active_index(), 1);
        assert_eq!(ws.focused_pane(), pane);
        assert_eq!(ws.active_space().tabs().len(), 1);
        assert!(ws.active_space().is_terminal());
    }

    #[test]
    fn a_new_tab_inside_a_terminal_is_a_new_terminal() {
        let mut ws = workspace();
        ws.add_terminal(None);
        let pane = ws.add_tab(None);
        assert_eq!(kinds(&ws), [SpaceKind::Space, SpaceKind::Terminal, SpaceKind::Terminal]);
        assert_eq!(ws.focused_pane(), pane);
        assert!(ws.spaces().iter().all(|space| space.tabs().len() == 1 || !space.is_terminal()));
    }

    #[test]
    fn a_terminal_never_receives_a_tab() {
        let (mut ws, _) = with_tabs(3);
        ws.add_terminal(None);
        ws.select_space(0);
        assert!(!ws.move_tab_to_space(0, 1), "space 1 is a terminal");
        assert_eq!(ws.spaces()[0].tabs().len(), 3);
        assert_eq!(ws.spaces()[1].tabs().len(), 1);
    }

    #[test]
    fn a_terminal_joins_a_space_as_its_shown_tab() {
        let mut ws = workspace();
        let first = ws.focused_pane();
        let terminal = ws.add_terminal(None);
        ws.add_space(None);
        // order: space 0, terminal 1, space 2
        assert!(ws.adopt_terminal(1, 2));
        assert_eq!(kinds(&ws), [SpaceKind::Space, SpaceKind::Space]);
        assert_eq!(ws.active_index(), 1, "the space it joined is now active");
        assert_eq!(ws.active_space().tabs().len(), 2);
        assert_eq!(ws.focused_pane(), terminal, "the adopted tab is the one shown");
        assert_eq!(ws.spaces()[0].active_tab().focused(), first);
    }

    #[test]
    fn adopting_follows_the_rules() {
        let mut ws = workspace();
        ws.add_terminal(None);
        ws.add_terminal(None);
        assert!(!ws.adopt_terminal(1, 2), "a terminal cannot join a terminal");
        assert!(!ws.adopt_terminal(0, 1), "a space is not a terminal");
        assert!(!ws.adopt_terminal(1, 9) && !ws.adopt_terminal(9, 0));
        assert_eq!(ws.spaces().len(), 3);
        // The target before the terminal keeps its index.
        assert!(ws.adopt_terminal(2, 0));
        assert_eq!(ws.active_index(), 0);
        assert_eq!(kinds(&ws), [SpaceKind::Space, SpaceKind::Terminal]);
    }

    #[test]
    fn a_tab_can_leave_its_space_to_be_a_terminal() {
        let (mut ws, ids) = with_tabs(3);
        assert!(ws.detach_tab(1));
        assert_eq!(kinds(&ws), [SpaceKind::Space, SpaceKind::Terminal]);
        assert_eq!(ws.active_index(), 1);
        assert_eq!(ws.focused_pane(), ids[1]);
        assert_eq!(ws.spaces()[0].tabs().len(), 2);
        assert_eq!(ws.spaces()[1].tabs().len(), 1);
    }

    #[test]
    fn a_space_keeps_at_least_one_tab_and_a_terminal_cannot_detach() {
        let mut ws = workspace();
        assert!(!ws.detach_tab(0), "the only tab stays");
        let (mut ws, _) = with_tabs(2);
        assert!(!ws.detach_tab(5));
        ws.add_terminal(None);
        assert!(!ws.detach_tab(0), "the active entry is a terminal");
    }

    #[test]
    fn a_terminal_can_become_a_space() {
        let mut ws = workspace();
        ws.add_terminal(None);
        assert!(!ws.promote_to_space(0), "space 0 already is one");
        assert!(ws.promote_to_space(1));
        assert_eq!(ws.spaces()[1].kind(), SpaceKind::Space);
        assert!(!ws.spaces()[1].name.is_empty());
        assert!(ws.spaces()[1].color.is_some());
        ws.add_tab(None);
        assert_eq!(ws.spaces()[1].tabs().len(), 2, "now it takes tabs");
    }

    #[test]
    fn renaming_a_terminal_names_its_tab() {
        let mut ws = workspace();
        ws.add_terminal(None);
        ws.rename_space(1, "logs".to_owned());
        assert_eq!(ws.spaces()[1].display_name(), "logs");
        ws.rename_space(1, String::new());
        assert_eq!(ws.spaces()[1].display_name(), "shell", "follows its program again");
        ws.rename_space(0, "work".to_owned());
        assert_eq!(ws.spaces()[0].display_name(), "work");
    }

    #[test]
    fn the_rename_box_starts_with_the_name_the_user_gave() {
        let mut ws = workspace();
        ws.add_terminal(None);
        assert_eq!(ws.spaces()[0].editable_name(), "Space 1");
        assert_eq!(ws.spaces()[1].editable_name(), "", "a terminal follows its program");
        ws.rename_space(1, "logs".to_owned());
        assert_eq!(ws.spaces()[1].editable_name(), "logs");
    }

    #[test]
    fn closing_the_last_pane_of_a_terminal_removes_it() {
        let mut ws = workspace();
        let pane = ws.add_terminal(None);
        assert_eq!(ws.close_pane(pane), CloseOutcome::SpaceClosed);
        assert_eq!(kinds(&ws), [SpaceKind::Space]);
        assert_eq!(ws.active_index(), 0);
        // The last entry of all, even a terminal, never disappears under the app.
        let mut only = Workspace::new(None);
        only.add_terminal(None);
        only.close_space(0);
        let pane = only.focused_pane();
        assert_eq!(only.close_pane(pane), CloseOutcome::WorkspaceEmpty);
    }

    #[test]
    fn terminals_sleep_and_wake_like_spaces() {
        let mut ws = workspace();
        ws.add_terminal(None);
        ws.select_space(0);
        assert!(ws.sleep_space(1));
        assert_eq!(ws.spaces()[1].state, SpaceState::Sleeping);
        assert!(ws.space_panes(1).len() == 1);
        ws.select_space(1);
        assert_eq!(ws.spaces()[1].state, SpaceState::Awake);
    }

    #[test]
    fn terminals_survive_saving_and_loading() {
        let mut ws = workspace();
        ws.add_terminal(Some(PathBuf::from("/var")));
        ws.add_space(None);
        let back = Workspace::from_data(ws.to_data()).expect("valid");
        assert_eq!(kinds(&back), kinds(&ws));
        assert_eq!(back.to_data(), ws.to_data());
    }

    #[test]
    fn saved_data_without_a_kind_means_spaces() {
        let ws = workspace();
        let mut json = serde_json::to_value(ws.to_data()).expect("serializes");
        for space in json["spaces"].as_array_mut().expect("spaces") {
            space.as_object_mut().expect("object").remove("kind");
        }
        let data: WorkspaceData = serde_json::from_value(json).expect("old data still reads");
        assert_eq!(Workspace::from_data(data).expect("valid").spaces()[0].kind(), SpaceKind::Space);
    }

    #[test]
    fn a_terminal_with_two_tabs_is_rejected_when_loading() {
        let (ws, _) = with_tabs(2);
        let mut data = ws.to_data();
        data.spaces[0].kind = SpaceKind::Terminal;
        assert_eq!(
            Workspace::from_data(data).unwrap_err(),
            InvalidData("terminal with more than one tab")
        );
    }
}
