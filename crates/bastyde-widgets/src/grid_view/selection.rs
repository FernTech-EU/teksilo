// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Rubber-band (marquee) selection for `GridView`.
//!
//! A drag starting on the empty background (not on a tile) sweeps out a
//! selection rectangle; on release every tile intersecting it is selected
//! (or, with Ctrl/Shift held at drag start, added to the selection). The
//! hit-test is geometric via the layout strategy, so tiles outside the
//! realized window are selected too. Follows the `bastyde-scene`
//! rubber-band pattern; the visual rectangle is painted by `GridOverlay`.

use std::cell::Cell;
use std::rc::Rc;

use bastyde_canvas::{Point, Rect};
use bastyde_core::gesture::DragPhase;
use bastyde_core::signal::Signal;
use bastyde_core::widget::EventContext;
use bastyde_data::SelectionModel;

use super::layout::GridLayoutStrategy;

/// An in-progress marquee. `origin`/`current` are widget-local
/// (viewport) coordinates, matching the pointer positions the drag
/// gesture reports.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MarqueeState {
    pub(crate) origin: Point,
    pub(crate) current: Point,
    /// `scroll_y` at the moment `origin` was captured. Edge auto-scroll
    /// (see [`marquee_auto_scroll_step`]) moves `scroll_y` out from under
    /// a stationary `origin` while the drag continues, so `origin` alone
    /// no longer identifies a fixed point in the content — this field
    /// lets [`MarqueeState::local_rect`] re-project it each frame,
    /// keeping the anchor pinned to the content it was pressed on rather
    /// than the viewport pixel it started at.
    pub(crate) origin_scroll: f32,
    /// Whether to union with (rather than replace) the existing selection.
    pub(crate) additive: bool,
}

impl MarqueeState {
    /// The normalized rectangle between `origin` and `current`, in
    /// widget-local (viewport) coordinates, at the given *live*
    /// `scroll_y`. `current` is always local (the drag gesture keeps
    /// reporting local positions regardless of scroll); `origin` is
    /// re-derived from its frozen content-space position
    /// (`origin.y + origin_scroll`) so the anchor visually tracks the
    /// same content as the view auto-scrolls underneath it.
    pub(crate) fn local_rect(&self, scroll_y: f32) -> Rect {
        let origin_y = self.origin.y + (self.origin_scroll - scroll_y);
        let x = self.origin.x.min(self.current.x);
        let y = origin_y.min(self.current.y);
        let w = (self.origin.x - self.current.x).abs();
        let h = (origin_y - self.current.y).abs();
        Rect::new(x, y, w, h)
    }
}

/// Edge-zone width (in dp) inside which the marquee ramps its auto-scroll
/// velocity up to [`MARQUEE_MAX_VELOCITY`]. Matches the edge-scroll ramp
/// `TabBar`/`TreeView` use for their own drag-tick auto-scroll, for a
/// consistent feel across the data views.
const MARQUEE_EDGE_ZONE: f32 = 32.0;
/// Cap on per-tick auto-scroll velocity at the marquee's viewport edges.
const MARQUEE_MAX_VELOCITY: f32 = 12.0;

/// Auto-scroll step (in content px, +down/-up) for a marquee whose
/// trailing corner sits at local `pointer_y` within a viewport of
/// `viewport_height`. Zero outside both edge bands; ramps linearly
/// inside a band and saturates at `MARQUEE_MAX_VELOCITY` beyond the
/// viewport edge (drag gestures keep reporting positions past the
/// boundary once the pointer is captured). Pure so it can be unit
/// tested without a widget tree.
pub(crate) fn marquee_auto_scroll_step(pointer_y: f32, viewport_height: f32) -> f32 {
    let above = (MARQUEE_EDGE_ZONE - pointer_y).max(0.0);
    let below = (pointer_y - (viewport_height - MARQUEE_EDGE_ZONE)).max(0.0);
    if above > 0.0 {
        -(above / MARQUEE_EDGE_ZONE).min(1.0) * MARQUEE_MAX_VELOCITY
    } else if below > 0.0 {
        (below / MARQUEE_EDGE_ZONE).min(1.0) * MARQUEE_MAX_VELOCITY
    } else {
        0.0
    }
}

/// Captured state for the marquee drag handler.
pub(crate) struct MarqueeConfig {
    pub(crate) marquee: Signal<Option<MarqueeState>>,
    pub(crate) selection: SelectionModel,
    pub(crate) strategy: Rc<dyn GridLayoutStrategy>,
    pub(crate) scroll_y: Signal<f32>,
    pub(crate) viewport_width: Rc<Cell<f32>>,
    pub(crate) len_fn: Rc<dyn Fn() -> usize>,
    /// Modifier state (ctrl||shift) at the most recent pointer-down, for
    /// additive selection. Updated by the container's pointer handler.
    pub(crate) additive_mods: Rc<Cell<bool>>,
}

/// Build the container `on_drag` closure that drives marquee selection.
pub(crate) fn build_marquee_handler(
    cfg: MarqueeConfig,
) -> impl FnMut(DragPhase, &mut EventContext) + 'static {
    move |phase, ctx| {
        let vp_w = cfg.viewport_width.get();
        let scroll = cfg.scroll_y.get();
        match phase {
            DragPhase::Started { position, .. } => {
                // Content-space point of the press; skip when it lands on a
                // tile (that's a potential item drag, not a marquee).
                let cp = Point::new(position.x, position.y + scroll);
                if cfg
                    .strategy
                    .index_at_point(cp, (cfg.len_fn)(), vp_w)
                    .is_some()
                {
                    return;
                }
                cfg.marquee.set(Some(MarqueeState {
                    origin: position,
                    current: position,
                    origin_scroll: scroll,
                    additive: cfg.additive_mods.get(),
                }));
                // A press right at an edge should start auto-scrolling
                // immediately, without waiting for the first move.
                ctx.request_frame();
            }
            DragPhase::Moved { position, .. } => {
                if let Some(mut st) = cfg.marquee.get() {
                    st.current = position;
                    cfg.marquee.set(Some(st));
                    // Kick the auto-scroll tick effect so entering an edge
                    // band from idle (pointer stopped moving) still starts
                    // scrolling on the very next frame.
                    ctx.request_frame();
                }
            }
            DragPhase::Ended { position } => {
                if let Some(mut st) = cfg.marquee.get() {
                    st.current = position;
                    let local = st.local_rect(scroll);
                    let content = Rect::new(local.x, local.y + scroll, local.width, local.height);
                    let hits = cfg
                        .strategy
                        .hit_indices_in_rect(content, (cfg.len_fn)(), vp_w);
                    cfg.selection.select_indices(hits, st.additive);
                    cfg.marquee.set(None);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(origin: Point, current: Point, origin_scroll: f32) -> MarqueeState {
        MarqueeState {
            origin,
            current,
            origin_scroll,
            additive: false,
        }
    }

    #[test]
    fn local_rect_matches_naive_diff_when_scroll_is_unchanged() {
        // No auto-scroll happened during the drag (origin_scroll == the
        // live scroll passed to local_rect) — same shape the old
        // `rect()` produced.
        let st = state(Point::new(10.0, 20.0), Point::new(60.0, 90.0), 0.0);
        let r = st.local_rect(0.0);
        assert!((r.x - 10.0).abs() < 0.001);
        assert!((r.y - 20.0).abs() < 0.001);
        assert!((r.width - 50.0).abs() < 0.001);
        assert!((r.height - 70.0).abs() < 0.001);
    }

    #[test]
    fn local_rect_tracks_anchor_as_scroll_changes() {
        // Press at local y=380 (near the bottom of a 400px viewport) with
        // scroll_y = 0 at press time. Auto-scroll then moves scroll_y to
        // 100 while the pointer stays pinned at the bottom edge (current
        // unchanged — this is exactly the "pointer stationary at the
        // edge" auto-scroll case). The anchor's local position must
        // track the scroll: local_origin_y = origin.y - (live_scroll -
        // origin_scroll) = 380 - 100 = 280.
        let st = state(Point::new(5.0, 380.0), Point::new(5.0, 380.0), 0.0);
        let r = st.local_rect(100.0);
        assert!(
            (r.y - 280.0).abs() < 0.001,
            "origin should have visually moved up by the scrolled amount, got y={}",
            r.y
        );
        // The pointer (current) hasn't moved locally, so the rect's far
        // edge is still at the original local y — height grows to match.
        assert!((r.height - 100.0).abs() < 0.001, "height = {}", r.height);
    }

    #[test]
    fn local_rect_content_span_grows_with_auto_scroll() {
        // The content-space rect (what selection hit-testing uses) must
        // grow as auto-scroll reveals more rows below the original
        // press point, even though the pointer's local position never
        // moved. content_rect = local_rect shifted down by the live
        // scroll (mirrors the existing `local.y + scroll` step at
        // DragPhase::Ended).
        let st = state(Point::new(5.0, 380.0), Point::new(5.0, 380.0), 0.0);
        let before = st.local_rect(0.0);
        let content_before = before.y + 0.0;
        let after = st.local_rect(100.0);
        let content_after = after.y + 100.0;
        // Content-space origin is invariant under scroll (the anchor is
        // pinned to content, not the viewport pixel) ...
        assert!(
            (content_before - content_after).abs() < 0.001,
            "content-space anchor drifted: {content_before} vs {content_after}"
        );
        // ... while the local rect's height (== content span, since
        // `current` didn't move) grew by exactly the scrolled amount.
        assert!((after.height - before.height - 100.0).abs() < 0.001);
    }

    #[test]
    fn auto_scroll_step_is_zero_away_from_edges() {
        assert_eq!(marquee_auto_scroll_step(200.0, 400.0), 0.0);
    }

    #[test]
    fn auto_scroll_step_ramps_near_bottom_edge() {
        // Just inside the bottom edge band.
        let near = marquee_auto_scroll_step(400.0 - 16.0, 400.0);
        assert!(near > 0.0, "should scroll down, got {near}");
        assert!(near < MARQUEE_MAX_VELOCITY);
        // At/beyond the viewport edge, saturates at the max velocity.
        let at_edge = marquee_auto_scroll_step(400.0, 400.0);
        assert!((at_edge - MARQUEE_MAX_VELOCITY).abs() < 0.001);
        let beyond = marquee_auto_scroll_step(500.0, 400.0);
        assert!((beyond - MARQUEE_MAX_VELOCITY).abs() < 0.001, "{beyond}");
    }

    #[test]
    fn auto_scroll_step_ramps_near_top_edge() {
        let near = marquee_auto_scroll_step(16.0, 400.0);
        assert!(near < 0.0, "should scroll up, got {near}");
        assert!(near > -MARQUEE_MAX_VELOCITY);
        let at_edge = marquee_auto_scroll_step(0.0, 400.0);
        assert!((at_edge + MARQUEE_MAX_VELOCITY).abs() < 0.001);
        let beyond = marquee_auto_scroll_step(-50.0, 400.0);
        assert!((beyond + MARQUEE_MAX_VELOCITY).abs() < 0.001, "{beyond}");
    }
}
