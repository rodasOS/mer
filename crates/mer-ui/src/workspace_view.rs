//! The main window content: sidebar of spaces, tab strip, split panes and the prompt overlay.

mod agents;
mod context_menu;
mod dialog;
mod drag;
mod settings_panel;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use gpui::{
    AnyElement, App, AppContext as _, BorrowAppContext as _, ClickEvent, Context, Entity,
    FocusHandle, FontWeight, Hsla, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Window, WindowBounds, div, hsla, prelude::FluentBuilder as _, px, relative,
};
use mer_core::{
    Accent, Axis, CloseOutcome, Direction, Layout, PaneId, Program, SpaceId, SpaceState, Workspace,
};
use mer_store::{Scrollback, StateDir, Store, UiState, WindowMode, WindowState, Writer};
use mer_term::snapshot::RESTORED_MARK;

use self::agents::{AgentInfo, status_color};
use self::context_menu::{ContextMenu, MenuTarget};
use self::settings_panel::SettingsState;
use crate::RunOptions;
use crate::commands::{self, Cmd};
use crate::settings::{AppSettings, Zoom, next_ui_scale};
use crate::terminal_view::{PaneLaunch, TerminalEvent, TerminalView};
use crate::theme::{Palette, accent_color, with_alpha};
use crate::tooltip;

const UI_FONT: &str = "Inter";
const SIDEBAR_WIDTH: f32 = 220.0;
/// Limits of the width the user can give the sidebar, and the width under which letting go
/// folds it into the rail.
const SIDEBAR_MIN: f32 = 160.0;
const SIDEBAR_MAX: f32 = 480.0;
const SIDEBAR_FOLD: f32 = 110.0;
const RAIL_WIDTH: f32 = 52.0;
/// How often a window resize is written to disk while it is being dragged.
const UI_SAVE_INTERVAL: Duration = Duration::from_millis(400);
const RESIZE_STEP: f32 = 0.03;
const PALETTE_ROWS: usize = 10;

struct LivePane {
    view: Entity<TerminalView>,
    _subscription: Subscription,
}

#[derive(Debug, Clone)]
enum PromptKind {
    Commands,
    RenameSpace(usize),
    RenameTab(usize),
    /// Sleeping would stop these running programs.
    ConfirmSleep {
        index: usize,
        busy: Vec<String>,
    },
    ConfirmSleepTab {
        index: usize,
        busy: Vec<String>,
    },
    /// Moving the tab into a sleeping space would stop these running programs.
    ConfirmMoveTab {
        tab: usize,
        space: usize,
        busy: Vec<String>,
    },
    /// Deleting removes the space, its terminals and their saved history.
    ConfirmDeleteSpace {
        index: usize,
        name: String,
        terminals: usize,
        busy: Vec<String>,
    },
}

struct Prompt {
    kind: PromptKind,
    query: String,
    selected: usize,
}

/// Saved state on disk: the database writer and the scrollback files.
struct Persistence {
    writer: Writer,
    scrollback: Scrollback,
}

/// Opens the saved state once and reads the interface state (sidebar, window size) from it, or
/// the defaults. The database goes on to [`open_persistence`], so it is opened a single time.
pub(crate) fn open_store() -> (UiState, Option<Store>) {
    let Some(store) =
        StateDir::default_location().and_then(|dir| Store::open(&dir.database()).ok())
    else {
        return (UiState::default(), None);
    };
    (store.load_ui().unwrap_or_default(), Some(store))
}

/// Loads the last saved workspace, if it is valid, and starts the background writer.
fn open_persistence(store: Store) -> Option<(Persistence, Option<Workspace>)> {
    let dir = StateDir::default_location()?;
    let saved = match store.load() {
        Ok(Some(data)) => match Workspace::from_data(data) {
            Ok(workspace) => Some(workspace),
            Err(error) => {
                tracing::warn!(%error, "ignoring invalid saved workspace");
                None
            }
        },
        Ok(None) => None,
        Err(error) => {
            tracing::warn!(%error, "could not read the saved workspace");
            None
        }
    };
    match Writer::with_store(store, &dir) {
        Ok(writer) => {
            Some((Persistence { writer, scrollback: Scrollback::new(dir.scrollback_dir()) }, saved))
        }
        Err(error) => {
            tracing::warn!(%error, "state will not be saved");
            None
        }
    }
}

/// What to remember about the window: its size and whether it is maximized.
fn window_state_of(window: &Window) -> WindowState {
    let (bounds, mode) = match window.window_bounds() {
        WindowBounds::Windowed(bounds) => (bounds, WindowMode::Windowed),
        WindowBounds::Maximized(bounds) => (bounds, WindowMode::Maximized),
        WindowBounds::Fullscreen(bounds) => (bounds, WindowMode::Fullscreen),
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let pixels = |value: gpui::Pixels| f32::from(value).round().max(0.0) as u32;
    WindowState { width: pixels(bounds.size.width), height: pixels(bounds.size.height), mode }
}

/// What should get the keyboard focus. Asked for anywhere, applied while drawing, because
/// focusing needs a window and there is none while the app runs in the background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Terminal,
    Prompt,
    Menu,
}

pub struct WorkspaceView {
    model: Workspace,
    live: HashMap<PaneId, LivePane>,
    palette: Palette,
    prompt: Option<Prompt>,
    prompt_focus: FocusHandle,
    /// Narrow rail instead of the full sidebar.
    sidebar_collapsed: bool,
    /// Width of the sidebar in unscaled pixels.
    sidebar_width: f32,
    /// The handle is being dragged; letting go below `SIDEBAR_FOLD` collapses the sidebar.
    resizing_sidebar: Option<bool>,
    menu: Option<ContextMenu>,
    menu_focus: FocusHandle,
    /// Last window size seen, saved with the interface state.
    window_state: Option<WindowState>,
    last_ui_save: Instant,
    /// The window showing this view, if any. With "keep running" the view outlives its windows.
    window: Option<gpui::AnyWindowHandle>,
    window_active: bool,
    /// Subscriptions that only make sense for the current window.
    window_subs: Vec<Subscription>,
    want_focus: Option<Focus>,
    window_title: String,
    /// Scale of the interface, refreshed from the config on every frame.
    ui_scale: f32,
    /// Side of the window the sidebar is on, refreshed from the config on every frame.
    sidebar_side: mer_config::SidebarSide,
    persistence: Option<Persistence>,
    settings: SettingsState,
    /// When each space was last the active one, for auto-sleep.
    last_active: HashMap<SpaceId, Instant>,
    tracked_space: SpaceId,
    _watch: Option<mer_config::WatchHandle>,
    /// `MER_BENCH`: print the time of the first painted frame.
    bench_pending: bool,
    /// When the last agent sound played, so a burst of them plays once.
    last_sound: Option<Instant>,
    /// When the program now running in each pane started, to tell how long a command took.
    command_started: HashMap<PaneId, Instant>,
    /// Agents seen in live panes.
    agents: HashMap<PaneId, AgentInfo>,
    /// Socket path handed to every pane (`MER_SOCKET`) when the IPC server is up.
    socket: Option<String>,
    _ipc: Option<mer_ipc::Server>,
    _tasks: Vec<gpui::Task<()>>,
    /// `mer -e <cmd>`: run this in the first terminal created.
    first_command: Option<Vec<String>>,
    _quit: Subscription,
}

/// The sidebar width as it is saved. It is kept between `SIDEBAR_MIN` and `SIDEBAR_MAX`.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn saved_width(width: f32) -> u16 {
    width.clamp(SIDEBAR_MIN, SIDEBAR_MAX).round() as u16
}

/// The color after `current` in the palette; after the last one, no color.
fn next_color(current: Option<Accent>) -> Option<Accent> {
    match current {
        None => Some(Accent::ALL[0]),
        Some(accent) => {
            let position = Accent::ALL.iter().position(|a| *a == accent).unwrap_or(0);
            Accent::ALL.get(position + 1).copied()
        }
    }
}

impl WorkspaceView {
    /// A length of the interface in pixels at the current interface scale.
    fn ui_px(&self, value: f32) -> gpui::Pixels {
        px(value * self.ui_scale)
    }

    #[allow(clippy::too_many_lines)]
    pub fn new(
        options: RunOptions,
        ui: UiState,
        store: Option<Store>,
        cx: &mut Context<Self>,
    ) -> Self {
        // `mer -e` is a throwaway instance: it neither restores nor overwrites saved state.
        let (persistence, saved) = match store.filter(|_| options.command.is_none()) {
            Some(store) => match open_persistence(store) {
                Some((persistence, saved)) => (Some(persistence), saved),
                None => (None, None),
            },
            None => (None, None),
        };
        let mut model = saved.unwrap_or_else(|| Workspace::new(std::env::current_dir().ok()));
        // A fresh start wakes only the space you were in; the rest stay asleep until visited.
        model.sleep_all_but_active();

        // A throwaway `mer -e` instance does not take over the socket.
        let ipc = if options.command.is_none() {
            mer_ipc::socket_path().and_then(|path| match mer_ipc::Server::bind(&path) {
                Ok((server, events)) => Some((server, events, path)),
                Err(error) => {
                    tracing::warn!(%error, "agent hooks and `mer ctl` are unavailable");
                    None
                }
            })
        } else {
            None
        };
        let (ipc_server, ipc_events, socket) = match ipc {
            Some((server, events, path)) => {
                (Some(server), Some(events), Some(path.to_string_lossy().into_owned()))
            }
            None => (None, None, None),
        };

        let tracked_space = model.active_space().id;
        let mut tasks = Vec::new();

        // Reload the config and themes when their files change.
        let (watch_tx, mut watch_rx) = futures::channel::mpsc::unbounded::<()>();
        let watch_paths: Vec<PathBuf> =
            mer_config::config_path().into_iter().chain(mer_config::themes_dir()).collect();
        let watch = match mer_config::watch(&watch_paths, move || {
            let _ = watch_tx.unbounded_send(());
        }) {
            Ok(handle) => Some(handle),
            Err(error) => {
                tracing::warn!(%error, "config changes will not reload live");
                None
            }
        };
        tasks.push(cx.spawn(async move |this, cx| {
            while watch_rx.next().await.is_some() {
                while watch_rx.try_recv().is_ok() {}
                if this.update(cx, Self::reload_settings).is_err() {
                    break;
                }
            }
        }));

        // Auto-sleep check, once a minute. Does nothing unless enabled in the settings.
        tasks.push(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_mins(1)).await;
                if this.update(cx, Self::auto_sleep_tick).is_err() {
                    break;
                }
            }
        }));

        if let Some(mut events) = ipc_events {
            tasks.push(cx.spawn(async move |this, cx| {
                while let Some(incoming) = events.next().await {
                    // Hooks and queries are answered with no window; anything the user is
                    // meant to see brings the window back first.
                    let wants_window =
                        this.update(cx, |this, _| this.needs_window_for(&incoming.request));
                    if wants_window.unwrap_or(false) {
                        cx.update(crate::open_main_window);
                    }
                    if this.update(cx, |this, cx| this.handle_incoming(incoming, cx)).is_err() {
                        break;
                    }
                }
            }));
        }

        let mut this = Self {
            model,
            live: HashMap::new(),
            palette: cx.global::<AppSettings>().palette.clone(),
            prompt: None,
            prompt_focus: cx.focus_handle(),
            sidebar_collapsed: ui.sidebar_collapsed,
            sidebar_width: ui
                .sidebar_width
                .map_or(SIDEBAR_WIDTH, |w| f32::from(w).clamp(SIDEBAR_MIN, SIDEBAR_MAX)),
            resizing_sidebar: None,
            menu: None,
            menu_focus: cx.focus_handle(),
            window_state: ui.window,
            last_ui_save: Instant::now(),
            window: None,
            window_active: false,
            window_subs: Vec::new(),
            want_focus: None,
            window_title: String::new(),
            ui_scale: 1.0,
            sidebar_side: mer_config::SidebarSide::default(),
            persistence,
            settings: SettingsState::default(),
            last_active: HashMap::new(),
            tracked_space,
            _watch: watch,
            bench_pending: std::env::var_os("MER_BENCH").is_some(),
            last_sound: None,
            command_started: HashMap::new(),
            agents: HashMap::new(),
            socket,
            _ipc: ipc_server,
            _tasks: tasks,
            first_command: options.command,
            _quit: cx.on_app_quit(|this, cx| {
                this.save_on_quit(cx);
                async {}
            }),
        };
        if let Some(persistence) = &this.persistence {
            persistence.scrollback.prune(&this.all_panes());
        }
        this.sync(cx);
        this.focus_active(cx);

        this
    }

    /// Starts showing this view in a window. Runs each time a window opens.
    pub(crate) fn attach(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.window = Some(window.window_handle());
        self.window_active = window.is_window_active();
        self.window_state = Some(window_state_of(window));
        self.window_title.clear();
        self.window_subs = vec![
            cx.observe_window_bounds(window, |this, window, _cx| {
                this.window_state = Some(window_state_of(window));
                if this.last_ui_save.elapsed() >= UI_SAVE_INTERVAL {
                    this.last_ui_save = Instant::now();
                    this.persist_ui();
                }
            }),
            cx.observe_window_activation(window, |this, window, _cx| {
                this.window_active = window.is_window_active();
            }),
        ];
        self.focus_active(cx);
    }

    /// The window was closed but the app keeps running: terminals and agents carry on.
    pub(crate) fn window_closed(&mut self, cx: &mut Context<Self>) {
        self.window = None;
        self.window_active = false;
        self.window_subs.clear();
        self.menu = None;
        self.prompt = None;
        self.settings.open = false;
        self.persist(cx);
        self.persist_ui();
    }

    pub(crate) fn has_window(&self) -> bool {
        self.window.is_some()
    }

    /// Where the window should open: where it was last time.
    pub(crate) fn saved_ui(&self) -> UiState {
        self.ui_state()
    }

    fn all_panes(&self) -> HashSet<PaneId> {
        (0..self.model.spaces().len())
            .flat_map(|index| self.model.space_panes(index))
            .map(|pane| pane.id)
            .collect()
    }

    /// Copies each live shell's current directory into the model.
    fn refresh_cwds(&mut self, cx: &App) {
        for (id, live) in &self.live {
            if let Some(cwd) = live.view.read(cx).cwd() {
                self.model.set_pane_cwd(*id, Some(cwd));
            }
        }
    }

    /// Queues the current workspace for saving (written on a background thread).
    fn persist(&mut self, cx: &App) {
        let active = self.model.active_space().id;
        if active != self.tracked_space {
            self.last_active.insert(self.tracked_space, Instant::now());
            self.tracked_space = active;
        }
        self.refresh_cwds(cx);
        if let Some(persistence) = &self.persistence {
            persistence.writer.save(self.model.to_data());
        }
    }

    fn ui_state(&self) -> UiState {
        UiState {
            sidebar_collapsed: self.sidebar_collapsed,
            sidebar_width: ((self.sidebar_width - SIDEBAR_WIDTH).abs() > 0.5)
                .then(|| saved_width(self.sidebar_width)),
            window: self.window_state,
        }
    }

    fn persist_ui(&self) {
        if let Some(persistence) = &self.persistence {
            persistence.writer.save_ui(self.ui_state());
        }
    }

    /// On exit: save every live pane's history and the workspace, and wait for the disk.
    fn save_on_quit(&mut self, cx: &App) {
        self.refresh_cwds(cx);
        if let Some(persistence) = self.persistence.take() {
            let ids: Vec<PaneId> = self.live.keys().copied().collect();
            for (id, ansi) in self.snapshots(&ids, cx) {
                persistence.writer.save_scrollback(id, ansi);
            }
            persistence.writer.save(self.model.to_data());
            persistence.writer.save_ui(self.ui_state());
            persistence.writer.close();
        }
    }

    /// Picks up edits made to `config.toml` or a theme file while the app runs.
    #[allow(clippy::unused_self)] // signature fixed by `update_in`
    fn reload_settings(&mut self, cx: &mut Context<Self>) {
        let (mut loaded, warnings) = AppSettings::load();
        for warning in warnings {
            tracing::warn!("{warning}");
        }
        let unchanged = {
            let current = cx.global::<AppSettings>();
            current.config == loaded.config && current.themes == loaded.themes
        };
        if unchanged {
            return;
        }
        for warning in crate::actions::apply_key_bindings(cx, &loaded.config.keys) {
            tracing::warn!("{warning}");
        }
        loaded.zoom = cx.global::<AppSettings>().zoom;
        cx.set_global(loaded);
        cx.notify();
    }

    /// Sleeps spaces that have not been visited for the configured time.
    fn auto_sleep_tick(&mut self, cx: &mut Context<Self>) {
        let minutes = cx.global::<AppSettings>().config.spaces.auto_sleep_minutes;
        if minutes == 0 {
            return;
        }
        let now = Instant::now();
        for space in self.model.spaces() {
            self.last_active.entry(space.id).or_insert(now);
        }
        let limit = Duration::from_secs(u64::from(minutes) * 60);
        let active = self.model.active_space().id;
        let due: Vec<usize> = self
            .model
            .spaces()
            .iter()
            .enumerate()
            .filter(|(_, space)| space.state == SpaceState::Awake && space.id != active)
            .filter(|(_, space)| {
                self.last_active
                    .get(&space.id)
                    .is_some_and(|seen| now.duration_since(*seen) >= limit)
            })
            .map(|(index, _)| index)
            .collect();
        for index in due {
            // Spaces running something are left alone.
            if self.busy_in_space(index, cx).is_empty() {
                self.sleep_space(index, cx);
            }
        }
    }

    /// Names of the programs running in the foreground of a space's terminals.
    fn busy_in_space(&self, index: usize, cx: &App) -> Vec<String> {
        let ids: Vec<PaneId> = self.model.space_panes(index).iter().map(|pane| pane.id).collect();
        self.busy_in_panes(&ids, cx)
    }

    fn busy_in_panes(&self, ids: &[PaneId], cx: &App) -> Vec<String> {
        ids.iter()
            .filter_map(|id| {
                let live = self.live.get(id)?;
                mer_agents::foreground(live.view.read(cx).pid()).map(|process| process.comm)
            })
            .collect()
    }

    /// Sleeps a space, asking first if it is running something.
    fn request_sleep(&mut self, index: usize, cx: &mut Context<Self>) {
        let sleepable =
            self.model.spaces().get(index).is_some_and(|s| s.state == SpaceState::Awake);
        let awake = self.model.spaces().iter().filter(|s| s.state == SpaceState::Awake).count();
        if !sleepable || awake <= 1 {
            return;
        }
        let busy = self.busy_in_space(index, cx);
        if busy.is_empty() {
            self.sleep_space(index, cx);
        } else {
            self.open_prompt(PromptKind::ConfirmSleep { index, busy }, String::new(), cx);
        }
    }

    /// Saves each pane's directory and history so its shell can be stopped.
    fn stash_panes(&mut self, ids: &[PaneId], cx: &App) {
        let cwds: Vec<(PaneId, Option<PathBuf>)> = ids
            .iter()
            .filter_map(|id| Some((*id, self.live.get(id)?.view.read(cx).cwd())))
            .collect();
        for (id, ansi) in self.snapshots(ids, cx) {
            if let Some(persistence) = &self.persistence {
                persistence.writer.save_scrollback(id, ansi);
            }
        }
        for (id, cwd) in cwds {
            if cwd.is_some() {
                self.model.set_pane_cwd(id, cwd);
            }
        }
    }

    /// Captures the history of several panes, one thread each, so the wait while saving
    /// does not add up.
    fn snapshots(&self, ids: &[PaneId], cx: &App) -> Vec<(PaneId, String)> {
        let captures: Vec<_> = ids
            .iter()
            .filter_map(|id| Some((*id, self.live.get(id)?.view.read(cx).snapshotter())))
            .collect();
        if captures.len() < 2 {
            return captures.into_iter().map(|(id, capture)| (id, capture())).collect();
        }
        std::thread::scope(|scope| {
            let threads: Vec<_> =
                captures.into_iter().map(|(id, capture)| (id, scope.spawn(capture))).collect();
            threads.into_iter().filter_map(|(id, thread)| Some((id, thread.join().ok()?))).collect()
        })
    }

    /// Saves each pane's directory and history, then stops its shell.
    fn sleep_space(&mut self, index: usize, cx: &mut Context<Self>) {
        let ids: Vec<PaneId> = self.model.space_panes(index).iter().map(|pane| pane.id).collect();
        self.stash_panes(&ids, cx);
        self.model.sleep_space(index);
        self.sync(cx);
        self.focus_active(cx);
        self.persist(cx);
        cx.notify();
    }

    /// Sleeps a tab of the active space, asking first if it is running something.
    fn request_sleep_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        let space = self.model.active_space();
        let sleepable = space.tabs().get(index).is_some_and(|tab| !tab.sleeping)
            && space.tabs().iter().filter(|tab| !tab.sleeping).count() > 1;
        if !sleepable {
            return;
        }
        let ids: Vec<PaneId> = self.model.tab_panes(index).iter().map(|pane| pane.id).collect();
        let busy = self.busy_in_panes(&ids, cx);
        if busy.is_empty() {
            self.sleep_tab(index, cx);
        } else {
            self.open_prompt(PromptKind::ConfirmSleepTab { index, busy }, String::new(), cx);
        }
    }

    /// Saves a tab's directories and history, then stops its shells. Returns false if the
    /// tab cannot sleep (already asleep, or the only awake one).
    fn sleep_tab(&mut self, index: usize, cx: &mut Context<Self>) -> bool {
        let ids: Vec<PaneId> = self.model.tab_panes(index).iter().map(|pane| pane.id).collect();
        self.stash_panes(&ids, cx);
        let slept = self.model.sleep_tab(index);
        self.after_model_change(cx);
        slept
    }

    /// Asks before deleting a space: it cannot be undone.
    fn request_delete_space(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(space) = self.model.spaces().get(index) else { return };
        let (name, terminals) = (space.name.clone(), space.pane_count());
        let busy = self.busy_in_space(index, cx);
        self.open_prompt(
            PromptKind::ConfirmDeleteSpace { index, name, terminals, busy },
            String::new(),
            cx,
        );
    }

    /// Deletes a space with its terminals and their saved history. Deleting the last space
    /// leaves a fresh one.
    fn delete_space(&mut self, index: usize, cx: &mut Context<Self>) {
        let cwd = self.focused_cwd(cx);
        for id in self.model.delete_space(index, cwd) {
            if let Some(persistence) = &self.persistence {
                persistence.writer.forget(id);
            }
        }
        self.after_model_change(cx);
    }

    /// Zooms the text of every terminal. Not saved: it lasts for the session.
    #[allow(clippy::unused_self)] // a method so callers read like the other commands
    fn zoom(&mut self, change: Zoom, cx: &mut Context<Self>) {
        cx.update_global::<AppSettings, _>(|settings, _| settings.apply_zoom(change));
        cx.notify();
    }

    /// Scales the interface (not the terminal text). Saved in `config.toml`.
    fn change_ui_scale(&mut self, change: Zoom, cx: &mut Context<Self>) {
        self.update_config(cx, |config| {
            config.appearance.ui_scale = next_ui_scale(config.appearance.ui_scale, change);
        });
    }

    fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        self.prompt = None;
        self.settings.open = true;
        self.settings.remember_position = crate::kwin::is_enabled();
        self.want_focus = Some(Focus::Prompt);
        cx.notify();
    }

    /// Sets the sidebar's width (unscaled pixels), kept between its limits, and saves it.
    fn set_sidebar_width(&mut self, width: f32, cx: &mut Context<Self>) {
        self.sidebar_width = width.clamp(SIDEBAR_MIN, SIDEBAR_MAX);
        self.sidebar_collapsed = false;
        self.persist_ui();
        cx.notify();
    }

    /// The handle follows the mouse: `x` is its position in window pixels.
    fn drag_sidebar(&mut self, x: f32, window_width: f32, cx: &mut Context<Self>) {
        if self.resizing_sidebar.is_none() {
            return;
        }
        let width = self.sidebar_side.width_from_x(x, window_width) / self.ui_scale;
        self.resizing_sidebar = Some(width < SIDEBAR_FOLD);
        self.sidebar_width = width.clamp(SIDEBAR_MIN, SIDEBAR_MAX);
        cx.notify();
    }

    /// The handle was let go. Saves the width, or folds the sidebar if it was pulled shut.
    fn end_sidebar_drag(&mut self, cx: &mut Context<Self>) {
        let Some(fold) = self.resizing_sidebar.take() else { return };
        if fold {
            self.set_sidebar_collapsed(true, cx);
        } else {
            self.persist_ui();
            cx.notify();
        }
    }

    fn set_sidebar_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        self.sidebar_collapsed = collapsed;
        self.menu = None;
        self.persist_ui();
        cx.notify();
    }

    /// Makes the live terminals match the model: starts missing ones, drops stale ones.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let wanted: Vec<(PaneId, Option<PathBuf>)> =
            self.model.awake_panes().iter().map(|pane| (pane.id, pane.cwd.clone())).collect();
        let ids: HashSet<PaneId> = wanted.iter().map(|(id, _)| *id).collect();

        // Panes that left `live` either went to sleep (keep their history) or were closed.
        let known = self.all_panes();
        let stale: Vec<PaneId> = self.live.keys().filter(|id| !ids.contains(id)).copied().collect();
        for id in stale {
            self.live.remove(&id);
            self.agents.remove(&id);
            // Sleeping or closing a pane is not a command that finished.
            self.command_started.remove(&id);
            self.model.set_pane_program(id, None);
            if !known.contains(&id)
                && let Some(persistence) = &self.persistence
            {
                persistence.writer.forget(id);
            }
        }

        for (id, cwd) in wanted {
            if self.live.contains_key(&id) {
                continue;
            }
            let config = cx.global::<AppSettings>().config.clone();
            let restore = self
                .persistence
                .as_ref()
                .filter(|_| config.spaces.restore_scrollback)
                .and_then(|persistence| persistence.scrollback.read(id))
                .map(|mut ansi| {
                    ansi.push_str(RESTORED_MARK);
                    ansi
                });
            let mut env = vec![(mer_ipc::ENV_PANE_ID.to_owned(), id.as_uuid().to_string())];
            if let Some(socket) = &self.socket {
                env.push((mer_ipc::ENV_SOCKET.to_owned(), socket.clone()));
            }
            let launch = PaneLaunch {
                cwd,
                restore,
                env,
                command: self.first_command.take(),
                scrollback: config.terminal.scrollback as usize,
                shell: config.terminal.shell.clone(),
            };
            let view = cx.new(|cx| TerminalView::new(launch, cx));
            let subscription =
                cx.subscribe(&view, move |this, _view, event: &TerminalEvent, cx| {
                    this.on_terminal_event(id, event, cx);
                });
            self.live.insert(id, LivePane { view, _subscription: subscription });
            self.resume_agent(id, cx);
        }
    }

    /// Gives the focused terminal the keyboard (applied the next time a window draws).
    fn focus_active(&mut self, cx: &mut Context<Self>) {
        let focused = self.model.focused_pane();
        self.acknowledge(focused);
        self.want_focus = Some(Focus::Terminal);
        cx.notify();
    }

    /// Opens the search bar over the focused terminal, or closes it if it is open.
    fn toggle_search(&mut self, cx: &mut Context<Self>) {
        let Some(live) = self.live.get(&self.model.focused_pane()) else { return };
        live.view.update(cx, |view, cx| {
            if view.is_searching() {
                view.close_search(cx);
            } else {
                view.open_search(None, cx);
            }
        });
    }

    /// `mer ctl search <text>`: searches in the focused terminal; empty closes the bar.
    pub(super) fn search_from_ctl(&mut self, query: &str, cx: &mut Context<Self>) -> bool {
        let Some(live) = self.live.get(&self.model.focused_pane()) else { return false };
        live.view.update(cx, |view, cx| view.open_search(Some(query), cx));
        true
    }

    fn focused_cwd(&self, cx: &App) -> Option<PathBuf> {
        let focused = self.model.focused_pane();
        self.live
            .get(&focused)
            .and_then(|live| live.view.read(cx).cwd())
            .or_else(|| self.model.pane(focused).and_then(|pane| pane.cwd.clone()))
    }

    /// A program (or the shell) set the title of a pane. The shell's own title (folder, branch)
    /// stays with the pane; a program's title belongs to that program, so it does not hide
    /// what is running once the shell takes over again.
    fn on_title(&mut self, id: PaneId, title: &str, cx: &mut Context<Self>) {
        let current = self.model.pane(id).and_then(|pane| pane.program.clone());
        let changed = if let Some(command) = mer_agents::command_from_title(title) {
            // Sent by the shell integration right before it runs a command.
            let program = Program {
                command: Some(command.to_owned()).filter(|line| !line.is_empty()),
                ..Program::default()
            };
            self.set_program(id, Some(program), cx)
        } else if let Some(process) =
            self.live.get(&id).and_then(|live| mer_agents::foreground(live.view.read(cx).pid()))
        {
            let program = Program {
                label: mer_agents::program_label(&process.comm, &process.cmdline),
                command: current.and_then(|program| program.command),
                title: Some(title.to_owned()).filter(|title| !title.is_empty()),
            };
            self.set_program(id, Some(program), cx)
        } else {
            let moved = self.model.pane(id).is_some_and(|pane| pane.title != title);
            if moved {
                self.model.set_pane_title(id, title.to_owned());
            }
            self.set_program(id, None, cx) || moved
        };
        if changed {
            cx.notify();
        }
    }

    fn on_terminal_event(&mut self, id: PaneId, event: &TerminalEvent, cx: &mut Context<Self>) {
        match event {
            TerminalEvent::Title(title) => self.on_title(id, title, cx),
            TerminalEvent::Activity => self.inspect_foreground(id, cx),
            TerminalEvent::Zoom(change) => self.zoom(*change, cx),
            TerminalEvent::Key(key) => self.agent_key(id, *key, cx),
            TerminalEvent::ContextMenu(position) => {
                self.open_menu(MenuTarget::Pane(id), *position, cx);
            }
            TerminalEvent::Exited => {
                if self.model.close_pane(id) == CloseOutcome::WorkspaceEmpty {
                    cx.quit();
                    return;
                }
                self.sync(cx);
                self.focus_active(cx);
                self.persist(cx);
                cx.notify();
            }
        }
    }

    #[allow(clippy::too_many_lines)] // one arm per command
    pub fn run(&mut self, cmd: Cmd, cx: &mut Context<Self>) {
        match cmd {
            Cmd::NewTab => {
                let cwd = self.focused_cwd(cx);
                self.model.add_tab(cwd);
            }
            Cmd::SplitRight => {
                let cwd = self.focused_cwd(cx);
                self.model.split_focused(Axis::Row, cwd);
            }
            Cmd::SplitDown => {
                let cwd = self.focused_cwd(cx);
                self.model.split_focused(Axis::Column, cwd);
            }
            Cmd::NewSpace => {
                let cwd = self.focused_cwd(cx);
                self.model.add_space(cwd);
            }
            Cmd::ClosePane => {
                let id = self.model.focused_pane();
                if self.model.close_pane(id) == CloseOutcome::WorkspaceEmpty {
                    cx.quit();
                    return;
                }
            }
            Cmd::CloseTab => {
                let index = self.model.active_space().active_tab_index();
                if self.model.close_tab(index) == CloseOutcome::WorkspaceEmpty {
                    cx.quit();
                    return;
                }
            }
            Cmd::CloseSpace => {
                let index = self.model.active_index();
                self.model.close_space(index);
            }
            Cmd::RenameSpace => {
                let index = self.model.active_index();
                let name = self.model.active_space().name.clone();
                self.open_prompt(PromptKind::RenameSpace(index), name, cx);
                return;
            }
            Cmd::RenameTab => {
                let index = self.model.active_space().active_tab_index();
                let title = self.model.active_tab().title.clone().unwrap_or_default();
                self.open_prompt(PromptKind::RenameTab(index), title, cx);
                return;
            }
            Cmd::SleepTab => {
                let index = self.model.active_space().active_tab_index();
                self.request_sleep_tab(index, cx);
                return;
            }
            Cmd::DeleteSpace => {
                let index = self.model.active_index();
                self.request_delete_space(index, cx);
                return;
            }
            Cmd::CycleSpaceColor => {
                let index = self.model.active_index();
                let next = next_color(self.model.active_space().color);
                self.model.set_space_color(index, next);
            }
            Cmd::CycleTabColor => {
                let index = self.model.active_space().active_tab_index();
                let next = next_color(self.model.active_tab().color);
                self.model.set_tab_color(index, next);
            }
            Cmd::OpenPalette => {
                self.open_prompt(PromptKind::Commands, String::new(), cx);
                return;
            }
            Cmd::SleepSpace => {
                let index = self.model.active_index();
                self.request_sleep(index, cx);
                return;
            }
            Cmd::SleepOthers => {
                let active = self.model.active_index();
                for index in 0..self.model.spaces().len() {
                    // Busy spaces are skipped here; use "Sleep space" on them to confirm.
                    if index != active && self.busy_in_space(index, cx).is_empty() {
                        self.sleep_space(index, cx);
                    }
                }
                return;
            }
            Cmd::NextTab => self.model.cycle_tab(true),
            Cmd::PrevTab => self.model.cycle_tab(false),
            Cmd::NextSpace => self.model.cycle_space(true),
            Cmd::PrevSpace => self.model.cycle_space(false),
            Cmd::FocusNext => self.model.focus_cycle(true),
            Cmd::FocusPrev => self.model.focus_cycle(false),
            Cmd::FocusLeft => self.model.focus_direction(Direction::Left),
            Cmd::FocusRight => self.model.focus_direction(Direction::Right),
            Cmd::FocusUp => self.model.focus_direction(Direction::Up),
            Cmd::FocusDown => self.model.focus_direction(Direction::Down),
            Cmd::ResizeLeft => self.model.resize_focused(Axis::Row, -RESIZE_STEP),
            Cmd::ResizeRight => self.model.resize_focused(Axis::Row, RESIZE_STEP),
            Cmd::ResizeUp => self.model.resize_focused(Axis::Column, -RESIZE_STEP),
            Cmd::ResizeDown => self.model.resize_focused(Axis::Column, RESIZE_STEP),
            Cmd::ToggleSidebar => {
                self.set_sidebar_collapsed(!self.sidebar_collapsed, cx);
                return;
            }
            Cmd::OpenSettings => {
                self.open_settings(cx);
                return;
            }
            Cmd::Quit => {
                cx.quit();
                return;
            }
            Cmd::ZoomIn => return self.zoom(Zoom::In, cx),
            Cmd::ZoomOut => return self.zoom(Zoom::Out, cx),
            Cmd::ZoomReset => return self.zoom(Zoom::Reset, cx),
            Cmd::SearchTerminal => {
                self.toggle_search(cx);
                return;
            }
            Cmd::MoveTabLeft => {
                let at = self.model.active_space().active_tab_index();
                self.move_tab(at, at.saturating_sub(1), cx);
                return;
            }
            Cmd::MoveTabRight => {
                let at = self.model.active_space().active_tab_index();
                self.move_tab(at, at + 1, cx);
                return;
            }
            Cmd::MoveSpaceUp => {
                let at = self.model.active_index();
                self.move_space(at, at.saturating_sub(1), cx);
                return;
            }
            Cmd::MoveSpaceDown => {
                let at = self.model.active_index();
                self.move_space(at, at + 1, cx);
                return;
            }
            Cmd::UiScaleUp => return self.change_ui_scale(Zoom::In, cx),
            Cmd::UiScaleDown => return self.change_ui_scale(Zoom::Out, cx),
            Cmd::UiScaleReset => return self.change_ui_scale(Zoom::Reset, cx),
        }
        self.sync(cx);
        self.focus_active(cx);
        self.persist(cx);
        cx.notify();
    }

    fn open_prompt(&mut self, kind: PromptKind, query: String, cx: &mut Context<Self>) {
        self.menu = None;
        self.settings.open = false;
        self.prompt = Some(Prompt { kind, query, selected: 0 });
        self.want_focus = Some(Focus::Prompt);
        cx.notify();
    }

    fn close_prompt(&mut self, cx: &mut Context<Self>) {
        self.prompt = None;
        self.focus_active(cx);
        cx.notify();
    }

    fn accept_prompt(&mut self, cx: &mut Context<Self>) {
        let Some(prompt) = self.prompt.take() else { return };
        match prompt.kind {
            PromptKind::Commands => {
                let entries = commands::filter(&prompt.query);
                self.focus_active(cx);
                if let Some(entry) = entries.get(prompt.selected) {
                    self.run(entry.cmd, cx);
                } else {
                    cx.notify();
                }
            }
            PromptKind::RenameSpace(index) => {
                self.model.rename_space(index, prompt.query.trim().to_owned());
                self.focus_active(cx);
                self.persist(cx);
                cx.notify();
            }
            PromptKind::RenameTab(index) => {
                self.model.rename_tab(index, prompt.query.trim().to_owned());
                self.focus_active(cx);
                self.persist(cx);
                cx.notify();
            }
            PromptKind::ConfirmSleep { index, .. } => {
                self.focus_active(cx);
                self.sleep_space(index, cx);
            }
            PromptKind::ConfirmSleepTab { index, .. } => {
                self.focus_active(cx);
                self.sleep_tab(index, cx);
            }
            PromptKind::ConfirmMoveTab { tab, space, .. } => {
                self.focus_active(cx);
                self.move_tab_to_space(tab, space, cx);
            }
            PromptKind::ConfirmDeleteSpace { index, .. } => {
                self.focus_active(cx);
                self.delete_space(index, cx);
            }
        }
    }

    fn on_prompt_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        let Some(prompt) = self.prompt.as_mut() else { return };
        match keystroke.key.as_str() {
            "escape" => return self.close_prompt(cx),
            "enter" => return self.accept_prompt(cx),
            "backspace" => {
                prompt.query.pop();
                prompt.selected = 0;
            }
            "up" => prompt.selected = prompt.selected.saturating_sub(1),
            "down" => {
                let last =
                    commands::filter(&prompt.query).len().min(PALETTE_ROWS).saturating_sub(1);
                prompt.selected = (prompt.selected + 1).min(last);
            }
            _ => {
                if modifiers.control || modifiers.alt || modifiers.platform {
                    return;
                }
                if let Some(text) = keystroke.key_char.as_deref() {
                    prompt.query.push_str(text);
                    prompt.selected = 0;
                }
            }
        }
        cx.notify();
    }

    fn select_space(&mut self, index: usize, cx: &mut Context<Self>) {
        // Selecting a sleeping space wakes it: `sync` starts its terminals again.
        self.model.select_space(index);
        self.sync(cx);
        self.focus_active(cx);
        self.persist(cx);
        cx.notify();
    }

    fn select_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        // Selecting a sleeping tab wakes it: `sync` starts its terminals again.
        self.model.select_tab(index);
        self.sync(cx);
        self.focus_active(cx);
        self.persist(cx);
        cx.notify();
    }

    fn close_tab_at(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.model.close_tab(index) == CloseOutcome::WorkspaceEmpty {
            cx.quit();
            return;
        }
        self.sync(cx);
        self.focus_active(cx);
        self.persist(cx);
        cx.notify();
    }

    fn focus_pane(&mut self, id: PaneId, cx: &mut Context<Self>) {
        if self.model.focused_pane() != id {
            self.model.focus_pane(id);
            self.persist(cx);
            cx.notify();
        }
        self.focus_active(cx);
    }

    // ----- rendering -----

    /// A small square button of the sidebar or rail.
    fn chrome_button(
        &self,
        id: &'static str,
        glyph: &'static str,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let chrome = &self.palette.chrome;
        let (text, dim) = (chrome.text, chrome.text_dim);
        let hover = self.palette.hover();
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .size(self.ui_px(28.0))
            .flex_none()
            .rounded_lg()
            .cursor_pointer()
            .text_color(dim)
            .hover(move |button| button.bg(hover).text_color(text))
            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                on_click(this, cx);
            }))
            .child(glyph)
            .into_any_element()
    }

    fn space_color(&self, color: Option<Accent>) -> Hsla {
        color.map_or(self.palette.chrome.text_dim, accent_color)
    }

    /// Lines for a tab's tooltip: what runs, where, and what the shell titled the pane.
    fn tab_details(tab: &mer_core::Tab) -> Vec<String> {
        let mut lines = Vec::new();
        let Some(pane) = tab.pane(tab.focused()) else { return lines };
        if let Some(program) = &pane.program {
            let running = program.command.as_deref().unwrap_or(&program.label);
            if !running.is_empty() {
                lines.push(format!("Running: {running}"));
            }
        }
        if let Some(cwd) = &pane.cwd {
            lines.push(Self::shorten_home(&cwd.to_string_lossy()));
        }
        if !pane.title.is_empty() && pane.program.is_some() {
            lines.push(pane.title.clone());
        }
        if tab.sleeping {
            lines.push("Sleeping".to_owned());
        }
        lines
    }

    /// `/home/user/x` as `~/x`.
    fn shorten_home(text: &str) -> String {
        static HOME: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        let Some(home) = HOME.get_or_init(|| std::env::var("HOME").ok()) else {
            return text.to_owned();
        };
        if text == home {
            return "~".to_owned();
        }
        match text.strip_prefix(home.as_str()).and_then(|rest| rest.strip_prefix('/')) {
            Some(rest) => format!("~/{rest}"),
            None => text.to_owned(),
        }
    }

    #[allow(clippy::too_many_lines)] // builder chain
    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.sidebar_collapsed {
            return self.render_rail(cx);
        }
        let palette = &self.palette;
        let chrome = &palette.chrome;
        let (text, dim) = (chrome.text, chrome.text_dim);
        let (hover, hairline) = (palette.hover(), palette.hairline());
        let active = self.model.active_index();
        let clear = hsla(0.0, 0.0, 0.0, 0.0);

        let rows = self
            .model
            .spaces()
            .iter()
            .enumerate()
            .map(|(index, space)| {
                let is_active = index == active;
                let sleeping = space.state == SpaceState::Sleeping;
                let terminals = space.pane_count();
                let color = self.space_color(space.color);
                let agent_color = self
                    .space_agent_status(index)
                    .map(|status| status_color(&self.palette, status));
                let detail = format!(
                    "{terminals} terminal{}{}",
                    if terminals == 1 { "" } else { "s" },
                    if sleeping { " · sleeping" } else { "" }
                );
                self.space_dnd(div().id(("space", index)), index, &space.name, cx)
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(self.ui_px(8.0))
                    .h(self.ui_px(34.0))
                    .px(self.ui_px(8.0))
                    .rounded_lg()
                    .border_1()
                    .border_color(if is_active { with_alpha(color, 0.35) } else { clear })
                    .cursor_pointer()
                    .when(is_active, move |row| row.bg(with_alpha(color, 0.16)))
                    .hover(move |row| {
                        row.bg(with_alpha(color, if is_active { 0.22 } else { 0.10 }))
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        this.select_space(index, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                            cx.stop_propagation();
                            this.open_menu(MenuTarget::Space(index), event.position, cx);
                        }),
                    )
                    .tooltip(tooltip::tip(palette, space.name.clone(), [detail]))
                    .tooltip_show_delay(tooltip::SHOW_DELAY)
                    .child(
                        div()
                            .w(self.ui_px(3.0))
                            .h(self.ui_px(16.0))
                            .flex_none()
                            .rounded_full()
                            .bg(color)
                            .when(sleeping, |bar| bar.opacity(0.6)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(if is_active { text } else { dim })
                            .when(is_active, |name| name.font_weight(FontWeight::SEMIBOLD))
                            .when(sleeping, |name| name.opacity(0.75))
                            .child(SharedString::from(space.name.clone())),
                    )
                    .children(agent_color.map(|status| {
                        div().size(self.ui_px(8.0)).flex_none().rounded_full().bg(status)
                    }))
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .h(self.ui_px(18.0))
                            .px(self.ui_px(6.0))
                            .rounded_full()
                            .bg(hover)
                            .text_xs()
                            .text_color(dim)
                            .child(if sleeping {
                                format!("zz {terminals}")
                            } else {
                                format!("{terminals}")
                            }),
                    )
            })
            .collect::<Vec<_>>();

        div()
            .relative()
            .w(self.ui_px(self.sidebar_width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .px_2()
            .py_3()
            .bg(chrome.sidebar)
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                    this.open_menu(MenuTarget::Sidebar, event.position, cx);
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .pl_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(dim)
                            .child("SPACES"),
                    )
                    .child(self.chrome_button(
                        "collapse-sidebar",
                        self.sidebar_side.chevron(true),
                        |this, cx| this.set_sidebar_collapsed(true, cx),
                        cx,
                    )),
            )
            .children(rows)
            .child(
                div()
                    .id("new-space")
                    .flex()
                    .items_center()
                    .h(self.ui_px(32.0))
                    .px_2()
                    .rounded_lg()
                    .border_1()
                    .border_dashed()
                    .border_color(hairline)
                    .cursor_pointer()
                    .text_color(dim)
                    .hover(move |row| row.bg(hover).text_color(text))
                    .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                        this.run(Cmd::NewSpace, cx);
                    }))
                    .child("+  New space"),
            )
            .children(self.render_agents_section(cx))
            .child(div().flex_1())
            .child(
                div()
                    .id("open-settings")
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .h(self.ui_px(34.0))
                    .px_2()
                    .rounded_lg()
                    .cursor_pointer()
                    .text_color(dim)
                    .hover(move |row| row.bg(hover).text_color(text))
                    .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                        this.open_settings(cx);
                    }))
                    .child("⚙  Settings")
                    .child(tooltip::kbd(palette, "Ctrl+,")),
            )
            .child(self.render_sidebar_handle(cx))
            .into_any_element()
    }

    /// The strip on the sidebar's right edge that is dragged to change its width. Double click
    /// brings the width back to the default.
    fn render_sidebar_handle(&self, cx: &mut Context<Self>) -> AnyElement {
        let (hairline, accent) = (self.palette.hairline(), self.palette.chrome.accent);
        let dragging = self.resizing_sidebar.is_some();
        // The handle is on the edge that faces the terminals.
        let on_right_edge = self.sidebar_side.sidebar_first();
        div()
            .id("sidebar-handle")
            .group("sidebar-handle")
            .absolute()
            .top_0()
            .bottom_0()
            .when(on_right_edge, Styled::right_0)
            .when(!on_right_edge, Styled::left_0)
            .w(self.ui_px(6.0))
            .flex()
            .when(on_right_edge, Styled::justify_end)
            .cursor_col_resize()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                    this.resizing_sidebar = Some(false);
                    cx.notify();
                }),
            )
            .on_click(cx.listener(|this, event: &ClickEvent, _window, cx| {
                if event.click_count() == 2 {
                    this.set_sidebar_width(SIDEBAR_WIDTH, cx);
                }
            }))
            .child(
                div()
                    .w(px(2.0))
                    .h_full()
                    .rounded_full()
                    .bg(if dragging { accent } else { gpui::hsla(0.0, 0.0, 0.0, 0.0) })
                    .group_hover("sidebar-handle", move |bar| {
                        bar.bg(if dragging { accent } else { hairline })
                    }),
            )
            .into_any_element()
    }

    /// The collapsed sidebar: one colored pill per space.
    #[allow(clippy::too_many_lines)] // builder chain
    fn render_rail(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = &self.palette;
        let chrome = &palette.chrome;
        let (text, dim) = (chrome.text, chrome.text_dim);
        let active = self.model.active_index();

        let pills = self
            .model
            .spaces()
            .iter()
            .enumerate()
            .map(|(index, space)| {
                let is_active = index == active;
                let sleeping = space.state == SpaceState::Sleeping;
                let color = self.space_color(space.color);
                let agent_status = self.space_agent_status(index);
                let label = match space.name.strip_prefix("Space ") {
                    Some(rest) if !rest.is_empty() => rest.to_owned(),
                    _ => space.name.chars().next().map_or_else(
                        || "?".to_owned(),
                        |initial| initial.to_uppercase().collect::<String>(),
                    ),
                };
                self.space_dnd(div().id(("space-pill", index)), index, &space.name, cx)
                    .relative()
                    .size(self.ui_px(36.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_xl()
                    .cursor_pointer()
                    .text_color(if is_active { text } else { dim })
                    .bg(with_alpha(color, if is_active { 0.40 } else { 0.20 }))
                    .hover(move |pill| pill.bg(with_alpha(color, 0.45)))
                    .when(sleeping, |pill| pill.opacity(0.8))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        this.select_space(index, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                            cx.stop_propagation();
                            this.open_menu(MenuTarget::Space(index), event.position, cx);
                        }),
                    )
                    .tooltip(tooltip::tip(palette, space.name.clone(), []))
                    .tooltip_show_delay(tooltip::SHOW_DELAY)
                    .child(SharedString::from(label))
                    // The active space has a marker on the rail's edge.
                    .when(is_active, |pill| {
                        pill.child(
                            div()
                                .absolute()
                                // On the rail's edge that faces the window's edge.
                                .when(self.sidebar_side.sidebar_first(), |bar| {
                                    bar.left(self.ui_px(-8.0))
                                })
                                .when(!self.sidebar_side.sidebar_first(), |bar| {
                                    bar.right(self.ui_px(-8.0))
                                })
                                .top(self.ui_px(9.0))
                                .w(self.ui_px(3.0))
                                .h(self.ui_px(18.0))
                                .rounded_full()
                                .bg(color),
                        )
                    })
                    .children(agent_status.map(|status| {
                        // A ring, bigger than the dot, for an agent that needs you.
                        let urgent = status == mer_agents::AgentStatus::Waiting;
                        let hue = status_color(&self.palette, status);
                        div()
                            .absolute()
                            .top(px(1.0))
                            .right(px(1.0))
                            .size(self.ui_px(if urgent { 12.0 } else { 8.0 }))
                            .rounded_full()
                            .bg(if urgent { with_alpha(hue, 0.35) } else { hue })
                            .when(urgent, |dot| dot.border_2().border_color(hue))
                    }))
            })
            .collect::<Vec<_>>();

        div()
            .w(self.ui_px(RAIL_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .py_3()
            .bg(chrome.sidebar)
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                    this.open_menu(MenuTarget::Sidebar, event.position, cx);
                }),
            )
            .child(self.chrome_button(
                "expand-sidebar",
                self.sidebar_side.chevron(false),
                |this, cx| this.set_sidebar_collapsed(false, cx),
                cx,
            ))
            .children(pills)
            .child(self.chrome_button(
                "rail-new-space",
                "+",
                |this, cx| this.run(Cmd::NewSpace, cx),
                cx,
            ))
            .child(div().flex_1())
            .child(self.chrome_button("rail-settings", "⚙", Self::open_settings, cx))
            .into_any_element()
    }

    #[allow(clippy::too_many_lines)] // builder chain
    fn render_tab_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = &self.palette;
        let chrome = &palette.chrome;
        let (text, dim) = (chrome.text, chrome.text_dim);
        let (base, hover, hairline) = (palette.background, palette.hover(), palette.hairline());
        let clear = hsla(0.0, 0.0, 0.0, 0.0);
        let space = self.model.active_space();
        let active = space.active_tab_index();
        let space_color = space.color.map(accent_color);
        // A tab can go to another space as long as this one keeps another tab.
        let movable = space.tabs().len() > 1;

        let tabs =
            space.tabs().iter().enumerate().map(|(index, tab)| {
                let is_active = index == active;
                let sleeping = tab.sleeping;
                let color = tab.color.map(accent_color);
                // A tab without its own color borrows the space's for its marker.
                let marker = color.or(if is_active { space_color } else { None });
                let agent_color =
                    self.tab_agent_status(tab).map(|status| status_color(&self.palette, status));
                let title = if sleeping {
                    format!("zz {}", tab.display_title())
                } else {
                    tab.display_title().to_owned()
                };
                let place = tab.display_place();
                self.tab_dnd(div().id(("tab", index)), index, &title, movable, cx)
                    .group("tab")
                    .relative()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(self.ui_px(8.0))
                    .h(self.ui_px(30.0))
                    .pl_3()
                    .pr_1()
                    // The name takes the room it needs; tabs only shrink when they do not all fit.
                    .min_w(self.ui_px(96.0))
                    .max_w(self.ui_px(360.0))
                    .rounded_lg()
                    .border_1()
                    .border_color(if is_active { hairline } else { clear })
                    .cursor_pointer()
                    .text_color(if is_active { text } else { dim })
                    .when(is_active, |tab| tab.bg(base))
                    .when_some(color, move |tab, color| {
                        tab.bg(with_alpha(color, if is_active { 0.22 } else { 0.10 }))
                    })
                    .when(sleeping, |tab| tab.opacity(0.6))
                    .hover(move |tab| tab.bg(color.map_or(hover, |color| with_alpha(color, 0.30))))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        this.select_tab(index, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                            cx.stop_propagation();
                            this.open_menu(MenuTarget::Tab(index), event.position, cx);
                        }),
                    )
                    .tooltip(tooltip::tip(palette, title.clone(), Self::tab_details(tab)))
                    .tooltip_show_delay(tooltip::SHOW_DELAY)
                    .children(agent_color.map(|color| {
                        div().size(self.ui_px(7.0)).flex_none().rounded_full().bg(color)
                    }))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_row()
                            .gap(self.ui_px(6.0))
                            .child(div().min_w_0().truncate().child(SharedString::from(title)))
                            // The folder, dimmer, next to what runs there.
                            .children(place.map(|place| {
                                div()
                                    .min_w_0()
                                    .max_w(self.ui_px(120.0))
                                    .truncate()
                                    .text_color(dim)
                                    .child(SharedString::from(place))
                            })),
                    )
                    .child(
                        div()
                            .id(("tab-close", index))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(self.ui_px(18.0))
                            .rounded_full()
                            .text_color(dim)
                            // The × shows on the open tab and on hover, but always takes its room.
                            .when(!is_active, |close| {
                                close.invisible().group_hover("tab", Styled::visible)
                            })
                            .hover(move |close| close.bg(hover).text_color(text))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                                cx.stop_propagation();
                                this.close_tab_at(index, cx);
                            }))
                            .child("×"),
                    )
                    // A straight marker under the name: the underline of a rounded tab would curve.
                    .children(marker.map(|marker| {
                        div()
                            .absolute()
                            .bottom_0()
                            .left(self.ui_px(10.0))
                            .right(self.ui_px(10.0))
                            .h(px(2.0))
                            .rounded_full()
                            .bg(if is_active { marker } else { with_alpha(marker, 0.5) })
                    }))
            });

        div()
            .w_full()
            .h(self.ui_px(44.0))
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_2()
            .bg(chrome.sidebar)
            .children(tabs)
            .child(
                div()
                    .id("new-tab")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(self.ui_px(28.0))
                    .flex_none()
                    .rounded_lg()
                    .cursor_pointer()
                    .text_color(dim)
                    .hover(move |button| button.bg(hover).text_color(text))
                    .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                        this.run(Cmd::NewTab, cx);
                    }))
                    .child("+"),
            )
            // Free space to the right: dropping a tab here sends it to the end.
            .child(div().flex_1().h_full().on_drop::<drag::DraggedTab>(cx.listener(
                |this, dragged: &drag::DraggedTab, _, cx| {
                    let last = this.model.active_space().tabs().len().saturating_sub(1);
                    this.move_tab(dragged.index, last, cx);
                },
            )))
            .into_any_element()
    }

    fn render_layout(
        &self,
        layout: &Layout,
        focused: PaneId,
        multiple: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match layout {
            Layout::Leaf(id) => self.render_pane(*id, focused == *id && multiple, cx),
            Layout::Split { axis, ratio, first, second } => {
                let first = self.render_layout(first, focused, multiple, cx);
                let second = self.render_layout(second, focused, multiple, cx);
                // Panes are cards: the gap between them is the divider.
                let gap = self.ui_px(6.0);
                match axis {
                    Axis::Row => div()
                        .size_full()
                        .flex()
                        .flex_row()
                        .child(div().h_full().w(relative(*ratio)).min_w_0().child(first))
                        .child(div().h_full().w(gap).flex_none())
                        .child(div().h_full().flex_1().min_w_0().child(second))
                        .into_any_element(),
                    Axis::Column => div()
                        .size_full()
                        .flex()
                        .flex_col()
                        .child(div().w_full().h(relative(*ratio)).min_h_0().child(first))
                        .child(div().w_full().h(gap).flex_none())
                        .child(div().w_full().flex_1().min_h_0().child(second))
                        .into_any_element(),
                }
            }
        }
    }

    fn render_pane(&self, id: PaneId, highlight: bool, cx: &mut Context<Self>) -> AnyElement {
        let Some(live) = self.live.get(&id) else {
            return div().size_full().into_any_element();
        };
        let border = if highlight {
            with_alpha(self.palette.chrome.accent, 0.7)
        } else {
            self.palette.hairline()
        };
        div()
            .size_full()
            .overflow_hidden()
            .rounded_xl()
            .border_1()
            .border_color(border)
            .bg(self.palette.background)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _window, cx| {
                    this.focus_pane(id, cx);
                }),
            )
            .child(live.view.clone())
            .into_any_element()
    }
}

macro_rules! bind_actions {
    ($element:expr, $cx:expr, { $($action:ident => $cmd:expr),* $(,)? }) => {{
        let element = $element;
        $(
            let element = element.on_action($cx.listener(
                |this, _: &crate::actions::$action, _window, cx| this.run($cmd, cx),
            ));
        )*
        element
    }};
}

impl Render for WorkspaceView {
    #[allow(clippy::too_many_lines)] // the root element and its action bindings
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.bench_pending {
            self.bench_pending = false;
            window.on_next_frame(|_, _| {
                eprintln!("first-frame: {} ms", crate::ms_since_start());
            });
        }
        self.palette = cx.global::<AppSettings>().palette.clone();
        self.ui_scale = cx.global::<AppSettings>().config.appearance.ui_scale;
        self.sidebar_side = cx.global::<AppSettings>().config.appearance.sidebar_side;
        // `rem`-based sizes (text_xs, p_2, rounded_lg...) follow the scale on their own.
        window.set_rem_size(px(16.0 * self.ui_scale));
        let title = {
            let tab = self.model.active_tab();
            let pane_title = tab.pane(tab.focused()).map_or("", |pane| {
                pane.program.as_ref().and_then(Program::name).unwrap_or(pane.title.as_str())
            });
            if pane_title.is_empty() { "mer".to_owned() } else { format!("{pane_title} — mer") }
        };
        if title != self.window_title {
            window.set_window_title(&title);
            self.window_title = title;
        }

        // Focus was asked for somewhere with no window at hand; apply it once drawing is done.
        if let Some(target) = self.want_focus.take() {
            let handle = match target {
                Focus::Terminal => self
                    .live
                    .get(&self.model.focused_pane())
                    .map(|live| live.view.read(cx).focus_handle()),
                Focus::Prompt => Some(self.prompt_focus.clone()),
                Focus::Menu => Some(self.menu_focus.clone()),
            };
            if let Some(handle) = handle {
                window.defer(cx, move |window, cx| window.focus(&handle, cx));
            }
        }

        let tab = self.model.active_tab();
        let multiple = tab.pane_count() > 1;
        let content = self.render_layout(tab.layout(), tab.focused(), multiple, cx);
        let sidebar = self.render_sidebar(cx);
        let tab_bar = self.render_tab_bar(cx);
        let prompt = self.render_prompt(cx);
        let settings_overlay = self.render_settings(cx);
        let menu = self.render_menu(cx);
        let chrome = &self.palette.chrome;

        let main = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(tab_bar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .px(self.ui_px(6.0))
                    .pb(self.ui_px(6.0))
                    .child(content),
            )
            .into_any_element();
        // The sidebar goes on the side the config says; both share the window's background.
        let (first, second) =
            if self.sidebar_side.sidebar_first() { (sidebar, main) } else { (main, sidebar) };

        let root = div()
            .id("mer")
            .size_full()
            .relative()
            .flex()
            .flex_row()
            .bg(chrome.sidebar)
            .font_family(UI_FONT)
            .text_size(self.ui_px(13.0))
            .text_color(chrome.text)
            // The sidebar handle is dragged with the mouse anywhere over the window.
            .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, window, cx| {
                if this.resizing_sidebar.is_some() {
                    let width = f32::from(window.viewport_size().width);
                    this.drag_sidebar(f32::from(event.position.x), width, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseUpEvent, _window, cx| this.end_sidebar_drag(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseUpEvent, _window, cx| this.end_sidebar_drag(cx)),
            );

        bind_actions!(root, cx, {
            NewTab => Cmd::NewTab,
            CloseTab => Cmd::CloseTab,
            ClosePane => Cmd::ClosePane,
            SplitRight => Cmd::SplitRight,
            SplitDown => Cmd::SplitDown,
            NewSpace => Cmd::NewSpace,
            CloseSpace => Cmd::CloseSpace,
            RenameSpace => Cmd::RenameSpace,
            RenameTab => Cmd::RenameTab,
            NextTab => Cmd::NextTab,
            PrevTab => Cmd::PrevTab,
            NextSpace => Cmd::NextSpace,
            PrevSpace => Cmd::PrevSpace,
            FocusNext => Cmd::FocusNext,
            FocusPrev => Cmd::FocusPrev,
            FocusLeft => Cmd::FocusLeft,
            FocusRight => Cmd::FocusRight,
            FocusUp => Cmd::FocusUp,
            FocusDown => Cmd::FocusDown,
            ResizeLeft => Cmd::ResizeLeft,
            ResizeRight => Cmd::ResizeRight,
            ResizeUp => Cmd::ResizeUp,
            ResizeDown => Cmd::ResizeDown,
            ToggleSidebar => Cmd::ToggleSidebar,
            ZoomIn => Cmd::ZoomIn,
            ZoomOut => Cmd::ZoomOut,
            ZoomReset => Cmd::ZoomReset,
            UiScaleUp => Cmd::UiScaleUp,
            UiScaleDown => Cmd::UiScaleDown,
            UiScaleReset => Cmd::UiScaleReset,
            MoveTabLeft => Cmd::MoveTabLeft,
            MoveTabRight => Cmd::MoveTabRight,
            MoveSpaceUp => Cmd::MoveSpaceUp,
            MoveSpaceDown => Cmd::MoveSpaceDown,
            SearchTerminal => Cmd::SearchTerminal,
            Quit => Cmd::Quit,
            OpenPalette => Cmd::OpenPalette,
            OpenSettings => Cmd::OpenSettings,
            SleepSpace => Cmd::SleepSpace,
            SleepOthers => Cmd::SleepOthers,
        })
        .child(first)
        .child(second)
        .children(settings_overlay)
        .children(prompt)
        .children(menu)
    }
}
