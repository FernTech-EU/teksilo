//! `Cycle` — show one of N children at a time, advancing on a fixed
//! period. The "rotating loading tip" / status display pattern.
//!
//! ```ignore
//! ctx.add(
//!     Cycle::new()
//!         .period(Duration::from_secs(3))
//!         .child(TextWidget::new_literal("Tip: press Cmd-K to search"))
//!         .child(TextWidget::new_literal("Tip: hold Shift to multi-select"))
//!         .child(TextWidget::new_literal("Tip: drag the divider to resize")),
//! );
//! ```
//!
//! Internally a [`Switcher`](crate::primitives::Switcher) whose
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
use std::time::Duration;

use fern_canvas::{Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::widget::{LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

use crate::primitives::Switcher;

const DEFAULT_PERIOD: Duration = Duration::from_secs(3);

/// A wrapper that cycles through its children on a fixed period.
pub struct Cycle {
    period: Duration,
    deferred_children: Vec<Box<dyn Widget>>,
    root_child_id: Option<WidgetId>,
}

impl Cycle {
    /// New cycle with default 3 s period.
    pub fn new() -> Self {
        Self {
            period: DEFAULT_PERIOD,
            deferred_children: Vec::new(),
            root_child_id: None,
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

        let period_secs = self.period.as_secs_f32().max(0.001);
        let elapsed = Rc::new(Cell::new(0.0_f32));
        let frame_request = ctx.frame_request_handle();
        let selected_for_tick = selected;
        ctx.effect(&ctx.frame_tick(), move |&delta| {
            let t = elapsed.get() + delta;
            if t >= period_secs {
                let next = (selected_for_tick.get() + 1) % n;
                selected_for_tick.set(next);
                // Carry over the overflow so jitter at the period
                // boundary doesn't accumulate (slow drift).
                elapsed.set(t - period_secs);
            } else {
                elapsed.set(t);
            }
            frame_request.set(true);
        });
        ctx.request_frame();

        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
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
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    #[test]
    fn cycle_builds_with_children() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(
            Cycle::new()
                .child(TextWidget::new_literal("A"))
                .child(TextWidget::new_literal("B"))
                .child(TextWidget::new_literal("C")),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let b = tree.bounds(id);
        assert!(b.width > 0.0);
    }

    #[test]
    fn empty_cycle_is_safe() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(Cycle::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let _ = tree.render();
    }

    #[test]
    fn single_child_cycle_does_not_animate() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(Cycle::new().child(TextWidget::new_literal("only")));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let _ = tree.render();
        assert!(
            !tree.has_active_animations(),
            "single-child cycle should not start a timer"
        );
    }

    #[test]
    fn reduced_motion_pins_first_child() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.set_accessibility_preferences(false, true, 1.0);
        tree.add(
            Cycle::new()
                .child(TextWidget::new_literal("A"))
                .child(TextWidget::new_literal("B")),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let _ = tree.render();
        assert!(
            !tree.has_active_animations(),
            "reduced-motion path must not register animations"
        );
    }
}
