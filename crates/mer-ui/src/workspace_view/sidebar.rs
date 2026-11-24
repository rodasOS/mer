//! The sidebar: one row per space or terminal, the buttons that make more, and the handle that
//! resizes it. Collapsed, it is not drawn at all (the terminals get the whole window); the
//! toggle in the tab bar brings it back.

use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, SharedString, StatefulInteractiveElement, Styled, div, hsla,
    prelude::FluentBuilder as _, px,
};
use mer_core::{Space, SpaceKind, SpaceState};

use super::context_menu::MenuTarget;
use super::{SIDEBAR_WIDTH, WorkspaceView, status_color};
use crate::commands::Cmd;
use crate::theme::with_alpha;
use crate::tooltip;

/// The longest folder shown under a terminal's name, in characters.
const PLACE_MAX: usize = 40;

impl WorkspaceView {
    /// The sidebar, or `None` while it is collapsed.
    #[allow(clippy::too_many_lines)] // builder chain
    pub(super) fn render_sidebar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.sidebar_collapsed {
            return None;
        }
        let palette = &self.palette;
        let chrome = &palette.chrome;
        let (text, dim) = (chrome.text, chrome.text_dim);
        let (hover, hairline) = (palette.hover(), palette.hairline());

        let rows = self
            .model
            .spaces()
            .iter()
            .enumerate()
            .map(|(index, space)| match space.kind() {
                SpaceKind::Space => self.space_row(index, space, cx),
                SpaceKind::Terminal => self.terminal_row(index, space, cx),
            })
            .collect::<Vec<_>>();

        // A dashed button that makes something new.
        let add_button =
            |id: &'static str, label: &'static str, cmd: Cmd, cx: &mut Context<Self>| {
                div()
                    .id(id)
                    .flex_1()
                    .min_w_0()
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
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        this.run(cmd, cx);
                    }))
                    .child(label)
            };

        Some(
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
                                .child("WORKSPACE"),
                        )
                        // Visible, the toggle is here; hidden, it moves to the bar or strip on top.
                        .child(self.render_sidebar_toggle(cx)),
                )
                .children(rows)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(add_button("new-terminal", "+  Terminal", Cmd::NewTerminal, cx))
                        .child(add_button("new-space", "+  Space", Cmd::NewSpace, cx)),
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
                .into_any_element(),
        )
    }

    /// The button of the tab bar that hides or shows the sidebar. It is always there, so a hidden
    /// sidebar can always be brought back.
    pub(super) fn render_sidebar_toggle(&self, cx: &mut Context<Self>) -> AnyElement {
        let chrome = &self.palette.chrome;
        let (text, dim) = (chrome.text, chrome.text_dim);
        let hover = self.palette.hover();
        let hidden = self.sidebar_collapsed;
        let title = if hidden { "Show sidebar" } else { "Hide sidebar" };
        div()
            .id("toggle-sidebar")
            .flex()
            .items_center()
            .justify_center()
            .size(self.ui_px(28.0))
            .flex_none()
            .rounded_lg()
            .cursor_pointer()
            .text_color(dim)
            .hover(move |button| button.bg(hover).text_color(text))
            .tooltip(tooltip::tip(&self.palette, title, ["Ctrl+Shift+B".to_owned()]))
            .tooltip_show_delay(tooltip::SHOW_DELAY)
            .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                this.set_sidebar_collapsed(!this.sidebar_collapsed, cx);
            }))
            // Pointing at the window's edge folds it; pointing inward brings it back.
            .child(self.sidebar_side.chevron(!hidden))
            .into_any_element()
    }

    /// A space: a colored bar, its name, how many terminals it holds.
    #[allow(clippy::too_many_lines)] // builder chain
    fn space_row(&self, index: usize, space: &Space, cx: &mut Context<Self>) -> AnyElement {
        let palette = &self.palette;
        let chrome = &palette.chrome;
        let (text, dim) = (chrome.text, chrome.text_dim);
        let hover = palette.hover();
        let clear = hsla(0.0, 0.0, 0.0, 0.0);
        let is_active = index == self.model.active_index();
        let sleeping = space.state == SpaceState::Sleeping;
        let terminals = space.pane_count();
        let color = self.space_color(space.color);
        let agent_color =
            self.space_agent_status(index).map(|status| status_color(&self.palette, status));
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
            .hover(move |row| row.bg(with_alpha(color, if is_active { 0.22 } else { 0.10 })))
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
            .children(
                agent_color.map(|status| {
                    div().size(self.ui_px(8.0)).flex_none().rounded_full().bg(status)
                }),
            )
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
            .into_any_element()
    }

    /// A terminal on its own: what it runs, and the folder under it.
    #[allow(clippy::too_many_lines)] // builder chain
    fn terminal_row(&self, index: usize, space: &Space, cx: &mut Context<Self>) -> AnyElement {
        let palette = &self.palette;
        let chrome = &palette.chrome;
        let (text, dim, accent) = (chrome.text, chrome.text_dim, chrome.accent);
        let hover = palette.hover();
        let clear = hsla(0.0, 0.0, 0.0, 0.0);
        let is_active = index == self.model.active_index();
        let sleeping = space.state == SpaceState::Sleeping;
        let tab = space.active_tab();
        // The live folder when the shell reports it, else the last one saved, with the branch.
        let place = tab
            .pane(tab.focused())
            .and_then(Self::place_line)
            .map(|line| tooltip::ellipsize_middle(&line, PLACE_MAX));
        let title = space.display_name().to_owned();
        let agent_color =
            self.space_agent_status(index).map(|status| status_color(&self.palette, status));
        self.space_dnd(div().id(("space", index)), index, &title, cx)
            .flex()
            .flex_row()
            .items_center()
            .gap(self.ui_px(8.0))
            .h(self.ui_px(44.0))
            .px(self.ui_px(8.0))
            .rounded_lg()
            .border_1()
            .border_color(if is_active { with_alpha(accent, 0.35) } else { clear })
            .cursor_pointer()
            .when(is_active, move |row| row.bg(with_alpha(accent, 0.16)))
            .hover(move |row| row.bg(with_alpha(accent, if is_active { 0.22 } else { 0.10 })))
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
            .tooltip(tooltip::tip(palette, title.clone(), Self::tab_details(tab)))
            .tooltip_show_delay(tooltip::SHOW_DELAY)
            .child(
                div()
                    .size(self.ui_px(24.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .bg(hover)
                    .text_xs()
                    .text_color(if is_active { accent } else { dim })
                    .when(sleeping, |icon| icon.opacity(0.6))
                    .child(">_"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .when(sleeping, |column| column.opacity(0.75))
                    .child(
                        div()
                            .truncate()
                            .text_color(if is_active { text } else { dim })
                            .when(is_active, |name| name.font_weight(FontWeight::SEMIBOLD))
                            .child(SharedString::from(title)),
                    )
                    .children(place.map(|place| {
                        div().truncate().text_xs().text_color(dim).child(SharedString::from(place))
                    })),
            )
            .children(
                agent_color.map(|status| {
                    div().size(self.ui_px(8.0)).flex_none().rounded_full().bg(status)
                }),
            )
            .children(sleeping.then(|| {
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
                    .child("zz")
            }))
            // A quiet ×: always there, brighter on the open row and when pointed at.
            .child(
                div()
                    .id(("terminal-close", index))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(self.ui_px(20.0))
                    .rounded_full()
                    .text_color(dim)
                    .opacity(if is_active { 0.7 } else { 0.4 })
                    .cursor_pointer()
                    .hover(move |close| close.opacity(1.0).bg(hover).text_color(text))
                    .tooltip(tooltip::tip(
                        palette,
                        "Close terminal".to_owned(),
                        Vec::<String>::new(),
                    ))
                    .tooltip_show_delay(tooltip::SHOW_DELAY)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        cx.stop_propagation();
                        this.close_terminal(index, cx);
                    }))
                    .child("×"),
            )
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
                    .bg(if dragging { accent } else { hsla(0.0, 0.0, 0.0, 0.0) })
                    .group_hover("sidebar-handle", move |bar| {
                        bar.bg(if dragging { accent } else { hairline })
                    }),
            )
            .into_any_element()
    }
}
