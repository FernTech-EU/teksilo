// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The area an overlay is allowed to occupy.
//!
//! Placement used to be handed a bare `(width, height)`, which says the window
//! is a rectangle starting at the origin and usable to its last pixel. On a
//! phone that is false at the top (a notch), at the bottom (a home indicator),
//! at the corners (a radius that clips), and — for as long as a keyboard is up
//! — over a whole band of the screen. Every one of those is the same shape of
//! error: an overlay positioned against the window rather than against the part
//! of it a person can see and touch.
//!
//! [`OverlayViewport`] carries all three facts, and
//! [`usable`](OverlayViewport::usable) reduces them to the one rectangle the
//! placement code clamps into. Its default — the whole window, no insets, no
//! occlusion — is exactly the old `(width, height)`, which is why
//! `From<(f32, f32)>` exists and why a desktop tree's placement is unchanged to
//! the last pixel.

use teksilo_canvas::{EdgeInsets, Rect, Size};

use crate::environment::LayoutDirection;

/// The window area available to overlays: its size, the platform's safe-area
/// insets, and whatever is currently covering part of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverlayViewport {
    /// The window's logical size. The full extent, insets included.
    pub size: Size,
    /// Platform-reported insets that content must stay clear of — a notch, a
    /// rounded corner, a home indicator. RTL-aware: `leading` is the left edge
    /// under [`LeftToRight`](LayoutDirection::LeftToRight) and the right edge
    /// under [`RightToLeft`](LayoutDirection::RightToLeft), so a device whose
    /// sensor housing sits at the start of the line does not need the caller
    /// to mirror it.
    pub safe_area: EdgeInsets,
    /// A rectangle of the window currently covered by something outside the
    /// tree — a soft keyboard, a platform IME candidate window. Placement
    /// treats the largest rectangle left over as the usable area, so an overlay
    /// lands above a keyboard rather than behind it. `None` when nothing is
    /// covering the window, which is every desktop frame.
    pub occluded: Option<Rect>,
}

impl OverlayViewport {
    /// A viewport that is the whole window: no insets, nothing covering it.
    pub fn new(size: Size) -> Self {
        Self {
            size,
            safe_area: EdgeInsets::ZERO,
            occluded: None,
        }
    }

    /// This viewport with `safe_area` applied.
    pub fn with_safe_area(mut self, safe_area: EdgeInsets) -> Self {
        self.safe_area = safe_area;
        self
    }

    /// This viewport with `occluded` covering part of it.
    pub fn with_occluded(mut self, occluded: Option<Rect>) -> Self {
        self.occluded = occluded;
        self
    }

    /// The whole window, insets and occlusion ignored.
    ///
    /// What a modal scrim uses: a scrim that respected the safe area would
    /// leave the notch undimmed and the content behind it legible, which is
    /// the one thing a scrim exists to prevent.
    pub fn full(&self) -> Rect {
        Rect::from_origin_size(teksilo_canvas::Point::ZERO, self.size)
    }

    /// The rectangle an overlay may occupy: the window, less the safe-area
    /// insets, less anything covering it.
    ///
    /// Occlusion is resolved by keeping the **largest** rectangle the covered
    /// area leaves behind rather than by insetting a named edge. A soft
    /// keyboard occupies the bottom, so the answer is the band above it; a
    /// candidate window docked to a side gives the band beside it; and neither
    /// case needs the platform to say which edge it came from — which it often
    /// cannot, because it reports a rectangle.
    ///
    /// An occlusion that covers everything leaves the safe rect alone: nowhere
    /// is better than anywhere, and collapsing the area to zero would send
    /// every overlay to the origin.
    pub fn usable(&self, direction: LayoutDirection) -> Rect {
        let rtl = matches!(direction, LayoutDirection::RightToLeft);
        let (left, right) = if rtl {
            (self.safe_area.trailing, self.safe_area.leading)
        } else {
            (self.safe_area.leading, self.safe_area.trailing)
        };
        let safe = Rect::new(
            left,
            self.safe_area.top,
            (self.size.width - left - right).max(0.0),
            (self.size.height - self.safe_area.top - self.safe_area.bottom).max(0.0),
        );
        match self.occluded {
            Some(occluded) => largest_free_slab(safe, occluded),
            None => safe,
        }
    }
}

impl Default for OverlayViewport {
    fn default() -> Self {
        Self::new(Size::ZERO)
    }
}

impl From<(f32, f32)> for OverlayViewport {
    fn from((width, height): (f32, f32)) -> Self {
        Self::new(Size::new(width, height))
    }
}

impl From<Size> for OverlayViewport {
    fn from(size: Size) -> Self {
        Self::new(size)
    }
}

/// The biggest axis-aligned rectangle of `area` that `blocked` does not cover.
///
/// Only the four edge slabs are considered — the region left over is in general
/// an L, and no single rectangle covers an L, so the choice is which arm to
/// keep. Largest area is the right tie-break for the case this exists for: a
/// keyboard spanning the full width leaves one arm of real size (the band
/// above) and three of zero.
fn largest_free_slab(area: Rect, blocked: Rect) -> Rect {
    let overlap_w = (area.right().min(blocked.right()) - area.x.max(blocked.x)).max(0.0);
    let overlap_h = (area.bottom().min(blocked.bottom()) - area.y.max(blocked.y)).max(0.0);
    if overlap_w <= 0.0 || overlap_h <= 0.0 {
        return area;
    }
    let above = Rect::new(area.x, area.y, area.width, (blocked.y - area.y).max(0.0));
    let below = Rect::new(
        area.x,
        blocked.bottom().max(area.y),
        area.width,
        (area.bottom() - blocked.bottom()).max(0.0),
    );
    let leading = Rect::new(area.x, area.y, (blocked.x - area.x).max(0.0), area.height);
    let trailing = Rect::new(
        blocked.right().max(area.x),
        area.y,
        (area.right() - blocked.right()).max(0.0),
        area.height,
    );
    let best = [above, below, leading, trailing]
        .into_iter()
        .max_by(|a, b| {
            (a.width * a.height)
                .partial_cmp(&(b.width * b.height))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(area);
    if best.width <= 0.0 || best.height <= 0.0 {
        area
    } else {
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LTR: LayoutDirection = LayoutDirection::LeftToRight;
    const RTL: LayoutDirection = LayoutDirection::RightToLeft;

    /// The plain `(w, h)` a desktop window supplies must reduce to exactly the
    /// old behaviour — the whole window, both directions.
    #[test]
    fn a_bare_size_is_the_whole_window() {
        let viewport = OverlayViewport::from((800.0, 600.0));
        for direction in [LTR, RTL] {
            let usable = viewport.usable(direction);
            assert_eq!((usable.x, usable.y), (0.0, 0.0));
            assert_eq!((usable.width, usable.height), (800.0, 600.0));
        }
        assert_eq!(viewport.full(), Rect::new(0.0, 0.0, 800.0, 600.0));
    }

    #[test]
    fn safe_area_insets_mirror_with_the_layout_direction() {
        let viewport = OverlayViewport::from((800.0, 600.0))
            .with_safe_area(EdgeInsets::new(44.0, 0.0, 34.0, 20.0));
        let ltr = viewport.usable(LTR);
        assert_eq!((ltr.x, ltr.y), (20.0, 44.0));
        assert_eq!((ltr.width, ltr.height), (780.0, 522.0));

        // The same leading inset is on the right in Arabic.
        let rtl = viewport.usable(RTL);
        assert_eq!((rtl.x, rtl.y), (0.0, 44.0));
        assert_eq!((rtl.width, rtl.height), (780.0, 522.0));
        assert_eq!(rtl.right(), 780.0);
    }

    #[test]
    fn a_bottom_occlusion_leaves_the_band_above_it() {
        let viewport = OverlayViewport::from((800.0, 600.0))
            .with_occluded(Some(Rect::new(0.0, 380.0, 800.0, 220.0)));
        let usable = viewport.usable(LTR);
        assert_eq!((usable.x, usable.y), (0.0, 0.0));
        assert_eq!((usable.width, usable.height), (800.0, 380.0));
    }

    #[test]
    fn a_side_occlusion_leaves_the_band_beside_it() {
        let viewport = OverlayViewport::from((800.0, 600.0))
            .with_occluded(Some(Rect::new(500.0, 0.0, 300.0, 600.0)));
        let usable = viewport.usable(LTR);
        assert_eq!((usable.x, usable.width), (0.0, 500.0));
    }

    #[test]
    fn an_occlusion_that_misses_the_window_changes_nothing() {
        let viewport = OverlayViewport::from((800.0, 600.0))
            .with_occluded(Some(Rect::new(900.0, 0.0, 100.0, 600.0)));
        assert_eq!(viewport.usable(LTR), Rect::new(0.0, 0.0, 800.0, 600.0));
    }

    /// Total occlusion is not a reason to collapse every overlay onto the
    /// origin — there is nowhere better, so the safe rect stands.
    #[test]
    fn a_total_occlusion_falls_back_to_the_safe_rect() {
        let viewport = OverlayViewport::from((800.0, 600.0))
            .with_occluded(Some(Rect::new(-10.0, -10.0, 900.0, 700.0)));
        assert_eq!(viewport.usable(LTR), Rect::new(0.0, 0.0, 800.0, 600.0));
    }
}
