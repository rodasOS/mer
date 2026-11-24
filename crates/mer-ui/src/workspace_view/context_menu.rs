//! Right-click menus for spaces, tabs, terminals and the sidebar.
//!
//! A menu only exists while it is open: it is built from the model when the user right-clicks
//! and dropped when they pick something, click elsewhere or press Escape. Nothing is kept per
//! row beyond one mouse listener, so closed menus cost nothing.

use gpui::{
    AnyElement, ClickEvent, Context, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, ParentElement, Pixels, Point, SharedString, StatefulInteractiveElement, Styled,
    anchored, div, hsla, prelude::FluentBuilder as _, px,
};
use mer_core::{Accent, Axis, PaneId, Space, SpaceState};

use super::{PromptKind, WorkspaceView};
use crate::commands::Cmd;
use crate::theme::accent_color;

/// What was right-clicked.
#[derive(Debug, Clone, Copy)]
pub(super) enum MenuTarget {
    Space(usize),
    Tab(usize),
    Pane(PaneId),
    Sidebar,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ColorTarget {
    Space(usize),
    Tab(usize),
}

/// What picking a menu row does.
#[derive(Debug, Clone, Copy)]
pub(super) enum MenuAction {
    RenameSpace(usize),
    SleepSpace(usize),
    WakeSpace(usize),
    NewTabIn(usize),
    DeleteSpace(usize),
    SpaceColor(usize, Option<Accent>),
    RenameTab(usize),
    SleepTab(usize),
    WakeTab(usize),
    CloseTab(usize),
    TabColor(usize, Option<Accent>),
    MoveTab(usize, usize),
    MoveTabToSpace(usize, usize),
    MoveSpace(usize, usize),
    TabSplit(usize, Axis),
    PaneSplit(PaneId, Axis),
    Copy(PaneId),
    Paste(PaneId),
    ClosePane(PaneId),
    NewSpace,
    NewTerminal,
    /// Turn a terminal into a space, so it can have tabs.
    PromoteToSpace(usize),
    /// Take a tab of the active space out, as a terminal on its own.
    DetachTab(usize),
    OpenSettings,
    ToggleSidebar,
    Quit,
}

pub(super) enum MenuEntry {
    Item { label: String, hint: &'static str, action: MenuAction, danger: bool, enabled: bool },
    Colors { target: ColorTarget, current: Option<Accent> },
    Separator,
}

pub(super) struct ContextMenu {
    position: Point<Pixels>,
    entries: Vec<MenuEntry>,
}

fn item(label: &str, hint: &'static str, action: MenuAction) -> MenuEntry {
    MenuEntry::Item { label: label.to_owned(), hint, action, danger: false, enabled: true }
}

fn item_if(enabled: bool, label: &str, hint: &'static str, action: MenuAction) -> MenuEntry {
    MenuEntry::Item { label: label.to_owned(), hint, action, danger: false, enabled }
}

fn danger(label: &str, hint: &'static str, action: MenuAction) -> MenuEntry {
    MenuEntry::Item { label: label.to_owned(), hint, action, danger: true, enabled: true }
}

impl WorkspaceView {
    pub(super) fn open_menu(
        &mut self,
        target: MenuTarget,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let entries = self.menu_entries(target);
        if entries.is_empty() {
            return;
        }
        self.menu = Some(ContextMenu { position, entries });
        self.want_focus = Some(super::Focus::Menu);
        cx.notify();
    }

    pub(super) fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            self.focus_active(cx);
            cx.notify();
        }
    }

    #[allow(clippy::too_many_lines)] // one flat list of entries per target
    fn menu_entries(&self, target: MenuTarget) -> Vec<MenuEntry> {
        match target {
            MenuTarget::Space(index) => {
                let Some(space) = self.model.spaces().get(index) else { return Vec::new() };
                let sleeping = space.state == SpaceState::Sleeping;
                let awake =
                    self.model.spaces().iter().filter(|s| s.state == SpaceState::Awake).count();
                if space.is_terminal() {
                    return vec![
                        item("Rename…", "", MenuAction::RenameSpace(index)),
                        if sleeping {
                            item("Wake terminal", "", MenuAction::WakeSpace(index))
                        } else {
                            item_if(
                                awake > 1,
                                "Sleep terminal",
                                "Ctrl+Shift+Z",
                                MenuAction::SleepSpace(index),
                            )
                        },
                        MenuEntry::Separator,
                        item(
                            "Split right",
                            "Ctrl+Shift+D",
                            MenuAction::PaneSplit(space.active_tab().focused(), Axis::Row),
                        ),
                        item(
                            "Split down",
                            "Ctrl+Shift+E",
                            MenuAction::PaneSplit(space.active_tab().focused(), Axis::Column),
                        ),
                        MenuEntry::Separator,
                        item("Turn into space", "", MenuAction::PromoteToSpace(index)),
                        item_if(
                            index > 0,
                            "Move up",
                            "",
                            MenuAction::MoveSpace(index, index.saturating_sub(1)),
                        ),
                        item_if(
                            index + 1 < self.model.spaces().len(),
                            "Move down",
                            "",
                            MenuAction::MoveSpace(index, index + 1),
                        ),
                        MenuEntry::Separator,
                        danger("Delete terminal…", "", MenuAction::DeleteSpace(index)),
                    ];
                }
                vec![
                    item("Rename…", "", MenuAction::RenameSpace(index)),
                    MenuEntry::Colors { target: ColorTarget::Space(index), current: space.color },
                    MenuEntry::Separator,
                    if sleeping {
                        item("Wake space", "", MenuAction::WakeSpace(index))
                    } else {
                        item_if(
                            awake > 1,
                            "Sleep space",
                            "Ctrl+Shift+Z",
                            MenuAction::SleepSpace(index),
                        )
                    },
                    item("New tab here", "", MenuAction::NewTabIn(index)),
                    MenuEntry::Separator,
                    item_if(
                        index > 0,
                        "Move up",
                        "",
                        MenuAction::MoveSpace(index, index.saturating_sub(1)),
                    ),
                    item_if(
                        index + 1 < self.model.spaces().len(),
                        "Move down",
                        "",
                        MenuAction::MoveSpace(index, index + 1),
                    ),
                    MenuEntry::Separator,
                    danger("Delete space…", "", MenuAction::DeleteSpace(index)),
                ]
            }
            MenuTarget::Tab(index) => {
                let space = self.model.active_space();
                let Some(tab) = space.tabs().get(index) else { return Vec::new() };
                let awake = space.tabs().iter().filter(|t| !t.sleeping).count();
                let tabs = space.tabs().len();
                let mut entries = vec![
                    item("Rename…", "", MenuAction::RenameTab(index)),
                    MenuEntry::Colors { target: ColorTarget::Tab(index), current: tab.color },
                    MenuEntry::Separator,
                    if tab.sleeping {
                        item("Wake tab", "", MenuAction::WakeTab(index))
                    } else {
                        item_if(awake > 1, "Sleep tab", "", MenuAction::SleepTab(index))
                    },
                    item("Split right", "Ctrl+Shift+D", MenuAction::TabSplit(index, Axis::Row)),
                    item("Split down", "Ctrl+Shift+E", MenuAction::TabSplit(index, Axis::Column)),
                    item_if(tabs > 1, "Open as terminal", "", MenuAction::DetachTab(index)),
                    MenuEntry::Separator,
                    item_if(
                        index > 0,
                        "Move left",
                        "",
                        MenuAction::MoveTab(index, index.saturating_sub(1)),
                    ),
                    item_if(
                        index + 1 < tabs,
                        "Move right",
                        "",
                        MenuAction::MoveTab(index, index + 1),
                    ),
                ];
                // Move it into another space (one that keeps at least one other tab).
                if tabs > 1 {
                    let active = self.model.active_index();
                    for (other, target) in self.model.spaces().iter().enumerate().take(8) {
                        // A terminal has exactly one tab, so it cannot take another.
                        if other != active && !target.is_terminal() {
                            entries.push(item(
                                &format!("Move to “{}”", target.name),
                                "",
                                MenuAction::MoveTabToSpace(index, other),
                            ));
                        }
                    }
                }
                entries.push(MenuEntry::Separator);
                entries.push(danger("Close tab", "", MenuAction::CloseTab(index)));
                entries
            }
            MenuTarget::Pane(id) => vec![
                item("Copy", "Ctrl+Shift+C", MenuAction::Copy(id)),
                item("Paste", "Ctrl+Shift+V", MenuAction::Paste(id)),
                MenuEntry::Separator,
                item("Split right", "Ctrl+Shift+D", MenuAction::PaneSplit(id, Axis::Row)),
                item("Split down", "Ctrl+Shift+E", MenuAction::PaneSplit(id, Axis::Column)),
                MenuEntry::Separator,
                danger("Close pane", "Ctrl+Shift+W", MenuAction::ClosePane(id)),
            ],
            MenuTarget::Sidebar => vec![
                item("New terminal", "", MenuAction::NewTerminal),
                item("New space", "Ctrl+Shift+N", MenuAction::NewSpace),
                item("Settings", "Ctrl+,", MenuAction::OpenSettings),
                MenuEntry::Separator,
                item(
                    if self.sidebar_collapsed { "Show sidebar" } else { "Hide sidebar" },
                    "Ctrl+Shift+B",
                    MenuAction::ToggleSidebar,
                ),
                MenuEntry::Separator,
                danger("Quit mer", "Ctrl+Shift+Q", MenuAction::Quit),
            ],
        }
    }

    pub(super) fn apply_menu(&mut self, action: MenuAction, cx: &mut Context<Self>) {
        self.menu = None;
        self.focus_active(cx);
        cx.notify();
        match action {
            MenuAction::RenameSpace(index) => {
                let name = self.model.spaces().get(index).map(Space::editable_name);
                if let Some(name) = name {
                    self.open_prompt(PromptKind::RenameSpace(index), name, cx);
                }
            }
            MenuAction::SleepSpace(index) => self.request_sleep(index, cx),
            MenuAction::WakeSpace(index) => self.select_space(index, cx),
            MenuAction::NewTabIn(index) => {
                self.model.select_space(index);
                self.run(Cmd::NewTab, cx);
            }
            MenuAction::DeleteSpace(index) => self.request_delete_space(index, cx),
            MenuAction::SpaceColor(index, color) => {
                self.model.set_space_color(index, color);
                self.after_model_change(cx);
            }
            MenuAction::RenameTab(index) => {
                let title = self
                    .model
                    .active_space()
                    .tabs()
                    .get(index)
                    .map(|tab| tab.title.clone().unwrap_or_default());
                if let Some(title) = title {
                    self.open_prompt(PromptKind::RenameTab(index), title, cx);
                }
            }
            MenuAction::SleepTab(index) => self.request_sleep_tab(index, cx),
            MenuAction::WakeTab(index) => self.select_tab(index, cx),
            MenuAction::CloseTab(index) => self.close_tab_at(index, cx),
            MenuAction::TabColor(index, color) => {
                self.model.set_tab_color(index, color);
                self.after_model_change(cx);
            }
            MenuAction::MoveTab(from, to) => {
                self.move_tab(from, to, cx);
            }
            MenuAction::MoveTabToSpace(tab, space) => {
                self.request_move_tab_to_space(tab, space, cx);
            }
            MenuAction::MoveSpace(from, to) => {
                self.move_space(from, to, cx);
            }
            MenuAction::TabSplit(index, axis) => {
                self.model.select_tab(index);
                self.run(split_command(axis), cx);
            }
            MenuAction::PaneSplit(id, axis) => {
                self.model.focus_pane(id);
                self.run(split_command(axis), cx);
            }
            MenuAction::Copy(id) => {
                if let Some(live) = self.live.get(&id) {
                    live.view.update(cx, |view, cx| view.copy_selection(cx));
                }
            }
            MenuAction::Paste(id) => {
                if let Some(live) = self.live.get(&id) {
                    live.view.update(cx, |view, cx| view.paste(cx));
                }
            }
            MenuAction::ClosePane(id) => {
                self.model.focus_pane(id);
                self.run(Cmd::ClosePane, cx);
            }
            MenuAction::NewSpace => self.run(Cmd::NewSpace, cx),
            MenuAction::NewTerminal => self.run(Cmd::NewTerminal, cx),
            MenuAction::PromoteToSpace(index) => {
                if self.model.promote_to_space(index) {
                    self.after_model_change(cx);
                }
            }
            MenuAction::DetachTab(index) => {
                if self.model.detach_tab(index) {
                    self.after_model_change(cx);
                }
            }
            MenuAction::OpenSettings => self.open_settings(cx),
            MenuAction::ToggleSidebar => self.run(Cmd::ToggleSidebar, cx),
            MenuAction::Quit => self.run(Cmd::Quit, cx),
        }
    }

    #[allow(clippy::too_many_lines)] // one flat list of element builders
    pub(super) fn render_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let chrome = &self.palette.chrome;
        let (surface, text, dim, border) =
            (chrome.surface, chrome.text, chrome.text_dim, self.palette.hairline());
        let clear = hsla(0.0, 0.0, 0.0, 0.0);

        let rows = menu.entries.iter().enumerate().map(|(n, entry)| match entry {
            MenuEntry::Separator => div().h(px(1.0)).mx_2().my_1().bg(border).into_any_element(),
            MenuEntry::Item { label, hint, action, danger, enabled } => {
                let action = *action;
                let color = if !*enabled {
                    dim
                } else if *danger {
                    accent_color(Accent::Red)
                } else {
                    text
                };
                div()
                    .id(("menu-item", n))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .h(self.ui_px(30.0))
                    .px_3()
                    .rounded_lg()
                    .text_color(color)
                    .when(!*enabled, |row| row.opacity(0.5))
                    .when(*enabled, |row| {
                        row.cursor_pointer().hover(move |row| row.bg(surface)).on_click(
                            cx.listener(move |this, _: &ClickEvent, _window, cx| {
                                this.apply_menu(action, cx);
                            }),
                        )
                    })
                    .child(SharedString::from(label.clone()))
                    .children((!hint.is_empty()).then(|| crate::tooltip::kbd(&self.palette, *hint)))
                    .into_any_element()
            }
            MenuEntry::Colors { target, current } => {
                let target = *target;
                let current = *current;
                let swatch = |k: usize, color: Option<Accent>| {
                    let selected = color == current;
                    let base = color.map_or(clear, accent_color);
                    div()
                        .id(("swatch", n * 16 + k))
                        .size(self.ui_px(18.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .cursor_pointer()
                        .bg(base)
                        .border_2()
                        .border_color(if selected {
                            text
                        } else if color.is_none() {
                            dim
                        } else {
                            clear
                        })
                        .text_xs()
                        .text_color(dim)
                        .hover(move |s| s.border_color(text))
                        .when(color.is_none(), |s| s.child("×"))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                            let action = match target {
                                ColorTarget::Space(index) => MenuAction::SpaceColor(index, color),
                                ColorTarget::Tab(index) => MenuAction::TabColor(index, color),
                            };
                            this.apply_menu(action, cx);
                        }))
                };
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.0))
                    .px_3()
                    .py_2()
                    .child(swatch(0, None))
                    .children(
                        Accent::ALL
                            .iter()
                            .enumerate()
                            .map(|(k, accent)| swatch(k + 1, Some(*accent))),
                    )
                    .into_any_element()
            }
        });

        let panel = div()
            .w(self.ui_px(300.0))
            .flex()
            .flex_col()
            .p_1()
            .bg(chrome.bar)
            .border_1()
            .border_color(border)
            .rounded_xl()
            .shadow(crate::theme::overlay_shadow(self.palette.is_dark()))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .children(rows);

        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .track_focus(&self.menu_focus)
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                    if event.keystroke.key == "escape" {
                        this.close_menu(cx);
                    }
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _window, cx| this.close_menu(cx)),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|this, _: &MouseDownEvent, _window, cx| this.close_menu(cx)),
                )
                .child(anchored().position(menu.position).snap_to_window().child(panel))
                .into_any_element(),
        )
    }
}

const fn split_command(axis: Axis) -> Cmd {
    match axis {
        Axis::Row => Cmd::SplitRight,
        Axis::Column => Cmd::SplitDown,
    }
}
