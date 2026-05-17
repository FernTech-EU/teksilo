//! `Scale` — wraps a child and animates a uniform 2D scale on its
//! entire subtree when an external `Prop<bool>` toggles. Drives a
//! `progress: Signal<f32>` ∈ [0, 1] (0 = invisible, 1 = at rest) and
//! applies it as a centered (or origin-pivoted) scale transform via
//! [`BuildContext::set_transform`] — the renderer's transform stack
//! composes it onto the subtree.
//!
//! ```ignore
//! let visible = ctx.signal(false);
//! ctx.add(Scale::new(visible.clone()).child(card));
//! visible.set(true);   // scale-in around the slot center
//! ```
//!
//! ## Two layout modes
//!
//! - **Visual-only (default)** — `reflow=false`. The slot stays at the
//!   child's natural size at all scale values; only the *visual content*
//!   shrinks/grows around the chosen origin. Use for: overlay enter/exit,
//!   "boop" feedback on a Card, focus emphasis. Pair with `Center`
//!   origin (the default).
//! - **Reflow** — `.reflow(true)`. The wrapper's `layout_response`
//!   returns `child_size * progress`, so siblings reflow as the child
//!   shrinks to nothing. The visual content scales by the same factor,
//!   fitting exactly within the shrunken slot. Use for: a Card that
//!   disappears by shrinking with surrounding cards filling the gap.
//!   Pair with `TopLeading` origin (so the visual stays anchored at
//!   the slot's top-left as it shrinks — otherwise the visual drifts
//!   while the slot shrinks).
//!
//! ## Why this isn't just `Collapse`
//!
//! `Collapse` animates only one axis (height by default) and "wipes"
//! content via clipping — text inside stays at full size, only the
//! visible portion shrinks. `Scale` shrinks uniformly on both axes,
//! and text/icons visually get smaller. Different visual vocabulary,
//! different use cases.
//!
//! ## Reduced motion
//!
//! Honours `prefers-reduced-motion`: snaps progress to its end value
//! (visible / hidden) instead of tweening.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use bastyde_canvas::{Point, Rect, Size, SizeProposal, Transform2D};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::{Prop, Signal};
use bastyde_core::widget::{LayoutContext, LayoutResponse, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::Easing;

/// Pivot point for the scale matrix, expressed relative to the
/// wrapper's slot rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleOrigin {
    Center,
    TopLeading,
    TopTrailing,
    BottomLeading,
    BottomTrailing,
}

impl ScaleOrigin {
    /// Compute the world-space pivot point given the wrapper's
    /// rendered slot. Honours RTL by flipping Leading/Trailing.
    /// Shared by `Scale` and `Rotate`.
    pub(crate) fn pivot_world(self, bounds: Rect, is_rtl: bool) -> Point {
        let (x_anchor, y_anchor) = match self {
            Self::Center => (Anchor::Mid, Anchor::Mid),
            Self::TopLeading => (Anchor::Leading, Anchor::Start),
            Self::TopTrailing => (Anchor::Trailing, Anchor::Start),
            Self::BottomLeading => (Anchor::Leading, Anchor::End),
            Self::BottomTrailing => (Anchor::Trailing, Anchor::End),
        };
        let x = match (x_anchor, is_rtl) {
            (Anchor::Mid, _) => bounds.x + bounds.width * 0.5,
            (Anchor::Leading, false) | (Anchor::Trailing, true) => bounds.x,
            (Anchor::Trailing, false) | (Anchor::Leading, true) => bounds.x + bounds.width,
            (Anchor::Start, _) | (Anchor::End, _) => unreachable!(),
        };
        let y = match y_anchor {
            Anchor::Start => bounds.y,
            Anchor::Mid => bounds.y + bounds.height * 0.5,
            Anchor::End => bounds.y + bounds.height,
            Anchor::Leading | Anchor::Trailing => unreachable!(),
        };
        Point::new(x, y)
    }
}

#[derive(Clone, Copy)]
enum Anchor {
    Leading,
    Trailing,
    Start,
    Mid,
    End,
}

/// Scale matrix `T(pivot) * S(scale) * T(-pivot)` — uniform scale
/// around a pivot point in world coords.
fn centered_scale(pivot: Point, scale: f32) -> Transform2D {
    Transform2D {
        m: [
            scale,
            0.0,
            0.0,
            scale,
            pivot.x * (1.0 - scale),
            pivot.y * (1.0 - scale),
        ],
    }
}

/// Wraps a child and animates a uniform 2D scale on its subtree
/// driven by an external `Prop<bool>`.
pub struct Scale {
    visible: Prop<bool>,
    reflow: bool,
    origin: ScaleOrigin,
    duration: Option<Duration>,
    easing: Option<Easing>,
    pending_child: Option<PendingChild>,
    child_id: Option<WidgetId>,
    /// 0 = fully scaled out, 1 = at rest. Animated.
    progress: Option<Signal<f32>>,
    /// Output: the actual transform matrix the render walker reads via
    /// `set_transform`. Updated from `place_children` once we know the
    /// world-space pivot (bounds aren't available in `layout_response`).
    transform_signal: Option<Signal<Transform2D>>,
    /// Last natural-size measurement; `place_children` reads it to
    /// place the child at full natural while the slot may be smaller.
    natural_size: Cell<Size>,
    /// Last bounds the wrapper was placed in. Used by the progress
    /// observer to recompute the transform matrix on every animation
    /// tick *without* triggering relayout via `Relayout` binding —
    /// avoids hitting the layout pipeline 60× per second for purely
    /// visual scale animations.
    last_bounds: Rc<Cell<Rect>>,
    /// Captured at build(); place_children uses it to resolve
    /// Leading/Trailing origins when the layout context's RTL flag
    /// can't otherwise be threaded into the matrix-recompute observer.
    last_is_rtl: Rc<Cell<bool>>,
}

impl Scale {
    /// Build a scale wrapper bound to `visible`. Defaults: visual-only
    /// (no layout reflow), `Center` origin, `MotionTokens::duration_normal`
    /// + `easing_standard`.
    pub fn new(visible: impl Into<Prop<bool>>) -> Self {
        Self {
            visible: visible.into(),
            reflow: false,
            origin: ScaleOrigin::Center,
            duration: None,
            easing: None,
            pending_child: None,
            child_id: None,
            progress: None,
            transform_signal: None,
            natural_size: Cell::new(Size::ZERO),
            last_bounds: Rc::new(Cell::new(Rect::ZERO)),
            last_is_rtl: Rc::new(Cell::new(false)),
        }
    }

    /// When `true`, the wrapper's reported size shrinks with progress
    /// (siblings reflow). Pair with `.origin(ScaleOrigin::TopLeading)`
    /// for the "card removal" pattern. Default: `false` (visual-only).
    pub fn reflow(mut self, reflow: bool) -> Self {
        self.reflow = reflow;
        self
    }

    /// Pivot point for the scale matrix. Default `Center` for visual-
    /// only mode; consider `TopLeading` when `reflow=true`.
    pub fn origin(mut self, origin: ScaleOrigin) -> Self {
        self.origin = origin;
        self
    }

    /// Override the tween duration. Default: `MotionTokens::duration_normal`.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Override the easing. Default: `MotionTokens::easing_standard`.
    pub fn easing(mut self, easing: Easing) -> Self {
        self.easing = Some(easing);
        self
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

impl std::fmt::Debug for Scale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scale")
            .field("reflow", &self.reflow)
            .field("origin", &self.origin)
            .finish()
    }
}

impl Widget for Scale {
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

        let initial = if self.visible.get() { 1.0 } else { 0.0 };
        let progress = ctx.animated_signal(initial);
        let transform_signal = ctx.signal(Transform2D::IDENTITY);

        // Apply the transform via the render walker scope.
        let id = ctx.self_id();
        ctx.set_transform(id, transform_signal.clone());

        // Reflow mode: progress also drives the wrapper's reported
        // size — bind at Relayout so each tick re-runs layout_response.
        // Visual-only mode: progress only drives repaint via the
        // transform_signal observer (registered below); no Relayout
        // binding on progress itself.
        if self.reflow {
            let registry = ctx.binding_registry();
            progress.bind_to(id, registry, BindingLevel::Relayout);
        }

        // Recompute the transform matrix on every progress tick. Reads
        // last_bounds (set by place_children) and writes to
        // transform_signal — that signal's RepaintOnly binding then
        // marks self for repaint, no relayout for visual-only mode.
        let last_bounds = self.last_bounds.clone();
        let last_is_rtl = self.last_is_rtl.clone();
        let origin = self.origin;
        let transform_for_observer = transform_signal.clone();
        ctx.effect(&progress, move |&p| {
            let p = p.clamp(0.0, 1.0);
            let bounds = last_bounds.get();
            let pivot = origin.pivot_world(bounds, last_is_rtl.get());
            transform_for_observer.set(centered_scale(pivot, p));
        });

        self.progress = Some(progress.clone());
        self.transform_signal = Some(transform_signal);

        // Drive the scale on visibility flips.
        if let Prop::Bound(visible_signal) = &self.visible {
            let visible_signal = visible_signal.clone();
            let scale_anim = if let Some(d) = self.duration {
                ctx.animate().duration(d)
            } else {
                ctx.animate().normal()
            };
            let scale_anim = if let Some(e) = self.easing {
                scale_anim.easing(e)
            } else {
                scale_anim.standard()
            };
            let progress_for_effect = progress;
            ctx.effect(&visible_signal, move |&v| {
                let target = if v { 1.0 } else { 0.0 };
                scale_anim.to_or_snap(&progress_for_effect, target);
            });
        }

        vec![child_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let Some(child_id) = self.child_id else {
            return proposal.resolve(0.0, 0.0).into();
        };
        let natural = ctx.child_size(child_id, proposal).unwrap_or(Size::ZERO);
        self.natural_size.set(natural);
        if self.reflow {
            let p = self
                .progress
                .as_ref()
                .map(|s| s.get().clamp(0.0, 1.0))
                .unwrap_or(1.0);
            Size::new(natural.width * p, natural.height * p).into()
        } else {
            natural.into()
        }
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        // Capture bounds + RTL for the progress observer; recompute
        // and publish the transform now so the very first frame paints
        // at the correct matrix even before any animation tick.
        self.last_bounds.set(bounds);
        self.last_is_rtl.set(ctx.is_rtl());
        if let (Some(progress), Some(t_sig)) = (&self.progress, &self.transform_signal) {
            let p = progress.get().clamp(0.0, 1.0);
            let pivot = self.origin.pivot_world(bounds, ctx.is_rtl());
            t_sig.set(centered_scale(pivot, p));
        }

        // Lay the child at full natural — the transform scales it to
        // fit the wrapper's (potentially shrunken in reflow mode)
        // bounds. Same trick Collapse uses for clean clipping.
        let natural = self.natural_size.get();
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x, bounds.y);
            child.size = natural;
        }
    }

    fn clips_children(&self) -> bool {
        // Reflow mode: child renders at natural, slot is smaller —
        // clip the overflow. Visual-only mode: scaled-up content can
        // still overshoot the slot; clip to keep siblings safe.
        true
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // Visual-modulation wrapper. Child owns its own a11y.
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

    #[test]
    fn starts_visible_when_signal_true_emits_identity_skip() {
        // Initial visible=true → progress=1 → scale matrix = identity →
        // walker should NOT emit a PushTransform pair (identity skip).
        let visible = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Scale::new(visible).child(TextWidget::new_literal("hello")));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });
        let frame = tree.render();
        let push_count = frame
            .draw_order
            .iter()
            .filter(|c| matches!(c, bastyde_canvas::DrawCommand::PushTransform(_)))
            .count();
        assert_eq!(
            push_count, 0,
            "identity transform must be skipped, draw_order = {:?}",
            frame.draw_order
        );
    }

    #[test]
    fn starts_hidden_when_signal_false_emits_zero_scale() {
        // Initial visible=false → progress=0 → scale matrix has scale
        // factor 0 (degenerate). PushTransform should emit with that
        // matrix.
        let visible = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Scale::new(visible).child(TextWidget::new_literal("hello")));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });
        let frame = tree.render();
        let pushes: Vec<&Transform2D> = frame
            .draw_order
            .iter()
            .filter_map(|c| match c {
                bastyde_canvas::DrawCommand::PushTransform(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(pushes.len(), 1);
        // Scale factor lives at m[0] and m[3].
        assert!(pushes[0].m[0].abs() < 1e-3);
        assert!(pushes[0].m[3].abs() < 1e-3);
    }

    #[test]
    fn reflow_true_changes_layout_size() {
        // With reflow=true, the wrapper's bounds shrink as progress
        // ticks toward 0.
        let visible = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            Scale::new(visible.clone())
                .reflow(true)
                .duration(Duration::from_millis(100))
                .child(TextWidget::new_literal("content")),
        );
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let initial_h = tree.bounds(id).height;
        assert!(initial_h > 0.0);

        visible.set(false);
        // Drain pending animation onto scheduler, then tick to roughly
        // halfway through the 100ms tween.
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        tree.tick_animations(Duration::from_millis(50));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let mid_h = tree.bounds(id).height;
        assert!(
            mid_h < initial_h * 0.95,
            "halfway through scale-out, height ({}) should be visibly less than initial ({})",
            mid_h,
            initial_h,
        );
        assert!(
            mid_h > 0.0,
            "halfway through scale-out, height ({}) should not yet be zero",
            mid_h,
        );
    }

    #[test]
    fn reflow_false_keeps_layout_size_constant() {
        // Default (reflow=false): wrapper bounds stay at natural at
        // all progress values; only the visual scales.
        let visible = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            Scale::new(visible.clone())
                .duration(Duration::from_millis(100))
                .child(TextWidget::new_literal("content")),
        );
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let initial_size = tree.bounds(id).size();

        visible.set(false);
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        tree.tick_animations(Duration::from_millis(50));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let mid_size = tree.bounds(id).size();
        assert_eq!(
            initial_size, mid_size,
            "visual-only scale must not change layout"
        );
    }

    #[test]
    fn reduced_motion_snaps_scale() {
        let visible = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.set_accessibility_preferences(false, true, 1.0);
        tree.add(Scale::new(visible.clone()).child(TextWidget::new_literal("x")));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });

        visible.set(false);
        // to_or_snap under reduced motion sets directly — no animation
        // should be queued onto the scheduler.
        assert!(
            !tree.has_active_animations(),
            "reduced-motion path must not register a scale animation"
        );
    }
}
