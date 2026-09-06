// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Overlay placement geometry: turning an anchor rect, a viewport and a
//! [`OverlayPlacement`] into the overlay's on-screen bounds — leading-edge
//! alignment, the flip-when-it-does-not-fit fallbacks and the viewport clamps.

use super::*;

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
/// `max(0.0)` last, so a viewport narrower than the overlay pins it to the
/// leading edge and clips at the trailing one, rather than pushing its start
/// off-screen where the first thing the reader needs would be the part lost.
fn leading_aligned_x(anchor: Rect, actual_width: f32, vw: f32, rtl: bool) -> f32 {
    let leading = if rtl {
        anchor.x + anchor.width - actual_width
    } else {
        anchor.x
    };
    leading.min(vw - actual_width).max(0.0)
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
        viewport: (f32, f32),
        layout_direction: LayoutDirection,
    ) {
        let (vw, vh) = viewport;
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
                    let x = leading_aligned_x(anchor, actual_width, vw, rtl);
                    Rect::new(
                        x,
                        anchor.y + anchor.height + 4.0,
                        actual_width,
                        content_size.height,
                    )
                }
                OverlayPlacement::Above => {
                    let actual_width = content_size.width.max(anchor.width);
                    let x = leading_aligned_x(anchor, actual_width, vw, rtl);
                    Rect::new(
                        x,
                        anchor.y - content_size.height - 4.0,
                        actual_width,
                        content_size.height,
                    )
                }
                OverlayPlacement::TrailingEdge => {
                    // In LTR trailing is to the right; in RTL trailing is to the left.
                    let x = if rtl {
                        let x_left = anchor.x - content_size.width - 2.0;
                        if x_left >= 0.0 {
                            x_left
                        } else {
                            // Fallback: open to the leading side (right in RTL)
                            anchor.x + anchor.width + 2.0
                        }
                    } else {
                        let x_right = anchor.x + anchor.width + 2.0;
                        if x_right + content_size.width <= vw {
                            x_right
                        } else {
                            // Fallback: open to the leading side (left in LTR)
                            anchor.x - content_size.width - 2.0
                        }
                    };
                    let y = anchor.y.min(vh - content_size.height).max(0.0);
                    Rect::new(x, y, content_size.width, content_size.height)
                }
                OverlayPlacement::AtPointer(point) => {
                    // Clamp to viewport so menus don't overflow off-screen
                    let x = point.x.min(vw - content_size.width).max(0.0);
                    let y = if point.y + content_size.height <= vh {
                        point.y
                    } else {
                        // Not enough space below pointer — open above
                        (point.y - content_size.height).max(0.0)
                    };
                    Rect::new(x, y, content_size.width, content_size.height)
                }
                OverlayPlacement::NearAnchor { offset } => {
                    // Prefer below the anchor at `offset` + 4 px.
                    // Flip above when the content would otherwise spill
                    // past the viewport bottom — same pattern as
                    // `BelowPreferred`. Without this, a tooltip whose
                    // anchor sits near the window edge gets clipped by
                    // the surface bounds (overlays paint unclipped, but
                    // the window itself still bounds the framebuffer).
                    let below_y = anchor.y + anchor.height + offset.y + 4.0;
                    let fits_below = below_y + content_size.height <= vh;
                    let y = if fits_below {
                        below_y
                    } else {
                        // Symmetric offset above: same gap as below.
                        let above_y = anchor.y - content_size.height - offset.y - 4.0;
                        above_y.max(0.0)
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
                    let x = unclamped_x.min(vw - content_size.width).max(0.0);
                    Rect::new(x, y, content_size.width, content_size.height)
                }
                OverlayPlacement::Centered => Rect::new(
                    ((vw - content_size.width) / 2.0).max(0.0),
                    ((vh - content_size.height) / 2.0).max(0.0),
                    content_size.width.min(vw),
                    content_size.height.min(vh),
                ),
                OverlayPlacement::BottomCenter => Rect::new(
                    ((vw - content_size.width) / 2.0).max(0.0),
                    (vh - content_size.height - 24.0).max(0.0),
                    content_size.width.min(vw),
                    content_size.height.min(vh),
                ),
                OverlayPlacement::BelowPreferred => {
                    let below_y = anchor.y + anchor.height + 4.0;
                    let fits_below = below_y + content_size.height <= vh;
                    let y = if fits_below {
                        below_y
                    } else {
                        anchor.y - content_size.height - 4.0
                    };
                    let actual_width = content_size.width.max(anchor.width);
                    // Align leading edges, same logic as Below.
                    let x = if rtl {
                        (anchor.x + anchor.width - actual_width)
                            .min(vw - actual_width)
                            .max(0.0)
                    } else {
                        anchor.x.min(vw - actual_width).max(0.0)
                    };
                    Rect::new(x, y, actual_width, content_size.height)
                }
                OverlayPlacement::ViewportCorner { corner, margin } => {
                    let (x, y) = corner.resolve(
                        (content_size.width, content_size.height),
                        (vw, vh),
                        (margin.x, margin.y),
                        rtl,
                    );
                    Rect::new(
                        x,
                        y,
                        content_size.width.min(vw),
                        content_size.height.min(vh),
                    )
                }
                OverlayPlacement::FullViewport => Rect::new(0.0, 0.0, vw, vh),
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
