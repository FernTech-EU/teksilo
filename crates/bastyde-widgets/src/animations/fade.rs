//! `Fade` — a wrapper widget that animates its child between hidden
//! (opacity 0) and visible (opacity 1) when an external
//! `Signal<bool>` toggles.
//!
//! Drives an `opacity: Signal<f32>` ∈ [0, 1] and applies it to its
//! own subtree via [`BuildContext::set_opacity`]. The framework's
//! render walker emits `SetOpacity(value)` before this widget's
//! paint and `RestoreOpacity` afterwards, so the multiplier composes
//! correctly with ancestor opacity scopes via the canvas's stacked
//! opacity model.
//!
//! ```ignore
//! let visible = ctx.signal(false);
//! ctx.add(Fade::new(visible.clone()).child(tooltip_content));
//! // ...elsewhere:
//! visible.set(true);  // fades in over `motion.duration_fast`
//! ```
//!
//! ## Layout semantics
//!
//! `Fade` does not change layout. The wrapped child reports its full
//! natural size at all opacity values, so reserving space for a
//! to-be-faded-in widget works the same whether the widget is fully
//! visible or fully hidden.
//!
//! For overlays where the dismiss should be *deferred* until the
//! fade-out completes (tooltip / popover / snackbar / dialog),
//! prefer [`OverlayRequest::with_fade`](crate::OverlayRequest)
//! instead — that path coordinates the dismiss with the tween so the
//! overlay survives until the opacity reaches zero.
//!
//! ## Reduced motion
//!
//! Honours `prefers-reduced-motion`: under reduced motion the
//! opacity snaps to its end value instead of tweening.

use bastyde_canvas::{Point, Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::{Prop, Signal};
use bastyde_core::widget::{LayoutContext, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

/// Wraps a child and animates the entire subtree's opacity between
/// 0 and 1, driven by an external `Signal<bool>`.
pub struct Fade {
    visible: Prop<bool>,
    pending_child: Option<PendingChild>,
    child_id: Option<WidgetId>,
    /// Cached so external integrations (and tests) can read the
    /// current animated opacity. Filled in on `build()`.
    opacity: Option<Signal<f32>>,
}

impl Fade {
    /// Build a fade wrapper bound to `visible`. Initially hidden iff
    /// `visible.get()` is `false` at the first `build()`.
    ///
    /// Accepts any `Prop<bool>` source — `Signal<bool>`, `Prop<bool>`,
    /// or a plain `bool` (for static "always visible" / "always
    /// hidden" cases without a tween).
    pub fn new(visible: impl Into<Prop<bool>>) -> Self {
        Self {
            visible: visible.into(),
            pending_child: None,
            child_id: None,
            opacity: None,
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

impl std::fmt::Debug for Fade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fade").finish()
    }
}

impl Widget for Fade {
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

        let initial = if self.visible.get() { 1.0 } else { 0.0 };
        let opacity = ctx.animated_signal(initial);
        self.opacity = Some(opacity.clone());

        // Apply the opacity scope to *this* widget so the entire
        // subtree (the wrapped child) inherits the multiplier.
        let id = ctx.self_id();
        ctx.set_opacity(id, opacity.clone());

        // Tween on visibility flips. Static `Prop::Static(_)` doesn't
        // need an observer — the initial opacity already matches.
        if let Prop::Bound(visible_signal) = &self.visible {
            let visible_signal = visible_signal.clone();
            let fade_anim = ctx.animate().fast().standard();
            let opacity_for_effect = opacity;
            ctx.effect(&visible_signal, move |&v| {
                let target = if v { 1.0 } else { 0.0 };
                fade_anim.to_or_snap(&opacity_for_effect, target);
            });
        }

        vec![child_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // Layout-transparent: report the child's natural size at all
        // opacity values. A faded-out tooltip still occupies its
        // future visible footprint so `Fade` doesn't drive layout
        // jitter when used purely as a visual modulator.
        self.child_id
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
            child.origin = Point::new(bounds.x, bounds.y);
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // Fade is a visual-modulation wrapper. The wrapped subtree
        // owns its own a11y semantics; this wrapper is intentionally
        // a11y-transparent. Note: a fully-faded-out widget is still
        // reported by AT — callers who want true visibility-driven
        // a11y should pair `Fade` with `visible_when` on the same
        // signal.
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::primitives::{RectWidget, TextWidget};
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_tokens::Color;

    fn count_set_opacity(frame: &bastyde_canvas::RenderFrame) -> Vec<f32> {
        frame
            .draw_order
            .iter()
            .filter_map(|c| match c {
                bastyde_canvas::DrawCommand::SetOpacity(v) => Some(*v),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn starts_hidden_when_signal_is_false() {
        let visible = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Fade::new(visible.clone()).child(RectWidget::new().background(Color::RED)));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();
        // Sub-perceptual opacity skips the subtree entirely — no
        // SetOpacity, and the red child must not paint.
        assert!(count_set_opacity(&frame).is_empty());
        assert!(
            !frame
                .shapes
                .iter()
                .any(|s| s.color == Color::RED.to_array()),
            "hidden subtree must not paint"
        );
    }

    #[test]
    fn starts_visible_when_signal_is_true() {
        let visible = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Fade::new(visible.clone()).child(RectWidget::new().background(Color::RED)));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();
        // Initially visible: opacity 1.0 emits exactly one SetOpacity
        // pair (the framework still wraps the subtree even at 1.0 so
        // descendant opacity scopes compose correctly).
        let ops = count_set_opacity(&frame);
        assert_eq!(ops.len(), 1);
        assert!((ops[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn flipping_signal_drives_animation() {
        let visible = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Fade::new(visible.clone()).child(TextWidget::new_literal("payload")));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        visible.set(true);
        // Halfway through the 120 ms fast tween: opacity should be
        // visibly between 0 and 1.
        tree.tick_animations(Duration::from_millis(60));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();
        let ops = count_set_opacity(&frame);
        assert_eq!(ops.len(), 1, "exactly one opacity scope should be active");
        assert!(
            ops[0] > 0.05 && ops[0] < 0.95,
            "mid-tween opacity should be between 0 and 1, got {}",
            ops[0]
        );
    }

    #[test]
    fn animation_completes_at_target() {
        let visible = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Fade::new(visible.clone()).child(TextWidget::new_literal("payload")));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        visible.set(true);
        tree.tick_animations(Duration::from_millis(200));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();
        let ops = count_set_opacity(&frame);
        assert_eq!(ops.len(), 1);
        assert!(
            (ops[0] - 1.0).abs() < 0.01,
            "post-tween opacity should be 1.0, got {}",
            ops[0]
        );
    }

    #[test]
    fn fade_does_not_change_layout() {
        // The wrapped child reports its natural size; the wrapper
        // bounds match it regardless of opacity.
        let visible = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(Fade::new(visible.clone()).child(TextWidget::new_literal("hello")));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let hidden_bounds = tree.bounds(id);

        visible.set(true);
        tree.tick_animations(Duration::from_millis(200));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let visible_bounds = tree.bounds(id);

        assert_eq!(
            hidden_bounds.size(),
            visible_bounds.size(),
            "Fade must not change its own size based on opacity"
        );
    }

    #[test]
    fn static_visible_does_not_register_observer() {
        // `Fade::new(true)` (literal bool) is `Prop::Static(true)` —
        // no observer registered, no animation kick-off, just a
        // static fully-visible scope. Verify the wrapper still works
        // (renders the child) but that no animation is queued.
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Fade::new(true).child(RectWidget::new().background(Color::RED)));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let _ = tree.render();
        assert!(
            !tree.has_active_animations(),
            "static Prop must not start a fade animation"
        );
    }
}
