//! FocusRing — Int UI focus-ring wrapper.
//!
//! Wraps a child widget, reserves an envelope of
//! `theme.shape.focus_ring_offset + theme.shape.focus_ring_width` on every
//! side, and paints a 2 dp ring **outside** the child's visual bounds when
//! the bound `focused` signal is `true`. The ring is drawn inside the
//! outer bounds so it is never clipped by a parent layout.
//!
//! This implements the Int UI convention (Section 7 of the v2 reference):
//! emphasis comes from a ring outside the control, not from thickening its
//! border. Widget size is the visual size plus `2 * envelope`, so parent
//! layouts must be aware that a focusable control takes up slightly more
//! space than its painted footprint.
//!
//! Usage:
//!
//! ```ignore
//! let visual = ctx.add(/* build the widget's visual subtree */);
//! let focused = interaction.map(|s| *s == InteractionState::Focused);
//! let wrapped = ctx.add(
//!     FocusRing::new(focused)
//!         .corner_radius(theme.components.button.corner_radius)
//!         .set_child(visual),
//! );
//! ```

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::binding::BindingLevel;
use fern_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius};

use crate::primitives::{Padding, RectWidget, ZStack};

/// A focus-ring wrapper. See module docs for the geometry.
pub struct FocusRing {
    focused: Signal<bool>,
    inner_corner_radius: f32,
    pending_child: Option<PendingChild>,
    root_id: Option<WidgetId>,
}

impl std::fmt::Debug for FocusRing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusRing")
            .field("inner_corner_radius", &self.inner_corner_radius)
            .finish()
    }
}

impl FocusRing {
    /// Create a new focus ring bound to a `Signal<bool>` where `true` means
    /// "focused, show the ring". The inner corner radius defaults to 4 dp
    /// (matching `radius_control`); override with [`corner_radius`].
    pub fn new(focused: Signal<bool>) -> Self {
        Self {
            focused,
            inner_corner_radius: 4.0,
            pending_child: None,
            root_id: None,
        }
    }

    /// Set the corner radius of the wrapped control. The ring's own corner
    /// radius is computed as `inner_corner_radius + focus_ring_offset +
    /// focus_ring_width / 2` so the ring looks concentric with the control.
    pub fn corner_radius(mut self, radius: f32) -> Self {
        self.inner_corner_radius = radius;
        self
    }

    /// Wrap a pre-registered child (recommended — matches the widget's own
    /// subtree-building pattern).
    pub fn set_child(mut self, id: WidgetId) -> Self {
        self.pending_child = Some(PendingChild::Id(id));
        self
    }

    /// Wrap an inline child.
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_child = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }
}

impl Widget for FocusRing {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let offset = theme.shape.focus_ring_offset;
        let width = theme.shape.focus_ring_width;
        let half_stroke = width * 0.5;
        let envelope = offset + width;

        // Resolve the wrapped visual
        let inner_id = match self.pending_child.take() {
            Some(PendingChild::Id(id)) => id,
            Some(PendingChild::Deferred(w)) => ctx.add_boxed(w),
            None => {
                self.root_id = None;
                return Vec::new();
            }
        };

        // Bind the ring color to the focused signal. The RectWidget registers
        // its border_color Prop for repaint automatically.
        let focus_color = theme.colors.focus_ring;
        let ring_color = self.focused.map(move |f| {
            if *f {
                focus_color
            } else {
                Color::TRANSPARENT
            }
        });

        // Ring rect: drawn at `outer - half_stroke` on each side so the
        // stroke fits entirely inside the outer bounds. Its corner radius
        // is set so the stroke centerline is concentric with the visual.
        let ring_radius = self.inner_corner_radius + offset + half_stroke;
        let ring_rect = RectWidget::new()
            .bind_border_color(ring_color)
            .border_width(width)
            .corner_radius(CornerRadius::uniform(ring_radius));
        let ring_rect_id = ctx.add(ring_rect);
        let ring_padded = ctx.add(Padding::uniform(half_stroke).set_child(ring_rect_id));

        // Visual: inset by the full envelope so its edge is `offset` away
        // from the ring's inner edge.
        let visual_padded = ctx.add(Padding::uniform(envelope).set_child(inner_id));

        let root = ctx.add(
            ZStack::new()
                .add_child(ring_padded)
                .add_child(visual_padded),
        );
        self.root_id = Some(root);

        // Make sure the widget is repainted when focus state flips — even
        // though the derived `ring_color` Prop already triggers the ring
        // rect, we also bind the source signal to this widget for safety.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.focused
            .bind_to(self_id, registry, BindingLevel::RepaintOnly);

        vec![root]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        self.root_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
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

    fn paint(&self, _bounds: Rect, _canvas: &mut Canvas, _ctx: &PaintContext) {}

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}

    fn children(&self) -> Vec<WidgetId> {
        self.root_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn size_that_fits(&self, _p: SizeProposal, _ctx: &LayoutContext) -> Size {
            Size::new(self.0, self.1)
        }
    }

    #[test]
    fn focus_ring_reserves_envelope_margin() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let focused = Signal::new(false);
        let leaf = tree.add(FixedLeaf(20.0, 20.0));
        let wrapped = tree.add(FocusRing::new(focused).set_child(leaf));
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let bounds = tree.bounds(wrapped);
        // Default theme: focus_ring_offset=2, focus_ring_width=2 → envelope=4
        // Wrapped size = 20 + 2*4 = 28
        assert!((bounds.width - 28.0).abs() < 0.01, "{}", bounds.width);
        assert!((bounds.height - 28.0).abs() < 0.01, "{}", bounds.height);
    }

    #[test]
    fn ring_only_paints_when_focused() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let focused = Signal::new(false);
        let leaf = tree.add(FixedLeaf(20.0, 20.0));
        let _wrapped = tree.add(FocusRing::new(focused.clone()).set_child(leaf));
        tree.layout(SizeProposal::exact(28.0, 28.0));

        // Rounded-rect strokes land in `shapes` (ShapeQuad with stroke_width > 0).
        // Unfocused: the ring's border_color is TRANSPARENT, so no visible stroke.
        let frame = tree.render();
        let visible_ring = frame
            .shapes
            .iter()
            .any(|s| s.stroke_width > 0.0 && s.color[3] > 0.0);
        assert!(
            !visible_ring,
            "unfocused FocusRing should not paint a visible stroke"
        );

        focused.set(true);
        tree.layout(SizeProposal::exact(28.0, 28.0));
        let frame = tree.render();
        let visible_ring = frame
            .shapes
            .iter()
            .any(|s| s.stroke_width > 0.0 && s.color[3] > 0.0);
        assert!(
            visible_ring,
            "focused FocusRing should paint a visible stroke"
        );
    }
}
