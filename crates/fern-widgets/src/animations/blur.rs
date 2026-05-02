//! `Blur` — a wrapper widget that applies a Gaussian-equivalent blur
//! to its child subtree, driven by a `Prop<f32>` radius (in logical
//! pixels).
//!
//! Built on [`BuildContext::set_blur`], a per-node paint scope parallel
//! to `set_opacity` and `set_transform`. The framework's render walker
//! emits `BeginBlurredSubtree { bounds, radius }` before this widget's
//! paint and `EndBlurredSubtree` afterwards; the renderer redirects
//! drawing into an intermediate texture, runs a dual-Kawase blur chain
//! at the requested radius, and composites the blurred result back into
//! the parent pass.
//!
//! Sub-perceptual radii (< 0.5 px) skip the Begin/End pair entirely so
//! animated `0 → target_radius` enable patterns have zero per-frame
//! cost when fully off.
//!
//! ```ignore
//! // Static frosted-glass backdrop:
//! ctx.add(Blur::new(15.0).child(modal_backdrop));
//!
//! // Click-to-reveal sensitive content:
//! let visible = ctx.signal(false);
//! let radius = visible.map(|&v| if v { 0.0 } else { 12.0 });
//! ctx.add(Blur::new(radius).child(secret_text));
//!
//! // Animated frosted-glass on modal show:
//! let radius = ctx.animated_signal(0.0_f32);
//! ctx.animate().normal().standard().to_or_snap(&radius, 15.0);
//! ctx.add(Blur::new(radius).child(content));
//! ```
//!
//! ## Layout semantics
//!
//! `Blur` does not change layout. The wrapped child reports its full
//! natural size at all blur radii; only the visual paint output is
//! affected.
//!
//! ## Performance
//!
//! Blur is the most expensive paint scope in the framework — every
//! enabled blur scope drives N+M+1 small render passes per frame
//! (N downsamples, M upsamples, +1 composite). Don't put it on
//! widgets that animate every frame at full radius. For "fade-blur on
//! reveal" patterns, animate the radius up to a static value and leave
//! it there. See `docs/animation.md` §5.8.

use fern_canvas::{Point, Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::signal::Prop;
use fern_core::widget::{LayoutContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

/// Wraps a child and applies a Gaussian-equivalent blur to the entire
/// subtree, driven by an external `Prop<f32>` radius (logical pixels).
pub struct Blur {
    radius: Prop<f32>,
    pending_child: Option<PendingChild>,
    child_id: Option<WidgetId>,
}

impl Blur {
    /// Build a blur wrapper bound to `radius` (in logical pixels).
    /// Accepts any `Prop<f32>` source — `f32`, `Signal<f32>`, or
    /// `Prop<f32>`. Sub-perceptual radii (< 0.5) are a no-op.
    pub fn new(radius: impl Into<Prop<f32>>) -> Self {
        Self {
            radius: radius.into(),
            pending_child: None,
            child_id: None,
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

impl std::fmt::Debug for Blur {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Blur").finish()
    }
}

impl Widget for Blur {
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

        let id = ctx.self_id();
        ctx.set_blur(id, self.radius.clone());

        vec![child_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        // Layout-transparent: report the child's natural size at all
        // blur radii.
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
        // Blur is a visual-modulation wrapper. The wrapped subtree owns
        // its own a11y semantics; this wrapper is intentionally
        // a11y-transparent. Note: a blurred-out widget is still reported
        // by AT — callers who want to actually hide content from
        // assistive tech should pair `Blur` with `visible_when`.
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{FixedSize, RectWidget};
    use fern_core::signal::Signal;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::{Color, Theme};

    fn collect_blur_radii(frame: &fern_canvas::RenderFrame) -> Vec<f32> {
        frame
            .draw_order
            .iter()
            .filter_map(|c| match c {
                fern_canvas::DrawCommand::BeginBlurredSubtree { radius, .. } => Some(*radius),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn static_radius_emits_begin_end_pair() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(Blur::new(8.0_f32).child(RectWidget::new().background(Color::RED)));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();

        let radii = collect_blur_radii(&frame);
        assert_eq!(radii.len(), 1);
        assert!((radii[0] - 8.0).abs() < 1e-6);
        let ends = frame
            .draw_order
            .iter()
            .filter(|c| matches!(c, fern_canvas::DrawCommand::EndBlurredSubtree))
            .count();
        assert_eq!(ends, 1);
    }

    #[test]
    fn subperceptual_radius_skipped() {
        // Below the 0.5 threshold: walker emits no Begin/End pair so
        // animated 0→target patterns have zero cost when fully off.
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(Blur::new(0.0_f32).child(RectWidget::new().background(Color::RED)));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();
        assert!(collect_blur_radii(&frame).is_empty());
    }

    #[test]
    fn dynamic_radius_signal_drives_emitted_value() {
        let radius = Signal::new(4.0_f32);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(Blur::new(radius.clone()).child(RectWidget::new().background(Color::RED)));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();
        assert_eq!(collect_blur_radii(&frame), vec![4.0]);

        radius.set(20.0);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();
        assert_eq!(collect_blur_radii(&frame), vec![20.0]);
    }

    #[test]
    fn layout_size_unchanged_by_blur() {
        let radius = Signal::new(0.0_f32);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(
            Blur::new(radius.clone())
                .child(FixedSize::new().bind_width(120.0).bind_height(40.0).child(RectWidget::new())),
        );
        tree.layout(SizeProposal::exact(300.0, 200.0));
        let off_bounds = tree.bounds(id);

        radius.set(20.0);
        tree.layout(SizeProposal::exact(300.0, 200.0));
        let on_bounds = tree.bounds(id);

        assert_eq!(
            off_bounds.size(),
            on_bounds.size(),
            "Blur must not change its own size with radius"
        );
    }

    #[test]
    fn begin_carries_widget_bounds() {
        // The Begin command's `bounds` field must match the wrapper
        // widget's actual placed bounds — that's what the renderer uses
        // to size the intermediate texture and to position the
        // composite blit.
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(
            Blur::new(8.0_f32).child(FixedSize::new().bind_width(80.0).bind_height(40.0).child(RectWidget::new())),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let bounds = tree.bounds(id);
        let frame = tree.render();

        let begin = frame
            .draw_order
            .iter()
            .find_map(|c| match c {
                fern_canvas::DrawCommand::BeginBlurredSubtree { bounds, radius } => {
                    Some((*bounds, *radius))
                }
                _ => None,
            })
            .expect("Begin emitted");
        assert_eq!(begin.0, bounds);
        assert!((begin.1 - 8.0).abs() < 1e-6);
    }
}
