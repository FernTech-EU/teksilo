// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `Cycle` — show one of N children at a time, advancing on a fixed
//! period. The "rotating loading tip" / status display pattern.
//!
//! ```ignore
//! ctx.add(
//!     Cycle::new()
//!         .period(Duration::from_secs(3))
//!         .child(TextWidget::new(lit!("Tip: press Cmd-K to search")))
//!         .child(TextWidget::new(lit!("Tip: hold Shift to multi-select")))
//!         .child(TextWidget::new(lit!("Tip: drag the divider to resize"))),
//! );
//! ```
//!
//! Internally a [`Switcher`] whose
//! `Signal<usize>` index is incremented by a per-frame effect.
//! Children share a `ZStack` slot — at any given moment only the
//! selected child is visible (others are dormant).
//!
//! ## Reduced motion
//!
//! Honours `prefers-reduced-motion`: pins on the first child and
//! does not install the timer driver. Subsequent children are still
//! built (so widget construction is identical) but are never shown.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::frame_tick_scheduler::FrameTickSubscription;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;

use crate::primitives::Switcher;

const DEFAULT_PERIOD: Duration = Duration::from_secs(3);

/// A wrapper that cycles through its children on a fixed period.
pub struct Cycle {
    period: Duration,
    deferred_children: Vec<Box<dyn Widget>>,
    root_child_id: Option<WidgetId>,
    /// RAII guard for the per-frame-effect subscription. See
    /// [`Pulse::frame_tick_sub`](super::pulse::Pulse) for the same
    /// pattern.
    frame_tick_sub: Option<FrameTickSubscription>,
}

impl Cycle {
    /// New cycle with default 3 s period.
    pub fn new() -> Self {
        Self {
            period: DEFAULT_PERIOD,
            deferred_children: Vec::new(),
            root_child_id: None,
            frame_tick_sub: None,
        }
    }

    /// Step interval — how long each child is visible before
    /// advancing to the next. Default 3 s.
    pub fn period(mut self, period: Duration) -> Self {
        self.period = period;
        self
    }

    /// Append a child to the rotation.
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.deferred_children.push(Box::new(widget));
        self
    }

    /// Append a pre-boxed child to the rotation.
    pub fn child_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.deferred_children.push(widget);
        self
    }

    /// Append children from an iterator.
    pub fn children(mut self, iter: impl IntoIterator<Item = impl Widget + 'static>) -> Self {
        for w in iter {
            self.deferred_children.push(Box::new(w));
        }
        self
    }
}

impl Default for Cycle {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Cycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cycle")
            .field("period", &self.period)
            .field("num_children", &self.deferred_children.len())
            .finish()
    }
}

impl Widget for Cycle {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let children = std::mem::take(&mut self.deferred_children);
        let n = children.len();
        let selected = Signal::new(0_usize);

        let mut switcher = Switcher::new(selected.clone());
        for child in children {
            switcher = switcher.child_boxed(child);
        }
        let root = ctx.add(switcher);
        self.root_child_id = Some(root);

        // Reduced-motion or trivial case (≤1 child): no timer, sticks
        // on the first child.
        if ctx.prefers_reduced_motion() || n <= 1 {
            return vec![root];
        }

        // Discrete index advance. Cycle only changes its visible child
        // once per period, so it subscribes *throttled* rather than
        // per-frame: the event loop sleeps to the period deadline instead
        // of rendering ~90 identical frames per boundary at 60 Hz. The
        // visibility gate is unchanged — a Cycle parked in a non-selected
        // `Switcher` branch (or an off-screen tab) still ticks zero times,
        // and resumes when shown again (its `last_advance` clock keeps
        // running on real time, so it advances on the first wake past the
        // next boundary).
        //
        // Timing is absolute (`Instant::now`) rather than accumulated
        // frame deltas: at the throttled cadence the tree's per-frame
        // delta is clamped to 0.1 s (a spike guard) and cannot measure a
        // multi-second period. Absolute time is also self-correcting when
        // a wake lands late.
        let period = self.period;
        let last_advance: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
        let selected_for_tick = selected;
        ctx.effect(&ctx.frame_tick(), move |_delta| {
            let now = Instant::now();
            match last_advance.get() {
                // First tick after (re)build: start the clock, don't jump.
                None => last_advance.set(Some(now)),
                Some(prev) if now.duration_since(prev) >= period => {
                    let next = (selected_for_tick.get() + 1) % n;
                    selected_for_tick.set(next);
                    last_advance.set(Some(now));
                }
                Some(_) => {}
            }
        });
        self.frame_tick_sub = None;
        self.frame_tick_sub = Some(ctx.subscribe_frame_tick_throttled(period));

        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // Visual rotator. The active child owns its own a11y; the
        // wrapper is a11y-transparent.
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::TextWidget;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_i18n::lit;

    #[test]
    fn cycle_builds_with_children() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            Cycle::new()
                .child(TextWidget::new(lit!("A")))
                .child(TextWidget::new(lit!("B")))
                .child(TextWidget::new(lit!("C"))),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let b = tree.bounds(id);
        assert!(b.width > 0.0);
    }

    #[test]
    fn empty_cycle_is_safe() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(Cycle::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let _ = tree.render();
    }

    #[test]
    fn single_child_cycle_does_not_animate() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(Cycle::new().child(TextWidget::new(lit!("only"))));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let _ = tree.render();
        assert!(
            !tree.has_active_animations(),
            "single-child cycle should not start a timer"
        );
    }

    #[test]
    fn reduced_motion_pins_first_child() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.set_accessibility_preferences(false, true, 1.0);
        tree.add(
            Cycle::new()
                .child(TextWidget::new(lit!("A")))
                .child(TextWidget::new(lit!("B"))),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let _ = tree.render();
        assert!(
            !tree.has_active_animations(),
            "reduced-motion path must not register animations"
        );
    }
}
