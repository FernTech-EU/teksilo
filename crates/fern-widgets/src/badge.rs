//! Badge — a pill-shaped label for tags, status indicators, and counts.

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::color_prop::ColorProp;
use fern_core::widget::{LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::CornerRadius;

use crate::primitives::{Padding, RectWidget, TextWidget, ZStack};

/// A pill-shaped label for displaying tags, counts, or status.
pub struct Badge {
    label: String,
    color: Option<ColorProp>,
    text_color: Option<ColorProp>,
    root_child_id: Option<WidgetId>,
}

impl Badge {
    pub fn new(label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        Self {
            label: ls.resolve_now(),
            color: None,
            text_color: None,
            root_child_id: None,
        }
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw string in `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn new_literal(label: impl Into<String>) -> Self {
        Self::new(fern_i18n::LocalizedString::literal(label))
    }

    /// Override the badge background. Accepts `Color`, a
    /// [`SurfaceRole`](fern_tokens::SurfaceRole) / [`TextRole`](fern_tokens::TextRole),
    /// or a `Signal<Color>`. Default (unset) is `SurfaceRole::AccentSubtle`.
    pub fn color(mut self, color: impl Into<ColorProp>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Override the badge text color. Accepts `Color`, a role, or a signal.
    /// Default (unset) is the theme's `status_info_fg`.
    pub fn text_color(mut self, color: impl Into<ColorProp>) -> Self {
        self.text_color = Some(color.into());
        self
    }
}

impl std::fmt::Debug for Badge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Badge").field("label", &self.label).finish()
    }
}

impl Widget for Badge {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        use fern_tokens::SurfaceRole;
        let theme_signal = ctx.theme_signal();
        let snapshot = theme_signal.get();
        let badge_style = snapshot.components.badge;
        let typography_tiny = snapshot.typography.tiny.clone();

        // Defaults: soft accent tint + status_info_fg (reactive via theme
        // signal). Callers override with `.color(...)` / `.text_color(...)`;
        // either branch is a reactive `ColorProp`, so roles, signals, and
        // static colors all work.
        let bg: ColorProp = self
            .color
            .take()
            .unwrap_or(ColorProp::SurfaceRole(SurfaceRole::AccentSubtle));
        let text: ColorProp = self.text_color.take().unwrap_or_else(|| {
            // No matching role — use a derived signal so theme changes
            // still propagate.
            ColorProp::Bound(theme_signal.map(|t| t.colors.status_info_fg))
        });

        let text_widget = TextWidget::new_literal(&self.label)
            .style(typography_tiny)
            .color(text)
            .single_line()
            .a11y_hidden();
        let bg_rect = RectWidget::new()
            .background(bg)
            .corner_radius(CornerRadius::uniform(badge_style.corner_radius));

        let text_id = ctx.add(text_widget);
        let padding = Padding::symmetric(
            badge_style.padding_vertical,
            badge_style.padding_horizontal,
        )
        .child_id(text_id);
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
        let badge = tree.add(Badge::new_literal("New"));
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let b = tree.bounds(badge);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    #[test]
    fn badge_accessibility() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let badge = tree.add(Badge::new_literal("3"));
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let info = tree.accessibility_node(badge);
        assert_eq!(info.role(), fern_core::accesskit::Role::Label);
        assert_eq!(info.name(), Some("3"));
    }
}
