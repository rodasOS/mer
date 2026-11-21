//! Small tooltips for names the interface has to cut short, and the key chip used in menus.

use std::time::Duration;

use gpui::{
    AnyView, App, AppContext as _, Context, Div, FontWeight, Hsla, IntoElement, ParentElement,
    Render, SharedString, Styled, Window, div, px,
};

use crate::theme::{Palette, tooltip_shadow};

/// How long the mouse rests on something before its tooltip shows.
pub const SHOW_DELAY: Duration = Duration::from_millis(450);

/// The tooltip's content: a title and, optionally, a second dimmer line.
struct Tip {
    title: SharedString,
    details: Vec<SharedString>,
    fill: Hsla,
    border: Hsla,
    text: Hsla,
    dim: Hsla,
    dark: bool,
}

impl Render for Tip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .px_3()
            .py_2()
            .rounded_lg()
            .border_1()
            .border_color(self.border)
            .bg(self.fill)
            .shadow(tooltip_shadow(self.dark))
            .child(
                div()
                    .whitespace_nowrap()
                    .text_color(self.text)
                    .font_weight(FontWeight::MEDIUM)
                    .child(self.title.clone()),
            )
            .children(self.details.iter().map(|detail| {
                div().whitespace_nowrap().text_xs().text_color(self.dim).child(detail.clone())
            }))
    }
}

/// The longest line a tooltip shows, in characters.
const MAX_LINE: usize = 70;

/// Shortens `text` to at most `max` characters by replacing its middle with `…`, so both the
/// start and the end (the interesting part of a path or command) stay visible.
pub fn ellipsize_middle(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max || max < 2 {
        return text.to_owned();
    }
    let keep = max - 1;
    let head = keep.div_ceil(2);
    let tail = keep - head;
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(count - tail).collect();
    format!("{start}…{end}")
}

/// A tooltip builder for `.tooltip(...)`. GPUI lays tooltips out at their minimum width, so
/// lines never wrap: long ones are shortened in the middle instead.
pub fn tip(
    palette: &Palette,
    title: impl Into<SharedString>,
    details: impl IntoIterator<Item = String>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let title = SharedString::from(ellipsize_middle(&title.into(), MAX_LINE));
    let details: Vec<SharedString> =
        details.into_iter().map(|line| ellipsize_middle(&line, MAX_LINE).into()).collect();
    let chrome = palette.chrome.clone();
    let (border, dark) = (palette.hairline(), palette.is_dark());
    move |_window, cx| {
        cx.new(|_| Tip {
            title: title.clone(),
            details: details.clone(),
            fill: chrome.bar,
            border,
            text: chrome.text,
            dim: chrome.text_dim,
            dark,
        })
        .into()
    }
}

/// A small rounded chip showing a key, such as `Ctrl+,` or `Esc`.
pub fn kbd(palette: &Palette, label: impl Into<SharedString>) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .h(px(18.0))
        .px(px(6.0))
        .rounded_md()
        .bg(palette.hover())
        .text_xs()
        .text_color(palette.chrome.text_dim)
        .child(label.into())
}

#[cfg(test)]
mod tests {
    use super::ellipsize_middle;

    #[test]
    fn short_text_is_untouched() {
        assert_eq!(ellipsize_middle("~/dev/mer", 20), "~/dev/mer");
    }

    #[test]
    fn long_text_keeps_both_ends() {
        let text = "~/Documentos/proyectos/aplicacion-web/src/app/shared";
        let short = ellipsize_middle(text, 20);
        assert_eq!(short.chars().count(), 20);
        assert!(short.starts_with("~/Documen"));
        assert!(short.ends_with("shared"));
        assert!(short.contains('…'));
    }

    #[test]
    fn counts_characters_not_bytes() {
        let short = ellipsize_middle("ñandú-ñandú-ñandú-ñandú", 9);
        assert_eq!(short.chars().count(), 9);
    }
}
