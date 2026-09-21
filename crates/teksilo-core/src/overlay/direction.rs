// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Inline-direction resolution: the one place a reading direction turns into a
//! physical side.
//!
//! Three unrelated-looking mirrors in the touch programme are the same
//! question asked three times — which physical side is the *start* of the
//! line? A context menu that must not open under the finger prefers the
//! inline-start quadrant; a selection handle labelled `Start` sits on the left
//! in English and on the right in Arabic; a swipe that means "go forward"
//! travels left in one and right in the other. Deriving that answer three
//! times is how two of them end up disagreeing, so it is derived once, here.
//! The text-affordance layer reads it today; the placement code and the
//! swipe/auto-scroll consumers are still to be moved onto it.
//!
//! What is deliberately **not** here: the pan axes. [`TouchAction::PAN_X`] and
//! [`TouchAction::PAN_Y`] are axis-relative and stay that way — "this subtree
//! may pan horizontally" is a statement about the x axis, not about reading
//! order, and mirroring it under RTL would silently forbid the very gesture the
//! author permitted. [`InlineDirection::from_swipe`] answering `None` for a
//! vertical swipe is the same rule seen from the other end: only the inline
//! axis resolves through a direction, the block axis never does.
//!
//! [`TouchAction::PAN_X`]: crate::pointer::touch_action::TouchAction::PAN_X
//! [`TouchAction::PAN_Y`]: crate::pointer::touch_action::TouchAction::PAN_Y

use teksilo_canvas::{Point, Rect};

use crate::environment::LayoutDirection;
use crate::gesture::SwipeDirection;

/// A physical horizontal side, after a [`LayoutDirection`] has been applied.
///
/// The vocabulary the *painter* needs. `leading` / `trailing` describe reading
/// order and are what an author writes; by the time geometry is computed the
/// question is only ever "left or right", and saying so outright is what stops
/// a second mirror being applied to an already-mirrored value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HorizontalSide {
    /// Physically left, in window coordinates.
    Left,
    /// Physically right, in window coordinates.
    Right,
}

impl HorizontalSide {
    /// The other side.
    pub const fn opposite(self) -> Self {
        match self {
            HorizontalSide::Left => HorizontalSide::Right,
            HorizontalSide::Right => HorizontalSide::Left,
        }
    }
}

/// A direction along the inline (reading) axis, independent of any script.
///
/// `Forward` is the direction text advances in: rightwards under
/// [`LeftToRight`](LayoutDirection::LeftToRight), leftwards under
/// [`RightToLeft`](LayoutDirection::RightToLeft). `Backward` is its opposite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InlineDirection {
    /// The direction text advances in — inline-end.
    Forward,
    /// Against the direction text advances in — inline-start.
    Backward,
}

impl InlineDirection {
    /// The other direction.
    pub const fn opposite(self) -> Self {
        match self {
            InlineDirection::Forward => InlineDirection::Backward,
            InlineDirection::Backward => InlineDirection::Forward,
        }
    }

    /// Which physical side of a rectangle this direction points at.
    pub const fn side(self, direction: LayoutDirection) -> HorizontalSide {
        match (self, direction) {
            (InlineDirection::Forward, LayoutDirection::LeftToRight)
            | (InlineDirection::Backward, LayoutDirection::RightToLeft) => HorizontalSide::Right,
            (InlineDirection::Backward, LayoutDirection::LeftToRight)
            | (InlineDirection::Forward, LayoutDirection::RightToLeft) => HorizontalSide::Left,
        }
    }

    /// The physical swipe that means this inline direction.
    ///
    /// The inverse of [`from_swipe`](Self::from_swipe): a "swipe forward to
    /// advance" binding turns into `Right` for English and `Left` for Arabic.
    pub const fn to_swipe(self, direction: LayoutDirection) -> SwipeDirection {
        match self.side(direction) {
            HorizontalSide::Left => SwipeDirection::Left,
            HorizontalSide::Right => SwipeDirection::Right,
        }
    }

    /// What a physical swipe means in reading order, or `None` for a vertical
    /// one.
    ///
    /// Vertical swipes are on the block axis and have no inline meaning; a
    /// caller that wants "swipe up" gets it from [`SwipeDirection`] unchanged.
    /// See the module docs for why the pan axes stay axis-relative for the
    /// same reason.
    pub const fn from_swipe(swipe: SwipeDirection, direction: LayoutDirection) -> Option<Self> {
        let side = match swipe {
            SwipeDirection::Left => HorizontalSide::Left,
            SwipeDirection::Right => HorizontalSide::Right,
            SwipeDirection::Up | SwipeDirection::Down => return None,
        };
        Some(match (side, direction) {
            (HorizontalSide::Right, LayoutDirection::LeftToRight)
            | (HorizontalSide::Left, LayoutDirection::RightToLeft) => InlineDirection::Forward,
            (HorizontalSide::Left, LayoutDirection::LeftToRight)
            | (HorizontalSide::Right, LayoutDirection::RightToLeft) => InlineDirection::Backward,
        })
    }
}

/// The inline edge strip of `area`, `band` logical pixels deep.
///
/// The geometry behind a drag-and-drop auto-scroll band: hovering a drag in the
/// backward strip scrolls back, the forward strip scrolls on. Derived through
/// [`InlineDirection::side`] so the two strips swap under RTL without the call
/// site knowing a mirror happened — which is the bug this replaces, where every
/// auto-scrolling view hardcoded "left edge scrolls left".
///
/// A `band` wider than half of `area` is clamped to half, so the two strips
/// never overlap and a point can only ever be in one of them.
pub fn inline_edge_band(
    area: Rect,
    band: f32,
    edge: InlineDirection,
    direction: LayoutDirection,
) -> Rect {
    let band = band.clamp(0.0, area.width / 2.0);
    match edge.side(direction) {
        HorizontalSide::Left => Rect::new(area.x, area.y, band, area.height),
        HorizontalSide::Right => Rect::new(area.right() - band, area.y, band, area.height),
    }
}

/// Which auto-scroll band `point` is in, if any.
///
/// `Backward` wins a tie, which can only happen when `area` is at most `2 ×
/// band` wide and the two strips meet: a view too narrow to have a middle
/// scrolls back rather than doing nothing.
pub fn inline_band_at(
    area: Rect,
    band: f32,
    point: Point,
    direction: LayoutDirection,
) -> Option<InlineDirection> {
    [InlineDirection::Backward, InlineDirection::Forward]
        .into_iter()
        .find(|&edge| inline_edge_band(area, band, edge, direction).contains(point))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LTR: LayoutDirection = LayoutDirection::LeftToRight;
    const RTL: LayoutDirection = LayoutDirection::RightToLeft;

    #[test]
    fn inline_directions_mirror_between_scripts() {
        assert_eq!(InlineDirection::Forward.side(LTR), HorizontalSide::Right);
        assert_eq!(InlineDirection::Forward.side(RTL), HorizontalSide::Left);
        assert_eq!(InlineDirection::Backward.side(LTR), HorizontalSide::Left);
        assert_eq!(InlineDirection::Backward.side(RTL), HorizontalSide::Right);
        assert_eq!(HorizontalSide::Left.opposite(), HorizontalSide::Right);
        assert_eq!(
            InlineDirection::Forward.opposite(),
            InlineDirection::Backward
        );
    }

    /// The same physical swipe means opposite things in the two scripts, and
    /// the round trip through [`InlineDirection`] is lossless on the inline
    /// axis.
    #[test]
    fn a_swipe_resolves_through_the_layout_direction() {
        assert_eq!(
            InlineDirection::from_swipe(SwipeDirection::Right, LTR),
            Some(InlineDirection::Forward)
        );
        assert_eq!(
            InlineDirection::from_swipe(SwipeDirection::Right, RTL),
            Some(InlineDirection::Backward)
        );
        assert_eq!(
            InlineDirection::from_swipe(SwipeDirection::Left, LTR),
            Some(InlineDirection::Backward)
        );
        assert_eq!(
            InlineDirection::from_swipe(SwipeDirection::Left, RTL),
            Some(InlineDirection::Forward)
        );
        for direction in [LTR, RTL] {
            for inline in [InlineDirection::Forward, InlineDirection::Backward] {
                assert_eq!(
                    InlineDirection::from_swipe(inline.to_swipe(direction), direction),
                    Some(inline),
                    "{inline:?} under {direction:?} must survive the round trip"
                );
            }
        }
    }

    /// Only the inline axis resolves through a direction. A vertical swipe has
    /// no forward/backward reading, which is the same rule that keeps
    /// `TouchAction::PAN_X` axis-relative — see the module docs.
    #[test]
    fn the_block_axis_has_no_inline_meaning() {
        for direction in [LTR, RTL] {
            assert_eq!(
                InlineDirection::from_swipe(SwipeDirection::Up, direction),
                None
            );
            assert_eq!(
                InlineDirection::from_swipe(SwipeDirection::Down, direction),
                None
            );
        }
    }

    #[test]
    fn auto_scroll_bands_mirror() {
        let area = Rect::new(0.0, 0.0, 400.0, 100.0);
        let back_ltr = inline_edge_band(area, 40.0, InlineDirection::Backward, LTR);
        let back_rtl = inline_edge_band(area, 40.0, InlineDirection::Backward, RTL);
        assert_eq!((back_ltr.x, back_ltr.width), (0.0, 40.0));
        assert_eq!((back_rtl.x, back_rtl.width), (360.0, 40.0));

        // A drag hovering the physical left edge scrolls *back* in English and
        // *forward* in Arabic.
        let left = Point::new(10.0, 50.0);
        assert_eq!(
            inline_band_at(area, 40.0, left, LTR),
            Some(InlineDirection::Backward)
        );
        assert_eq!(
            inline_band_at(area, 40.0, left, RTL),
            Some(InlineDirection::Forward)
        );
        assert_eq!(
            inline_band_at(area, 40.0, Point::new(200.0, 50.0), LTR),
            None,
            "the middle of a wide view is in neither band"
        );
    }

    /// A band wider than half the view would make both strips claim the middle;
    /// clamping keeps them disjoint so a point is only ever in one.
    #[test]
    fn an_oversized_band_is_clamped_to_half_the_view() {
        let area = Rect::new(0.0, 0.0, 100.0, 20.0);
        let back = inline_edge_band(area, 400.0, InlineDirection::Backward, LTR);
        let fore = inline_edge_band(area, 400.0, InlineDirection::Forward, LTR);
        assert_eq!(back.width, 50.0);
        assert_eq!(fore.x, 50.0);
    }
}
