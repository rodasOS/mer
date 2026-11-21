//! Searching the screen and scrollback for a piece of text.
//!
//! The text is searched as typed (not as a regular expression); upper and lower case are told
//! apart only when the text has an uppercase letter ("smart case"). Everything here is pure
//! logic over a [`Term`], so it is tested without a window.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Boundary, Column, Direction, Line, Point, Side};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};

/// Escapes what a regular expression would read as syntax, so the text matches itself.
pub fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(
            c,
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$'
        ) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// A search in progress: the text, its compiled form and the match the user is looking at.
#[derive(Default)]
pub struct Search {
    query: String,
    regex: Option<RegexSearch>,
    current: Option<Match>,
}

impl std::fmt::Debug for Search {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Search")
            .field("query", &self.query)
            .field("current", &self.current)
            .finish_non_exhaustive()
    }
}

impl Search {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// Changes the text. The current match is forgotten; an empty text searches for nothing.
    pub fn set_query(&mut self, query: &str) {
        query.clone_into(&mut self.query);
        self.regex = (!query.is_empty()).then(|| RegexSearch::new(&escape(query)).ok()).flatten();
        self.current = None;
    }

    /// Is there something to look for?
    pub fn is_active(&self) -> bool {
        self.regex.is_some()
    }

    /// The match the view last jumped to.
    pub fn current(&self) -> Option<&Match> {
        self.current.as_ref()
    }

    /// Jumps to the next match and scrolls it into view: older (up) or newer (down), wrapping
    /// around at the ends. The first jump after a new text goes to the newest match. Returns
    /// whether there is any match.
    pub fn jump<T: EventListener>(&mut self, term: &mut Term<T>, older: bool) -> bool {
        let Some(regex) = self.regex.as_mut() else { return false };
        let (direction, side) =
            if older { (Direction::Left, Side::Right) } else { (Direction::Right, Side::Left) };
        let origin = match &self.current {
            Some(current) if older => current.start().sub(term, Boundary::None, 1),
            Some(current) => current.end().add(term, Boundary::None, 1),
            // Nothing selected yet: from the very end, going up.
            None => Point::new(term.bottommost_line(), term.last_column()),
        };
        let found = term.search_next(regex, origin, direction, side, None);
        if let Some(found) = &found {
            term.scroll_to_point(*found.start());
        }
        self.current = found;
        self.current.is_some()
    }

    /// The matches on the screen as it is now (scrolled or not). Only the visible lines are
    /// looked at, so this is cheap enough to run on every frame while a search is open.
    pub fn visible<T>(&mut self, term: &Term<T>) -> Vec<Match> {
        let Some(regex) = self.regex.as_mut() else { return Vec::new() };
        let offset = i32::try_from(term.grid().display_offset()).unwrap_or(0);
        let lines = i32::try_from(term.screen_lines()).unwrap_or(0);
        let start = Point::new(Line(-offset), Column(0));
        let end = Point::new(Line(lines - 1 - offset), term.last_column());
        RegexIter::new(start, end, Direction::Right, term, regex).collect()
    }

    /// How many matches there are in the whole history, counting up to `cap`.
    pub fn count<T>(&mut self, term: &Term<T>, cap: usize) -> usize {
        let Some(regex) = self.regex.as_mut() else { return 0 };
        let start = Point::new(term.topmost_line(), Column(0));
        let end = Point::new(term.bottommost_line(), term.last_column());
        RegexIter::new(start, end, Direction::Right, term, regex).take(cap).count()
    }
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

    use super::*;
    use crate::GridSize;

    /// A 6-line terminal that has been fed `lines`, one per row, so most of them are history.
    fn term_with(lines: &[&str]) -> Term<VoidListener> {
        let size = GridSize { cols: 40, lines: 6, cell_width: 8, cell_height: 16 };
        let mut term = Term::new(Config::default(), &size, VoidListener);
        let text = lines.join("\r\n");
        Processor::<StdSyncHandler>::new().advance(&mut term, text.as_bytes());
        term
    }

    /// 30 lines of filler with the word at two of them.
    fn history() -> Term<VoidListener> {
        let lines: Vec<String> = (0..30)
            .map(|n| {
                if n == 3 || n == 20 { format!("row {n}: needle here") } else { format!("row {n}") }
            })
            .collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        term_with(&refs)
    }

    fn line_of(found: &Match) -> i32 {
        found.start().line.0
    }

    #[test]
    fn text_is_searched_as_typed_not_as_a_regex() {
        assert_eq!(escape("a.b*c"), "a\\.b\\*c");
        assert_eq!(escape("(x)|[y]"), "\\(x\\)\\|\\[y\\]");
        let mut term = term_with(&["cost is $5.00 (net)", "cost is 5x00"]);
        let mut search = Search::new();
        search.set_query("5.00");
        assert_eq!(search.count(&term, 100), 1, "the dot is a dot, not any character");
        search.set_query("(net)");
        assert_eq!(search.count(&term, 100), 1);
        assert!(search.jump(&mut term, true));
    }

    #[test]
    fn an_empty_text_finds_nothing() {
        let mut term = history();
        let mut search = Search::new();
        search.set_query("");
        assert!(!search.is_active());
        assert!(!search.jump(&mut term, true));
        assert_eq!(search.count(&term, 100), 0);
        assert!(search.visible(&term).is_empty());
    }

    #[test]
    fn the_whole_history_is_counted_up_to_the_cap() {
        let term = history();
        let mut search = Search::new();
        search.set_query("needle");
        assert_eq!(search.count(&term, 100), 2);
        assert_eq!(search.count(&term, 1), 1, "stops at the cap");
        search.set_query("nothing like this");
        assert_eq!(search.count(&term, 100), 0);
    }

    #[test]
    fn jumping_starts_at_the_newest_match_and_walks_up_then_wraps() {
        let mut term = history();
        let mut search = Search::new();
        search.set_query("needle");
        assert!(search.jump(&mut term, true));
        let newest = line_of(search.current().unwrap());
        assert!(search.jump(&mut term, true));
        let older = line_of(search.current().unwrap());
        assert!(older < newest, "the second jump goes up: {older} < {newest}");
        // Going up again wraps around to the newest.
        assert!(search.jump(&mut term, true));
        assert_eq!(line_of(search.current().unwrap()), newest);
        // And one down from there wraps to the oldest.
        assert!(search.jump(&mut term, false));
        assert_eq!(line_of(search.current().unwrap()), older);
    }

    #[test]
    fn jumping_scrolls_the_match_into_view() {
        let mut term = history();
        let mut search = Search::new();
        search.set_query("needle");
        // The first jump goes to the newest match; the second to the oldest, far in history.
        search.jump(&mut term, true);
        search.jump(&mut term, true);
        let current = search.current().unwrap().clone();
        let visible = search.visible(&term);
        assert!(visible.contains(&current), "the current match is on screen after the jump");
        assert!(term.grid().display_offset() > 0, "the view moved into the history");
    }

    #[test]
    fn smart_case_matches_any_case_unless_the_text_has_a_capital() {
        let term = term_with(&["Error: one", "error: two", "ERROR: three"]);
        let mut search = Search::new();
        search.set_query("error");
        assert_eq!(search.count(&term, 100), 3);
        search.set_query("Error");
        assert_eq!(search.count(&term, 100), 1);
    }

    #[test]
    fn the_visible_matches_are_only_those_on_screen() {
        let term = history();
        let mut search = Search::new();
        search.set_query("needle");
        // The screen shows the last six rows (24..30): neither match is there.
        assert!(search.visible(&term).is_empty());
        assert_eq!(search.count(&term, 100), 2);
    }
}
