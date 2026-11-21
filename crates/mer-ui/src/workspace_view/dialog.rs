//! The modal dialogs: the command palette, rename boxes and confirmations.
//!
//! They all share one layout: a panel that hugs its content, a title, wrapped text, an
//! optional input box and real buttons, so they work with the mouse as well as with
//! Enter and Escape.

use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, Hsla, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder as _,
};
use mer_core::Accent;

use super::{PALETTE_ROWS, Prompt, PromptKind, WorkspaceView};
use crate::commands;
use crate::theme::accent_color;

const DIALOG_WIDTH: f32 = 440.0;
const PALETTE_WIDTH: f32 = 560.0;

fn plural(count: usize, noun: &str) -> String {
    if count == 1 { format!("1 {noun}") } else { format!("{count} {noun}s") }
}

impl Prompt {
    pub(super) fn is_palette(&self) -> bool {
        matches!(self.kind, PromptKind::Commands)
    }

    fn placeholder(&self) -> &'static str {
        match self.kind {
            PromptKind::Commands => "Type a command…",
            PromptKind::RenameSpace(_) => "Space name",
            _ => "Tab name",
        }
    }

    fn takes_text(&self) -> bool {
        matches!(
            self.kind,
            PromptKind::Commands | PromptKind::RenameSpace(_) | PromptKind::RenameTab(_)
        )
    }

    /// Label of the confirming button.
    fn primary_label(&self) -> &'static str {
        match self.kind {
            PromptKind::Commands => "Run",
            PromptKind::RenameSpace(_) | PromptKind::RenameTab(_) => "Rename",
            PromptKind::ConfirmSleep { .. } | PromptKind::ConfirmSleepTab { .. } => "Sleep",
            PromptKind::ConfirmMoveTab { .. } => "Move",
            PromptKind::ConfirmDeleteSpace { .. } => "Delete",
        }
    }

    fn is_destructive(&self) -> bool {
        matches!(self.kind, PromptKind::ConfirmDeleteSpace { .. })
    }
}

impl WorkspaceView {
    fn space_name(&self, index: usize) -> String {
        self.model.spaces().get(index).map(|s| s.name.clone()).unwrap_or_default()
    }

    fn tab_name(&self, index: usize) -> String {
        self.model
            .active_space()
            .tabs()
            .get(index)
            .map(|tab| tab.display_title().to_owned())
            .unwrap_or_default()
    }

    /// Title and explanation of a dialog. The palette has neither.
    fn dialog_text(&self, prompt: &Prompt) -> (String, Vec<String>) {
        let running = |busy: &[String]| {
            (!busy.is_empty()).then(|| format!("Still running: {}.", busy.join(", ")))
        };
        match &prompt.kind {
            PromptKind::Commands => (String::new(), Vec::new()),
            PromptKind::RenameSpace(_) => ("Rename space".to_owned(), Vec::new()),
            PromptKind::RenameTab(_) => (
                "Rename tab".to_owned(),
                vec!["Leave it empty to follow the running program.".to_owned()],
            ),
            PromptKind::ConfirmSleep { index, busy } => (
                format!("Sleep \"{}\"?", self.space_name(*index)),
                std::iter::once(
                    "Its terminals stop. They come back in the same folders, with their history, when you open the space."
                        .to_owned(),
                )
                .chain(running(busy))
                .collect(),
            ),
            PromptKind::ConfirmSleepTab { index, busy } => (
                format!("Sleep \"{}\"?", self.tab_name(*index)),
                std::iter::once(
                    "Its terminals stop. They come back in the same folders, with their history, when you open the tab."
                        .to_owned(),
                )
                .chain(running(busy))
                .collect(),
            ),
            PromptKind::ConfirmMoveTab { tab, space, busy } => (
                format!(
                    "Move \"{}\" to \"{}\"?",
                    self.tab_name(*tab),
                    self.space_name(*space)
                ),
                std::iter::once(
                    "That space is asleep, so the tab sleeps with it: its terminals stop and come back, with their history, when you open it."
                        .to_owned(),
                )
                .chain(running(busy))
                .collect(),
            ),
            PromptKind::ConfirmDeleteSpace { name, terminals, busy, .. } => (
                format!("Delete \"{name}\"?"),
                std::iter::once(format!(
                    "{} and their saved history will be removed. This cannot be undone.",
                    plural(*terminals, "terminal")
                ))
                .chain(running(busy))
                .collect(),
            ),
        }
    }

    #[allow(clippy::too_many_lines)] // one flat builder chain
    pub(super) fn render_prompt(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let prompt = self.prompt.as_ref()?;
        let chrome = &self.palette.chrome;
        let (surface, text, dim, accent, border) =
            (chrome.surface, chrome.text, chrome.text_dim, chrome.accent, self.palette.hairline());
        let warning = accent_color(Accent::Amber);
        let palette = prompt.is_palette();
        let (title, lines) = self.dialog_text(prompt);

        let input = prompt.takes_text().then(|| {
            let shown = if prompt.query.is_empty() {
                div().text_color(dim).child(prompt.placeholder())
            } else {
                div().text_color(text).child(SharedString::from(format!("{}▏", prompt.query)))
            };
            div()
                .w_full()
                .h(self.ui_px(36.0))
                .px_3()
                .flex()
                .items_center()
                .rounded_lg()
                .bg(if palette { chrome.bar } else { surface })
                .border_1()
                .border_color(if palette { chrome.bar } else { accent })
                .overflow_hidden()
                .child(shown.min_w_0())
        });

        let body = lines.iter().enumerate().map(|(n, line)| {
            let color: Hsla = if line.starts_with("Still running") { warning } else { dim };
            div()
                .id(("dialog-line", n))
                .w_full()
                .min_w_0()
                .text_color(color)
                .child(SharedString::from(line.clone()))
        });

        let rows = if palette {
            let entries = commands::filter(&prompt.query);
            if entries.is_empty() {
                vec![
                    div()
                        .h(self.ui_px(32.0))
                        .px_3()
                        .text_color(dim)
                        .child("No matching command")
                        .into_any_element(),
                ]
            } else {
                entries
                    .into_iter()
                    .take(PALETTE_ROWS)
                    .enumerate()
                    .map(|(index, entry)| {
                        let selected = index == prompt.selected;
                        div()
                            .id(("palette-row", index))
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .h(self.ui_px(32.0))
                            .px_3()
                            .rounded_lg()
                            .cursor_pointer()
                            .text_color(if selected { text } else { dim })
                            .when(selected, |row| row.bg(surface))
                            .hover(move |row| row.bg(surface).text_color(text))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                                if let Some(prompt) = this.prompt.as_mut() {
                                    prompt.selected = index;
                                }
                                this.accept_prompt(cx);
                            }))
                            .child(entry.label)
                            .children(
                                (!entry.shortcut.is_empty())
                                    .then(|| crate::tooltip::kbd(&self.palette, entry.shortcut)),
                            )
                            .into_any_element()
                    })
                    .collect()
            }
        } else {
            Vec::new()
        };

        let primary_fill = if prompt.is_destructive() { accent_color(Accent::Red) } else { accent };
        let footer = (!palette).then(|| {
            // A chip on the neutral button; on the colored one it is a tint of the button itself.
            let key_hint = |label: &'static str, on_fill: bool| {
                crate::tooltip::kbd(&self.palette, label).map(|chip| {
                    if on_fill {
                        chip.bg(crate::theme::with_alpha(self.palette.background, 0.25))
                            .text_color(self.palette.background)
                    } else {
                        chip.bg(crate::theme::with_alpha(text, 0.10))
                    }
                })
            };
            div()
                .w_full()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .pt_1()
                .child(
                    div()
                        .id("dialog-cancel")
                        .flex()
                        .items_center()
                        .gap_2()
                        .h(self.ui_px(32.0))
                        .px_4()
                        .rounded_lg()
                        .cursor_pointer()
                        .bg(surface)
                        .text_color(text)
                        .hover(move |button| button.bg(border))
                        .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                            this.close_prompt(cx);
                        }))
                        .child("Cancel")
                        .child(key_hint("Esc", false)),
                )
                .child(
                    div()
                        .id("dialog-confirm")
                        .flex()
                        .items_center()
                        .gap_2()
                        .h(self.ui_px(32.0))
                        .px_4()
                        .rounded_lg()
                        .cursor_pointer()
                        .bg(primary_fill)
                        .text_color(self.palette.background)
                        .font_weight(FontWeight::SEMIBOLD)
                        .hover(|button| button.opacity(0.88))
                        .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                            this.accept_prompt(cx);
                        }))
                        .child(prompt.primary_label())
                        .child(key_hint("Enter", true)),
                )
        });

        let panel = div()
            .track_focus(&self.prompt_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                this.on_prompt_key(event, cx);
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(self.ui_px(if palette { PALETTE_WIDTH } else { DIALOG_WIDTH }))
            .flex()
            .flex_col()
            .gap_3()
            .p(self.ui_px(if palette { 8.0 } else { 20.0 }))
            .bg(chrome.bar)
            .border_1()
            .border_color(border)
            .rounded_xl()
            .shadow(crate::theme::overlay_shadow(self.palette.is_dark()))
            .when(!title.is_empty(), |panel| {
                panel.child(
                    div()
                        .text_size(self.ui_px(15.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(text)
                        .child(SharedString::from(title.clone())),
                )
            })
            .children(body)
            .children(input)
            .children(rows)
            .children(footer);

        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .justify_center()
                .when(palette, |overlay| overlay.items_start().pt(self.ui_px(90.0)))
                .when(!palette, gpui::Styled::items_center)
                .bg(gpui::hsla(0.0, 0.0, 0.0, 0.45))
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _window, cx| {
                        this.close_prompt(cx);
                    }),
                )
                .child(panel)
                .into_any_element(),
        )
    }
}
