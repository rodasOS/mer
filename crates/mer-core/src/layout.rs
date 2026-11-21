use serde::{Deserialize, Serialize};

use crate::PaneId;

/// How a split arranges its two children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    /// Children sit side by side (split to the right).
    Row,
    /// Children are stacked (split downwards).
    Column,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    fn axis(self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::Row,
            Self::Up | Self::Down => Axis::Column,
        }
    }

    fn sign(self) -> f32 {
        match self {
            Self::Left | Self::Up => -1.0,
            Self::Right | Self::Down => 1.0,
        }
    }
}

const MIN_RATIO: f32 = 0.1;
const MAX_RATIO: f32 = 0.9;

/// Binary tree of splits. Leaves are panes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Layout {
    Leaf(PaneId),
    Split { axis: Axis, ratio: f32, first: Box<Layout>, second: Box<Layout> },
}

#[derive(Debug, Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Layout {
    /// Panes in visual order (left to right, top to bottom).
    pub fn panes(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<PaneId>) {
        match self {
            Self::Leaf(id) => out.push(*id),
            Self::Split { first, second, .. } => {
                first.collect(out);
                second.collect(out);
            }
        }
    }

    pub fn contains(&self, pane: PaneId) -> bool {
        match self {
            Self::Leaf(id) => *id == pane,
            Self::Split { first, second, .. } => first.contains(pane) || second.contains(pane),
        }
    }

    /// Splits `target` in two, putting `new` second. Returns `false` if `target` is not here.
    pub fn split(&mut self, target: PaneId, axis: Axis, new: PaneId) -> bool {
        match self {
            Self::Leaf(id) if *id == target => {
                *self = Self::Split {
                    axis,
                    ratio: 0.5,
                    first: Box::new(Self::Leaf(target)),
                    second: Box::new(Self::Leaf(new)),
                };
                true
            }
            Self::Leaf(_) => false,
            Self::Split { first, second, .. } => {
                first.split(target, axis, new) || second.split(target, axis, new)
            }
        }
    }

    /// Removes `target` and collapses its parent split. `None` if nothing is left.
    pub fn without(self, target: PaneId) -> Option<Self> {
        match self {
            Self::Leaf(id) => (id != target).then_some(Self::Leaf(id)),
            Self::Split { axis, ratio, first, second } => {
                match (first.without(target), second.without(target)) {
                    (None, None) => None,
                    (Some(only), None) | (None, Some(only)) => Some(only),
                    (Some(first), Some(second)) => Some(Self::Split {
                        axis,
                        ratio,
                        first: Box::new(first),
                        second: Box::new(second),
                    }),
                }
            }
        }
    }

    /// Moves the divider of the closest `axis` split that contains `target`.
    /// `delta` is a fraction of that split; positive grows the first child.
    pub fn adjust_ratio(&mut self, target: PaneId, axis: Axis, delta: f32) -> bool {
        let Self::Split { axis: own_axis, ratio, first, second } = self else {
            return false;
        };
        // Prefer the deepest matching split.
        if first.adjust_ratio(target, axis, delta) || second.adjust_ratio(target, axis, delta) {
            return true;
        }
        if *own_axis == axis && self_contains(first, second, target) {
            *ratio = (*ratio + delta).clamp(MIN_RATIO, MAX_RATIO);
            return true;
        }
        false
    }

    /// The pane next to `from` in `direction`, based on where panes are drawn.
    pub fn neighbor(&self, from: PaneId, direction: Direction) -> Option<PaneId> {
        let mut rects = Vec::new();
        self.rects(Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }, &mut rects);
        let source = rects.iter().find(|(id, _)| *id == from)?.1;
        let horizontal = direction.axis() == Axis::Row;

        rects
            .iter()
            .filter(|(id, _)| *id != from)
            .filter_map(|(id, rect)| {
                let (gap, overlap) = if horizontal {
                    let gap = if direction.sign() > 0.0 {
                        rect.x - (source.x + source.w)
                    } else {
                        source.x - (rect.x + rect.w)
                    };
                    (gap, overlap(source.y, source.h, rect.y, rect.h))
                } else {
                    let gap = if direction.sign() > 0.0 {
                        rect.y - (source.y + source.h)
                    } else {
                        source.y - (rect.y + rect.h)
                    };
                    (gap, overlap(source.x, source.w, rect.x, rect.w))
                };
                (gap > -1e-4 && overlap > 1e-4).then_some((*id, gap, overlap))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1).then(b.2.total_cmp(&a.2)))
            .map(|(id, ..)| id)
    }

    fn rects(&self, rect: Rect, out: &mut Vec<(PaneId, Rect)>) {
        match self {
            Self::Leaf(id) => out.push((*id, rect)),
            Self::Split { axis, ratio, first, second } => match axis {
                Axis::Row => {
                    let w = rect.w * ratio;
                    first.rects(Rect { w, ..rect }, out);
                    second.rects(Rect { x: rect.x + w, w: rect.w - w, ..rect }, out);
                }
                Axis::Column => {
                    let h = rect.h * ratio;
                    first.rects(Rect { h, ..rect }, out);
                    second.rects(Rect { y: rect.y + h, h: rect.h - h, ..rect }, out);
                }
            },
        }
    }
}

fn self_contains(first: &Layout, second: &Layout, target: PaneId) -> bool {
    first.contains(target) || second.contains(target)
}

fn overlap(a_start: f32, a_len: f32, b_start: f32, b_len: f32) -> f32 {
    (a_start + a_len).min(b_start + b_len) - a_start.max(b_start)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn three_panes() -> (Layout, PaneId, PaneId, PaneId) {
        // [ a | b over c ]
        let (a, b, c) = (PaneId::new(), PaneId::new(), PaneId::new());
        let mut layout = Layout::Leaf(a);
        assert!(layout.split(a, Axis::Row, b));
        assert!(layout.split(b, Axis::Column, c));
        (layout, a, b, c)
    }

    #[test]
    fn split_keeps_visual_order() {
        let (layout, a, b, c) = three_panes();
        assert_eq!(layout.panes(), vec![a, b, c]);
    }

    #[test]
    fn split_of_unknown_pane_does_nothing() {
        let mut layout = Layout::Leaf(PaneId::new());
        assert!(!layout.split(PaneId::new(), Axis::Row, PaneId::new()));
    }

    #[test]
    fn removing_a_pane_collapses_its_parent() {
        let (layout, a, b, c) = three_panes();
        let layout = layout.without(b).expect("two panes remain");
        assert_eq!(layout.panes(), vec![a, c]);
        let layout = layout.without(a).expect("one pane remains");
        assert_eq!(layout, Layout::Leaf(c));
        assert_eq!(layout.without(c), None);
    }

    #[test]
    fn neighbors_follow_the_drawn_geometry() {
        let (layout, a, b, c) = three_panes();
        assert_eq!(layout.neighbor(a, Direction::Right), Some(b));
        assert_eq!(layout.neighbor(c, Direction::Left), Some(a));
        assert_eq!(layout.neighbor(b, Direction::Down), Some(c));
        assert_eq!(layout.neighbor(c, Direction::Up), Some(b));
        assert_eq!(layout.neighbor(a, Direction::Left), None);
        assert_eq!(layout.neighbor(a, Direction::Down), None);
    }

    #[test]
    fn ratio_changes_the_closest_split_of_that_axis_and_is_clamped() {
        let (mut layout, _a, b, c) = three_panes();
        assert!(layout.adjust_ratio(c, Axis::Column, 0.2));
        let Layout::Split { second, .. } = &layout else { panic!("root is a split") };
        let Layout::Split { ratio, .. } = second.as_ref() else { panic!("inner is a split") };
        assert!((ratio - 0.7).abs() < 1e-6);

        assert!(layout.adjust_ratio(b, Axis::Column, 5.0));
        let Layout::Split { second, .. } = &layout else { panic!("root is a split") };
        let Layout::Split { ratio, .. } = second.as_ref() else { panic!("inner is a split") };
        assert!((ratio - MAX_RATIO).abs() < 1e-6);
    }

    #[test]
    fn ratio_of_missing_axis_is_not_changed() {
        let (mut layout, a, ..) = three_panes();
        assert!(!layout.adjust_ratio(a, Axis::Column, 0.1));
    }
}
