// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The touch magnifier: a lens raised above the contact while a finger places
//! a caret or drags a selection handle, showing the text the finger covers.
//!
//! # Why it is a replay, not a readback
//!
//! The obvious implementation — copy the framebuffer under the finger and blit
//! it magnified — is unavailable: [`RenderFrame`](teksilo_canvas::RenderFrame)
//! is a display list that the renderer consumes once per frame, and nothing in
//! the pipeline reads a rendered texture back. The list, however, carries
//! `SetClip` / `ClearClip` and `SetTransform`, so the same content can simply be
//! **emitted a second time** inside a transform-and-clip scope. That is what
//! [`replay`](crate::widget::PaintContext::replay) does, and it is why the host
//! supplies its text layer as a painting closure rather than as pixels.
//!
//! Two consequences follow, and both are contracts rather than conveniences:
//!
//! * The closure is **re-entered during the same frame**. Anything it mutates
//!   happens twice per frame, and anything it borrows must already have been
//!   released by the host's own `paint`. The second half is checkable — a
//!   closure that re-borrows a `RefCell` the host still holds panics
//!   deterministically, because overlay content is painted in a separate walk
//!   *after* the main tree's paint has returned. The first half is not: a
//!   counter advanced twice, or a cache keyed by "the last paint", is invisible
//!   to the framework and shows up only as wrong content in the lens.
//! * Only the **text layer** is replayed. The host's closure must not draw its
//!   own chrome — border, focus ring, scroll bars — or the lens shows a
//!   magnified copy of the editor's frame floating over the document.
//!
//! # The lens is a rectangle
//!
//! [`DrawCommand::SetClip`](teksilo_canvas::DrawCommand::SetClip) takes a
//! `Rect` and the renderer realises it as a scissor rectangle. There is no path
//! clip, no stencil and no mask pass in the pipeline, so a **rounded** clip is
//! not expressible today, and a frame drawn with corner radius `r` therefore
//! leaves `r × (√2 − 1)` dp of magnified content standing outside each of its
//! corners — 4 dp at a 10 dp radius, far more than a hairline frame covers.
//! The shipped style answers this by drawing a **rectangular** frame, which is
//! exactly the clip, so nothing escapes. A style may raise the radius and
//! accept the corners; a genuinely rounded loupe needs a masked clip in the
//! renderer, which is deliberately out of this module's reach.

use teksilo_canvas::{Point, Rect, Transform2D};

/// Half the lens's width, in dp.
pub const MAGNIFIER_RADIUS: f32 = 48.0;

/// Magnification applied to the replayed content.
///
/// `1.25` is the glyph atlas's raster-scale ladder's first step above `1.0`
/// (`quantize_raster_scale`, geometric with ratio `1.25`), so a future
/// implementation that raised the raster scale for the replay would land on a
/// bucket rather than adding one. The **shipped** replay does not: it re-emits
/// the host's text layer through the same
/// [`PaintContext`](crate::widget::PaintContext), so the glyphs are shaped and
/// rasterised at their normal size and the lens magnifies them on the GPU. The
/// factor is chosen to keep that door open, not because it is walked through
/// today.
pub const MAGNIFIER_SCALE: f32 = 1.25;

/// How far above the contact point the lens's bottom edge sits, in dp — enough
/// to clear a fingertip.
pub const MAGNIFIER_RISE: f32 = 40.0;

/// Half the lens's height, in dp. Shallower than it is wide: what a reader
/// needs magnified is the line under the finger and a hint of its neighbours,
/// not a square of the page.
pub const MAGNIFIER_HALF_HEIGHT: f32 = 22.0;

/// A raised magnifier, as the controller publishes it.
///
/// Geometry is in **window** coordinates, the same space as everything else in
/// [`crate::text_touch`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MagnifierRequest {
    /// The point being magnified — where the finger is.
    pub focus: Point,
    /// The lens rectangle.
    pub lens: Rect,
    /// Magnification applied to the replayed content.
    pub scale: f32,
}

impl MagnifierRequest {
    /// The lens for a contact at `focus`, sized by `radius` × `half_height`,
    /// raised `rise` above the contact and clamped into `viewport`.
    ///
    /// Clamping moves the lens rather than shrinking it, and the horizontal
    /// clamp is applied independently of the vertical one, so a contact in a
    /// corner still gets a whole lens. When the lens does not fit above the
    /// contact it flips **below** it — the same reasoning as a handle that
    /// cannot hang under the last line: an affordance the user cannot see is
    /// worse than one on the unexpected side.
    pub fn new(
        focus: Point,
        viewport: Rect,
        radius: f32,
        half_height: f32,
        rise: f32,
        scale: f32,
    ) -> Self {
        let width = radius * 2.0;
        let height = half_height * 2.0;
        let above_top = focus.y - rise - height;
        let below_top = focus.y + rise;
        let top = if above_top >= viewport.y {
            above_top
        } else if below_top + height <= viewport.bottom() {
            below_top
        } else {
            above_top
        };
        let left = focus.x - radius;
        let lens = Rect::new(left, top, width, height);
        Self {
            focus,
            lens: clamp_into(lens, viewport),
            scale,
        }
    }

    /// The transform that maps the host's window-space content into the lens:
    /// [`focus`](Self::focus) lands at the lens centre, magnified by
    /// [`scale`](Self::scale).
    ///
    /// Composed for [`Canvas::apply_transform`](teksilo_canvas::Canvas::apply_transform),
    /// whose semantics are "apply this first, then whatever is already on the
    /// stack" — so a lens inside an outer transform scope still lands where the
    /// outer scope puts it.
    ///
    /// This is what the lens widget paints with — `TextMagnifier::paint` hands
    /// it straight to [`PaintContext::replay`](crate::widget::PaintContext::replay).
    /// Keep it that way: a second copy of the composition in the paint path is
    /// a rule two places can disagree about, and the tests would then be
    /// exercising the copy nobody sees.
    pub fn transform(&self) -> Transform2D {
        let centre = self.lens.center();
        Transform2D::translate(-self.focus.x, -self.focus.y)
            .then(&Transform2D::scale(self.scale, self.scale))
            .then(&Transform2D::translate(centre.x, centre.y))
    }
}

/// `rect` moved — never resized — so it lies inside `bounds` where it can.
///
/// A rectangle larger than `bounds` on an axis is pinned to that axis's leading
/// edge, which keeps the clamp total: the caller never has to handle a `None`.
fn clamp_into(rect: Rect, bounds: Rect) -> Rect {
    let x = if rect.width >= bounds.width {
        bounds.x
    } else {
        rect.x.clamp(bounds.x, bounds.right() - rect.width)
    };
    let y = if rect.height >= bounds.height {
        bounds.y
    } else {
        rect.y.clamp(bounds.y, bounds.bottom() - rect.height)
    };
    Rect::new(x, y, rect.width, rect.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 300.0,
    };

    fn request(focus: Point) -> MagnifierRequest {
        MagnifierRequest::new(
            focus,
            VIEWPORT,
            MAGNIFIER_RADIUS,
            MAGNIFIER_HALF_HEIGHT,
            MAGNIFIER_RISE,
            MAGNIFIER_SCALE,
        )
    }

    /// The point under the finger is what the reader is trying to see, so it —
    /// and not the lens's own geometry — is what the transform pins to the
    /// centre of the lens.
    #[test]
    fn the_contact_point_lands_at_the_lens_centre() {
        let req = request(Point::new(200.0, 150.0));
        let mapped = req.transform().apply_point(req.focus);
        let centre = req.lens.center();
        assert!(
            (mapped.x - centre.x).abs() < 1e-3 && (mapped.y - centre.y).abs() < 1e-3,
            "focus {:?} mapped to {mapped:?}, lens centre {centre:?}",
            req.focus
        );
    }

    /// A lens that did not magnify would be a decoration. Two points a fixed
    /// distance apart must come out `scale` times further apart.
    #[test]
    fn content_is_magnified_by_the_scale() {
        let req = request(Point::new(200.0, 150.0));
        let t = req.transform();
        let a = t.apply_point(Point::new(200.0, 150.0));
        let b = t.apply_point(Point::new(210.0, 150.0));
        assert!(
            ((b.x - a.x) - 10.0 * MAGNIFIER_SCALE).abs() < 1e-3,
            "10 dp apart became {} dp apart",
            b.x - a.x
        );
    }

    #[test]
    fn the_lens_rises_above_the_contact() {
        let req = request(Point::new(200.0, 150.0));
        assert!(
            req.lens.bottom() <= 150.0 - MAGNIFIER_RISE + 1e-3,
            "lens bottom {} should clear the contact by the rise",
            req.lens.bottom()
        );
    }

    /// Near the top of the viewport there is no room above the contact. The
    /// lens flips below rather than being clipped away or clamped on top of the
    /// finger.
    #[test]
    fn a_contact_near_the_top_flips_the_lens_below_it() {
        let req = request(Point::new(200.0, 8.0));
        assert!(
            req.lens.y >= 8.0 + MAGNIFIER_RISE - 1e-3,
            "lens should sit below a contact with no room above it, got {:?}",
            req.lens
        );
    }

    /// Clamping moves the lens; it never shrinks it, or the magnification
    /// would silently change with position.
    #[test]
    fn a_clamped_lens_keeps_its_size_and_stays_in_the_viewport() {
        for focus in [
            Point::new(2.0, 150.0),
            Point::new(398.0, 150.0),
            Point::new(2.0, 4.0),
            Point::new(398.0, 296.0),
        ] {
            let req = request(focus);
            assert_eq!(
                (req.lens.width, req.lens.height),
                (MAGNIFIER_RADIUS * 2.0, MAGNIFIER_HALF_HEIGHT * 2.0),
                "lens resized at {focus:?}"
            );
            assert!(
                req.lens.x >= VIEWPORT.x - 1e-3
                    && req.lens.right() <= VIEWPORT.right() + 1e-3
                    && req.lens.y >= VIEWPORT.y - 1e-3
                    && req.lens.bottom() <= VIEWPORT.bottom() + 1e-3,
                "lens {:?} escaped the viewport at {focus:?}",
                req.lens
            );
        }
    }

    /// A lens wider than the surface it is over is pinned rather than pushed
    /// off the leading edge — the degenerate case a narrow window produces.
    #[test]
    fn a_lens_wider_than_the_viewport_pins_to_the_leading_edge() {
        let narrow = Rect::new(10.0, 10.0, 40.0, 20.0);
        let req = MagnifierRequest::new(
            Point::new(30.0, 20.0),
            narrow,
            MAGNIFIER_RADIUS,
            MAGNIFIER_HALF_HEIGHT,
            MAGNIFIER_RISE,
            MAGNIFIER_SCALE,
        );
        assert_eq!((req.lens.x, req.lens.y), (10.0, 10.0));
    }
}
