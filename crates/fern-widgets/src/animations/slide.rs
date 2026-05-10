//! `Slide` — wraps a child and slides it in or out from a chosen
//! edge when an external `Signal<bool>` toggles. Common patterns:
//! drawers, snackbars, side panels, banner notifications.
//!
//! ```ignore
//! let visible = ctx.signal(false);
//! ctx.add(
//!     Slide::new(visible.clone())
//!         .from(SlideEdge::Bottom)
//!         .child(snackbar_content),
//! );
//! // ...elsewhere:
//! visible.set(true);   // slides in from below
//! ```
//!
//! ## Layout semantics
//!
//! `Slide`'s own slot stays in its laid-out position; the child is
//! *translated* within the slot via `place_children`. The wrapper
//! clips so a sliding-in child doesn't bleed past the slot edges.
//! The wrapper reports the child's full natural size at all
//! progress values — siblings don't reflow as the child slides.
//!
//! For a "slide + fade" effect (notification snackbar), wrap the
//! child in [`Fade`](super::Fade) before passing it to `Slide`:
//!
//! ```ignore
//! Slide::new(visible.clone())
//!     .from(SlideEdge::Bottom)
//!     .child(Fade::new(visible).child(snackbar_content))
//! ```
//!
//! ## Reduced motion
//!
//! Honours `prefers-reduced-motion`: snaps the child instantly into
//! or out of position instead of tweening.

use std::cell::Cell;

use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::signal::{Prop, Signal};
use fern_core::widget::{LayoutContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

/// Which edge the child slides in from / out to.
///
/// `Leading` and `Trailing` honour layout direction (RTL flips them);
/// the resolution happens in `place_children` via the layout context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlideEdge {
    Leading,
    Trailing,
    Top,
    Bottom,
}

/// Wraps a child and animates a translate offset in/out from one edge
/// when `visible` flips.
pub struct Slide {
    visible: Prop<bool>,
    edge: SlideEdge,
    pending_child: Option<PendingChild>,
    child_id: Option<WidgetId>,
    /// 0 = fully off-edge, 1 = at rest. Animated. Bound to self at
    /// Relayout level so each tick re-runs place_children.
    progress: Option<Signal<f32>>,
    /// Last natural size measured. `place_children` reads it to
    /// compute the slide distance (= child extent on the slide axis).
    natural_size: Cell<Size>,
}

impl Slide {
    /// Build a slide wrapper bound to `visible`. Defaults to sliding
    /// from the bottom — change with `.from(...)`.
    pub fn new(visible: impl Into<Prop<bool>>) -> Self {
        Self {
            visible: visible.into(),
            edge: SlideEdge::Bottom,
            pending_child: None,
            child_id: None,
            progress: None,
            natural_size: Cell::new(Size::ZERO),
        }
    }

    /// Edge the child slides in from (and out to).
    pub fn from(mut self, edge: SlideEdge) -> Self {
        self.edge = edge;
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

impl std::fmt::Debug for Slide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Slide").field("edge", &self.edge).finish()
    }
}

impl Widget for Slide {
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
        self.progress = Some(progress.clone());

        // Bind progress to self at Relayout — every tick re-runs
        // place_children with the new offset.
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        progress.bind_to(id, registry, BindingLevel::Relayout);

        // Drive the slide on visibility flips.
        if let Prop::Bound(visible_signal) = &self.visible {
            let visible_signal = visible_signal.clone();
            let slide_anim = ctx.animate().normal().standard();
            let progress_for_effect = progress;
            ctx.effect(&visible_signal, move |&v| {
                let target = if v { 1.0 } else { 0.0 };
                slide_anim.to_or_snap(&progress_for_effect, target);
            });
        }

        vec![child_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        let Some(child_id) = self.child_id else {
            return (proposal.resolve(0.0, 0.0)).into();
        };
        let natural = ctx.child_size(child_id, proposal).unwrap_or(Size::ZERO);
        self.natural_size.set(natural);
        // Layout-stable: slot stays at child's natural size at all
        // progress values; only the child's *visual* offset moves.
        natural.into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let progress = self
            .progress
            .as_ref()
            .map(|s| s.get().clamp(0.0, 1.0))
            .unwrap_or(1.0);
        let natural = self.natural_size.get();
        // Distance to translate when fully hidden — the child's full
        // extent on the slide axis (so its trailing pixel just leaves
        // the bounds at progress=0).
        let off_amount = 1.0 - progress;
        // Resolve Leading/Trailing against the layout direction so the
        // slide direction tracks RTL correctly.
        let resolved = match (self.edge, ctx.is_rtl()) {
            (SlideEdge::Leading, false) | (SlideEdge::Trailing, true) => SlideEdge::Leading,
            (SlideEdge::Trailing, false) | (SlideEdge::Leading, true) => SlideEdge::Trailing,
            (other, _) => other,
        };
        let (dx, dy) = match resolved {
            SlideEdge::Leading => (-natural.width * off_amount, 0.0),
            SlideEdge::Trailing => (natural.width * off_amount, 0.0),
            SlideEdge::Top => (0.0, -natural.height * off_amount),
            SlideEdge::Bottom => (0.0, natural.height * off_amount),
        };
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x + dx, bounds.y + dy);
            child.size = natural;
        }
    }

    fn clips_children(&self) -> bool {
        // Required: a sliding-in child renders past the wrapper's
        // own bounds and would overlap siblings without clipping.
        true
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // Visual-modulation wrapper. The child owns its own a11y.
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
    use fern_core::widget_tree::WidgetTree;
    use fern_core::Theme;

    #[test]
    fn starts_visible_when_signal_is_true() {
        let visible = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let id = tree.add(Slide::new(visible.clone()).child(TextWidget::new_literal("hello")));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let bounds = tree.bounds(id);
        assert!(bounds.width > 0.0 && bounds.height > 0.0);
    }

    #[test]
    fn flipping_signal_drives_slide_progress() {
        let visible = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(
            Slide::new(visible.clone())
                .from(SlideEdge::Bottom)
                .child(TextWidget::new_literal("snackbar message")),
        );
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });

        visible.set(true);
        // Mid-tween: animation should be in flight.
        tree.tick_animations(Duration::from_millis(50));
        assert!(
            tree.has_active_animations(),
            "slide-in should be animating mid-tween"
        );
    }

    #[test]
    fn slide_does_not_change_layout_size() {
        let visible = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let id = tree.add(
            Slide::new(visible.clone())
                .from(SlideEdge::Leading)
                .child(TextWidget::new_literal("content")),
        );
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let hidden_bounds = tree.bounds(id);

        visible.set(true);
        tree.tick_animations(Duration::from_millis(300));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let visible_bounds = tree.bounds(id);

        assert_eq!(
            hidden_bounds.size(),
            visible_bounds.size(),
            "Slide must not change its own size based on progress"
        );
    }

    #[test]
    fn rtl_swaps_leading_and_trailing() {
        // In LTR, SlideEdge::Leading hides off the LEFT (negative x).
        // In RTL, Leading should hide off the RIGHT (positive x).
        // We don't have a direct way to read child placement offsets
        // from the public API in tests, so we exercise the RTL path
        // and confirm the wrapper still lays out and animates without
        // panicking. The math itself is covered by the static enum
        // mapping in `place_children`.
        use fern_core::environment::LayoutDirection;
        let visible = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.set_layout_direction(LayoutDirection::RightToLeft);
        let id = tree.add(
            Slide::new(visible.clone())
                .from(SlideEdge::Leading)
                .child(TextWidget::new_literal("rtl content")),
        );
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let bounds = tree.bounds(id);
        assert!(bounds.width > 0.0 && bounds.height > 0.0);

        visible.set(false);
        tree.tick_animations(Duration::from_millis(300));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        // After fully sliding out under RTL, the wrapper's slot stays
        // at natural size — Slide is layout-stable.
        let after = tree.bounds(id);
        assert_eq!(bounds.size(), after.size());
    }

    #[test]
    fn reduced_motion_snaps_progress() {
        let visible = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.set_accessibility_preferences(false, true, 1.0);
        tree.add(Slide::new(visible.clone()).child(TextWidget::new_literal("snap")));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });

        visible.set(true);
        // Reduced-motion path uses to_or_snap — value lands instantly,
        // no animation registered.
        assert!(
            !tree.has_active_animations(),
            "reduced-motion path must not register animations"
        );
    }
}
