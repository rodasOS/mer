//! Agent tracking: which agent runs in which pane, what it is doing, and how the user finds out.
//!
//! Detection has two layers. Looking at the terminal's foreground process (on output, at most
//! every few hundred milliseconds) says *which* agent is there. The agent's hooks, delivered
//! through `mer hook`, say what it is *doing*.

use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, Hsla, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div,
    prelude::FluentBuilder as _, px,
};
use mer_agents::{AgentKey, AgentStatus, ForegroundProcess, HookKind, program_label};
use mer_core::{Accent, AgentMemo, PaneId, Program, SpaceState, Tab};
use mer_ipc::{CtlCommand, Incoming, Request, Response, SpaceInfo};

use super::PromptKind;
use super::WorkspaceView;
use super::context_menu::MenuTarget;
use crate::settings::{AppSettings, Zoom};
use crate::theme::{Palette, with_alpha};

/// An agent seen in a live pane.
#[derive(Debug, Clone)]
pub(super) struct AgentInfo {
    pub name: String,
    pub status: AgentStatus,
    pub session_id: Option<String>,
    /// A hook has reported at least once. Until then only the process is known, and the status
    /// is a guess ("detected"), not a fact.
    pub hooked: bool,
}

/// A color name from `mer ctl`; `None` (or `none`) clears the color.
fn parse_color(name: Option<&str>) -> Result<Option<Accent>, String> {
    match name {
        None | Some("none") => Ok(None),
        Some(name) => Accent::from_name(name).map(Some).ok_or_else(|| {
            let known: Vec<_> = Accent::ALL.iter().map(|a| a.name()).collect();
            format!("unknown color `{name}` (try: {}, none)", known.join(", "))
        }),
    }
}

fn rank(status: AgentStatus) -> u8 {
    match status {
        AgentStatus::Waiting => 3,
        AgentStatus::Working => 2,
        AgentStatus::Done => 1,
        AgentStatus::Idle => 0,
    }
}

pub(super) fn status_color(palette: &Palette, status: AgentStatus) -> Hsla {
    match status {
        AgentStatus::Working => palette.chrome.accent,
        AgentStatus::Waiting => palette.ansi[3],
        AgentStatus::Done => palette.ansi[2],
        AgentStatus::Idle => palette.chrome.text_dim,
    }
}

fn status_label(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Working => "working",
        AgentStatus::Waiting => "waiting",
        AgentStatus::Done => "done",
        AgentStatus::Idle => "idle",
    }
}

/// Session ids are pasted into a shell command on wake, so only plain ids are accepted.
fn is_safe_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl WorkspaceView {
    /// Looks at what runs in the foreground of a pane's terminal and updates what the pane
    /// says it runs (for the tab's name) and its agent entry.
    pub(super) fn inspect_foreground(&mut self, id: PaneId, cx: &mut Context<Self>) {
        let Some(live) = self.live.get(&id) else { return };
        let pid = live.view.read(cx).pid();
        let registry = Rc::clone(&cx.global::<AppSettings>().registry);
        let process = mer_agents::foreground(pid);
        let detected =
            process.as_ref().and_then(|process| registry.detect(process).map(str::to_owned));
        let program_changed = self.refresh_program(id, process.as_ref(), cx);

        let changed = if let Some(name) = detected {
            if self.agents.get(&id).is_some_and(|agent| agent.name == name) {
                false
            } else {
                // A resumed agent keeps the session id remembered before sleeping.
                let memo = self.model.pane(id).and_then(|pane| pane.agent.clone());
                let session_id = memo.filter(|m| m.kind == name).and_then(|m| m.session_id);
                self.model.set_pane_agent(
                    id,
                    Some(AgentMemo { kind: name.clone(), session_id: session_id.clone() }),
                );
                self.agents.insert(
                    id,
                    AgentInfo { name, status: AgentStatus::Idle, session_id, hooked: false },
                );
                true
            }
        } else {
            // The agent left the foreground: forget it, so it is not resumed on wake.
            let removed = self.agents.remove(&id).is_some();
            if removed {
                self.model.set_pane_agent(id, None);
            }
            removed
        };
        if changed {
            self.persist(cx);
        }
        if changed || program_changed {
            cx.notify();
        }
    }

    /// Updates a pane's running program from its foreground process. Returns whether it changed.
    fn refresh_program(
        &mut self,
        id: PaneId,
        process: Option<&ForegroundProcess>,
        cx: &mut Context<Self>,
    ) -> bool {
        let current = self.model.pane(id).and_then(|pane| pane.program.clone());
        let next = match (process, current) {
            (Some(process), current) => Some(Program {
                label: program_label(&process.comm, &process.cmdline),
                command: current.as_ref().and_then(|program| program.command.clone()),
                title: current.and_then(|program| program.title),
            }),
            // The shell just announced a command that has not started yet: keep it.
            (None, Some(program)) if program.label.is_empty() => return false,
            (None, _) => None,
        };
        self.set_program(id, next, cx)
    }

    /// Records what a pane runs, and times it: when a program ends after long enough while
    /// nobody is looking, that is worth an alert. The one place that changes a pane's program.
    pub(super) fn set_program(
        &mut self,
        id: PaneId,
        program: Option<Program>,
        cx: &mut Context<Self>,
    ) -> bool {
        let before = self.model.pane(id).and_then(|pane| pane.program.clone());
        let running = program.is_some();
        let changed = self.model.set_pane_program(id, program);
        match (before, running) {
            (None, true) => {
                self.command_started.insert(id, std::time::Instant::now());
            }
            (Some(before), false) => {
                if let Some(since) = self.command_started.remove(&id) {
                    self.command_finished(id, &before, since.elapsed(), cx);
                }
            }
            _ => {}
        }
        changed
    }

    /// A command ended: alert, if it was long, out of sight and not an interactive session.
    fn command_finished(
        &mut self,
        id: PaneId,
        program: &Program,
        elapsed: std::time::Duration,
        cx: &Context<Self>,
    ) {
        let agents = &cx.global::<AppSettings>().config.agents;
        let (threshold, notify, sound) = (agents.long_command_seconds, agents.notify, agents.sound);
        let name = program.name().unwrap_or_default();
        let wanted = mer_agents::should_notify(
            elapsed,
            threshold,
            self.pane_on_screen(id),
            self.agents.contains_key(&id),
            name,
        );
        if !wanted {
            return;
        }
        if notify {
            let shown: String = name.chars().take(60).collect();
            let summary = format!("{shown} finished · {}", mer_agents::format_duration(elapsed));
            self.notify_desktop(summary, id);
        }
        if sound {
            self.play_attention_sound(AgentStatus::Done, cx);
        }
    }

    /// Is the user looking at this pane right now?
    fn pane_on_screen(&self, id: PaneId) -> bool {
        self.window_active
            && self.model.active_tab().contains(id)
            && self.model.focused_pane() == id
    }

    /// Applies a state reported by an agent's hook.
    pub(super) fn apply_hook(
        &mut self,
        pane: PaneId,
        agent: &str,
        kind: HookKind,
        session_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if !self.live.contains_key(&pane) {
            return;
        }
        let on_screen = self.pane_on_screen(pane);
        let entry = self.agents.entry(pane).or_insert_with(|| AgentInfo {
            name: agent.to_owned(),
            status: AgentStatus::Idle,
            session_id: None,
            hooked: false,
        });
        let before = entry.status;
        let first_report = !entry.hooked;
        entry.hooked = true;
        agent.clone_into(&mut entry.name);
        entry.status = entry.status.after(kind);
        let new_session = session_id.is_some() && session_id != entry.session_id;
        if session_id.is_some() {
            entry.session_id = session_id;
        }
        // Agents fire a hook for every tool they run: when nothing changed, do no work.
        if entry.status == before && !new_session && !first_report {
            return;
        }

        if on_screen && entry.status == AgentStatus::Done {
            entry.status = AgentStatus::Idle;
        }
        let (name, status, session_id) =
            (entry.name.clone(), entry.status, entry.session_id.clone());

        self.model.set_pane_agent(pane, Some(AgentMemo { kind: name.clone(), session_id }));
        let agents = &cx.global::<AppSettings>().config.agents;
        let (notify, sound) = (agents.notify, agents.sound);
        let needs_you = status.wants_attention() && status != before && !on_screen;
        if needs_you && notify {
            self.notify_attention(&name, status, pane);
        }
        if needs_you && sound {
            self.play_attention_sound(status, cx);
        }
        self.persist(cx);
        cx.notify();
    }

    /// A key pressed in a pane: Esc or Ctrl+C stops an agent, and Enter starts a turn for the
    /// agents whose hooks do not say so. Claude Code sends no hook when interrupted.
    pub(super) fn agent_key(&mut self, id: PaneId, key: AgentKey, cx: &mut Context<Self>) {
        let Some(agent) = self.agents.get_mut(&id) else { return };
        let before = agent.status;
        agent.status = match key {
            AgentKey::Interrupt => before.after_interrupt(),
            AgentKey::Submit => before.after_submit(mer_agents::reports_turn_start(&agent.name)),
        };
        if agent.status != before {
            cx.notify();
        }
    }

    /// Plays the sound for an agent that waits or is done, unless one just played.
    pub(super) fn play_attention_sound(&mut self, status: AgentStatus, cx: &Context<Self>) {
        let now = std::time::Instant::now();
        if !crate::sound::gap_has_passed(self.last_sound, now) {
            return;
        }
        self.last_sound = Some(now);
        let agents = &cx.global::<AppSettings>().config.agents;
        let sound = match status {
            AgentStatus::Waiting => &agents.sound_waiting,
            _ => &agents.sound_done,
        };
        crate::sound::play(sound, agents.sound_volume);
    }

    /// Desktop notification (KDE) for an agent that wants the user.
    fn notify_attention(&self, name: &str, status: AgentStatus, pane: PaneId) {
        let summary = match status {
            AgentStatus::Waiting => format!("{name} needs you"),
            _ => format!("{name} finished"),
        };
        self.notify_desktop(summary, pane);
    }

    /// A desktop notification whose body says where it happened (`Space · tab`).
    fn notify_desktop(&self, summary: String, pane: PaneId) {
        let body = self.pane_label(pane);
        std::thread::spawn(move || {
            let result = notify_rust::Notification::new()
                .appname("mer")
                .summary(&summary)
                .body(&body)
                .icon("mer")
                .show();
            if let Err(error) = result {
                tracing::debug!(%error, "desktop notification failed");
            }
        });
    }

    /// `Space · tab` for a pane.
    pub(super) fn pane_label(&self, id: PaneId) -> String {
        for space in self.model.spaces() {
            for tab in space.tabs() {
                if tab.contains(id) {
                    return format!("{} · {}", space.name, tab.display_title());
                }
            }
        }
        String::new()
    }

    /// The user is looking at a pane: a finished agent there is no longer news.
    pub(super) fn acknowledge(&mut self, id: PaneId) {
        if let Some(agent) = self.agents.get_mut(&id)
            && agent.status == AgentStatus::Done
        {
            agent.status = AgentStatus::Idle;
        }
    }

    /// Most urgent agent state among a space's panes.
    pub(super) fn space_agent_status(&self, index: usize) -> Option<AgentStatus> {
        self.model
            .space_panes(index)
            .iter()
            .filter_map(|pane| self.agents.get(&pane.id).map(|agent| agent.status))
            .max_by_key(|status| rank(*status))
    }

    pub(super) fn tab_agent_status(&self, tab: &Tab) -> Option<AgentStatus> {
        tab.pane_ids()
            .iter()
            .filter_map(|id| self.agents.get(id).map(|agent| agent.status))
            .max_by_key(|status| rank(*status))
    }

    /// Does this request show something to the user, so the window must be open for it?
    /// Hooks, queries and housekeeping are answered with no window.
    pub(super) fn needs_window_for(&self, request: &Request) -> bool {
        self.window.is_none()
            && matches!(
                request,
                Request::Ctl(
                    CtlCommand::Open { .. }
                        | CtlCommand::Show { .. }
                        | CtlCommand::ShowWindow
                        | CtlCommand::SpaceNew { .. }
                        | CtlCommand::SpaceWake { .. }
                        | CtlCommand::Search { .. }
                )
            )
    }

    /// Something arrived on the socket from `mer hook` or `mer ctl`.
    pub(super) fn handle_incoming(&mut self, incoming: Incoming, cx: &mut Context<Self>) {
        let quit = matches!(incoming.request, Request::Ctl(CtlCommand::Quit));
        let response = match &incoming.request {
            Request::Hook { pane, agent, kind, session_id } => {
                if let Some(id) = PaneId::parse(pane) {
                    let session = session_id.clone().filter(|id| is_safe_session_id(id));
                    self.apply_hook(id, agent, *kind, session, cx);
                }
                Response::Ok
            }
            Request::Ctl(command) => {
                let command = command.clone();
                self.run_ctl(&command, cx)
            }
            Request::Ping => Response::Pong,
        };
        incoming.reply(response);
        if quit {
            cx.quit();
        }
    }

    #[allow(clippy::too_many_lines)] // one arm per command
    fn run_ctl(&mut self, command: &CtlCommand, cx: &mut Context<Self>) -> Response {
        match command {
            CtlCommand::SpaceList => Response::Spaces(self.space_infos()),
            CtlCommand::SpaceNew { cwd } => {
                let cwd = cwd.as_ref().map(PathBuf::from).or_else(|| self.focused_cwd(cx));
                self.model.add_space(cwd);
                self.after_model_change(cx);
                Response::Ok
            }
            CtlCommand::Open { dir } => {
                self.model.add_tab(Some(PathBuf::from(dir)));
                self.after_model_change(cx);
                Response::Ok
            }
            CtlCommand::SpaceSleep { index } => {
                let index = index.unwrap_or_else(|| self.model.active_index());
                self.sleep_from_ctl(index, cx)
            }
            CtlCommand::SpaceDelete { index } => {
                if *index >= self.model.spaces().len() {
                    return Response::Error(format!("no space with index {index}"));
                }
                self.delete_space(*index, cx);
                Response::Ok
            }
            CtlCommand::SpaceColor { index, color } => {
                let color = match parse_color(color.as_deref()) {
                    Ok(color) => color,
                    Err(error) => return Response::Error(error),
                };
                if *index >= self.model.spaces().len() {
                    return Response::Error(format!("no space with index {index}"));
                }
                self.model.set_space_color(*index, color);
                self.after_model_change(cx);
                Response::Ok
            }
            CtlCommand::TabSleep { index } => {
                if self.sleep_tab(*index, cx) {
                    Response::Ok
                } else {
                    Response::Error(
                        "that tab cannot sleep (no such tab, asleep, or the only awake one)"
                            .to_owned(),
                    )
                }
            }
            CtlCommand::TabColor { index, color } => {
                let color = match parse_color(color.as_deref()) {
                    Ok(color) => color,
                    Err(error) => return Response::Error(error),
                };
                if *index >= self.model.active_space().tabs().len() {
                    return Response::Error(format!("no tab with index {index}"));
                }
                self.model.set_tab_color(*index, color);
                self.after_model_change(cx);
                Response::Ok
            }
            CtlCommand::Sidebar { collapsed } => {
                self.set_sidebar_collapsed(*collapsed, cx);
                Response::Ok
            }
            CtlCommand::Search { query } => {
                if self.search_from_ctl(query, cx) {
                    Response::Ok
                } else {
                    Response::Error("no terminal is focused".to_owned())
                }
            }
            CtlCommand::TabMove { from, to } => {
                let tabs = self.model.active_space().tabs().len();
                if *from >= tabs || *to >= tabs {
                    return Response::Error(format!("the active space has {tabs} tabs"));
                }
                self.move_tab(*from, *to, cx);
                Response::Ok
            }
            CtlCommand::SpaceMove { from, to } => {
                let spaces = self.model.spaces().len();
                if *from >= spaces || *to >= spaces {
                    return Response::Error(format!("there are {spaces} spaces"));
                }
                self.move_space(*from, *to, cx);
                Response::Ok
            }
            CtlCommand::TabToSpace { tab, space } => {
                let tabs = self.model.active_space().tabs().len();
                if *tab >= tabs {
                    return Response::Error(format!("the active space has {tabs} tabs"));
                }
                if *space >= self.model.spaces().len() {
                    return Response::Error(format!("no space with index {space}"));
                }
                if *space == self.model.active_index() {
                    return Response::Error("that is the active space already".to_owned());
                }
                if tabs < 2 {
                    return Response::Error("a space keeps at least one tab".to_owned());
                }
                // Into a sleeping space the tab goes asleep: refuse if it still runs something.
                let ids: Vec<_> = self.model.tab_panes(*tab).iter().map(|p| p.id).collect();
                let sleeping = self.model.spaces()[*space].state == SpaceState::Sleeping;
                let busy = self.busy_in_panes(&ids, cx);
                if sleeping && !busy.is_empty() {
                    return Response::Error(format!("still running: {}", busy.join(", ")));
                }
                self.move_tab_to_space(*tab, *space, cx);
                Response::Ok
            }
            CtlCommand::SidebarWidth { width } => {
                self.set_sidebar_width(f32::from(*width), cx);
                Response::Ok
            }
            CtlCommand::Show { what } => self.show(what, cx),
            // The window was already opened for these, before the request got here.
            CtlCommand::ShowWindow | CtlCommand::Quit => Response::Ok,
            CtlCommand::CloseWindow => {
                if let Some(handle) = self.window {
                    cx.defer(move |cx| {
                        let _ = handle.update(cx, |_, window, _| window.remove_window());
                    });
                }
                Response::Ok
            }
            CtlCommand::Zoom { change } => {
                let change = match change.as_str() {
                    "in" => Zoom::In,
                    "out" => Zoom::Out,
                    "reset" => Zoom::Reset,
                    other => return Response::Error(format!("unknown zoom `{other}`")),
                };
                self.zoom(change, cx);
                Response::Ok
            }
            CtlCommand::UiScale { change } => {
                let change = match change.as_str() {
                    "in" => Zoom::In,
                    "out" => Zoom::Out,
                    "reset" => Zoom::Reset,
                    other => return Response::Error(format!("unknown scale change `{other}`")),
                };
                self.change_ui_scale(change, cx);
                Response::Ok
            }
            CtlCommand::SpaceWake { index } => {
                if *index >= self.model.spaces().len() {
                    return Response::Error(format!("no space with index {index}"));
                }
                // Waking a space also makes it the active one.
                self.select_space(*index, cx);
                Response::Ok
            }
        }
    }

    /// `mer ctl show ...`: opens part of the interface, so it can be checked without a mouse.
    fn show(&mut self, what: &str, cx: &mut Context<Self>) -> Response {
        let at = gpui::point(px(260.0), px(140.0));
        match what {
            "settings" => self.open_settings(cx),
            "space-menu" => {
                let index = self.model.active_index();
                self.open_menu(MenuTarget::Space(index), at, cx);
            }
            "tab-menu" => {
                let index = self.model.active_space().active_tab_index();
                self.open_menu(MenuTarget::Tab(index), at, cx);
            }
            "pane-menu" => {
                let id = self.model.focused_pane();
                self.open_menu(MenuTarget::Pane(id), at, cx);
            }
            "sidebar-menu" => self.open_menu(MenuTarget::Sidebar, at, cx),
            "palette" => self.open_prompt(PromptKind::Commands, String::new(), cx),
            "rename-space" => {
                let index = self.model.active_index();
                let name = self.model.active_space().name.clone();
                self.open_prompt(PromptKind::RenameSpace(index), name, cx);
            }
            "rename-tab" => {
                let index = self.model.active_space().active_tab_index();
                self.open_prompt(PromptKind::RenameTab(index), String::new(), cx);
            }
            "confirm-delete" => {
                let index = self.model.active_index();
                self.request_delete_space(index, cx);
            }
            "confirm-sleep" => {
                let index = self.model.active_index();
                let busy = vec!["node".to_owned(), "claude".to_owned()];
                self.open_prompt(PromptKind::ConfirmSleep { index, busy }, String::new(), cx);
            }
            other if other.starts_with("settings:") => {
                if !self.open_settings_section(&other["settings:".len()..], cx) {
                    return Response::Error(format!("unknown settings section in `{other}`"));
                }
            }
            other => return Response::Error(format!("unknown interface part `{other}`")),
        }
        Response::Ok
    }

    pub(super) fn after_model_change(&mut self, cx: &mut Context<Self>) {
        self.sync(cx);
        self.focus_active(cx);
        self.persist(cx);
        cx.notify();
    }

    fn sleep_from_ctl(&mut self, index: usize, cx: &mut Context<Self>) -> Response {
        let Some(space) = self.model.spaces().get(index) else {
            return Response::Error(format!("no space with index {index}"));
        };
        if space.state == SpaceState::Sleeping {
            return Response::Error("that space is already asleep".to_owned());
        }
        let awake = self.model.spaces().iter().filter(|s| s.state == SpaceState::Awake).count();
        if awake <= 1 {
            return Response::Error("at least one space must stay awake".to_owned());
        }
        let busy = self.busy_in_space(index, cx);
        if !busy.is_empty() {
            return Response::Error(format!("still running: {}", busy.join(", ")));
        }
        self.sleep_space(index, cx);
        Response::Ok
    }

    fn space_infos(&self) -> Vec<SpaceInfo> {
        let active = self.model.active_index();
        self.model
            .spaces()
            .iter()
            .enumerate()
            .map(|(index, space)| SpaceInfo {
                index,
                name: space.name.clone(),
                state: if space.state == SpaceState::Sleeping { "sleeping" } else { "awake" }
                    .to_owned(),
                panes: space.pane_count(),
                active: index == active,
            })
            .collect()
    }

    /// Types the resume command for an agent remembered before the space went to sleep.
    /// Only Claude Code is supported: it is the one whose session id we receive.
    pub(super) fn resume_agent(&self, id: PaneId, cx: &mut Context<Self>) {
        let resume = cx.global::<AppSettings>().config.agents.resume_on_wake;
        let Some(memo) = self.model.pane(id).and_then(|pane| pane.agent.clone()) else { return };
        let Some(live) = self.live.get(&id) else { return };
        if !resume || memo.kind != "claude" {
            return;
        }
        if let Some(session) = memo.session_id.filter(|session| is_safe_session_id(session)) {
            live.view.read(cx).write_text(&format!("claude --resume {session}\r"));
        }
    }

    /// The "AGENTS" block of the sidebar, most urgent first. `None` when no agent is running.
    #[allow(clippy::too_many_lines)] // builder chain
    pub(super) fn render_agents_section(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let palette = &self.palette;
        let (text, dim) = (palette.chrome.text, palette.chrome.text_dim);
        let hover = palette.hover();

        let mut rows = Vec::new();
        for space in self.model.spaces() {
            for tab in space.tabs() {
                for id in tab.pane_ids() {
                    let Some(agent) = self.agents.get(&id) else { continue };
                    rows.push((id, agent.clone(), self.pane_label(id)));
                }
            }
        }
        if rows.is_empty() {
            return None;
        }
        // Who needs you first. The sort is stable: equally urgent agents keep their order.
        rows.sort_by_key(|(_, agent, _)| std::cmp::Reverse(rank(agent.status)));
        let waiting =
            rows.iter().filter(|(_, agent, _)| agent.status == AgentStatus::Waiting).count();
        let attention = status_color(palette, AgentStatus::Waiting);

        let items = rows.into_iter().enumerate().map(|(index, (id, agent, label))| {
            let status = agent.status;
            let color = status_color(palette, status);
            let urgent = status == AgentStatus::Waiting;
            let quiet = status == AgentStatus::Idle;
            let state = agent_state_text(&agent);
            let subtitle =
                if urgent { format!("Needs your attention · {label}") } else { label.clone() };
            div()
                .id(("agent", index))
                .flex()
                .flex_col()
                .gap(self.ui_px(2.0))
                .py(self.ui_px(6.0))
                .px(self.ui_px(8.0))
                .rounded_lg()
                .border_1()
                .border_color(if urgent { with_alpha(color, 0.45) } else { with_alpha(color, 0.0) })
                .when(urgent, |row| row.bg(with_alpha(color, 0.12)))
                .when(quiet, |row| row.opacity(0.75))
                .cursor_pointer()
                .hover(move |row| row.bg(if urgent { with_alpha(color, 0.18) } else { hover }))
                .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                    this.focus_pane(id, cx);
                }))
                .tooltip(crate::tooltip::tip(
                    palette,
                    agent.name.clone(),
                    [format!("{state} · {label}")],
                ))
                .tooltip_show_delay(crate::tooltip::SHOW_DELAY)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(self.ui_px(8.0))
                        .child(
                            div()
                                .size(self.ui_px(if urgent { 10.0 } else { 8.0 }))
                                .flex_none()
                                .rounded_full()
                                .bg(color),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(text)
                                .font_weight(FontWeight::MEDIUM)
                                .child(SharedString::from(agent.name)),
                        )
                        .child(
                            div()
                                .flex_none()
                                .flex()
                                .items_center()
                                .h(self.ui_px(18.0))
                                .px(self.ui_px(6.0))
                                .rounded_full()
                                .bg(with_alpha(color, if quiet { 0.0 } else { 0.16 }))
                                .text_xs()
                                .text_color(color)
                                .child(state),
                        ),
                )
                .child(
                    div()
                        .pl(self.ui_px(18.0))
                        .truncate()
                        .text_xs()
                        .text_color(if urgent { color } else { dim })
                        .child(SharedString::from(subtitle)),
                )
        });

        Some(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .mt_2()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_1()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(dim)
                                .child("AGENTS"),
                        )
                        .children((waiting > 0).then(|| {
                            div()
                                .flex()
                                .items_center()
                                .h(self.ui_px(16.0))
                                .px(self.ui_px(6.0))
                                .rounded_full()
                                .bg(with_alpha(attention, 0.2))
                                .text_xs()
                                .text_color(attention)
                                .child(format!("{waiting} waiting"))
                        })),
                )
                .children(items)
                .into_any_element(),
        )
    }
}

/// What an agent is doing, in a word. Before any hook has reported, the status is only a
/// guess from the process, so it says "detected".
fn agent_state_text(agent: &AgentInfo) -> &'static str {
    if agent.hooked || agent.status != AgentStatus::Idle {
        status_label(agent.status)
    } else {
        "detected"
    }
}
