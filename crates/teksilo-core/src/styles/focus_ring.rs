// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Focus-ring stroke geometry shared by every painter that reads
//! [`ShapeTokens::focus_ring_offset`].
//!
//! A ring is a stroke centred on a rounded rectangle at a signed distance
//! from the control edge. The distance is negative when a theme draws the
//! ring partly or wholly inside the control (a negative
//! `focus_ring_offset`), so the arithmetic here floors the two quantities
//! that would otherwise invert: the rectangle's size, which stays centred
//! on the control and never drops below zero, and the corner radius, which
//! never drops below zero either.

use teksilo_canvas::Rect;
use teksilo_tokens::ShapeTokens;

/// The rounded rectangle a focus-ring stroke is centred on.
///
/// Pass [`rect`](Self::rect) and [`corner_radius`](Self::corner_radius) to
/// `Canvas::stroke_rounded_rect` with the ring's width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FocusRingStroke {
    /// The stroke's centre line. Its width and height are never negative.
    pub rect: Rect,
    /// Corner radius of the centre line. Never negative.
    pub corner_radius: f32,
}

impl FocusRingStroke {
    /// The centre line `distance` outside `control`, whose corners have
    /// radius `control_radius`. A negative `distance` lies inside the
    /// control.
    ///
    /// The rectangle grows (or, for a negative distance, shrinks) by
    /// `distance` on every side and keeps the control's centre; a shrink
    /// past the control's half-size stops at zero rather than inverting.
    /// The radius follows the edge at the same distance, floored at zero.
    pub fn at_distance(control: Rect, control_radius: f32, distance: f32) -> Self {
        let width = (control.width + distance * 2.0).max(0.0);
        let height = (control.height + distance * 2.0).max(0.0);
        let cx = control.x + control.width * 0.5;
        let cy = control.y + control.height * 0.5;
        Self {
            rect: Rect::new(cx - width * 0.5, cy - height * 0.5, width, height),
            corner_radius: (control_radius + distance).max(0.0),
        }
    }

    /// The theme's ring around `control`: centred
    /// [`ShapeTokens::focus_ring_stroke_distance`] outside it, so its inner
    /// edge sits `focus_ring_offset` from the control edge.
    pub fn around(control: Rect, control_radius: f32, shape: &ShapeTokens) -> Self {
        Self::at_distance(control, control_radius, shape.focus_ring_stroke_distance())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape_with_offset(offset: f32) -> ShapeTokens {
        ShapeTokens {
            focus_ring_offset: offset,
            focus_ring_width: 2.0,
            ..ShapeTokens::default()
        }
    }

    #[test]
    fn a_positive_offset_grows_the_ring_and_its_radius() {
        let control = Rect::new(10.0, 10.0, 40.0, 20.0);
        let ring = FocusRingStroke::around(control, 4.0, &shape_with_offset(2.0));
        assert_eq!(ring.rect, Rect::new(7.0, 7.0, 46.0, 26.0));
        assert_eq!(ring.corner_radius, 7.0);
    }

    #[test]
    fn a_negative_offset_straddles_the_edge_and_is_smaller_than_an_outer_ring() {
        let control = Rect::new(10.0, 10.0, 40.0, 20.0);
        let inner = FocusRingStroke::around(control, 4.0, &shape_with_offset(-1.0));
        let outer = FocusRingStroke::around(control, 4.0, &shape_with_offset(2.0));
        // Offset -1, width 2: centred on the control edge.
        assert_eq!(inner.rect, control);
        assert_eq!(inner.corner_radius, 4.0);
        assert!(inner.rect.width < outer.rect.width);
        assert!(inner.rect.height < outer.rect.height);
        assert!(inner.corner_radius < outer.corner_radius);
    }

    #[test]
    fn a_deep_negative_offset_never_inverts() {
        let control = Rect::new(10.0, 10.0, 6.0, 4.0);
        let ring = FocusRingStroke::at_distance(control, 2.0, -10.0);
        assert!(ring.rect.width >= 0.0);
        assert!(ring.rect.height >= 0.0);
        assert_eq!(ring.corner_radius, 0.0);
        // Collapsed onto the control's centre, not thrown off it.
        assert_eq!(ring.rect, Rect::new(13.0, 12.0, 0.0, 0.0));
    }
}
