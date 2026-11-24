//! Scrollbar geometry: pure arithmetic, no window and no terminal needed.
//!
//! The terminal keeps `display_offset` lines scrolled up out of `history` lines of scrollback.
//! The thumb maps that range onto a track of `track` pixels. Everything here is a function of
//! three integers, so it is cheap to run every frame and easy to test.

/// How far a terminal is scrolled, read in one go from the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScrollState {
    /// Lines scrolled up from the bottom (0 = following the output).
    pub offset: usize,
    /// Lines of scrollback above the visible screen.
    pub history: usize,
    /// Lines visible at once.
    pub screen: usize,
}

impl ScrollState {
    /// There is something above the screen to scroll to.
    pub fn has_history(self) -> bool {
        self.history > 0
    }

    /// The view is not at the bottom.
    pub fn is_scrolled(self) -> bool {
        self.offset > 0
    }

    /// Total scrollable lines: history plus the screen.
    fn total(self) -> usize {
        self.history + self.screen
    }
}

/// The draggable part of the bar, in pixels from the top of the track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thumb {
    pub top: f32,
    pub height: f32,
}

/// Shortest thumb, so it stays easy to grab with a huge scrollback.
pub const MIN_THUMB: f32 = 24.0;

impl Thumb {
    /// The thumb for `state` on a track `track` pixels tall. `None` without history (nothing
    /// to scroll) or without a track.
    #[allow(clippy::cast_precision_loss)] // line counts are far below f32's exact range
    pub fn compute(state: ScrollState, track: f32) -> Option<Self> {
        if !state.has_history() || track <= 0.0 {
            return None;
        }
        let total = state.total() as f32;
        let height = (track * state.screen as f32 / total).clamp(MIN_THUMB.min(track), track);
        let travel = track - height;
        // Offset 0 is the bottom of the track; `history` is the top.
        let from_top = 1.0 - state.offset.min(state.history) as f32 / state.history as f32;
        Some(Self { top: travel * from_top, height })
    }
}

/// The offset that puts the thumb's top at `top` pixels: the inverse of [`Thumb::compute`],
/// used while dragging and when clicking on the track.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn offset_for_thumb_top(state: ScrollState, track: f32, thumb_height: f32, top: f32) -> usize {
    let travel = track - thumb_height;
    if !state.has_history() || travel <= 0.0 {
        return 0;
    }
    let from_top = (top / travel).clamp(0.0, 1.0);
    ((1.0 - from_top) * state.history as f32).round() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRACK: f32 = 400.0;

    fn state(offset: usize, history: usize) -> ScrollState {
        ScrollState { offset, history, screen: 40 }
    }

    #[test]
    fn no_history_means_no_thumb() {
        assert_eq!(Thumb::compute(state(0, 0), TRACK), None);
        assert_eq!(Thumb::compute(state(0, 100), 0.0), None);
    }

    #[test]
    fn at_the_bottom_the_thumb_touches_the_end_of_the_track() {
        let thumb = Thumb::compute(state(0, 360), TRACK).unwrap();
        assert!((thumb.top + thumb.height - TRACK).abs() < 0.01);
    }

    #[test]
    fn at_the_top_the_thumb_touches_the_start_of_the_track() {
        let thumb = Thumb::compute(state(360, 360), TRACK).unwrap();
        assert!(thumb.top.abs() < 0.01);
    }

    #[test]
    fn the_thumb_shrinks_with_more_history_but_never_below_the_minimum() {
        let small = Thumb::compute(state(0, 40), TRACK).unwrap();
        let huge = Thumb::compute(state(0, 1_000_000), TRACK).unwrap();
        assert!(huge.height < small.height);
        assert!((huge.height - MIN_THUMB).abs() < 0.01);
    }

    #[test]
    fn an_offset_beyond_the_history_is_clamped() {
        let thumb = Thumb::compute(state(5_000, 360), TRACK).unwrap();
        assert!(thumb.top.abs() < 0.01);
    }

    #[test]
    fn dragging_round_trips_to_the_same_offset() {
        let history = 2_000;
        for offset in [0, 1, 250, 999, 1_999, 2_000] {
            let s = state(offset, history);
            let thumb = Thumb::compute(s, TRACK).unwrap();
            let back = offset_for_thumb_top(s, TRACK, thumb.height, thumb.top);
            // One pixel of thumb travel is several lines, so allow that much error.
            let slack = history / 300 + 1;
            assert!(back.abs_diff(offset) <= slack, "offset {offset} came back as {back}");
        }
    }

    #[test]
    fn dragging_past_either_end_stops_at_the_end() {
        let s = state(100, 500);
        let thumb = Thumb::compute(s, TRACK).unwrap();
        assert_eq!(offset_for_thumb_top(s, TRACK, thumb.height, -50.0), 500);
        assert_eq!(offset_for_thumb_top(s, TRACK, thumb.height, TRACK + 50.0), 0);
    }

    #[test]
    fn scrolled_means_not_at_the_bottom() {
        assert!(!state(0, 10).is_scrolled());
        assert!(state(1, 10).is_scrolled());
    }
}
