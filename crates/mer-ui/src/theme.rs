//! Color palette and resolution of terminal colors to GPUI colors.

use std::sync::Arc;

use gpui::{BoxShadow, Hsla, Rgba, hsla, px, rgb};
use mer_core::Accent;
use mer_term::alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

/// Colors of the application chrome (sidebar, tab bar, overlays).
#[derive(Debug, Clone)]
pub struct Chrome {
    pub sidebar: Hsla,
    pub bar: Hsla,
    pub surface: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub text_dim: Hsla,
    pub accent: Hsla,
}

#[derive(Debug, Clone)]
pub struct Palette {
    pub foreground: Hsla,
    pub background: Hsla,
    pub cursor: Hsla,
    pub ansi: [Hsla; 16],
    pub chrome: Chrome,
    /// The 256 indexed colors, resolved once: painting looks them up cell by cell.
    indexed: Arc<[Hsla; 256]>,
}

fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

impl Palette {
    pub fn from_theme(theme: &mer_config::Theme) -> Self {
        let color = |c: mer_config::Rgb| {
            hex((u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b))
        };
        let ansi = theme.ansi.map(color);
        Self {
            foreground: color(theme.foreground),
            background: color(theme.background),
            cursor: color(theme.cursor),
            indexed: Arc::new(indexed_table(&ansi)),
            ansi,
            chrome: Chrome {
                sidebar: color(theme.chrome.sidebar),
                bar: color(theme.chrome.bar),
                surface: color(theme.chrome.surface),
                border: color(theme.chrome.border),
                text: color(theme.chrome.text),
                text_dim: color(theme.chrome.text_dim),
                accent: color(theme.chrome.accent),
            },
        }
    }

    /// A dark theme (its terminal background is dark).
    pub fn is_dark(&self) -> bool {
        self.background.l < 0.5
    }

    /// Background of a row or button under the mouse, on the sidebar or the tab bar.
    pub fn hover(&self) -> Hsla {
        with_alpha(self.chrome.surface, 0.6)
    }

    /// A fine line for cards and overlays: softer than the theme's border.
    pub fn hairline(&self) -> Hsla {
        with_alpha(self.chrome.border, 0.7)
    }

    pub fn resolve(&self, color: Color) -> Hsla {
        match color {
            Color::Spec(rgb) => from_rgb(rgb),
            Color::Indexed(index) => self.indexed(index),
            Color::Named(name) => self.named(name),
        }
    }

    /// Resolves a color index used by `OSC 4/10/11` queries.
    pub fn resolve_index(&self, index: usize) -> Hsla {
        match index {
            0..=255 => self.indexed(u8::try_from(index).unwrap_or(0)),
            257 => self.background,
            258 => self.cursor,
            _ => self.foreground, // 256 and the bright/dim foreground variants
        }
    }

    fn named(&self, name: NamedColor) -> Hsla {
        match name {
            NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::DimForeground => {
                self.foreground
            }
            NamedColor::Background => self.background,
            NamedColor::Cursor => self.cursor,
            other => {
                let index = other as usize;
                if index < 16 {
                    self.ansi[index]
                } else if (259..=266).contains(&index) {
                    let mut dim = self.ansi[index - 259];
                    dim.l *= 0.66;
                    dim
                } else {
                    self.foreground
                }
            }
        }
    }

    fn indexed(&self, index: u8) -> Hsla {
        self.indexed[usize::from(index)]
    }
}

/// The 16 theme colors, the 6x6x6 color cube and the 24 grays.
fn indexed_table(ansi: &[Hsla; 16]) -> [Hsla; 256] {
    let mut table = [Hsla::default(); 256];
    for (index, slot) in (0..=255_u8).zip(table.iter_mut()) {
        *slot = match index {
            0..=15 => ansi[usize::from(index)],
            16..=231 => {
                let n = index - 16;
                let level = |c: u8| if c == 0 { 0 } else { 55 + 40 * c };
                from_rgb(Rgb { r: level(n / 36), g: level((n / 6) % 6), b: level(n % 6) })
            }
            232..=255 => {
                let gray = 8 + 10 * (index - 232);
                from_rgb(Rgb { r: gray, g: gray, b: gray })
            }
        };
    }
    table
}

/// Resolves a color like [`Palette::resolve`], remembering the last truecolor conversion:
/// neighbouring cells usually share it.
pub fn resolve_cached(palette: &Palette, color: Color, last: &mut Option<(Rgb, Hsla)>) -> Hsla {
    match color {
        Color::Spec(rgb) => match *last {
            Some((seen, hsla)) if seen == rgb => hsla,
            _ => {
                let hsla = from_rgb(rgb);
                *last = Some((rgb, hsla));
                hsla
            }
        },
        other => palette.resolve(other),
    }
}

/// Vivid tone of a user-chosen color. Saturated and light enough to read on the dark chrome.
pub fn accent_color(accent: Accent) -> Hsla {
    let hue = match accent {
        Accent::Red => 0.0,
        Accent::Orange => 24.0,
        Accent::Amber => 43.0,
        Accent::Lime => 86.0,
        Accent::Green => 138.0,
        Accent::Teal => 170.0,
        Accent::Cyan => 192.0,
        Accent::Blue => 216.0,
        Accent::Violet => 266.0,
        Accent::Pink => 322.0,
    };
    hsla(hue / 360.0, 0.82, 0.60, 1.0)
}

/// Shadow of a menu, dialog or settings panel. Nothing else in the interface has one: the
/// shadow is painted only while such an overlay is open.
pub fn overlay_shadow(dark: bool) -> Vec<BoxShadow> {
    let (wide, close) = if dark { (0.45, 0.30) } else { (0.15, 0.08) };
    vec![
        BoxShadow::new(px(0.0), px(8.0), hsla(0.0, 0.0, 0.0, wide))
            .blur_radius(px(24.0))
            .spread_radius(px(-4.0)),
        BoxShadow::new(px(0.0), px(2.0), hsla(0.0, 0.0, 0.0, close)).blur_radius(px(6.0)),
    ]
}

/// A lighter shadow for tooltips.
pub fn tooltip_shadow(dark: bool) -> Vec<BoxShadow> {
    let alpha = if dark { 0.30 } else { 0.10 };
    vec![BoxShadow::new(px(0.0), px(2.0), hsla(0.0, 0.0, 0.0, alpha)).blur_radius(px(6.0))]
}

/// The same color at a given opacity, for tinted backgrounds.
pub fn with_alpha(color: Hsla, alpha: f32) -> Hsla {
    Hsla { a: alpha, ..color }
}

pub fn from_rgb(color: Rgb) -> Hsla {
    hex((u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b))
}

/// Converts back for `OSC` color replies.
pub fn to_rgb(color: Hsla) -> Rgb {
    let rgba = Rgba::from(color);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgb { r: channel(rgba.r), g: channel(rgba.g), b: channel(rgba.b) }
}
