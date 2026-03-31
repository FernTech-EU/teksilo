//! Panel — a themed container with background, border, corner radius, and padding.
//!
//! Like Qt's QFrame: a single-child wrapper that provides visual framing.
//! Visual properties come from the theme by default but can be overridden.
//!
//! Panel is a Level 2 Widget (not a CompositeWidget) because its internal
//! structure is fixed and doesn't need reactive state or rebuild on theme change.
//! It reads theme tokens during layout and paint directly from the context.

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::widget::{LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius};

/// A themed container with background, border, corner radius, and padding.
#[derive(Debug)]
pub struct Panel {
    child_id: Option<WidgetId>,
    background: Option<Color>,
    border_color: Option<Color>,
    border_width: Option<f32>,
    corner_radius: Option<f32>,
    padding: Option<f32>,
}

impl Panel {
    pub fn new() -> Self {
        Self {
            child_id: None,
            background: None,
            border_color: None,
            border_width: None,
            corner_radius: None,
            padding: None,
        }
    }

    pub fn set_child(mut self, id: WidgetId) -> Self {
        self.child_id = Some(id);
        self
    }

    /// Override the background color (default: theme `surface_secondary`).
    pub fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    /// Override the border color (default: theme `border`).
    pub fn border_color(mut self, color: Color) -> Self {
        self.border_color = Some(color);
        self
    }

    /// Override the border width (default: 0 — no border).
    pub fn border_width(mut self, width: f32) -> Self {
        self.border_width = Some(width);
        self
    }

    /// Override the corner radius (default: theme `radius_md`).
    pub fn corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = Some(radius);
        self
    }

    /// Override the padding (default: theme `content_padding`).
    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = Some(padding);
        self
    }

    fn resolve_padding(&self, theme: &fern_tokens::Theme) -> f32 {
        self.padding.unwrap_or(theme.spacing.content_padding)
    }
}

impl Default for Panel {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Panel {
    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let pad = self.resolve_padding(ctx.theme);
        let inset = pad * 2.0;

        if let Some(child_id) = self.child_id {
            let inner_proposal = SizeProposal {
                width: proposal.width.map(|w| (w - inset).max(0.0)),
                height: proposal.height.map(|h| (h - inset).max(0.0)),
            };
            if let Some(child_size) = ctx.child_size(child_id, inner_proposal) {
                return Size::new(child_size.width + inset, child_size.height + inset);
            }
        }

        proposal.resolve(inset, inset)
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
        let bg = self.background.unwrap_or(ctx.theme.colors.surface_secondary);
        let radius = self
            .corner_radius
            .unwrap_or(ctx.theme.shape.radius_md);
        let border_w = self.border_width.unwrap_or(0.0);

        canvas.fill_rounded_rect(bounds, CornerRadius::uniform(radius), bg);

        if border_w > 0.0 {
            let border = self.border_color.unwrap_or(ctx.theme.colors.border);
            canvas.stroke_rounded_rect(bounds, CornerRadius::uniform(radius), border, border_w);
        }
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
        fn size_that_fits(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
            Size::new(self.0, self.1)
        }
    }

    #[test]
    fn panel_adds_padding_to_child_size() {
        let theme = Theme::light_default();
        let mut tree = WidgetTree::new().with_theme(theme.clone());
        let child = tree.add(FixedLeaf(80.0, 40.0));
        let panel = tree.add(Panel::new().padding(10.0).set_child(child));
        tree.layout(SizeProposal::unspecified());

        let pb = tree.bounds(panel);
        assert!((pb.width - 100.0).abs() < 0.01); // 80 + 10*2
        assert!((pb.height - 60.0).abs() < 0.01); // 40 + 10*2
    }

    #[test]
    fn panel_child_positioned_with_padding() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let child = tree.add(FixedLeaf(80.0, 40.0));
        let _panel = tree.add(Panel::new().padding(12.0).set_child(child));
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
                .set_child(child),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let frame = tree.render();
        assert!(!frame.shapes.is_empty(), "panel should render a background shape");
    }
}
