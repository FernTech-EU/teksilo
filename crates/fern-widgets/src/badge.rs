//! Badge — a pill-shaped label for tags, status indicators, and counts.

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::widget::{LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius};

use crate::primitives::{Padding, RectWidget, TextWidget, ZStack};

/// A pill-shaped label for displaying tags, counts, or status.
pub struct Badge {
    label: String,
    color: Option<Color>,
    text_color: Option<Color>,
    root_child_id: Option<WidgetId>,
}

impl Badge {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            color: None,
            text_color: None,
            root_child_id: None,
        }
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    pub fn text_color(mut self, color: Color) -> Self {
        self.text_color = Some(color);
        self
    }
}

impl std::fmt::Debug for Badge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Badge")
            .field("label", &self.label)
            .finish()
    }
}

impl Widget for Badge {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let bg = self.color.unwrap_or(theme.colors.secondary);
        let text = self.text_color.unwrap_or(theme.colors.on_secondary);

        let text_widget = TextWidget::new(&self.label)
            .style(theme.typography.caption.clone())
            .color(text);
        let bg_rect = RectWidget::new()
            .background(bg)
            .corner_radius(CornerRadius::uniform(theme.shape.radius_full));

        let text_id = ctx.add(text_widget);
        let padding = Padding::symmetric(4.0, 12.0).set_child(text_id);
        let padding_id = ctx.add(padding);
        let bg_id = ctx.add(bg_rect);

        let root = ctx.add(ZStack::new().add_child(bg_id).add_child(padding_id));
        self.root_child_id = Some(root);
        vec![root]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        if let Some(root) = self.root_child_id
            && let Some(size) = ctx.child_size(root, proposal)
        {
            return size;
        }
        proposal.resolve(0.0, 0.0)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = fern_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Label);
        builder.set_name(&self.label);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    #[test]
    fn badge_builds_and_renders() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let badge = tree.add(Badge::new("New"));
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let b = tree.bounds(badge);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    #[test]
    fn badge_accessibility() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let badge = tree.add(Badge::new("3"));
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let info = tree.accessibility_node(badge);
        assert_eq!(info.role(), fern_core::accesskit::Role::Label);
        assert_eq!(info.name(), Some("3"));
    }
}
