//! Serializes the terminal grid (scrollback plus screen) to ANSI text.
//!
//! The output keeps colors and text attributes so a sleeping space can show what
//! was on screen when it wakes up. It is plain text, so it compresses well.

use std::fmt::Write as _;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::vte::ansi::{Color, NamedColor};

/// Line shown between restored history and the new shell.
pub const RESTORED_MARK: &str = "\x1b[2m─── session restored ───\x1b[0m\r\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Style {
    fg: Color,
    bg: Color,
    flags: Flags,
}

const VISIBLE_FLAGS: Flags = Flags::BOLD
    .union(Flags::DIM)
    .union(Flags::ITALIC)
    .union(Flags::UNDERLINE)
    .union(Flags::INVERSE)
    .union(Flags::HIDDEN)
    .union(Flags::STRIKEOUT);

impl Style {
    fn of(cell: &Cell) -> Self {
        Self { fg: cell.fg, bg: cell.bg, flags: cell.flags & VISIBLE_FLAGS }
    }

    fn is_default(self) -> bool {
        self.fg == Color::Named(NamedColor::Foreground)
            && self.bg == Color::Named(NamedColor::Background)
            && self.flags.is_empty()
    }

    fn write_sgr(self, out: &mut String) {
        out.push_str("\x1b[0");
        for (flag, code) in [
            (Flags::BOLD, 1),
            (Flags::DIM, 2),
            (Flags::ITALIC, 3),
            (Flags::UNDERLINE, 4),
            (Flags::INVERSE, 7),
            (Flags::HIDDEN, 8),
            (Flags::STRIKEOUT, 9),
        ] {
            if self.flags.contains(flag) {
                let _ = write!(out, ";{code}");
            }
        }
        write_color(out, self.fg, 30, 38);
        write_color(out, self.bg, 40, 48);
        out.push('m');
    }
}

fn write_color(out: &mut String, color: Color, base: u8, extended: u8) {
    match color {
        Color::Named(name) => {
            let index = name as usize;
            if index < 8 {
                let _ = write!(out, ";{}", usize::from(base) + index);
            } else if index < 16 {
                let _ = write!(out, ";{}", usize::from(base) + 60 + index - 8);
            }
            // Foreground, background and cursor mean "default": nothing to emit.
        }
        Color::Indexed(index) => {
            let _ = write!(out, ";{extended};5;{index}");
        }
        Color::Spec(rgb) => {
            let _ = write!(out, ";{extended};2;{};{};{}", rgb.r, rgb.g, rgb.b);
        }
    }
}

fn is_blank(cell: &Cell) -> bool {
    (cell.c == ' ' || cell.c == '\0') && Style::of(cell).is_default() && cell.zerowidth().is_none()
}

/// Everything in the grid, top of scrollback to the last non-empty line.
pub fn snapshot_ansi<T: EventListener>(term: &Term<T>) -> String {
    let grid = term.grid();
    let columns = grid.columns();
    let top = grid.topmost_line().0;
    let bottom = grid.bottommost_line().0;

    let last_used = (top..=bottom)
        .rev()
        .find(|&line| (0..columns).any(|col| !is_blank(&grid[Line(line)][Column(col)])));
    let Some(last_used) = last_used else {
        return String::new();
    };

    // About half of each line is text once trailing blanks are trimmed.
    let rows = usize::try_from(last_used - top + 1).unwrap_or(0);
    let mut out = String::with_capacity(rows * (columns / 2 + 1));
    for line in top..=last_used {
        let row = &grid[Line(line)];
        let used =
            (0..columns).rev().find(|&col| !is_blank(&row[Column(col)])).map_or(0, |c| c + 1);

        let mut current = Style {
            fg: Color::Named(NamedColor::Foreground),
            bg: Color::Named(NamedColor::Background),
            flags: Flags::empty(),
        };
        for col in 0..used {
            let cell = &row[Column(col)];
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let style = Style::of(cell);
            if style != current {
                style.write_sgr(&mut out);
                current = style;
            }
            out.push(if cell.c == '\0' { ' ' } else { cell.c });
            for extra in cell.zerowidth().into_iter().flatten() {
                out.push(*extra);
            }
        }
        if !current.is_default() {
            out.push_str("\x1b[0m");
        }

        let wrapped = columns > 0 && row[Column(columns - 1)].flags.contains(Flags::WRAPLINE);
        if !wrapped {
            out.push_str("\r\n");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

    use super::*;
    use crate::GridSize;

    fn term_with(input: &[u8], cols: u16) -> Term<VoidListener> {
        let size = GridSize { cols, lines: 6, cell_width: 8, cell_height: 16 };
        let mut term = Term::new(Config::default(), &size, VoidListener);
        Processor::<StdSyncHandler>::new().advance(&mut term, input);
        term
    }

    fn text_of(term: &Term<VoidListener>) -> Vec<String> {
        let grid = term.grid();
        (grid.topmost_line().0..=grid.bottommost_line().0)
            .map(|line| {
                (0..grid.columns())
                    .map(|col| grid[Line(line)][Column(col)].c)
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn empty_terminal_has_empty_snapshot() {
        assert_eq!(snapshot_ansi(&term_with(b"", 20)), "");
    }

    #[test]
    fn text_and_colors_survive_a_round_trip() {
        let original =
            term_with(b"hello \x1b[31mred\x1b[0m \x1b[1;38;2;1;2;3mbold\x1b[0m\r\nline2", 30);
        let ansi = snapshot_ansi(&original);
        let restored = term_with(ansi.as_bytes(), 30);

        assert_eq!(text_of(&restored)[..2], ["hello red bold", "line2"]);
        let cell = |term: &Term<VoidListener>, col| term.grid()[Line(0)][Column(col)].clone();
        assert_eq!(cell(&restored, 6).fg, Color::Named(NamedColor::Red));
        assert_eq!(cell(&restored, 0).fg, Color::Named(NamedColor::Foreground));
        let bold = cell(&restored, 10);
        assert!(bold.flags.contains(Flags::BOLD));
        assert_eq!(bold.fg, Color::Spec(alacritty_terminal::vte::ansi::Rgb { r: 1, g: 2, b: 3 }));
    }

    #[test]
    fn trailing_blank_lines_and_spaces_are_dropped() {
        let ansi = snapshot_ansi(&term_with(b"a   \r\n\r\n\r\n", 20));
        assert_eq!(ansi, "a\r\n");
    }

    #[test]
    fn scrollback_is_included() {
        let mut input = Vec::new();
        for n in 0..20 {
            input.extend_from_slice(format!("row {n}\r\n").as_bytes());
        }
        let ansi = snapshot_ansi(&term_with(&input, 20));
        assert!(ansi.starts_with("row 0\r\n"));
        assert!(ansi.contains("row 19\r\n"));
    }

    #[test]
    fn wrapped_lines_are_not_split() {
        let ansi = snapshot_ansi(&term_with(b"abcdefghijklmnopqrstuvwxyz", 10));
        assert_eq!(ansi, "abcdefghijklmnopqrstuvwxyz\r\n");
    }
}
