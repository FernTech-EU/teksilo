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

/// An in-progress marquee, in widget-local coordinates.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MarqueeState {
    pub(crate) origin: Point,
    pub(crate) current: Point,
    /// Whether to union with (rather than replace) the existing selection.
    pub(crate) additive: bool,
}

impl MarqueeState {
    /// The normalized rectangle between `origin` and `current`.
    pub(crate) fn rect(&self) -> Rect {
        let x = self.origin.x.min(self.current.x);
        let y = self.origin.y.min(self.current.y);
        let w = (self.origin.x - self.current.x).abs();
        let h = (self.origin.y - self.current.y).abs();
        Rect::new(x, y, w, h)
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
    move |phase, _ctx| {
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
                    additive: cfg.additive_mods.get(),
                }));
            }
            DragPhase::Moved { position, .. } => {
                if let Some(mut st) = cfg.marquee.get() {
                    st.current = position;
                    cfg.marquee.set(Some(st));
                }
            }
            DragPhase::Ended { position } => {
                if let Some(mut st) = cfg.marquee.get() {
                    st.current = position;
                    let local = st.rect();
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
