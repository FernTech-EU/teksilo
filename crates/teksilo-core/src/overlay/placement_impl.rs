// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Overlay placement geometry: turning an anchor rect, a viewport and a
//! [`OverlayPlacement`] into the overlay's on-screen bounds — leading-edge
//! alignment, the flip-when-it-does-not-fit fallbacks and the viewport clamps.
//!
//! Everything here clamps into [`OverlayViewport::usable`] rather than into the
//! window rectangle. The one exception is
//! [`FullViewport`](OverlayPlacement::FullViewport), which is the modal scrim
//! and must cover the notch too.
//!
//! The clamp is not unconditional. An anchored panel may leave the usable area
//! rather than cover the control that raised it, and where it can be shrunk
//! instead it is shrunk — see [`above_anchor`].

use super::*;

/// The gap between a panel and whatever it is placed against.
const ANCHOR_GAP: f32 = 4.0;

/// The gap a contact-avoiding panel keeps from the contact patch.
///
/// Small on purpose: the clearance that matters is the patch itself, which the
/// digitiser measured. The gap only stops the panel's border from touching it.
const AVOID_GAP: f32 = 4.0;

/// The gap between a selection and the toolbar floating over it.
///
/// Wider than [`ANCHOR_GAP`] because the thing underneath is text the user is
/// reading, and a toolbar 4 dp off the top of a line looks attached to it.
const SELECTION_GAP: f32 = 8.0;

/// Manages the overlay stack — creation, positioning, dismissal, cascading.
/// Leading-edge-aligned x for a `Below` / `Above` overlay, clamped so the
/// overlay stays inside the viewport.
///
/// In LTR the leading edge is `anchor.x`; in RTL it is the anchor's physical
/// right edge. **Both are clamped.** The LTR arm used to be a bare `anchor.x`,
/// which silently ran a popover off the right edge of the window whenever its
/// trigger sat near that edge and its content was wider than the trigger — the
/// ordinary case for a status-bar or toolbar-trailing control. The RTL arm has
/// always clamped; there was no reason for the two to differ.
///
/// `max(x_min)` last, so a usable area narrower than the overlay pins it to the
/// leading edge and clips at the trailing one, rather than pushing its start
/// off-screen where the first thing the reader needs would be the part lost.
fn leading_aligned_x_in(anchor: Rect, actual_width: f32, x_min: f32, x_max: f32, rtl: bool) -> f32 {
    let leading = if rtl {
        anchor.x + anchor.width - actual_width
    } else {
        anchor.x
    };
    leading.min(x_max - actual_width).max(x_min)
}

/// [`leading_aligned_x_in`] against a window that is usable to its last pixel
/// — the shape the alignment tests below state their cases in, and exactly what
/// the production call reduces to when nothing is inset or occluded.
#[cfg(test)]
fn leading_aligned_x(anchor: Rect, actual_width: f32, vw: f32, rtl: bool) -> f32 {
    leading_aligned_x_in(anchor, actual_width, 0.0, vw, rtl)
}

/// The room a panel has between the anchor's top edge and the top of `area`,
/// [`ANCHOR_GAP`] already taken out. Never negative.
fn room_above(anchor: Rect, area: Rect) -> f32 {
    (anchor.y - ANCHOR_GAP - area.y).max(0.0)
}

/// The room a panel has between the anchor's bottom edge and the bottom of
/// `area`, [`ANCHOR_GAP`] already taken out. Never negative.
fn room_below(anchor: Rect, area: Rect) -> f32 {
    (area.bottom() - anchor.bottom() - ANCHOR_GAP).max(0.0)
}

/// `(y, height)` for a panel of `height` stacked **above** `anchor`.
///
/// A panel that fits in the room above sits at its ideal `y`, exactly as it
/// always has. One that does not is pinned to the top of `area` and **shrunk
/// to the room that is there** — it is not slid down until it fits, because
/// the only thing under it is the control that opened it, and a list the user
/// cannot see the trigger under is a list they are choosing blind.
///
/// Shrinking is real, not cosmetic: the overlay pass lays the content out with
/// `SizeProposal::exact(bounds.width, bounds.height)` (see
/// `WidgetTree::layout`), so a shorter rect is a shorter list that scrolls,
/// not a full-height list drawn off the top of the window with its first rows
/// unreachable.
///
/// The exception is an anchor with **no** room above it at all — one that
/// reaches the top of the usable area, which is what a control occupying the
/// whole window does. Shrinking there would mean a panel of zero height, and
/// an empty panel is not an improvement on a badly placed one, so the ideal
/// position is kept. The anchor stays visible either way: the panel is off the
/// anchor's edge, not over it.
fn above_anchor(anchor: Rect, height: f32, area: Rect) -> (f32, f32) {
    let room = room_above(anchor, area);
    if height <= room || room <= 0.0 {
        (anchor.y - ANCHOR_GAP - height, height)
    } else {
        (area.y, room)
    }
}

/// `(y, height)` for a panel of `height` stacked **below** `anchor`, the
/// transpose of [`above_anchor`].
fn below_anchor(anchor: Rect, height: f32, area: Rect) -> (f32, f32) {
    let room = room_below(anchor, area);
    let height = if room > 0.0 { height.min(room) } else { height };
    (anchor.bottom() + ANCHOR_GAP, height)
}

/// Place `size` so it clears `avoid` entirely, preferring the inline-start
/// quadrant, and stays inside `area`.
///
/// Four candidates, in preference order: inline-start of the contact, then
/// inline-end, then below it, then above it. The first two clear on the
/// horizontal axis and so accept *any* vertical position, which is what lets
/// the vertical clamp run without ever undoing the clearance; the last two are
/// the transpose. Only when the panel fits in none of the four strips — a panel
/// as large as the usable area — does the fallback clamp both axes and accept
/// the overlap, because at that size there is no placement that does not
/// overlap.
fn avoiding_bounds(avoid: Rect, size: Size, area: Rect, rtl: bool) -> Rect {
    let (w, h) = (size.width, size.height);
    let clamp_x = |x: f32| x.min(area.right() - w).max(area.x);
    let clamp_y = |y: f32| y.min(area.bottom() - h).max(area.y);

    // Inline-start is left under LTR and right under RTL: a hand reaches in
    // from the reader's own side, so the far side is the one that stays
    // visible under it.
    let start_x = if rtl {
        avoid.right() + AVOID_GAP
    } else {
        avoid.x - AVOID_GAP - w
    };
    let end_x = if rtl {
        avoid.x - AVOID_GAP - w
    } else {
        avoid.right() + AVOID_GAP
    };
    let below_y = avoid.bottom() + AVOID_GAP;
    let above_y = avoid.y - AVOID_GAP - h;

    let fits_x = |x: f32| x >= area.x && x + w <= area.right();
    let fits_y = |y: f32| y >= area.y && y + h <= area.bottom();

    for x in [start_x, end_x] {
        if fits_x(x) {
            let y = if fits_y(below_y) {
                below_y
            } else if fits_y(above_y) {
                above_y
            } else {
                clamp_y(below_y)
            };
            return Rect::new(x, y, w, h);
        }
    }
    for y in [below_y, above_y] {
        if fits_y(y) {
            return Rect::new(clamp_x(start_x), y, w, h);
        }
    }
    Rect::new(clamp_x(start_x), clamp_y(below_y), w, h)
}

/// Place `size` above `selection`, centred on it, flipping below when the
/// selection is against the top of `area`.
///
/// The horizontal clamp is ordered by direction. Centring is what a reader
/// expects while it is possible; when the toolbar is wider than the space it
/// has, one edge has to be sacrificed, and the one to keep is where the line
/// starts — the left under LTR, the right under RTL.
fn above_selection_bounds(selection: Rect, size: Size, area: Rect, rtl: bool) -> Rect {
    let (w, h) = (size.width, size.height);
    let above_y = selection.y - SELECTION_GAP - h;
    let below_y = selection.bottom() + SELECTION_GAP;
    let y = if above_y >= area.y {
        above_y
    } else if below_y + h <= area.bottom() {
        below_y
    } else {
        above_y.max(area.y)
    };
    let centred = selection.center().x - w / 2.0;
    let x = if rtl {
        centred.max(area.x).min(area.right() - w)
    } else {
        centred.min(area.right() - w).max(area.x)
    };
    Rect::new(x, y, w, h)
}

impl OverlayManager {
    /// Compute overlay positions based on anchor bounds.
    /// Called after layout to position overlays correctly.
    /// `viewport` is (width, height) used for clamping overlays to the visible area.
    ///
    /// `anchor_bounds_fn` returns `None` when the anchor widget is no
    /// longer in the arena (destroyed by a host's rebuild while the
    /// overlay is still up). In that case the overlay's bounds are
    /// left untouched — keeping it at its last valid position rather
    /// than collapsing to the (0,0) origin from a `Rect::ZERO`
    /// fallback.
    pub fn position_overlays(
        &mut self,
        anchor_bounds_fn: impl Fn(WidgetId) -> Option<Rect>,
        viewport: impl Into<OverlayViewport>,
        layout_direction: LayoutDirection,
    ) {
        let viewport: OverlayViewport = viewport.into();
        let area = viewport.usable(layout_direction);
        let rtl = matches!(layout_direction, LayoutDirection::RightToLeft);
        for overlay in &mut self.stack {
            let anchor = match anchor_bounds_fn(overlay.anchor) {
                Some(a) => a,
                None => {
                    // Anchor destroyed. Anchor-independent placements must still
                    // be positioned — e.g. a `Centered` modal opened from a menu
                    // item that has since closed (the menu item is the anchor,
                    // but `Centered` doesn't use it). Anchor-relative placements
                    // keep their previous bounds.
                    if matches!(
                        overlay.placement,
                        OverlayPlacement::Centered
                            | OverlayPlacement::FullViewport
                            | OverlayPlacement::BottomCenter
                            | OverlayPlacement::ViewportCorner { .. }
                            | OverlayPlacement::AtPointer(_)
                            | OverlayPlacement::AtPointerAvoiding { .. }
                            | OverlayPlacement::AboveSelection { .. }
                    ) {
                        Rect::ZERO
                    } else {
                        continue;
                    }
                }
            };
            let content_size = overlay.bounds.size(); // Will be set from content layout

            overlay.bounds = match &overlay.placement {
                OverlayPlacement::Below => {
                    let actual_width = content_size.width.max(anchor.width);
                    let x = leading_aligned_x_in(anchor, actual_width, area.x, area.right(), rtl);
                    Rect::new(
                        x,
                        anchor.y + anchor.height + ANCHOR_GAP,
                        actual_width,
                        content_size.height,
                    )
                }
                OverlayPlacement::Above => {
                    let actual_width = content_size.width.max(anchor.width);
                    let x = leading_aligned_x_in(anchor, actual_width, area.x, area.right(), rtl);
                    let (y, height) = above_anchor(anchor, content_size.height, area);
                    Rect::new(x, y, actual_width, height)
                }
                OverlayPlacement::TrailingEdge => {
                    // In LTR trailing is to the right; in RTL trailing is to the left.
                    let x = if rtl {
                        let x_left = anchor.x - content_size.width - 2.0;
                        if x_left >= area.x {
                            x_left
                        } else {
                            // Fallback: open to the leading side (right in RTL)
                            anchor.x + anchor.width + 2.0
                        }
                    } else {
                        let x_right = anchor.x + anchor.width + 2.0;
                        if x_right + content_size.width <= area.right() {
                            x_right
                        } else {
                            // Fallback: open to the leading side (left in LTR)
                            anchor.x - content_size.width - 2.0
                        }
                    };
                    let y = anchor
                        .y
                        .min(area.bottom() - content_size.height)
                        .max(area.y);
                    Rect::new(x, y, content_size.width, content_size.height)
                }
                OverlayPlacement::AtPointer(point) => {
                    // Clamp to the usable area so menus don't overflow off-screen
                    let x = point.x.min(area.right() - content_size.width).max(area.x);
                    let y = if point.y + content_size.height <= area.bottom() {
                        point.y
                    } else {
                        // Not enough space below pointer — open above
                        (point.y - content_size.height).max(area.y)
                    };
                    Rect::new(x, y, content_size.width, content_size.height)
                }
                OverlayPlacement::AtPointerAvoiding { avoid, .. } => {
                    avoiding_bounds(*avoid, content_size, area, rtl)
                }
                OverlayPlacement::AboveSelection { selection } => {
                    above_selection_bounds(*selection, content_size, area, rtl)
                }
                OverlayPlacement::NearAnchor { offset } => {
                    // Prefer below the anchor at `offset` + 4 px.
                    // Flip above when the content would otherwise spill
                    // past the viewport bottom — same pattern as
                    // `BelowPreferred`. Without this, a tooltip whose
                    // anchor sits near the window edge gets clipped by
                    // the surface bounds (overlays paint unclipped, but
                    // the window itself still bounds the framebuffer).
                    let below_y = anchor.y + anchor.height + offset.y + ANCHOR_GAP;
                    let fits_below = below_y + content_size.height <= area.bottom();
                    let y = if fits_below {
                        below_y
                    } else {
                        // Symmetric offset above: same gap as below.
                        let above_y = anchor.y - content_size.height - offset.y - ANCHOR_GAP;
                        above_y.max(area.y)
                    };
                    // Horizontal anchoring is direction-aware: LTR aligns
                    // the content's leading (left) edge to the anchor's
                    // left edge + offset; RTL mirrors it, aligning the
                    // content's trailing (right) edge to the anchor's
                    // right edge - offset. The clamp then keeps it in view
                    // when the anchor is near a viewport edge.
                    let unclamped_x = if rtl {
                        anchor.x + anchor.width - content_size.width - offset.x
                    } else {
                        anchor.x + offset.x
                    };
                    let x = unclamped_x
                        .min(area.right() - content_size.width)
                        .max(area.x);
                    Rect::new(x, y, content_size.width, content_size.height)
                }
                // A modal recomputes against the *usable* area, so a soft
                // keyboard shifts it up rather than sitting on top of it, and
                // a panel taller than what is left pins to the top — the one
                // edge from which the rest can still be scrolled into view.
                OverlayPlacement::Centered => Rect::new(
                    area.x + ((area.width - content_size.width) / 2.0).max(0.0),
                    area.y + ((area.height - content_size.height) / 2.0).max(0.0),
                    content_size.width.min(area.width),
                    content_size.height.min(area.height),
                ),
                OverlayPlacement::BottomCenter => Rect::new(
                    area.x + ((area.width - content_size.width) / 2.0).max(0.0),
                    (area.bottom() - content_size.height - 24.0).max(area.y),
                    content_size.width.min(area.width),
                    content_size.height.min(area.height),
                ),
                OverlayPlacement::BelowPreferred => {
                    let wanted = content_size.height;
                    let above = room_above(anchor, area);
                    let below = room_below(anchor, area);
                    // Below while it fits — the placement's name. Otherwise
                    // above while *it* fits, which is where this placement has
                    // always flipped. When the panel fits on neither side,
                    // whichever side has more room, shrunk to it; a tie keeps
                    // the flip. Sliding the panel down over the anchor is the
                    // one answer never taken: a list the user cannot see the
                    // combo box under is a list they are choosing blind.
                    let (y, height) = if wanted <= below || (wanted > above && below > above) {
                        below_anchor(anchor, wanted, area)
                    } else {
                        above_anchor(anchor, wanted, area)
                    };
                    let actual_width = content_size.width.max(anchor.width);
                    // Align leading edges, same logic as Below.
                    let x = leading_aligned_x_in(anchor, actual_width, area.x, area.right(), rtl);
                    Rect::new(x, y, actual_width, height)
                }
                OverlayPlacement::ViewportCorner { corner, margin } => {
                    let (x, y) = corner.resolve(
                        (content_size.width, content_size.height),
                        (area.width, area.height),
                        (margin.x, margin.y),
                        rtl,
                    );
                    Rect::new(
                        area.x + x,
                        area.y + y,
                        content_size.width.min(area.width),
                        content_size.height.min(area.height),
                    )
                }
                // The scrim, and only the scrim, ignores the safe area: one
                // that respected it would leave the notch undimmed and the
                // content behind it legible.
                OverlayPlacement::FullViewport => viewport.full(),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::tests::fake_id;

    /// A `Below`/`Above` overlay must stay inside the viewport in **LTR**, not
    /// only RTL.
    ///
    /// The LTR arm was a bare `anchor.x`, so a popover whose trigger sat near
    /// the right edge — a status-bar button, a toolbar-trailing control — ran
    /// off the screen and lost its trailing edge. Nothing caught it because the
    /// RTL arm, which has always clamped, is the one that looks like it needs
    /// the arithmetic.
    #[test]
    fn a_wide_overlay_near_the_trailing_edge_is_clamped_into_the_viewport() {
        let vw = 1200.0;
        // A 380 px-wide popover under a 90 px button whose left edge is at 1035:
        // unclamped it would end at 1415, 215 px past the window.
        let anchor = Rect::new(1035.0, 760.0, 90.0, 28.0);
        let x = leading_aligned_x(anchor, 380.0, vw, false);
        assert!(
            x + 380.0 <= vw + 0.01,
            "overlay must not extend past the viewport: x={x}"
        );
        assert!(x >= 0.0, "and must not start off the leading edge: x={x}");

        // Comfortably inside, the leading edge is still honoured exactly —
        // clamping must not nudge overlays that already fit.
        let inside = Rect::new(100.0, 760.0, 90.0, 28.0);
        assert_eq!(leading_aligned_x(inside, 380.0, vw, false), 100.0);

        // RTL keeps aligning to the anchor's physical right edge.
        let x_rtl = leading_aligned_x(inside, 380.0, vw, true);
        assert!(x_rtl >= 0.0 && x_rtl + 380.0 <= vw + 0.01);
    }

    /// A viewport narrower than the overlay pins the **leading** edge and clips
    /// the trailing one — losing the start of the content would hide the first
    /// thing the reader needs (a search field, a title).
    #[test]
    fn an_overlay_wider_than_the_viewport_keeps_its_leading_edge_visible() {
        let x = leading_aligned_x(Rect::new(40.0, 10.0, 60.0, 20.0), 900.0, 500.0, false);
        assert_eq!(x, 0.0);
    }

    #[test]
    fn centered_placement_uses_viewport_center() {
        let mut mgr = OverlayManager::new();
        let id = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Centered,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        mgr.set_content_bounds(id, Size::new(240.0, 120.0));
        mgr.position_overlays(
            |_| Some(Rect::new(0.0, 0.0, 10.0, 10.0)),
            (800.0, 600.0),
            LayoutDirection::LeftToRight,
        );

        let bounds = mgr
            .stack
            .iter()
            .find(|overlay| overlay.id == id)
            .unwrap()
            .bounds;
        assert!((bounds.x - 280.0).abs() < 0.01);
        assert!((bounds.y - 240.0).abs() < 0.01);
    }

    #[test]
    fn bottom_center_placement_uses_viewport_bottom_margin() {
        let mut mgr = OverlayManager::new();
        let id = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::BottomCenter,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        mgr.set_content_bounds(id, Size::new(240.0, 64.0));
        mgr.position_overlays(
            |_| Some(Rect::new(0.0, 0.0, 10.0, 10.0)),
            (800.0, 600.0),
            LayoutDirection::LeftToRight,
        );

        let bounds = mgr
            .stack
            .iter()
            .find(|overlay| overlay.id == id)
            .unwrap()
            .bounds;
        assert!((bounds.x - 280.0).abs() < 0.01);
        assert!((bounds.y - 512.0).abs() < 0.01);
    }

    // --- ViewportCorner placement ---

    fn show_corner_overlay(
        mgr: &mut OverlayManager,
        corner: Corner,
        margin: Vec2,
        size: Size,
    ) -> OverlayId {
        let id = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::ViewportCorner { corner, margin },
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        mgr.set_content_bounds(id, size);
        id
    }

    fn overlay_bounds(mgr: &OverlayManager, id: OverlayId) -> Rect {
        mgr.stack.iter().find(|o| o.id == id).unwrap().bounds
    }

    #[test]
    fn viewport_corner_top_leading_ltr() {
        let mut mgr = OverlayManager::new();
        let id = show_corner_overlay(
            &mut mgr,
            Corner::TopLeading,
            Vec2::new(24.0, 24.0),
            Size::new(380.0, 100.0),
        );
        mgr.position_overlays(
            |_| Some(Rect::ZERO),
            (800.0, 600.0),
            LayoutDirection::LeftToRight,
        );
        let b = overlay_bounds(&mgr, id);
        assert!((b.x - 24.0).abs() < 0.01, "x = {}", b.x);
        assert!((b.y - 24.0).abs() < 0.01, "y = {}", b.y);
    }

    #[test]
    fn viewport_corner_top_trailing_ltr() {
        let mut mgr = OverlayManager::new();
        let id = show_corner_overlay(
            &mut mgr,
            Corner::TopTrailing,
            Vec2::new(24.0, 24.0),
            Size::new(380.0, 100.0),
        );
        mgr.position_overlays(
            |_| Some(Rect::ZERO),
            (800.0, 600.0),
            LayoutDirection::LeftToRight,
        );
        let b = overlay_bounds(&mgr, id);
        // 800 - 380 - 24 = 396
        assert!((b.x - 396.0).abs() < 0.01, "x = {}", b.x);
        assert!((b.y - 24.0).abs() < 0.01);
    }

    #[test]
    fn viewport_corner_bottom_leading_ltr() {
        let mut mgr = OverlayManager::new();
        let id = show_corner_overlay(
            &mut mgr,
            Corner::BottomLeading,
            Vec2::new(24.0, 24.0),
            Size::new(380.0, 100.0),
        );
        mgr.position_overlays(
            |_| Some(Rect::ZERO),
            (800.0, 600.0),
            LayoutDirection::LeftToRight,
        );
        let b = overlay_bounds(&mgr, id);
        // 600 - 100 - 24 = 476
        assert!((b.x - 24.0).abs() < 0.01);
        assert!((b.y - 476.0).abs() < 0.01, "y = {}", b.y);
    }

    #[test]
    fn viewport_corner_bottom_trailing_ltr() {
        let mut mgr = OverlayManager::new();
        let id = show_corner_overlay(
            &mut mgr,
            Corner::BottomTrailing,
            Vec2::new(24.0, 24.0),
            Size::new(380.0, 100.0),
        );
        mgr.position_overlays(
            |_| Some(Rect::ZERO),
            (800.0, 600.0),
            LayoutDirection::LeftToRight,
        );
        let b = overlay_bounds(&mgr, id);
        assert!((b.x - 396.0).abs() < 0.01);
        assert!((b.y - 476.0).abs() < 0.01);
    }

    #[test]
    fn viewport_corner_top_trailing_rtl_flips_to_left() {
        let mut mgr = OverlayManager::new();
        let id = show_corner_overlay(
            &mut mgr,
            Corner::TopTrailing,
            Vec2::new(24.0, 24.0),
            Size::new(380.0, 100.0),
        );
        mgr.position_overlays(
            |_| Some(Rect::ZERO),
            (800.0, 600.0),
            LayoutDirection::RightToLeft,
        );
        let b = overlay_bounds(&mgr, id);
        // RTL flips Trailing to physical left
        assert!((b.x - 24.0).abs() < 0.01, "x = {}", b.x);
        assert!((b.y - 24.0).abs() < 0.01);
    }

    #[test]
    fn viewport_corner_bottom_leading_rtl_flips_to_right() {
        let mut mgr = OverlayManager::new();
        let id = show_corner_overlay(
            &mut mgr,
            Corner::BottomLeading,
            Vec2::new(24.0, 24.0),
            Size::new(380.0, 100.0),
        );
        mgr.position_overlays(
            |_| Some(Rect::ZERO),
            (800.0, 600.0),
            LayoutDirection::RightToLeft,
        );
        let b = overlay_bounds(&mgr, id);
        assert!((b.x - 396.0).abs() < 0.01, "x = {}", b.x);
        assert!((b.y - 476.0).abs() < 0.01);
    }

    #[test]
    fn viewport_corner_ignores_anchor_bounds() {
        let mut mgr = OverlayManager::new();
        let id = show_corner_overlay(
            &mut mgr,
            Corner::BottomTrailing,
            Vec2::new(0.0, 0.0),
            Size::new(100.0, 100.0),
        );
        // Even with an absurd anchor location, ViewportCorner only uses viewport.
        mgr.position_overlays(
            |_| Some(Rect::new(123.0, 456.0, 7.0, 8.0)),
            (800.0, 600.0),
            LayoutDirection::LeftToRight,
        );
        let b = overlay_bounds(&mgr, id);
        assert_eq!((b.x, b.y), (700.0, 500.0));
    }

    #[test]
    fn near_anchor_horizontal_is_direction_aware() {
        // NearAnchor (used by tooltips): LTR aligns the content's leading
        // (left) edge to the anchor's left edge; RTL mirrors it, aligning
        // the content's trailing (right) edge to the anchor's right edge.
        // Anchor x=600, w=100 (right edge 700); content w=200; offset 0.
        // Viewport 800×600 — wide enough that the clamp doesn't bite.
        let anchor = Rect::new(600.0, 100.0, 100.0, 20.0);
        let resolved_x = |dir: LayoutDirection| {
            let mut mgr = OverlayManager::new();
            let id = mgr.show(OverlayRequest {
                content_id: fake_id(10),
                anchor: fake_id(1),
                placement: OverlayPlacement::NearAnchor {
                    offset: Vec2::new(0.0, 8.0),
                },
                dismiss: DismissBehavior::Manual,
                layer: OverlayLayer::InTree,
                parent_overlay: None,
                on_dismiss: None,
                fade_duration: None,
            });
            mgr.set_content_bounds(id, Size::new(200.0, 50.0));
            mgr.position_overlays(|_| Some(anchor), (800.0, 600.0), dir);
            overlay_bounds(&mgr, id).x
        };
        // LTR: anchor.x + offset.x = 600.
        assert!(
            (resolved_x(LayoutDirection::LeftToRight) - 600.0).abs() < 0.01,
            "LTR x = {}",
            resolved_x(LayoutDirection::LeftToRight)
        );
        // RTL: anchor.x + anchor.width - content.w - offset.x = 500.
        assert!(
            (resolved_x(LayoutDirection::RightToLeft) - 500.0).abs() < 0.01,
            "RTL x = {}",
            resolved_x(LayoutDirection::RightToLeft)
        );
    }

    // -----------------------------------------------------------------
    // Contact avoidance
    // -----------------------------------------------------------------

    fn overlaps(a: Rect, b: Rect) -> bool {
        a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
    }

    /// Show one overlay with `placement` at `size` and return where it landed.
    fn placed(
        placement: OverlayPlacement,
        size: Size,
        anchor: Rect,
        viewport: impl Into<OverlayViewport>,
        direction: LayoutDirection,
    ) -> Rect {
        let mut mgr = OverlayManager::new();
        let id = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        mgr.set_content_bounds(id, size);
        mgr.position_overlays(|_| Some(anchor), viewport, direction);
        overlay_bounds(&mgr, id)
    }

    /// A finger is an opaque disc, so the menu it raises must not open beneath
    /// it — **at every corner of the screen**, which is where the obvious
    /// "offset it down and right" fix stops working and quietly puts the panel
    /// back under the hand (or off-screen).
    #[test]
    fn a_coarse_menu_clears_the_contact_at_every_corner() {
        let viewport = (800.0, 600.0);
        let menu = Size::new(220.0, 260.0);
        for point in [
            Point::new(6.0, 6.0),
            Point::new(794.0, 6.0),
            Point::new(6.0, 594.0),
            Point::new(794.0, 594.0),
        ] {
            for direction in [LayoutDirection::LeftToRight, LayoutDirection::RightToLeft] {
                let placement = OverlayPlacement::at_pointer_for(
                    point,
                    &crate::pointer::PointerInfo::touch(
                        crate::pointer::PointerId::MOUSE,
                        crate::pointer::EventTime::ZERO,
                    ),
                );
                let OverlayPlacement::AtPointerAvoiding { avoid, .. } = placement else {
                    panic!("a coarse pointer must get the avoiding placement");
                };
                let bounds = placed(placement.clone(), menu, Rect::ZERO, viewport, direction);
                assert!(
                    !overlaps(bounds, avoid),
                    "menu at {point:?} ({direction:?}) sits under the contact: \
                     bounds {bounds:?}, contact {avoid:?}"
                );
                assert!(
                    bounds.x >= -0.01
                        && bounds.y >= -0.01
                        && bounds.right() <= 800.01
                        && bounds.bottom() <= 600.01,
                    "menu at {point:?} ({direction:?}) left the viewport: {bounds:?}"
                );
            }
        }
    }

    /// The preferred quadrant is inline-start — the far side from the hand —
    /// and it mirrors, so an Arabic user's menu opens where an English user's
    /// does relative to the *line*, not relative to the screen.
    #[test]
    fn the_avoiding_quadrant_mirrors_under_rtl() {
        let point = Point::new(400.0, 300.0);
        let avoid = crate::overlay::rect_centred_on(point, ASSUMED_CONTACT_PATCH);
        let menu = Size::new(220.0, 260.0);
        let at = |direction| {
            placed(
                OverlayPlacement::AtPointerAvoiding { point, avoid },
                menu,
                Rect::ZERO,
                (800.0, 600.0),
                direction,
            )
        };
        let ltr = at(LayoutDirection::LeftToRight);
        let rtl = at(LayoutDirection::RightToLeft);
        assert!(
            ltr.right() <= avoid.x,
            "LTR opens to the left of the contact: {ltr:?} vs {avoid:?}"
        );
        assert!(
            rtl.x >= avoid.right(),
            "RTL opens to the right of the contact: {rtl:?} vs {avoid:?}"
        );
        // Same distance from the contact on either side — a mirror, not two
        // independently tuned offsets.
        assert!(
            ((avoid.x - ltr.right()) - (rtl.x - avoid.right())).abs() < 0.01,
            "the two sides must be symmetric: {ltr:?} / {rtl:?}"
        );
        assert_eq!(ltr.y, rtl.y, "only the inline axis mirrors");
    }

    /// A finger, with whatever contact patch the digitiser reported.
    fn finger(contact: Option<Size>) -> crate::pointer::PointerInfo {
        let mut pointer = crate::pointer::PointerInfo::touch(
            crate::pointer::PointerId::MOUSE,
            crate::pointer::EventTime::ZERO,
        );
        pointer.axes.contact = contact;
        pointer
    }

    /// The rectangle a menu clears, and where that put it.
    fn avoided(point: Point, contact: Option<Size>) -> (Rect, Rect) {
        let placement = OverlayPlacement::at_pointer_for(point, &finger(contact));
        let OverlayPlacement::AtPointerAvoiding { avoid, .. } = placement else {
            panic!("a coarse pointer must get the avoiding placement");
        };
        let bounds = placed(
            placement,
            Size::new(220.0, 260.0),
            Rect::ZERO,
            (800.0, 600.0),
            LayoutDirection::LeftToRight,
        );
        (avoid, bounds)
    }

    /// A device that reports a **bigger** patch than the floor is believed: a
    /// thumb covers more than the 24 dp a fingertip is assumed to, and the menu
    /// has to clear the thumb, not the assumption.
    ///
    /// The check is on where the panel *lands*, not only on the rectangle,
    /// because ignoring `PointerAxes::contact` entirely leaves a perfectly
    /// self-consistent 24 dp answer that only a comparison against the floor
    /// can tell apart.
    #[test]
    fn a_contact_patch_larger_than_the_floor_widens_what_the_menu_clears() {
        let point = Point::new(400.0, 300.0);
        let (avoid, bounds) = avoided(point, Some(Size::new(60.0, 40.0)));
        assert_eq!(
            avoid,
            Rect::new(370.0, 280.0, 60.0, 40.0),
            "the reported patch, centred on the contact"
        );
        assert!(
            bounds.right() <= avoid.x + 0.01,
            "the menu must clear the thumb: {bounds:?} vs {avoid:?}"
        );
        let (_, floored) = avoided(point, None);
        assert!(
            bounds.right() < floored.right() - 0.01,
            "a 60 dp patch must push the menu further out than the 24 dp floor \
             would ({} vs {})",
            bounds.right(),
            floored.right()
        );
    }

    /// A device that reports a **smaller** patch than the floor is not: 24 dp
    /// is the smallest thing a finger is ever asked to hit, so it is the
    /// smallest rectangle a finger can be assumed to cover, and a digitiser
    /// claiming 6 dp is under-reporting a fingertip rather than describing one.
    #[test]
    fn a_contact_patch_smaller_than_the_floor_is_raised_to_it() {
        let point = Point::new(400.0, 300.0);
        let (avoid, bounds) = avoided(point, Some(Size::new(6.0, 6.0)));
        assert_eq!(
            avoid,
            crate::overlay::rect_centred_on(point, ASSUMED_CONTACT_PATCH),
            "the floor, not the 6 dp the device claimed"
        );
        let claimed = crate::overlay::rect_centred_on(point, Size::new(6.0, 6.0));
        assert!(
            bounds.right() < claimed.x - 0.01,
            "clearing only the reported 6 dp leaves the menu under the finger: \
             {bounds:?} vs the 24 dp floor at {avoid:?}"
        );
        let (_, floored) = avoided(point, None);
        assert_eq!(
            bounds, floored,
            "an under-reported patch must place exactly as no report at all"
        );
    }

    /// A precise pointer keeps the placement it always had, to the pixel.
    #[test]
    fn a_mouse_still_gets_the_plain_at_pointer_placement() {
        let point = Point::new(400.0, 300.0);
        let mouse = crate::pointer::PointerInfo::mouse(crate::pointer::EventTime::ZERO);
        assert!(matches!(
            OverlayPlacement::at_pointer_for(point, &mouse),
            OverlayPlacement::AtPointer(p) if p == point
        ));
    }

    // -----------------------------------------------------------------
    // An anchored panel never covers its anchor
    // -----------------------------------------------------------------

    /// The rule, stated once: **a panel never covers the control that opened
    /// it.** A viewport clamp that slides a flipped-up panel down onto its own
    /// anchor buys an on-screen rectangle at the price of the thing the user
    /// is choosing *for*, and that is never the better trade.
    ///
    /// The case is the degenerate one, because it is the one where a clamp is
    /// most tempting and most wrong: the combo box fills the window, so no
    /// placement can hold the list and the panel comes out empty rather than
    /// on top of the trigger. `teksilo-widgets`'
    /// `below_preferred_opens_above_when_no_space` is the same geometry seen
    /// from the widget side; this test is here so the rule cannot be broken
    /// from inside core alone.
    #[test]
    fn a_drop_down_never_covers_the_combo_box_that_opened_it() {
        let anchor = Rect::new(0.0, 0.0, 300.0, 60.0);
        let bounds = placed(
            OverlayPlacement::BelowPreferred,
            Size::new(300.0, 80.0),
            anchor,
            (300.0, 60.0),
            LayoutDirection::LeftToRight,
        );
        assert!(
            !overlaps(bounds, anchor),
            "the list sits on top of its own combo box: {bounds:?} over {anchor:?}"
        );
        assert!(
            bounds.bottom() <= anchor.y,
            "the list must stay above the trigger: {bounds:?}"
        );
        // And it keeps its height. There is no room to shrink into here, and a
        // zero-height list is not an improvement on a badly placed one.
        assert_eq!(bounds.height, 80.0, "an empty panel is not the answer");
        assert_eq!(bounds.y, -84.0, "hung off the anchor's top edge");
    }

    /// Too tall for either side, so it takes the side with more room and is
    /// **shrunk to that room** — 166 dp below beats 6 dp above, and neither a
    /// list hanging off the top of the window nor one clamped down over the
    /// trigger is on offer.
    #[test]
    fn a_drop_down_that_fits_neither_side_takes_the_roomier_one_and_shrinks() {
        let anchor = Rect::new(100.0, 10.0, 80.0, 20.0);
        let bounds = placed(
            OverlayPlacement::BelowPreferred,
            Size::new(120.0, 200.0),
            anchor,
            // Short viewport: 6 dp of room above the anchor, 166 below.
            (800.0, 200.0),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(bounds.y, 34.0, "stays below — that is the roomier side");
        assert_eq!(bounds.height, 166.0, "shrunk to the room that is there");
        assert!(
            bounds.bottom() <= 200.0,
            "and inside the window: {bounds:?}"
        );
    }

    /// The flip itself is untouched: while the panel fits above, it goes there
    /// at full height and at the y it has always had.
    #[test]
    fn a_drop_down_that_fits_above_still_flips_there_unchanged() {
        let anchor = Rect::new(100.0, 400.0, 80.0, 20.0);
        let bounds = placed(
            OverlayPlacement::BelowPreferred,
            Size::new(120.0, 200.0),
            anchor,
            (800.0, 460.0),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(bounds.y, 196.0, "400 - 200 - 4");
        assert_eq!(bounds.height, 200.0, "no shrink: it fits");
    }

    /// `Above` is the same rule without the side choice: a panel taller than
    /// the room above is pinned to the top of the usable area and shrunk to
    /// that room, never slid down over the anchor.
    #[test]
    fn an_above_panel_too_tall_for_the_room_shrinks_rather_than_covering() {
        let anchor = Rect::new(100.0, 60.0, 80.0, 20.0);
        let bounds = placed(
            OverlayPlacement::Above,
            Size::new(120.0, 200.0),
            anchor,
            (800.0, 600.0),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(bounds.y, 0.0, "pinned to the top of the usable area");
        assert_eq!(
            bounds.height, 56.0,
            "the 60 dp above the anchor, less the gap"
        );
        assert!(
            bounds.bottom() <= anchor.y,
            "and clear of the anchor: {bounds:?}"
        );
    }

    /// The safe area moves the ceiling, not the rule: under a 40 dp notch the
    /// panel starts at 40 and is shrunk to the 156 dp between the notch and
    /// the anchor.
    #[test]
    fn a_shrunk_panel_starts_below_the_notch() {
        let viewport = OverlayViewport::from((800.0, 600.0))
            .with_safe_area(teksilo_canvas::EdgeInsets::new(40.0, 0.0, 0.0, 0.0));
        let anchor = Rect::new(100.0, 200.0, 80.0, 20.0);
        let bounds = placed(
            OverlayPlacement::Above,
            Size::new(120.0, 220.0),
            anchor,
            viewport,
            LayoutDirection::LeftToRight,
        );
        assert_eq!(bounds.y, 40.0, "below the notch");
        assert_eq!(bounds.height, 156.0, "200 - 40 - 4");
    }

    // -----------------------------------------------------------------
    // AboveSelection
    // -----------------------------------------------------------------

    #[test]
    fn a_selection_toolbar_floats_centred_above_the_selection() {
        let selection = Rect::new(300.0, 300.0, 100.0, 20.0);
        let bounds = placed(
            OverlayPlacement::AboveSelection { selection },
            Size::new(200.0, 40.0),
            Rect::ZERO,
            (800.0, 600.0),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(bounds.x, 250.0, "centred on the selection");
        assert_eq!(bounds.y, 252.0, "8 dp above it");
    }

    /// A selection on the first line has nothing above it, so the toolbar goes
    /// under rather than off the top.
    #[test]
    fn a_selection_toolbar_flips_below_at_the_top_of_the_window() {
        let selection = Rect::new(300.0, 10.0, 100.0, 20.0);
        let bounds = placed(
            OverlayPlacement::AboveSelection { selection },
            Size::new(200.0, 40.0),
            Rect::ZERO,
            (800.0, 600.0),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(bounds.y, 38.0, "8 dp below the selection");
    }

    /// When the toolbar cannot be centred, the edge that survives is the one
    /// the line starts at — left in English, right in Arabic.
    #[test]
    fn a_selection_toolbar_keeps_its_inline_start_edge_when_it_cannot_fit() {
        let selection = Rect::new(10.0, 300.0, 20.0, 20.0);
        let toolbar = Size::new(200.0, 40.0);
        let at = |direction| {
            placed(
                OverlayPlacement::AboveSelection { selection },
                toolbar,
                Rect::ZERO,
                (100.0, 600.0),
                direction,
            )
        };
        let ltr = at(LayoutDirection::LeftToRight);
        let rtl = at(LayoutDirection::RightToLeft);
        assert_eq!(ltr.x, 0.0, "LTR keeps the left edge");
        assert_eq!(rtl.right(), 100.0, "RTL keeps the right edge");
    }

    // -----------------------------------------------------------------
    // The viewport
    // -----------------------------------------------------------------

    /// A modal centres in the part of the window a person can see, not in the
    /// window.
    #[test]
    fn a_centered_modal_centres_inside_the_safe_area() {
        let viewport = OverlayViewport::from((800.0, 600.0))
            .with_safe_area(teksilo_canvas::EdgeInsets::new(100.0, 0.0, 0.0, 0.0));
        let bounds = placed(
            OverlayPlacement::Centered,
            Size::new(200.0, 100.0),
            Rect::ZERO,
            viewport,
            LayoutDirection::LeftToRight,
        );
        assert_eq!(bounds.y, 300.0, "centred in the 500 dp below the notch");
    }

    /// The scrim is the one placement that ignores the safe area: one that
    /// respected it would leave the notch undimmed.
    #[test]
    fn the_scrim_still_covers_the_whole_window() {
        let viewport = OverlayViewport::from((800.0, 600.0))
            .with_safe_area(teksilo_canvas::EdgeInsets::uniform(40.0))
            .with_occluded(Some(Rect::new(0.0, 400.0, 800.0, 200.0)));
        let bounds = placed(
            OverlayPlacement::FullViewport,
            Size::ZERO,
            Rect::ZERO,
            viewport,
            LayoutDirection::LeftToRight,
        );
        assert_eq!(bounds, Rect::new(0.0, 0.0, 800.0, 600.0));
    }

    /// A menu raised while a keyboard is up opens in the band above it, not
    /// behind it.
    #[test]
    fn an_occluded_band_is_not_a_place_to_put_a_menu() {
        let viewport = OverlayViewport::from((800.0, 600.0))
            .with_occluded(Some(Rect::new(0.0, 340.0, 800.0, 260.0)));
        let bounds = placed(
            OverlayPlacement::AtPointer(Point::new(100.0, 300.0)),
            Size::new(200.0, 120.0),
            Rect::ZERO,
            viewport,
            LayoutDirection::LeftToRight,
        );
        assert!(
            bounds.bottom() <= 340.01,
            "the menu must stay above the keyboard: {bounds:?}"
        );
    }

    #[test]
    fn viewport_corner_zero_margin_snaps_to_edge() {
        let mut mgr = OverlayManager::new();
        let id = show_corner_overlay(
            &mut mgr,
            Corner::TopLeading,
            Vec2::ZERO,
            Size::new(50.0, 50.0),
        );
        mgr.position_overlays(
            |_| Some(Rect::ZERO),
            (800.0, 600.0),
            LayoutDirection::LeftToRight,
        );
        let b = overlay_bounds(&mgr, id);
        assert_eq!((b.x, b.y), (0.0, 0.0));
    }
}
