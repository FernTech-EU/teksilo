// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A thin invisible widget that forwards a window resize gesture to the
//! platform host when the user presses the primary button inside it. Used
//! to build a 6-px resize frame around a borderless window on Wayland.
//!
//! This is the frame complement to [`crate::title_bar::DragRegion`]: drag
//! moves the window, resize strips drag the window edges. On platforms
//! that don't expose `Window::drag_resize_window` (notably winit's macOS
//! backend), [`PlatformTitleBarHost::begin_resize`] returns
//! `PlatformError::Unsupported` and the strip becomes a silent no-op —
//! macOS handles edge resize via its own native chrome.
//!
//! ## Reaching a 6 dp edge with a finger
//!
//! The strip's thickness is fixed at every density — it is an overlay drawn
//! *over* the window's own edge, so widening it would eat into content rather
//! than into empty space, and the frame would start swallowing presses meant
//! for the app.
//!
//! The grab is [`Widget::hit_outset`] instead: for a direct pointer the strip
//! is offered the press against bounds inflated inward to the density's target
//! size (24 dp Compact, 44 dp Touch), and the arena's outset pre-pass runs
//! before the ordinary reverse-sibling walk, so the widened band beats the
//! content underneath. An edge strip inflates only across its thickness; a
//! corner cell inflates on all four sides, since both of its axes are the
//! diagonal grab. For a precise pointer the outset is zero and a mouse press
//! resolves exactly where it always did.
//!
//! Two strips whose bands overlap are settled by distance to their own
//! uninflated rectangles, not by sibling order, so the midpoint between two
//! adjacent edges belongs to the nearer one.
//!
//! ## One resize per gesture
//!
//! [`PlatformTitleBarHost::begin_resize`] hands the window to the compositor
//! for the rest of the gesture, so it must be asked once. The press that asks
//! is gated on the pointer being the **primary** one: a second finger landing
//! on the frame while a resize is already running would ask for a second
//! interactive resize of the same window, which on Wayland means a second
//! `xdg_toplevel::resize` against a live one. A mouse is always primary, so the
//! gate never fires for one.

use std::rc::Rc;

use teksilo_canvas::{EdgeInsets, Rect, Size, SizeProposal};
use teksilo_core::TouchAction;
use teksilo_core::event::{EventResponse, PointerButton, WidgetEvent};
use teksilo_core::styles::density::dp;
use teksilo_core::widget::{CursorIcon, LayoutContext, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::{PlatformTitleBarHost, ResizeEdge};
use teksilo_tokens::{DragActivation, InputTokens, PointerKind, TargetRole};

/// A single edge of a resize frame. Construct one per side and lay them
/// out around your content (HStack of left + content + right inside a
/// VStack of top + middle + bottom is the conventional shape — see the
/// title bar demo for an example).
pub struct ResizeStrip {
    host: Rc<dyn PlatformTitleBarHost>,
    edge: ResizeEdge,
    width: f32,
    height: f32,
}

impl std::fmt::Debug for ResizeStrip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResizeStrip")
            .field("edge", &self.edge)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl ResizeStrip {
    /// Build a horizontal (top / bottom) strip of the given height. The
    /// width is unconstrained — the strip claims whatever its parent
    /// container offers, so it can stretch across the full window width.
    pub fn horizontal(
        host: Rc<dyn PlatformTitleBarHost>,
        edge: ResizeEdge,
        thickness: f32,
    ) -> Self {
        debug_assert!(matches!(edge, ResizeEdge::Top | ResizeEdge::Bottom));
        Self {
            host,
            edge,
            width: 0.0,
            height: thickness,
        }
    }

    /// Build a vertical (left / right) strip of the given width. The
    /// height is unconstrained.
    pub fn vertical(host: Rc<dyn PlatformTitleBarHost>, edge: ResizeEdge, thickness: f32) -> Self {
        debug_assert!(matches!(edge, ResizeEdge::Left | ResizeEdge::Right));
        Self {
            host,
            edge,
            width: thickness,
            height: 0.0,
        }
    }

    /// Build a square corner cell of the given size. The corner handles a
    /// diagonal resize gesture (e.g. `TopLeft` does NW/SE resize). Should
    /// be placed *on top of* the edge strips at the four corners so the
    /// framework's hit-test routes the click to the corner rather than
    /// the adjacent edge.
    pub fn corner(host: Rc<dyn PlatformTitleBarHost>, edge: ResizeEdge, size: f32) -> Self {
        debug_assert!(matches!(
            edge,
            ResizeEdge::TopLeft
                | ResizeEdge::TopRight
                | ResizeEdge::BottomLeft
                | ResizeEdge::BottomRight
        ));
        Self {
            host,
            edge,
            width: size,
            height: size,
        }
    }
}

/// Inward hit inflation that lifts a `visual`-thick window-frame grip to the
/// density's target size, for a **direct** pointer only.
///
/// Unlike the splitter's and the dock divider's gutters — which lie *between*
/// two panes and can be approached from either side, so their band is split
/// evenly — a window-frame strip sits on the outer boundary: everything outside
/// it is off-window. The arena clips an outset to the parent it descends
/// through, so inflating by the whole shortfall on both sides yields exactly
/// `target_size` of *reachable* band, all of it inward, and the outward half
/// is discarded by the clip rather than being wasted band.
///
/// Only a hit dimension is routed through [`dp`] here: the strip's painted
/// thickness is the same at every density, which is what keeps the frame from
/// eating into content — see `docs/density-inventory.md`.
fn grab_outset(visual: f32, kind: PointerKind, tokens: &InputTokens) -> f32 {
    if !kind.is_direct() || !visual.is_finite() || visual <= 0.0 {
        return 0.0;
    }
    (dp(visual, TargetRole::Target, tokens) - visual).max(0.0)
}

fn cursor_for_edge(edge: ResizeEdge) -> CursorIcon {
    match edge {
        ResizeEdge::Top | ResizeEdge::Bottom => CursorIcon::RowResize,
        ResizeEdge::Left | ResizeEdge::Right => CursorIcon::ColResize,
        // NW/SE diagonal — corners on the top-left and bottom-right.
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorIcon::NwseResize,
        // NE/SW diagonal — corners on the top-right and bottom-left.
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorIcon::NeswResize,
    }
}

impl Widget for ResizeStrip {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        let host = self.host.clone();
        let edge = self.edge;

        let handlers = HandlerSet::new()
            .cursor(cursor_for_edge(edge))
            // The edge drag *is* the interaction: forbid every default touch
            // behaviour in this subtree and arm at the slop rather than after a
            // long press. Direct-pointer policy only — a mouse reads neither.
            .touch_action(TouchAction::NONE)
            .drag_activation(DragActivation::Immediate)
            .on_pointer_event(move |evt, ctx| {
                if let WidgetEvent::PointerDown {
                    button: PointerButton::Primary,
                    ..
                } = evt
                {
                    // One interactive resize per gesture. `begin_resize` hands
                    // the window to the compositor for the rest of the drag, so
                    // a second contact must not ask for a second one against a
                    // live session. A mouse is always primary.
                    if !ctx.pointer().primary {
                        return EventResponse::Ignored;
                    }
                    let _ = host.begin_resize(edge);
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            });

        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // Horizontal strips: claim full proposed width, fixed height.
        // Vertical strips: claim full proposed height, fixed width.
        let w = if self.width > 0.0 {
            self.width
        } else {
            proposal.width.unwrap_or(0.0)
        };
        let h = if self.height > 0.0 {
            self.height
        } else {
            proposal.height.unwrap_or(0.0)
        };
        Size::new(w, h).into()
    }

    fn place_children(
        &self,
        _bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut teksilo_canvas::Canvas, _ctx: &PaintContext) {
        // Invisible.
    }

    /// The grab band.
    ///
    /// An edge strip inflates across its thickness axis only; inflating along
    /// its length would reach past the window's own corner into the adjacent
    /// edge, which the corner cells already serve. A corner cell inflates on
    /// all four sides, because both of its axes are the diagonal grab.
    ///
    /// The insets are symmetric on each inflated axis even though only the
    /// inward half is reachable: the outward half falls outside the frame's own
    /// bounds, and the arena has already tested those before it looks at a
    /// child, so the clip discards it. Symmetry is also what keeps this correct
    /// under RTL, where [`EdgeInsets`]'s reading-order `leading`/`trailing` map
    /// to the opposite screen edges from the [`ResizeEdge`] the strip was built
    /// for.
    fn hit_outset(&self, kind: PointerKind, tokens: &InputTokens) -> EdgeInsets {
        let is_corner = self.width > 0.0 && self.height > 0.0;
        let thickness = if is_corner {
            self.width.min(self.height)
        } else if self.height > 0.0 {
            self.height
        } else {
            self.width
        };
        let out = grab_outset(thickness, kind, tokens);
        if out <= 0.0 {
            return EdgeInsets::ZERO;
        }
        if is_corner {
            EdgeInsets::uniform(out)
        } else if self.height > 0.0 {
            // Horizontal strip: thin vertically.
            EdgeInsets::new(out, 0.0, out, 0.0)
        } else {
            EdgeInsets::new(0.0, out, 0.0, out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_canvas::Point;
    use teksilo_core::{HitRegions, PlatformError};
    use teksilo_tokens::TargetDensity;

    #[derive(Debug)]
    struct NoopHost;

    impl PlatformTitleBarHost for NoopHost {
        fn reserved_leading_inset(&self) -> Size {
            Size::ZERO
        }
        fn reserved_trailing_inset(&self) -> Size {
            Size::ZERO
        }
        fn renders_custom_controls(&self) -> bool {
            true
        }
        fn needs_custom_resize_handles(&self) -> bool {
            true
        }
        fn begin_drag(&self) -> Result<(), PlatformError> {
            Ok(())
        }
        fn begin_resize(&self, _edge: ResizeEdge) -> Result<(), PlatformError> {
            Ok(())
        }
        fn show_window_menu(&self, _at: Point) -> Result<(), PlatformError> {
            Ok(())
        }
        fn update_hit_regions(&self, _regions: &HitRegions) {}
    }

    fn host() -> Rc<dyn PlatformTitleBarHost> {
        Rc::new(NoopHost)
    }

    fn tokens(density: TargetDensity) -> InputTokens {
        InputTokens::for_density(density)
    }

    /// A frame strip inflates by the **whole** shortfall, not half of it: the
    /// outward half falls off the window and is discarded by the parent clip,
    /// so `thickness + inward` is exactly the target size. 6 dp + 18 dp = 24 dp
    /// at Compact; 6 + 38 = 44 at Touch.
    #[test]
    fn an_edge_strip_reaches_the_target_size_inward() {
        for (density, expected) in [
            (TargetDensity::Compact, 18.0_f32),
            (TargetDensity::Comfortable, 26.0),
            (TargetDensity::Touch, 38.0),
        ] {
            let t = tokens(density);
            let strip = ResizeStrip::horizontal(host(), ResizeEdge::Top, 6.0);
            let insets = strip.hit_outset(PointerKind::Touch, &t);
            assert_eq!(insets.top, expected, "{density:?}");
            assert_eq!(insets.bottom, expected, "{density:?}");
            assert_eq!(insets.leading, 0.0, "never along the strip's length");
            assert_eq!(insets.trailing, 0.0, "never along the strip's length");
            assert_eq!(
                6.0 + expected,
                t.target_size,
                "thickness plus the inward half is the target size"
            );
        }
    }

    /// A vertical strip is the transpose: it inflates on the reading axis only.
    #[test]
    fn a_vertical_strip_inflates_across_the_reading_axis() {
        let t = tokens(TargetDensity::Compact);
        let strip = ResizeStrip::vertical(host(), ResizeEdge::Left, 6.0);
        let insets = strip.hit_outset(PointerKind::Touch, &t);
        assert_eq!(insets.leading, 18.0);
        assert_eq!(insets.trailing, 18.0);
        assert_eq!(insets.top, 0.0);
        assert_eq!(insets.bottom, 0.0);
    }

    /// A corner inflates on all four sides — both of its axes are the diagonal
    /// grab.
    #[test]
    fn a_corner_inflates_on_every_side() {
        let t = tokens(TargetDensity::Compact);
        let strip = ResizeStrip::corner(host(), ResizeEdge::TopLeft, 6.0);
        let insets = strip.hit_outset(PointerKind::Touch, &t);
        assert_eq!(insets.top, 18.0);
        assert_eq!(insets.bottom, 18.0);
        assert_eq!(insets.leading, 18.0);
        assert_eq!(insets.trailing, 18.0);
    }

    /// Zero for every precise pointer at every density — the mouse invariant,
    /// stated at the arithmetic rather than only at the tree.
    #[test]
    fn a_precise_pointer_is_never_widened() {
        for density in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            let t = tokens(density);
            for strip in [
                ResizeStrip::horizontal(host(), ResizeEdge::Top, 6.0),
                ResizeStrip::vertical(host(), ResizeEdge::Left, 6.0),
                ResizeStrip::corner(host(), ResizeEdge::TopLeft, 6.0),
            ] {
                assert_eq!(
                    strip.hit_outset(PointerKind::Mouse, &t),
                    EdgeInsets::ZERO,
                    "{density:?}"
                );
            }
        }
    }

    /// A frame already built thick enough earns nothing: `dp` is a floor.
    #[test]
    fn a_strip_that_already_clears_the_floor_is_not_widened() {
        let t = tokens(TargetDensity::Compact);
        let strip = ResizeStrip::horizontal(host(), ResizeEdge::Bottom, 30.0);
        assert_eq!(strip.hit_outset(PointerKind::Touch, &t), EdgeInsets::ZERO);
    }
}
