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
//!
//! # Where the safe area comes from
//!
//! Both production calls to
//! [`position_overlays`](OverlayManager::position_overlays) are inside
//! [`WidgetTree::layout_with_ops`](crate::WidgetTree::layout_with_ops) — which
//! is what `teksilo-app` calls and what
//! [`layout`](crate::WidgetTree::layout) delegates to — and it now builds the
//! [`OverlayViewport`] from the tree's own
//! [`safe_area`](crate::WidgetTree::safe_area) and
//! [`occluded_inset`](crate::WidgetTree::occluded_inset), which `teksilo-app`
//! reads from the platform after every resize and scale change.
//!
//! On the desktop both are usually nothing, and where they are nothing the
//! arithmetic below reduces exactly to what it was before [`OverlayViewport`]
//! existed: only macOS reports a safe area at all, and only Windows has a
//! findable soft keyboard. So the notch and keyboard behaviour here is reached
//! in production, on the platforms that have one, and is inert everywhere else
//! — which is what the tests at the bottom of this file check by supplying the
//! numbers directly.

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

/// Leading-edge-aligned x for a `Below` / `Above` overlay, clamped so the
/// overlay stays inside the viewport.
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

/// The width an anchor-aligned panel is drawn at: its own, but never narrower
/// than the control it hangs off.
///
/// This is the drop-down convention on every desktop: the list is at least as
/// wide as the combo box, so it reads as the field opening downward rather than
/// as a loose panel parked under its leading third. A 240 dp combo box whose
/// 80 dp list clings to its left edge looks broken, and it looks broken in
/// exactly the case that is most common — a short list under a field sized to
/// the form.
///
/// It lives here rather than in the widget because only the placement pass has
/// both rectangles. The content's intrinsic size is measured with no reference
/// to the anchor (see `WidgetTree::layout`), so a widget wanting this for
/// itself would have to observe its own final laid-out width and re-propose its
/// popup a frame later — which every `ComboBox`-shaped widget in the catalogue
/// would then have to do, identically.
///
/// The floor is not only cosmetic: the result feeds
/// [`leading_aligned_x_in`], so it decides where the panel is clamped as well
/// as how wide it is. Applies to [`Below`](OverlayPlacement::Below),
/// [`Above`](OverlayPlacement::Above) and
/// [`BelowPreferred`](OverlayPlacement::BelowPreferred) — the three placements
/// that align to an anchor's leading edge.
fn at_least_anchor_width(content_width: f32, anchor: Rect) -> f32 {
    content_width.max(anchor.width)
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
    /// overlay is still up). An anchor-relative placement then keeps its
    /// bounds untouched — at its last valid position rather than
    /// collapsing to the (0,0) origin from a `Rect::ZERO` fallback. An
    /// anchor-independent one is still positioned, against a `Rect::ZERO`
    /// anchor it does not read.
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
                    let actual_width = at_least_anchor_width(content_size.width, anchor);
                    let x = leading_aligned_x_in(anchor, actual_width, area.x, area.right(), rtl);
                    Rect::new(
                        x,
                        anchor.y + anchor.height + ANCHOR_GAP,
                        actual_width,
                        content_size.height,
                    )
                }
                OverlayPlacement::Above => {
                    let actual_width = at_least_anchor_width(content_size.width, anchor);
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
                    let actual_width = at_least_anchor_width(content_size.width, anchor);
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

    // -----------------------------------------------------------------
    // Every placement is measured against the usable area, not the window
    // -----------------------------------------------------------------
    //
    // §1 of `docs/overlays.md` says placement clamps into
    // `OverlayViewport::usable`, and §2's table repeats the promise row by
    // row. On a desktop the two rectangles are the same rectangle, so a
    // placement that clamps against the window satisfies every existing
    // assertion and the guarantee holds by coincidence rather than by code.
    //
    // The tests below are the ones that stop coinciding: each uses a viewport
    // whose insets move every edge, and each anchor is placed where the two
    // rectangles give different answers. Swap any `area` in
    // `position_overlays` for the window and one of them reports a number
    // from the wrong rectangle.

    /// A viewport whose safe area moves **every** edge: a 60 dp sensor
    /// housing at the start of the line, a 40 dp rounded corner at its end, a
    /// 40 dp notch and a 30 dp home indicator, on an 800 × 600 window.
    ///
    /// Under LTR that leaves `Rect::new(60, 40, 700, 530)`, under RTL
    /// `Rect::new(40, 40, 700, 530)` — the two horizontal insets swap. No edge
    /// of either coincides with the window's, which is the whole point: a
    /// clamp against the wrong rectangle lands on a different number instead
    /// of accidentally the right one.
    fn inset_viewport() -> OverlayViewport {
        OverlayViewport::from((800.0, 600.0))
            .with_safe_area(teksilo_canvas::EdgeInsets::new(40.0, 40.0, 30.0, 60.0))
    }

    /// The rectangle [`inset_viewport`] leaves usable in `direction`.
    fn inset_usable(direction: LayoutDirection) -> Rect {
        inset_viewport().usable(direction)
    }

    /// `Below` clamps **x** into the usable area.
    ///
    /// Four cases, because the row makes four separate promises and a change
    /// can break them one at a time: the trailing bound (`area.right()`), the
    /// leading bound (`area.x`), the mirroring of both under RTL — the area is
    /// `usable(direction)`, not `usable(LTR)` — and the row's negative half,
    /// that **y** is *not* clamped, so a panel opened against the bottom drops
    /// past it rather than sliding back up over the control that opened it.
    #[test]
    fn a_below_panel_is_clamped_into_the_usable_area_not_the_window() {
        let ltr = inset_usable(LayoutDirection::LeftToRight);
        let panel = Size::new(300.0, 180.0);

        // A trigger against the trailing inset: the panel's far edge lands on
        // the usable edge (760), not on the window's (800).
        let trailing = placed(
            OverlayPlacement::Below,
            panel,
            Rect::new(690.0, 200.0, 60.0, 24.0),
            inset_viewport(),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(trailing.x, 460.0, "clamped to the usable trailing edge");
        assert_eq!(trailing.right(), ltr.right());

        // A trigger under the sensor housing: the panel starts at the usable
        // leading edge (60), not at the window's (0).
        let leading = placed(
            OverlayPlacement::Below,
            panel,
            Rect::new(20.0, 200.0, 60.0, 24.0),
            inset_viewport(),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(leading.x, ltr.x, "clamped to the usable leading edge");

        // RTL puts each inset on the other physical edge, so the same anchor
        // clamps to 440 rather than to the LTR 460.
        let rtl = inset_usable(LayoutDirection::RightToLeft);
        let mirrored = placed(
            OverlayPlacement::Below,
            panel,
            Rect::new(700.0, 200.0, 60.0, 24.0),
            inset_viewport(),
            LayoutDirection::RightToLeft,
        );
        assert_eq!(mirrored.x, 440.0, "the mirrored trailing edge");
        assert_eq!(mirrored.right(), rtl.right());

        // And y, which the row promises *not* to clamp.
        let low = placed(
            OverlayPlacement::Below,
            panel,
            Rect::new(100.0, 520.0, 60.0, 24.0),
            inset_viewport(),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(low.y, 548.0, "the anchor's bottom edge plus the gap");
        assert!(
            low.bottom() > ltr.bottom(),
            "and it is allowed to run past the usable bottom: {low:?}"
        );
    }

    /// `Above` aligns and clamps x exactly as `Below` does; only its y differs.
    #[test]
    fn an_above_panel_is_clamped_into_the_usable_area_not_the_window() {
        let ltr = inset_usable(LayoutDirection::LeftToRight);
        let panel = Size::new(300.0, 100.0);

        let trailing = placed(
            OverlayPlacement::Above,
            panel,
            Rect::new(690.0, 300.0, 60.0, 24.0),
            inset_viewport(),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(trailing.x, 460.0, "clamped to the usable trailing edge");
        assert_eq!(trailing.right(), ltr.right());
        assert_eq!(trailing.y, 196.0, "and still sits above the anchor");

        let leading = placed(
            OverlayPlacement::Above,
            panel,
            Rect::new(20.0, 300.0, 60.0, 24.0),
            inset_viewport(),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(leading.x, ltr.x, "clamped to the usable leading edge");
    }

    /// `BelowPreferred` uses the usable area twice: for the x clamp it shares
    /// with `Below`, and for the room it measures on each side before choosing
    /// one. The third case is the one only the second use can answer — a combo
    /// box with 172 dp of window below it but only 142 dp a person can see,
    /// which is a flip upward rather than a list running under the home
    /// indicator.
    #[test]
    fn a_below_preferred_panel_is_placed_against_the_usable_area_not_the_window() {
        let ltr = inset_usable(LayoutDirection::LeftToRight);
        let panel = Size::new(300.0, 100.0);

        let trailing = placed(
            OverlayPlacement::BelowPreferred,
            panel,
            Rect::new(690.0, 300.0, 60.0, 24.0),
            inset_viewport(),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(trailing.x, 460.0, "clamped to the usable trailing edge");
        assert_eq!(trailing.right(), ltr.right());
        assert_eq!(trailing.y, 328.0, "and drops below, which still fits");

        let leading = placed(
            OverlayPlacement::BelowPreferred,
            panel,
            Rect::new(20.0, 300.0, 60.0, 24.0),
            inset_viewport(),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(leading.x, ltr.x, "clamped to the usable leading edge");

        let anchor = Rect::new(100.0, 400.0, 60.0, 24.0);
        let flipped = placed(
            OverlayPlacement::BelowPreferred,
            Size::new(300.0, 160.0),
            anchor,
            inset_viewport(),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(
            flipped.y, 236.0,
            "flipped above: 160 dp does not fit in 142"
        );
        assert_eq!(flipped.height, 160.0, "with nothing shrunk away");
        assert!(
            flipped.bottom() <= anchor.y,
            "and clear of the anchor: {flipped:?}"
        );
    }

    /// `NearAnchor` — the tooltip placement — clamps x at both ends and flips
    /// above when the room below runs out, clamping to the top.
    ///
    /// The two vertical cases separate the two uses of the area: whether there
    /// is room below is asked of `area.bottom()` — under the first anchor's
    /// gap there are 124 dp of window and 94 dp of usable area, and the 100 dp
    /// tooltip fits the first of those and not the second — and the flipped
    /// position is floored at `area.y` rather than at the window's top edge,
    /// which is under the notch.
    #[test]
    fn a_near_anchor_tooltip_is_placed_against_the_usable_area_not_the_window() {
        let ltr = inset_usable(LayoutDirection::LeftToRight);
        let tooltip = Size::new(300.0, 100.0);
        let near = |anchor, size| {
            placed(
                OverlayPlacement::NearAnchor {
                    offset: Vec2::new(0.0, 8.0),
                },
                size,
                anchor,
                inset_viewport(),
                LayoutDirection::LeftToRight,
            )
        };

        let trailing = near(Rect::new(690.0, 200.0, 60.0, 24.0), tooltip);
        assert_eq!(trailing.x, 460.0, "clamped to the usable trailing edge");
        assert_eq!(trailing.right(), ltr.right());

        let leading = near(Rect::new(20.0, 200.0, 60.0, 24.0), tooltip);
        assert_eq!(leading.x, ltr.x, "clamped to the usable leading edge");

        let flipped = near(Rect::new(100.0, 440.0, 60.0, 24.0), tooltip);
        assert_eq!(
            flipped.y, 328.0,
            "no room below the anchor a person can see"
        );

        let pinned = near(Rect::new(100.0, 500.0, 60.0, 24.0), Size::new(300.0, 480.0));
        assert_eq!(pinned.y, ltr.y, "flipped and pinned below the notch");
    }

    /// `AtPointer` — the mouse context menu — clamps x at both ends, decides
    /// which way to open against the usable bottom, and floors the flipped
    /// position at the usable top.
    #[test]
    fn an_at_pointer_menu_is_placed_against_the_usable_area_not_the_window() {
        let ltr = inset_usable(LayoutDirection::LeftToRight);
        let at = |point, size| {
            placed(
                OverlayPlacement::AtPointer(point),
                size,
                Rect::ZERO,
                inset_viewport(),
                LayoutDirection::LeftToRight,
            )
        };
        let menu = Size::new(300.0, 100.0);

        let trailing = at(Point::new(700.0, 200.0), menu);
        assert_eq!(trailing.x, 460.0, "clamped to the usable trailing edge");
        assert_eq!(trailing.right(), ltr.right());

        let leading = at(Point::new(20.0, 200.0), menu);
        assert_eq!(leading.x, ltr.x, "clamped to the usable leading edge");

        // 80 dp of menu fits in the window below y = 500 and does not fit in
        // the usable area, so this opens upward.
        let flipped = at(Point::new(100.0, 500.0), Size::new(300.0, 80.0));
        assert_eq!(flipped.y, 420.0, "opened above the pointer");

        let pinned = at(Point::new(100.0, 500.0), Size::new(300.0, 480.0));
        assert_eq!(pinned.y, ltr.y, "flipped and pinned below the notch");
    }

    /// `TrailingEdge` — the submenu — asks whether it fits on the trailing
    /// side of its parent, flips to the leading side when it does not, and
    /// clamps y at both ends. All three questions are about the usable area.
    ///
    /// The LTR case is a submenu with 238 dp of window to its right and 198 of
    /// usable area: the window says it fits, the rounded corner says it does
    /// not. The RTL case is the transpose — the same submenu opening leftward,
    /// ruled out by the inset on *that* edge.
    #[test]
    fn a_submenu_flips_and_clamps_against_the_usable_area_not_the_window() {
        let ltr = inset_usable(LayoutDirection::LeftToRight);
        let submenu = Size::new(200.0, 100.0);
        let at = |anchor, direction| {
            placed(
                OverlayPlacement::TrailingEdge,
                submenu,
                anchor,
                inset_viewport(),
                direction,
            )
        };

        let flipped = at(
            Rect::new(500.0, 200.0, 60.0, 24.0),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(flipped.x, 298.0, "opened to the leading side instead");

        let low = at(
            Rect::new(500.0, 520.0, 60.0, 24.0),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(low.y, 470.0, "pulled up to sit above the home indicator");
        assert_eq!(low.bottom(), ltr.bottom());

        let high = at(
            Rect::new(500.0, 10.0, 60.0, 24.0),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(high.y, ltr.y, "and pushed down below the notch");

        let mirrored = at(
            Rect::new(230.0, 200.0, 60.0, 24.0),
            LayoutDirection::RightToLeft,
        );
        assert_eq!(
            mirrored.x, 292.0,
            "in RTL the flip is to the right, because 28 is under the rounded \
             corner: RTL puts `safe_area.trailing` (40 dp) on the left, and \
             the 60 dp sensor housing on the right"
        );
    }

    /// A coarse pointer's menu clears the contact patch **and** stays inside
    /// the usable area.
    ///
    /// The inline-start quadrant this placement prefers would start at x = 26
    /// here — inside the window, underneath the sensor housing. The area is
    /// what rules it out, so the menu takes the inline-end quadrant instead
    /// and the finger still sees all of it.
    #[test]
    fn a_coarse_menu_avoids_the_contact_without_leaving_the_usable_area() {
        let ltr = inset_usable(LayoutDirection::LeftToRight);
        let avoid = Rect::new(250.0, 200.0, 24.0, 24.0);
        let bounds = placed(
            OverlayPlacement::AtPointerAvoiding {
                point: avoid.center(),
                avoid,
            },
            Size::new(220.0, 260.0),
            Rect::ZERO,
            inset_viewport(),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(bounds.x, 278.0, "the inline-end quadrant, past the contact");
        assert!(
            bounds.x >= ltr.x && bounds.right() <= ltr.right(),
            "inside the usable area: {bounds:?}"
        );
        assert!(
            !overlaps(bounds, avoid),
            "and still clear of the finger: {bounds:?}"
        );
    }

    /// The selection toolbar flips below when the selection is against the top
    /// of the **usable** area — a line under the notch has nothing above it,
    /// even though the window says there are 60 dp there — and sacrifices the
    /// edge the line ends at when it cannot be centred inside that area.
    #[test]
    fn a_selection_toolbar_is_placed_against_the_usable_area_not_the_window() {
        let toolbar = Size::new(200.0, 40.0);
        let at = |selection, direction| {
            placed(
                OverlayPlacement::AboveSelection { selection },
                toolbar,
                Rect::ZERO,
                inset_viewport(),
                direction,
            )
        };

        let flipped = at(
            Rect::new(300.0, 60.0, 100.0, 20.0),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(flipped.y, 88.0, "below the selection, clear of the notch");

        let ltr = inset_usable(LayoutDirection::LeftToRight);
        let start = at(
            Rect::new(70.0, 300.0, 20.0, 20.0),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(start.x, ltr.x, "LTR keeps the usable left edge");

        let rtl = inset_usable(LayoutDirection::RightToLeft);
        let end = at(
            Rect::new(720.0, 300.0, 20.0, 20.0),
            LayoutDirection::RightToLeft,
        );
        assert_eq!(end.right(), rtl.right(), "RTL keeps the usable right edge");
    }

    /// A modal centres inside the usable area on **both** axes, and one bigger
    /// than that area is shrunk to it rather than to the window — the overlay
    /// pass lays content out at the bounds it is given, so the difference is a
    /// dialog that scrolls versus one whose last rows are behind the home
    /// indicator.
    #[test]
    fn a_centered_modal_is_centred_and_shrunk_inside_the_usable_area() {
        let ltr = inset_usable(LayoutDirection::LeftToRight);
        let centred = placed(
            OverlayPlacement::Centered,
            Size::new(200.0, 100.0),
            Rect::ZERO,
            inset_viewport(),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(centred.x, 310.0, "centred between the side insets");
        assert_eq!(centred.y, 255.0, "and between the notch and the indicator");

        let oversized = placed(
            OverlayPlacement::Centered,
            Size::new(900.0, 700.0),
            Rect::ZERO,
            inset_viewport(),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(oversized.size(), ltr.size(), "shrunk to the usable area");
        assert_eq!((oversized.x, oversized.y), (ltr.x, ltr.y));
    }

    /// A snackbar keeps its 24 dp margin from the bottom of the usable area,
    /// which on a phone is the top of the home indicator, not the bottom of
    /// the window.
    #[test]
    fn a_snackbar_sits_above_the_home_indicator() {
        let bounds = placed(
            OverlayPlacement::BottomCenter,
            Size::new(300.0, 60.0),
            Rect::ZERO,
            inset_viewport(),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(bounds.y, 486.0, "24 dp above the usable bottom");
        assert_eq!(bounds.x, 260.0, "centred between the side insets");
    }

    /// A toast's corner is a corner of the usable area. At zero margin the
    /// window's corner is under the rounded glass, which is exactly where a
    /// dismiss button must not be.
    #[test]
    fn a_toast_corner_is_a_corner_of_the_usable_area() {
        let ltr = inset_usable(LayoutDirection::LeftToRight);
        let corner = |corner| {
            placed(
                OverlayPlacement::ViewportCorner {
                    corner,
                    margin: Vec2::ZERO,
                },
                Size::new(100.0, 50.0),
                Rect::ZERO,
                inset_viewport(),
                LayoutDirection::LeftToRight,
            )
        };

        let top_leading = corner(Corner::TopLeading);
        assert_eq!((top_leading.x, top_leading.y), (ltr.x, ltr.y));

        let bottom_trailing = corner(Corner::BottomTrailing);
        assert_eq!(
            (bottom_trailing.right(), bottom_trailing.bottom()),
            (ltr.right(), ltr.bottom())
        );
    }

    /// A toast bigger than the usable area is shrunk to it.
    ///
    /// [`a_toast_corner_is_a_corner_of_the_usable_area`] pins the corner but
    /// never asks for the shrink: its toast is 100 x 50 with hundreds of dp to
    /// spare, so the `min` against the area is the identity there and would go
    /// on being the identity if it were deleted. Here the toast is 760 x 560 in
    /// a 700 x 530 usable area, and measured against the *window* it survives
    /// at full size — 60 dp past the rounded corner, 30 dp into the home
    /// indicator, with the dismiss button somewhere under the glass.
    ///
    /// The shrink is real, not a clip: the overlay pass lays content out at
    /// `SizeProposal::exact` of the bounds it is handed (see
    /// `WidgetTree::layout`), so this is a toast that wraps rather than one
    /// drawn off the edge of what a person can touch.
    #[test]
    fn an_oversized_toast_is_shrunk_to_the_usable_area() {
        let ltr = inset_usable(LayoutDirection::LeftToRight);
        let corner = |corner| {
            placed(
                OverlayPlacement::ViewportCorner {
                    corner,
                    margin: Vec2::ZERO,
                },
                Size::new(760.0, 560.0),
                Rect::ZERO,
                inset_viewport(),
                LayoutDirection::LeftToRight,
            )
        };

        // Corner-independent, because it is the size that is capped and a size
        // does not know which corner it was aligned to.
        for c in [
            Corner::TopLeading,
            Corner::TopTrailing,
            Corner::BottomLeading,
            Corner::BottomTrailing,
        ] {
            assert_eq!(
                corner(c).size(),
                ltr.size(),
                "{c:?}: shrunk to the usable area, not left at 760 x 560"
            );
        }

        // And at the leading corner, where a panel that fills the area has
        // nowhere left to slide, all four edges land on the area's own.
        let top_leading = corner(Corner::TopLeading);
        assert_eq!((top_leading.x, top_leading.y), (ltr.x, ltr.y));
        assert_eq!(
            (top_leading.right(), top_leading.bottom()),
            (ltr.right(), ltr.bottom()),
            "clear of the rounded corner and of the home indicator"
        );
    }

    /// The contact-avoiding fallback — the one branch that gives up on clearing
    /// the finger — still clamps **both** axes.
    ///
    /// Four strips are tried before it, and here none fits: the panel is very
    /// nearly the whole usable area, so no placement exists that does not
    /// overlap the contact. Accepting the overlap is the documented cost. What
    /// the last line is *for* is the rest of the promise — that the panel is
    /// still somewhere a person can see.
    ///
    /// Drop either clamp and what lands is the raw quadrant coordinate that fed
    /// it: `x` becomes the inline-start start, `380 - 4 - 700 = -324`, a menu
    /// with 324 dp off the left of the window and its leading half — where the
    /// first row is — gone; `y` becomes the below-the-contact start, 324,
    /// putting the last 244 dp of it below the bottom of the window. Neither is
    /// caught by the four-strip cases, which never reach the fallback, nor by
    /// [`a_coarse_menu_avoids_the_contact_without_leaving_the_usable_area`],
    /// whose menu is placed by `fits_x` and never touches a clamp.
    #[test]
    fn a_menu_too_big_to_clear_the_contact_is_still_clamped_on_both_axes() {
        let ltr = inset_usable(LayoutDirection::LeftToRight);
        let avoid = Rect::new(380.0, 280.0, 40.0, 40.0);
        let bounds = placed(
            OverlayPlacement::AtPointerAvoiding {
                point: avoid.center(),
                avoid,
            },
            // 700 x 520 against a 700 x 530 usable area: it fits on neither
            // side of the contact, on either axis.
            Size::new(700.0, 520.0),
            Rect::ZERO,
            inset_viewport(),
            LayoutDirection::LeftToRight,
        );

        assert_eq!(
            bounds.x, ltr.x,
            "clamped to the usable leading edge, not left at -324"
        );
        assert_eq!(
            bounds.y, 50.0,
            "and lifted off the usable bottom, not left at 324"
        );
        assert_eq!(
            (bounds.right(), bounds.bottom()),
            (ltr.right(), ltr.bottom())
        );
        assert!(
            bounds.x >= ltr.x
                && bounds.right() <= ltr.right()
                && bounds.y >= ltr.y
                && bounds.bottom() <= ltr.bottom(),
            "wholly inside the usable area: {bounds:?}"
        );

        // The overlap is the price, and this is the only branch that pays it.
        assert!(
            overlaps(bounds, avoid),
            "at this size there is nothing that clears the contact: {bounds:?}"
        );
    }

    /// `BelowPreferred` measures the room it **shrinks into** against the
    /// usable area, not only the side it picks.
    ///
    /// [`a_below_preferred_panel_is_placed_against_the_usable_area_not_the_window`]
    /// covers the choice, and every panel in it then fits on the side chosen —
    /// so the height that comes out is the height that was asked for, and the
    /// rectangle the shrink measured against never shows. These two do not fit
    /// on the side they pick, which makes `height` a report of the room itself.
    ///
    /// Flipping up: a 400 dp list over an anchor at y = 400 has 356 dp of
    /// usable room above and 396 dp of window. It is pinned under the notch at
    /// 40 and cut to 356. Measured against the window it starts at 0 — behind
    /// the notch, first row unreadable — and is drawn 40 dp taller than the
    /// room it has.
    ///
    /// Dropping down: a 500 dp list under an anchor at y = 100 takes the
    /// roomier side and is cut to the 442 dp above the home indicator, not to
    /// the 472 dp above the bottom of the glass.
    #[test]
    fn a_below_preferred_panel_shrinks_to_the_usable_room_not_the_window() {
        let ltr = inset_usable(LayoutDirection::LeftToRight);

        let flipped_up = placed(
            OverlayPlacement::BelowPreferred,
            Size::new(300.0, 400.0),
            Rect::new(100.0, 400.0, 60.0, 24.0),
            inset_viewport(),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(flipped_up.y, ltr.y, "pinned below the notch, not to y = 0");
        assert_eq!(
            flipped_up.height, 356.0,
            "and cut to the room between the notch and the anchor"
        );
        assert_eq!(
            flipped_up.bottom(),
            396.0,
            "which still leaves the anchor visible"
        );

        let dropped_down = placed(
            OverlayPlacement::BelowPreferred,
            Size::new(500.0, 500.0),
            Rect::new(100.0, 100.0, 60.0, 24.0),
            inset_viewport(),
            LayoutDirection::LeftToRight,
        );
        assert_eq!(
            dropped_down.y, 128.0,
            "below the anchor, which is the roomier side"
        );
        assert_eq!(
            dropped_down.height, 442.0,
            "cut at the home indicator, not at the bottom of the window"
        );
        assert_eq!(dropped_down.bottom(), ltr.bottom());
    }

    /// A drop-down is never narrower than the control it drops out of.
    ///
    /// [`at_least_anchor_width`] is the rule and it governs all three
    /// anchor-aligned placements. Nothing else in this file notices it: every
    /// other anchor-aligned case above gives its panel a width at least the
    /// anchor's — a 120 dp list off an 80 dp field, a 300 dp panel off a 60 dp
    /// control, a 300 dp list off a 300 dp combo box — so the floor sits at or
    /// under the content and `max` is the identity. Here it is the other way
    /// round — an 80 dp list under a 240 dp combo box comes out 240 wide and
    /// flush at both of the field's edges, which is what makes it read as the
    /// field opening rather than as a chit parked under its leading third.
    #[test]
    fn a_short_list_is_widened_to_the_control_that_opened_it() {
        let combo = Rect::new(100.0, 200.0, 240.0, 24.0);
        let window = (800.0, 600.0);
        let at = |placement| {
            placed(
                placement,
                Size::new(80.0, 120.0),
                combo,
                window,
                LayoutDirection::LeftToRight,
            )
        };

        for placement in [
            OverlayPlacement::Below,
            OverlayPlacement::Above,
            OverlayPlacement::BelowPreferred,
        ] {
            let bounds = at(placement.clone());
            assert_eq!(
                bounds.width, combo.width,
                "{placement:?}: widened to the control, not left at 80"
            );
            assert_eq!(
                bounds.x, combo.x,
                "{placement:?}: and still on the control's leading edge"
            );
            assert_eq!(
                bounds.right(),
                combo.right(),
                "{placement:?}: so it is flush at both"
            );
        }

        // A floor, not a width: content wider than its anchor keeps its own.
        let wide = placed(
            OverlayPlacement::Below,
            Size::new(420.0, 120.0),
            combo,
            window,
            LayoutDirection::LeftToRight,
        );
        assert_eq!(wide.width, 420.0, "wider than the anchor, so untouched");
    }

    // -----------------------------------------------------------------
    // Branches whose neighbours' assertions cannot see them
    //
    // Each of the following is a live mechanism that survived its own
    // deletion with the rest of this file green — a direction branch two
    // arms of which coincide at every existing fixture, a preference order
    // never asked to choose, a clamp on a path no case reaches, a shrink on
    // the one of three placements nobody sized past its area.
    // -----------------------------------------------------------------

    /// An RTL panel hangs off the anchor's physical **right** edge, and that
    /// is a choice something has to make — the existing RTL row cannot see it
    /// being made.
    ///
    /// [`a_below_panel_is_clamped_into_the_usable_area_not_the_window`] does
    /// place a panel under RTL, but its trailing clamp maps both arms of
    /// [`leading_aligned_x_in`] onto the same number, so replacing the RTL
    /// branch with the LTR one leaves it green. Here they diverge: a 300 dp
    /// panel under a 60 dp control at x = 100 hangs its right edge on the
    /// control's, which starts it at -140 and pins it to the leading edge at
    /// 0. Take the LTR arm and it starts at 100 instead — running from the
    /// control's *trailing* edge in Arabic, with 240 dp of it out past the
    /// far side of the thing it belongs to.
    #[test]
    fn an_rtl_panel_hangs_off_the_anchors_right_edge_not_its_left() {
        let anchor = Rect::new(100.0, 200.0, 60.0, 24.0);
        let panel = Size::new(300.0, 120.0);
        let at = |direction| {
            placed(
                OverlayPlacement::Below,
                panel,
                anchor,
                (800.0, 600.0),
                direction,
            )
        };

        assert_eq!(
            at(LayoutDirection::RightToLeft).x,
            0.0,
            "aligned to the anchor's right edge (100 + 60 - 300 = -140), then \
             pinned to the leading edge"
        );
        // Both arms fit here, so nothing but the branch decides between them.
        assert_eq!(
            at(LayoutDirection::LeftToRight).x,
            anchor.x,
            "which is 100 dp from where the LTR arm puts it"
        );
    }

    /// An RTL submenu opens to the **left** of the row that raised it, and the
    /// existing RTL submenu row cannot see that either.
    ///
    /// [`a_submenu_flips_and_clamps_against_the_usable_area_not_the_window`]
    /// exercises RTL-specific arithmetic — its `area.x` bound is load-bearing
    /// — but at that fixture the LTR arm's rightward `x` happens to land on
    /// the same number as the RTL arm's leftward one, so forcing the LTR
    /// branch leaves it green. Here the two are 264 dp apart, and both fit, so
    /// the answer is the branch and nothing else.
    #[test]
    fn an_rtl_submenu_opens_to_the_left_of_its_parent_row() {
        let anchor = Rect::new(400.0, 200.0, 60.0, 24.0);
        let submenu = Size::new(200.0, 100.0);
        let at = |direction| {
            placed(
                OverlayPlacement::TrailingEdge,
                submenu,
                anchor,
                (800.0, 600.0),
                direction,
            )
        };

        assert_eq!(
            at(LayoutDirection::RightToLeft).x,
            198.0,
            "trailing is leftward in Arabic: 400 - 200 - 2"
        );
        assert_eq!(
            at(LayoutDirection::LeftToRight).x,
            462.0,
            "and rightward in English: 400 + 60 + 2"
        );
    }

    /// A menu that fits beside the contact takes the room **below** the finger
    /// before the room above it.
    ///
    /// [`avoiding_bounds`]' own doc gives the order — 'below it, then above
    /// it' — and nothing exercises it: every other avoidance case either fits
    /// below on the first ask, so the second candidate never runs, or is too
    /// large for any strip and goes to the fallback, so the inner branch never
    /// runs. A 100 x 100 menu beside a 40 dp contact at y = 300 fits both
    /// ways, 344 below and 196 above. Swapped, it opens *above* the contact —
    /// on the side the hand is coming from, which is the arrangement this
    /// placement exists to avoid.
    ///
    /// The second case is the branch's own clamp. A 100 x 560 menu fits on
    /// neither side of the contact, and the raw below-the-contact `y` it falls
    /// back to would be 344, putting its last 304 dp past the bottom of the
    /// window.
    #[test]
    fn a_menu_beside_the_contact_prefers_the_room_below_it() {
        let avoid = Rect::new(400.0, 300.0, 40.0, 40.0);
        let beside = |size| {
            placed(
                OverlayPlacement::AtPointerAvoiding {
                    point: avoid.center(),
                    avoid,
                },
                size,
                Rect::ZERO,
                (800.0, 600.0),
                LayoutDirection::LeftToRight,
            )
        };

        let fits_either = beside(Size::new(100.0, 100.0));
        assert_eq!(
            fits_either.x, 296.0,
            "inline-start of the contact, which is what puts us in this branch"
        );
        assert_eq!(
            fits_either.y, 344.0,
            "below the contact, not at 196 above it"
        );

        let fits_neither = beside(Size::new(100.0, 560.0));
        assert_eq!(fits_neither.x, 296.0, "still inline-start of the contact");
        assert_eq!(
            fits_neither.y, 40.0,
            "lifted until it fits, not left at 344 with its foot at 904"
        );
        assert_eq!(fits_neither.bottom(), 600.0);
    }

    /// A menu too wide to sit beside the contact drops **below** it, and is
    /// clamped onto the screen on the axis the strip did not decide.
    ///
    /// This is the same pair of promises as the branch above, one branch
    /// later, and equally unasked: a 700 dp menu clears neither side of a
    /// contact at x = 400 in an 800 dp window, so the horizontal strips are
    /// out and the vertical ones answer. The order is the same one
    /// [`avoiding_bounds`] documents — below the finger before above it — and
    /// the `x` that comes with it is the inline-start start of -304, which is
    /// 304 dp off the leading edge of the window with the whole leading half
    /// of the menu, where the first row is, gone.
    ///
    /// [`a_menu_too_big_to_clear_the_contact_is_still_clamped_on_both_axes`]
    /// covers the same clamp in the final fallback; this one is the branch
    /// before it, which no case reached.
    #[test]
    fn a_menu_too_wide_to_sit_beside_the_contact_drops_below_it_and_stays_on_screen() {
        let avoid = Rect::new(400.0, 300.0, 40.0, 40.0);
        let bounds = placed(
            OverlayPlacement::AtPointerAvoiding {
                point: avoid.center(),
                avoid,
            },
            Size::new(700.0, 100.0),
            Rect::ZERO,
            (800.0, 600.0),
            LayoutDirection::LeftToRight,
        );

        assert_eq!(bounds.y, 344.0, "below the contact, not at 196 above it");
        assert_eq!(bounds.x, 0.0, "clamped onto the screen, not left at -304");
        assert_eq!(bounds.right(), 700.0);
        assert!(
            !overlaps(bounds, avoid),
            "and the clamp did not cost the clearance: {bounds:?}"
        );
    }

    /// A selection toolbar that fits neither above the selection nor below it
    /// is pinned to the top of the area, not left off it.
    ///
    /// The two existing flip cases each have somewhere to go — above at
    /// [`a_selection_toolbar_floats_centred_above_the_selection`], below at
    /// [`a_selection_toolbar_flips_below_at_the_top_of_the_window`] — so the
    /// last line of [`above_selection_bounds`], which is what runs when
    /// neither is available, has never been reached. A 580 dp toolbar over a
    /// selection on the first line has 2 dp above it and 562 below: its
    /// unfloored `y` is -578, which is the whole toolbar bar two rows above
    /// the top of the screen.
    ///
    /// The inset case says which rectangle the floor is: under a 40 dp notch
    /// the answer is 40, not 0.
    #[test]
    fn a_selection_toolbar_that_fits_on_neither_side_is_pinned_to_the_top() {
        let toolbar = Size::new(200.0, 580.0);
        let over = |selection, viewport: OverlayViewport| {
            placed(
                OverlayPlacement::AboveSelection { selection },
                toolbar,
                Rect::ZERO,
                viewport,
                LayoutDirection::LeftToRight,
            )
        };

        let window = over(
            Rect::new(300.0, 10.0, 100.0, 20.0),
            OverlayViewport::from((800.0, 600.0)),
        );
        assert_eq!(window.y, 0.0, "pinned to the top, not left at -578");

        let ltr = inset_usable(LayoutDirection::LeftToRight);
        let inset = over(Rect::new(300.0, 60.0, 100.0, 20.0), inset_viewport());
        assert_eq!(
            inset.y, ltr.y,
            "and the top it is pinned to is the usable one, under the notch"
        );
    }

    /// A snackbar bigger than the usable area is shrunk to it, and never
    /// starts above its top edge.
    ///
    /// [`a_snackbar_sits_above_the_home_indicator`] pins the 24 dp bottom
    /// margin but asks for neither: its snackbar is 300 x 60 with hundreds of
    /// dp to spare, so the `min` against the area and the `max` against its
    /// top are both the identity there and would go on being the identity if
    /// they were deleted. `BottomCenter` is the third of the three placements
    /// that shrink — [`an_oversized_toast_is_shrunk_to_the_usable_area`]
    /// covers `ViewportCorner`'s copy and
    /// [`a_centered_modal_is_centred_and_shrunk_inside_the_usable_area`]
    /// covers `Centered`'s — and it was the one left unasked.
    ///
    /// Measured against the *window*, a 760 x 560 snackbar survives at full
    /// size, 60 dp past the rounded corner and 30 into the home indicator, and
    /// the 24 dp margin under it then puts its top at -14: a notification whose
    /// first line is 14 dp above the top of the window, and 54 above the top
    /// of the area it was supposed to stay inside.
    ///
    /// Its **x** is the same story on the other axis. Centring subtracts the
    /// content's width from the area's, and a snackbar wider than the area
    /// makes that difference negative: 760 in a usable width of 700 halves to
    /// -30, so without the floor the shrunk snackbar starts at 30 — 30 dp back
    /// under the sensor housing at one end, and 30 dp short of the rounded
    /// corner at the other. The floor puts it flush at the usable leading
    /// edge, which for something exactly as wide as the area is the only
    /// position that fills it.
    ///
    /// The shrink is real, not a clip — the overlay pass lays content out at
    /// `SizeProposal::exact` of the bounds it is handed (see
    /// `WidgetTree::layout`) — so this is a snackbar that wraps rather than
    /// one drawn off the glass.
    #[test]
    fn an_oversized_snackbar_is_shrunk_to_the_usable_area() {
        let ltr = inset_usable(LayoutDirection::LeftToRight);
        let bounds = placed(
            OverlayPlacement::BottomCenter,
            Size::new(760.0, 560.0),
            Rect::ZERO,
            inset_viewport(),
            LayoutDirection::LeftToRight,
        );

        assert_eq!(
            (bounds.width, bounds.height),
            (ltr.width, ltr.height),
            "shrunk to the usable area, not left at 760 x 560"
        );
        assert_eq!(
            bounds.y, ltr.y,
            "and floored at the top of that area, not lifted to -14"
        );
        assert_eq!(
            bounds.x, ltr.x,
            "and flush against its leading edge, not pulled back to 30"
        );
        assert_eq!(
            bounds.right(),
            ltr.right(),
            "which is also what puts its far edge on the rounded corner"
        );
        assert!(
            bounds.y >= ltr.y && bounds.bottom() <= ltr.bottom(),
            "wholly inside the usable area: {bounds:?}"
        );
    }
}
