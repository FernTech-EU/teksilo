// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `Unroll` — the horizontal sibling of [`Collapse`](super::collapse::Collapse).
//!
//! Animates a child's *width* between zero and natural while the child
//! keeps its full natural layout — the framework's clip pass crops the
//! overflow, so the visible reveal tracks progress linearly across the
//! whole duration and the child never reflows mid-animation. This is the
//! same "lay out full, clip the shrinking axis" trick the docking
//! `Splitter` uses for its side expand/collapse (`ClipPane`).
//!
//! Two drivers:
//!
//! - [`Unroll::new(expanded)`](Unroll::new) — self-animated, like
//!   `Collapse`. Flips between 0 and natural width over
//!   `MotionTokens::duration_collapse` whenever `expanded` toggles.
//! - [`Unroll::from_progress(progress)`](Unroll::from_progress) — driven
//!   by an external animated `Signal<f32>` ∈ [0, 1]. Use when something
//!   *else* owns the tween — e.g. an overlay whose deferred dismissal
//!   rolls the width back into its anchor before going dormant.
//!
//! The reveal edge is chosen with [`reveal_from`](Unroll::reveal_from):
//! [`UnrollFrom::Leading`] (default) keeps the leading edge pinned and
//! grows trailing-ward — the "slide out from a button on the left"
//! shape; [`UnrollFrom::Trailing`] mirrors it.
//!
//! Honors `prefers-reduced-motion`: the self-animated driver snaps to
//! its end value instead of tweening (the external driver's owner is
//! responsible for its own reduced-motion policy).

use std::cell::Cell;

use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, LayoutResponse, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

/// Below this progress value the wrapper's reported width snaps to zero
/// so a fully-rolled-up `Unroll` claims no horizontal space (siblings
/// in a row must not be pushed aside by an invisible-but-natural-width
/// wrapper). Picked where the width is already sub-pixel anyway.
const ROLLED_UP_PROGRESS_EPSILON: f32 = 0.005;

/// Which edge stays anchored as the child unrolls.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum UnrollFrom {
    /// Pin the leading edge; reveal trailing-ward (default).
    Leading,
    /// Pin the trailing edge; reveal leading-ward.
    Trailing,
}

enum Driver {
    /// Self-animated from a `bool`.
    Expanded(Signal<bool>),
    /// Externally driven 0..1 progress.
    Progress(Signal<f32>),
}

/// Wraps a child and animates its width between rolled-up (progress=0)
/// and natural (progress=1). See the module docs.
pub struct Unroll {
    driver: Driver,
    pending_child: Option<PendingChild>,
    child_id: Option<WidgetId>,
    /// The live progress signal — created from the `bool` for the
    /// self-animated driver, or the supplied signal for the external
    /// one. Filled in on `build()`.
    progress: Option<Signal<f32>>,
    from: UnrollFrom,
    /// Last natural size from `layout_response`; `place_children` reads
    /// it to lay the child out at full width (then clip the overflow).
    natural_size: Cell<Size>,
}

impl Unroll {
    /// Self-animated wrapper bound to `expanded`. Initially rolled up
    /// iff `expanded.get()` is `false` at the first `build()`.
    pub fn new(expanded: Signal<bool>) -> Self {
        Self::with_driver(Driver::Expanded(expanded))
    }

    /// Externally-driven wrapper. `progress` (an animated 0..1 signal)
    /// is read every layout; the caller owns the tween. Use when an
    /// overlay or other coordinator drives the reveal lifecycle.
    pub fn from_progress(progress: Signal<f32>) -> Self {
        Self::with_driver(Driver::Progress(progress))
    }

    fn with_driver(driver: Driver) -> Self {
        Self {
            driver,
            pending_child: None,
            child_id: None,
            progress: None,
            from: UnrollFrom::Leading,
            natural_size: Cell::new(Size::ZERO),
        }
    }

    /// Inline child widget (deferred insertion).
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_child = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Pre-registered child by `WidgetId`.
    pub fn child_id(mut self, id: WidgetId) -> Self {
        self.pending_child = Some(PendingChild::Id(id));
        self
    }

    /// Set the edge that stays anchored as the child unrolls. Defaults
    /// to [`UnrollFrom::Leading`].
    pub fn reveal_from(mut self, from: UnrollFrom) -> Self {
        self.from = from;
        self
    }

    /// The live progress signal (0 = rolled up, 1 = unrolled). `None`
    /// before `build()`. Lets tests and external integrations read the
    /// current animated progress.
    pub fn progress_signal(&self) -> Option<Signal<f32>> {
        self.progress.clone()
    }
}

impl std::fmt::Debug for Unroll {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Unroll").field("from", &self.from).finish()
    }
}

impl Widget for Unroll {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        let Some(child_id) = self.child_id else {
            return vec![];
        };

        let self_id = ctx.self_id();
        match &self.driver {
            Driver::Expanded(expanded) => {
                let expanded = expanded.clone();
                let initial = if expanded.get() { 1.0 } else { 0.0 };
                let progress = ctx.animated_signal(initial);
                self.progress = Some(progress.clone());
                // Every tick re-runs `layout_response`, which reads
                // `progress` and updates the reported width.
                progress.bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);

                let anim = ctx.animate().collapse().standard();
                let progress_for_effect = progress;
                ctx.effect(&expanded, move |&expanded| {
                    let target = if expanded { 1.0 } else { 0.0 };
                    anim.to_or_snap(&progress_for_effect, target);
                });
            }
            Driver::Progress(sig) => {
                self.progress = Some(sig.clone());
                sig.bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
            }
        }

        vec![child_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let Some(child_id) = self.child_id else {
            return proposal.resolve(0.0, 0.0).into();
        };
        // Measure against the *unmodified* proposal — never propose a
        // clipped width, which would let text rewrap to the in-flight
        // animated value and re-enter a layout feedback loop.
        let natural = ctx.child_size(child_id, proposal).unwrap_or(Size::ZERO);
        self.natural_size.set(natural);

        let progress = self
            .progress
            .as_ref()
            .map(|s| s.get().clamp(0.0, 1.0))
            .unwrap_or(1.0);

        let width = if progress < ROLLED_UP_PROGRESS_EPSILON {
            0.0
        } else {
            natural.width * progress
        };
        Size::new(width, natural.height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Lay the child out at full natural width and let `clips_children`
        // crop the overflow against the (smaller) animated bounds. The
        // anchored edge stays put; the other edge is revealed/hidden.
        let natural = self.natural_size.get();
        let x = match self.from {
            UnrollFrom::Leading => bounds.x,
            UnrollFrom::Trailing => bounds.right() - natural.width,
        };
        for child in children.iter_mut() {
            child.origin = Point::new(x, bounds.y);
            child.size = Size::new(natural.width, natural.height);
        }
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // Pure layout/animation wrapper — a11y-transparent, like
        // `Collapse`. The control that toggles the state and the
        // child's own subtree own the semantics.
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::primitives::TextWidget;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_i18n::lit;

    fn tree() -> WidgetTree {
        WidgetTree::new().with_theme(bastyde_core::presets::intui::light())
    }

    #[test]
    fn starts_rolled_up_when_signal_is_false() {
        let expanded = Signal::new(false);
        let mut t = tree();
        let id = t.add(Unroll::new(expanded).child(TextWidget::new(lit!("hidden"))));
        t.layout(SizeProposal::unspecified());
        assert!(
            t.bounds(id).width < 1.0,
            "rolled-up width should be ~0, got {}",
            t.bounds(id).width
        );
    }

    #[test]
    fn starts_unrolled_when_signal_is_true() {
        let expanded = Signal::new(true);
        let mut t = tree();
        let id = t.add(Unroll::new(expanded).child(TextWidget::new(lit!("visible content"))));
        t.layout(SizeProposal::unspecified());
        assert!(
            t.bounds(id).width > 1.0,
            "unrolled width should be > 0, got {}",
            t.bounds(id).width
        );
    }

    #[test]
    fn width_grows_proportionally_during_tween() {
        let expanded = Signal::new(false);
        let mut t = tree();
        let id = t.add(Unroll::new(expanded.clone()).child(TextWidget::new(lit!("some content"))));
        t.layout(SizeProposal::unspecified());
        let rolled = t.bounds(id).width;

        expanded.set(true);
        t.tick_animations(Duration::from_millis(300));
        t.layout(SizeProposal::unspecified());
        let after = t.bounds(id).width;
        assert!(
            after > rolled,
            "after expanding, width ({after}) should exceed rolled-up ({rolled})"
        );
    }

    #[test]
    fn external_progress_drives_width() {
        // Half-progress → roughly half the natural width.
        let progress = Signal::new_animated(1.0);
        let mut t = tree();
        let child = t.add(TextWidget::new(lit!("0123456789")));
        let id = t.add(Unroll::from_progress(progress.clone()).child_id(child));
        t.layout(SizeProposal::unspecified());
        let full = t.bounds(id).width;
        assert!(full > 0.0);

        progress.set(0.5);
        t.layout(SizeProposal::unspecified());
        let half = t.bounds(id).width;
        assert!(
            (half - full * 0.5).abs() < full * 0.1,
            "half progress width ({half}) should be ~half of full ({full})"
        );
    }

    #[test]
    fn trailing_anchor_pins_trailing_edge() {
        let progress = Signal::new_animated(0.5);
        let mut t = tree();
        let child = t.add(TextWidget::new(lit!("0123456789")));
        let id = t.add(
            Unroll::from_progress(progress)
                .reveal_from(UnrollFrom::Trailing)
                .child_id(child),
        );
        t.layout(SizeProposal::unspecified());
        // The child is laid out at full natural width anchored so its
        // trailing edge aligns with the wrapper's trailing edge — i.e.
        // its origin sits left of the wrapper origin.
        let wrapper = t.bounds(id);
        let inner = t.bounds(child);
        assert!(
            inner.x < wrapper.x + 0.5,
            "trailing-anchored child origin ({}) should be at/left of wrapper origin ({})",
            inner.x,
            wrapper.x
        );
    }
}
