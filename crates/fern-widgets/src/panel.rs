//! Panel — a themed container with background, border, corner radius, and padding.
//!
//! Like Qt's QFrame: a single-child wrapper that provides visual framing.
//! Visual properties come from the theme by default but can be overridden.
//!
//! Panel is a Level 2 Widget (not a Widget) because its internal
//! structure is fixed and doesn't need reactive state or rebuild on theme change.
//! It reads theme tokens during layout and paint directly from the context.

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::color_prop::ColorProp;
use fern_core::signal::Prop;
use fern_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::CornerRadius;
#[cfg(test)]
use fern_tokens::Color;

/// A themed container with background, border, corner radius, and padding.
#[derive(Debug)]
pub struct Panel {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
    background: Option<ColorProp>,
    border_color: Option<ColorProp>,
    border_width: Option<Prop<f32>>,
    corner_radius: Option<Prop<f32>>,
    padding: Option<Prop<f32>>,
    a11y_presentational: bool,
}

impl Panel {
    pub fn new() -> Self {
        Self {
            child_id: None,
            pending_child: None,
            background: None,
            border_color: None,
            border_width: None,
            corner_radius: None,
            padding: None,
            a11y_presentational: false,
        }
    }

    /// Mark the panel as presentational for assistive tech: the panel's
    /// own a11y node is hidden so its wrapping chrome (background,
    /// border, padding) doesn't introduce a spurious `Group` node
    /// between an outer widget (Toolbar, StatusBar, etc.) and the
    /// real content. Children remain visible in the a11y tree.
    pub fn a11y_presentational(mut self) -> Self {
        self.a11y_presentational = true;
        self
    }

    /// Set child by pre-registered ID.
    pub fn child_id(mut self, id: WidgetId) -> Self {
        self.pending_child = Some(PendingChild::Id(id));
        self
    }

    /// Set an inline child widget (deferred insertion).
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_child = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Override the background. Accepts `Color`, a [`SurfaceRole`](fern_tokens::SurfaceRole),
    /// or a `Signal<Color>`. Default (unset) is `SurfaceRole::Main`.
    pub fn background(mut self, color: impl Into<ColorProp>) -> Self {
        self.background = Some(color.into());
        self
    }

    /// Override the border color. Accepts `Color`, a [`BorderRole`](fern_tokens::BorderRole),
    /// or a `Signal<Color>`. Default (unset) is `BorderRole::Default`.
    pub fn border_color(mut self, color: impl Into<ColorProp>) -> Self {
        self.border_color = Some(color.into());
        self
    }

    /// Override the border width (default: 0 — no border).
    /// Accepts a static `f32` or a reactive `Signal<f32>`.
    pub fn border_width(mut self, width: impl Into<Prop<f32>>) -> Self {
        self.border_width = Some(width.into());
        self
    }

    /// Override the corner radius (default: theme `radius_popup`).
    /// Accepts a static `f32` or a reactive `Signal<f32>`.
    pub fn corner_radius(mut self, radius: impl Into<Prop<f32>>) -> Self {
        self.corner_radius = Some(radius.into());
        self
    }

    /// Override the padding (default: theme `components.panel.padding`).
    /// Accepts a static `f32` or a reactive `Signal<f32>`.
    pub fn padding(mut self, padding: impl Into<Prop<f32>>) -> Self {
        self.padding = Some(padding.into());
        self
    }

    fn resolve_padding(&self, theme: &fern_tokens::Theme) -> f32 {
        self.padding
            .as_ref()
            .map(|p| p.get())
            .unwrap_or(theme.components.panel.padding)
    }
}

impl Default for Panel {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Panel {
    fn build(&mut self, ctx: &mut fern_core::build_context::BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        // Register dirty-tracking on any reactive props so signal updates
        // (e.g. theme signal changes) trigger a repaint / relayout.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        if let Some(p) = &self.background {
            p.register_if_bound(self_id, registry, fern_core::binding::BindingLevel::RepaintOnly);
        }
        if let Some(p) = &self.border_color {
            p.register_if_bound(self_id, registry, fern_core::binding::BindingLevel::RepaintOnly);
        }
        if let Some(p) = &self.border_width {
            p.register_if_bound(self_id, registry, fern_core::binding::BindingLevel::RepaintOnly);
        }
        if let Some(p) = &self.corner_radius {
            p.register_if_bound(self_id, registry, fern_core::binding::BindingLevel::RepaintOnly);
        }
        if let Some(p) = &self.padding {
            p.register_if_bound(self_id, registry, fern_core::binding::BindingLevel::Relayout);
        }
        self.child_id.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        let pad = self.resolve_padding(ctx.theme);
        let inset = pad * 2.0;

        if let Some(child_id) = self.child_id {
            let inner_proposal = SizeProposal {
                width: proposal.width.map(|w| (w - inset).max(0.0)),
                height: proposal.height.map(|h| (h - inset).max(0.0)),
            };
            if let Some(child_size) = ctx.child_size(child_id, inner_proposal) {
                return (Size::new(child_size.width + inset, child_size.height + inset)).into();
            }
        }

        proposal.resolve(inset, inset).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let pad = self.resolve_padding(ctx.theme);
        for child in children.iter_mut() {
            child.origin = fern_canvas::Point::new(bounds.x + pad, bounds.y + pad);
            child.size = Size::new(
                (bounds.width - pad * 2.0).max(0.0),
                (bounds.height - pad * 2.0).max(0.0),
            );
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let bg = self
            .background
            .as_ref()
            .map(|p| p.resolve(ctx.theme))
            .unwrap_or(ctx.theme.colors.surface_main);
        let radius = self
            .corner_radius
            .as_ref()
            .map(|p| p.get())
            .unwrap_or(ctx.theme.shape.radius_popup);
        let border_w = self.border_width.as_ref().map(|p| p.get()).unwrap_or(0.0);

        canvas.fill_rounded_rect(bounds, CornerRadius::uniform(radius), bg);

        if border_w > 0.0 {
            let border = self
                .border_color
                .as_ref()
                .map(|p| p.resolve(ctx.theme))
                .unwrap_or(ctx.theme.colors.border);
            canvas.stroke_rounded_rect(bounds, CornerRadius::uniform(radius), border, border_w);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        if self.a11y_presentational {
            builder.set_hidden();
            return;
        }
        builder.set_role(fern_core::accesskit::Role::Group);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
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
        fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    #[test]
    fn panel_adds_padding_to_child_size() {
        let theme = Theme::light_default();
        let mut tree = WidgetTree::new().with_theme(theme.clone());
        let child = tree.add(FixedLeaf(80.0, 40.0));
        let panel = tree.add(Panel::new().padding(10.0).child_id(child));
        tree.layout(SizeProposal::unspecified());

        let pb = tree.bounds(panel);
        assert!((pb.width - 100.0).abs() < 0.01); // 80 + 10*2
        assert!((pb.height - 60.0).abs() < 0.01); // 40 + 10*2
    }

    #[test]
    fn panel_child_positioned_with_padding() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let child = tree.add(FixedLeaf(80.0, 40.0));
        let _panel = tree.add(Panel::new().padding(12.0).child_id(child));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let cb = tree.bounds(child);
        assert!((cb.x - 12.0).abs() < 0.01);
        assert!((cb.y - 12.0).abs() < 0.01);
    }

    #[test]
    fn panel_paints_background() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let child = tree.add(FixedLeaf(50.0, 30.0));
        let _panel = tree.add(
            Panel::new()
                .background(Color::RED)
                .corner_radius(8.0)
                .child_id(child),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let frame = tree.render();
        assert!(
            !frame.shapes.is_empty(),
            "panel should render a background shape"
        );
    }
}
