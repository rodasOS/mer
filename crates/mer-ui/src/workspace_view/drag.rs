//! Drag and drop: reordering tabs and spaces, and moving a tab into another space.
//!
//! GPUI draws the ghost under the cursor and the drop marks only while a drag lasts, so none of
//! this costs anything at rest.

use gpui::{
    App, AppContext as _, Context, Div, FontWeight, Hsla, InteractiveElement, IntoElement,
    ParentElement, Pixels, Point, Render, SharedString, Stateful, StatefulInteractiveElement,
    Styled, Window, div, px,
};

use super::{PromptKind, WorkspaceView};
use crate::theme::{Palette, with_alpha};

/// A tab being dragged: where it was in the active space.
#[derive(Debug, Clone, Copy)]
pub(super) struct DraggedTab {
    pub index: usize,
    /// Another space may take it: its own space keeps at least one more tab.
    pub movable: bool,
}

/// A space being dragged.
#[derive(Debug, Clone, Copy)]
pub(super) struct DraggedSpace {
    pub index: usize,
}

/// The small pill that follows the cursor while dragging.
struct DragGhost {
    label: SharedString,
    fill: Hsla,
    border: Hsla,
    text: Hsla,
}

impl Render for DragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(px(280.0))
            .truncate()
            .px_3()
            .py_1()
            .rounded_lg()
            .border_1()
            .border_color(self.border)
            .bg(self.fill)
            .text_color(self.text)
            .font_weight(FontWeight::MEDIUM)
            .opacity(0.92)
            .child(self.label.clone())
    }
}

/// Builds the ghost for `.on_drag(...)`.
fn ghost<T: 'static>(
    palette: &Palette,
    label: String,
) -> impl Fn(&T, Point<Pixels>, &mut Window, &mut App) -> gpui::Entity<DragGhost> + 'static {
    let (fill, border, text) = (palette.chrome.surface, palette.chrome.accent, palette.chrome.text);
    move |_, _, _, cx| cx.new(|_| DragGhost { label: label.clone().into(), fill, border, text })
}

impl WorkspaceView {
    /// Makes a tab draggable and a place to drop tabs (to reorder).
    pub(super) fn tab_dnd(
        &self,
        tab: Stateful<Div>,
        index: usize,
        title: &str,
        movable: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let accent = self.palette.chrome.accent;
        tab.on_drag(
            DraggedTab { index, movable },
            ghost::<DraggedTab>(&self.palette, title.to_owned()),
        )
        .drag_over::<DraggedTab>(move |style, dragged, _, _| {
            if dragged.index == index {
                style
            } else {
                style.bg(with_alpha(accent, 0.18)).border_color(accent)
            }
        })
        .on_drop::<DraggedTab>(cx.listener(move |this, dragged: &DraggedTab, _, cx| {
            this.move_tab(dragged.index, index, cx);
        }))
    }

    /// Makes a space row (or rail pill) draggable, a place to drop spaces (to reorder) and a
    /// place to drop tabs (to move them into that space).
    pub(super) fn space_dnd(
        &self,
        row: Stateful<Div>,
        index: usize,
        name: &str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let accent = self.palette.chrome.accent;
        let is_active = index == self.model.active_index();
        row.on_drag(DraggedSpace { index }, ghost::<DraggedSpace>(&self.palette, name.to_owned()))
            .drag_over::<DraggedSpace>(move |style, dragged, _, _| {
                if dragged.index == index {
                    style
                } else {
                    style.bg(with_alpha(accent, 0.18)).border_color(accent)
                }
            })
            .on_drop::<DraggedSpace>(cx.listener(move |this, dragged: &DraggedSpace, _, cx| {
                this.move_space(dragged.index, index, cx);
            }))
            .drag_over::<DraggedTab>(move |style, dragged, _, _| {
                if dragged.movable && !is_active {
                    style.bg(with_alpha(accent, 0.28)).border_color(accent)
                } else {
                    style
                }
            })
            .on_drop::<DraggedTab>(cx.listener(move |this, dragged: &DraggedTab, _, cx| {
                this.request_move_tab_to_space(dragged.index, index, cx);
            }))
    }

    /// Reorders the tabs of the active space.
    pub(super) fn move_tab(&mut self, from: usize, to: usize, cx: &mut Context<Self>) -> bool {
        let moved = self.model.move_tab(from, to);
        if moved {
            self.after_reorder(cx);
        }
        moved
    }

    /// Reorders the spaces.
    pub(super) fn move_space(&mut self, from: usize, to: usize, cx: &mut Context<Self>) -> bool {
        let moved = self.model.move_space(from, to);
        if moved {
            self.after_reorder(cx);
        }
        moved
    }

    /// Indexes held by an open menu or dialog no longer mean the same thing.
    fn after_reorder(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        self.prompt = None;
        self.persist(cx);
        cx.notify();
    }

    /// Moves a tab of the active space into another space. When that space sleeps, the tab
    /// sleeps with it, so a program running in it is asked about first.
    pub(super) fn request_move_tab_to_space(
        &mut self,
        tab: usize,
        space: usize,
        cx: &mut Context<Self>,
    ) {
        let source = self.model.active_space();
        let Some(destination) = self.model.spaces().get(space) else { return };
        if space == self.model.active_index()
            || tab >= source.tabs().len()
            || source.tabs().len() < 2
        {
            return;
        }
        if destination.state == mer_core::SpaceState::Sleeping {
            let ids: Vec<_> = self.model.tab_panes(tab).iter().map(|pane| pane.id).collect();
            let busy = self.busy_in_panes(&ids, cx);
            if !busy.is_empty() {
                self.open_prompt(
                    PromptKind::ConfirmMoveTab { tab, space, busy },
                    String::new(),
                    cx,
                );
                return;
            }
        }
        self.move_tab_to_space(tab, space, cx);
    }

    /// Does the move: stops the tab's terminals first when it goes into a sleeping space.
    pub(super) fn move_tab_to_space(
        &mut self,
        tab: usize,
        space: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        let sleeping = self
            .model
            .spaces()
            .get(space)
            .is_some_and(|s| s.state == mer_core::SpaceState::Sleeping);
        let ids: Vec<_> = self.model.tab_panes(tab).iter().map(|pane| pane.id).collect();
        if sleeping {
            self.stash_panes(&ids, cx);
        }
        let moved = self.model.move_tab_to_space(tab, space);
        if moved {
            self.menu = None;
            self.prompt = None;
            self.after_model_change(cx);
        }
        moved
    }
}
