// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `Collapse` — a wrapper widget that animates its child between
//! hidden and natural size when an external `Signal<bool>` toggles.
//!
//! Drives a `progress: Signal<f32>` ∈ [0, 1] (0 = collapsed,
//! 1 = expanded) and reports its own size as `(natural_w, natural_h *
//! progress)` while the child lays out at full natural size — the
//! framework's clip pass crops the overflow. This keeps the animation
//! visible across the *whole* duration, instead of compressing the
//! visible portion into the final few milliseconds (which is what
//! happened when an animated `MaxSize::max_height` slid against a
//! 10000-px sentinel that vastly overshot the child's natural height).
//!
//! ```ignore
//! let expanded = ctx.signal(false);
//! ctx.add(Collapse::new(expanded.clone()).child(advanced_settings));
//! // ...elsewhere:
//! expanded.set(true);  // animates open over `motion.duration_collapse`
//! ```
//!
//! Honors `prefers-reduced-motion`: under reduced motion, progress
//! snaps to its end value instead of tweening.

use std::cell::Cell;

use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

/// Below this progress value, the wrapper's reported width snaps to
/// zero so a fully-collapsed Collapse doesn't claim any horizontal
/// space (siblings in a tooltip footer must not be pushed off the row
/// by an invisible-but-natural-width wrapper). Picked at the level
/// where the wrapper's height is already sub-pixel anyway, so the
/// width snap is invisible.
const COLLAPSED_PROGRESS_EPSILON: f32 = 0.005;

/// Wraps a child and animates it between hidden (progress=0) and
/// natural size (progress=1), driven by an external `Signal<bool>`.
pub struct Collapse {
    expanded: Signal<bool>,
    pending_child: Option<PendingChild>,
    child_id: Option<WidgetId>,
    /// Cached so external integrations (and tests) can read the
    /// current animated 0..1 progress. Filled in on `build()`.
    progress: Option<Signal<f32>>,
    /// Last natural size computed by `size_that_fits`. `place_children`
    /// reads it so the child is laid out at full natural dimensions
    /// (the framework clips the overflow against `Collapse`'s smaller
    /// reported bounds).
    natural_size: Cell<Size>,
}

impl Collapse {
    /// Build a collapse wrapper bound to `expanded`. Initially
    /// collapsed iff `expanded.get()` is `false` at the first
    /// `build()`.
    pub fn new(expanded: Signal<bool>) -> Self {
        Self {
            expanded,
            pending_child: None,
            child_id: None,
            progress: None,
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
}

impl std::fmt::Debug for Collapse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Collapse").finish()
    }
}

impl Widget for Collapse {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Resolve the child if it was provided inline.
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        let Some(child_id) = self.child_id else {
            return vec![];
        };

        let initial = if self.expanded.get() { 1.0 } else { 0.0 };
        let progress = ctx.animated_signal(initial);
        self.progress = Some(progress.clone());

        // Bind progress to *self* at relayout level: every animation
        // tick re-runs `size_that_fits`, which reads `progress.get()`
        // and updates the wrapper's reported height accordingly.
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        progress.bind_to(id, registry, BindingLevel::Relayout);

        // Drive the progress tween whenever `expanded` flips. The
        // observer survives across rebuilds via `effect_handles`.
        let collapse_anim = ctx.animate().collapse().standard();
        let progress_for_effect = progress;
        ctx.effect(&self.expanded, move |&expanded| {
            let target = if expanded { 1.0 } else { 0.0 };
            collapse_anim.to_or_snap(&progress_for_effect, target);
        });

        vec![child_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let Some(child_id) = self.child_id else {
            return (proposal.resolve(0.0, 0.0)).into();
        };
        // Ask the child for its size against the *unmodified* proposal.
        // We never propose a clipped height — that would let text
        // wrap or images letterbox to the in-flight animated value
        // and re-enter a layout feedback loop.
        let natural = ctx.child_size(child_id, proposal).unwrap_or(Size::ZERO);
        self.natural_size.set(natural);

        let progress = self
            .progress
            .as_ref()
            .map(|s| s.get().clamp(0.0, 1.0))
            .unwrap_or(1.0);

        // Width snaps to 0 only when fully collapsed — during the
        // tween the wrapper keeps its natural width so the child's
        // text / icons / etc. continue to render at the proper
        // measure (just clipped vertically by the framework).
        let width = if progress < COLLAPSED_PROGRESS_EPSILON {
            0.0
        } else {
            natural.width
        };
        Size::new(width, natural.height * progress).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Lay the child out at its FULL natural size and let the
        // framework's clip pass crop the bottom overflow against
        // `Collapse`'s reduced bounds. This is what makes the visible
        // shrink track the animated progress linearly across the full
        // duration: the child's internal layout doesn't reflow each
        // frame, only the clip rect changes.
        let natural = self.natural_size.get();
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x, bounds.y);
            child.size = natural;
        }
    }

    fn clips_children(&self) -> bool {
        // Required: the child is sized to its natural dimensions but
        // the wrapper's bounds are smaller during the tween, so the
        // overflow must be clipped. Without this the child would
        // render past the wrapper's reported size and overlap
        // siblings below.
        true
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // Collapse is a layout/animation wrapper. The control that
        // *toggles* the expanded state owns the a11y semantics
        // (Role::Button + set_expanded); the content is announced by
        // its own subtree when expanded. This widget itself is
        // intentionally a11y-transparent.
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

    #[test]
    fn starts_collapsed_when_signal_is_false() {
        let expanded = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree
            .add(Collapse::new(expanded.clone()).child(TextWidget::new(lit!("hidden content"))));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        assert!(
            tree.bounds(id).height < 1.0,
            "collapsed bounds should be ~0, got {}",
            tree.bounds(id).height
        );
    }

    #[test]
    fn starts_expanded_when_signal_is_true() {
        let expanded = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree
            .add(Collapse::new(expanded.clone()).child(TextWidget::new(lit!("visible content"))));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        assert!(
            tree.bounds(id).height > 1.0,
            "expanded bounds should be > 0, got {}",
            tree.bounds(id).height
        );
    }

    #[test]
    fn flipping_signal_drives_animation() {
        let expanded = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            Collapse::new(expanded.clone())
                .child(TextWidget::new(lit!("content with some natural height"))),
        );
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let collapsed = tree.bounds(id).height;

        expanded.set(true);

        tree.tick_animations(Duration::from_millis(300));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let after = tree.bounds(id).height;

        assert!(
            after > collapsed,
            "after expanding, height ({}) should exceed collapsed height ({})",
            after,
            collapsed
        );
    }

    #[test]
    fn collapse_height_shrinks_proportionally() {
        // Start expanded; flip to collapsed; verify the wrapper
        // height shrinks *proportionally* across the tween — the
        // intermediate 50%-progress sample must be roughly half the
        // initial height, NOT pinned at the natural height for 95% of
        // the animation (the bug where max_h tweens against a 10000
        // sentinel).
        let expanded = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let root =
            tree.add(Collapse::new(expanded.clone()).child(TextWidget::new(lit!("content"))));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let initial_h = tree.bounds(root).height;
        assert!(initial_h > 0.0);

        expanded.set(false);

        // Tick to ~halfway through the 200ms collapse and check the
        // height is meaningfully below the initial — not snapped to 0
        // and not still pinned at natural.
        tree.tick_animations(Duration::from_millis(100));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let mid_h = tree.bounds(root).height;
        assert!(
            mid_h < initial_h * 0.95,
            "halfway through collapse, height ({}) should be visibly less than initial ({})",
            mid_h,
            initial_h
        );
        assert!(
            mid_h > initial_h * 0.05,
            "halfway through collapse, height ({}) should not yet be near zero ({})",
            mid_h,
            initial_h
        );
    }

    #[test]
    fn collapse_height_monotonically_decreases() {
        let expanded = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let root =
            tree.add(Collapse::new(expanded.clone()).child(TextWidget::new(lit!("content"))));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let initial_h = tree.bounds(root).height;

        expanded.set(false);

        let mut prev = f32::INFINITY;
        for step in 0..5 {
            tree.tick_animations(Duration::from_millis(50));
            tree.layout(SizeProposal {
                width: Some(300.0),
                height: None,
            });
            let h = tree.bounds(root).height;
            assert!(
                h <= prev + 0.01,
                "height must never grow during collapse: step {} got {} after {}",
                step,
                h,
                prev,
            );
            assert!(
                h <= initial_h + 0.01,
                "step {} height {} must not exceed initial expanded height {}",
                step,
                h,
                initial_h,
            );
            prev = h;
        }
    }

    #[test]
    fn animation_is_active_mid_tween() {
        let expanded = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Collapse::new(expanded.clone()).child(TextWidget::new(lit!("content"))));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });

        expanded.set(true);
        tree.tick_animations(Duration::from_millis(50));
        assert!(
            tree.has_active_animations(),
            "tween should be in flight 50 ms in"
        );
    }
}
