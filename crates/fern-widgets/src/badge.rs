//! Badge — a pill-shaped label for tags, status indicators, and counts.

use std::rc::Rc;

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::color_prop::ColorProp;
use fern_core::styles::{BadgeStyleConfig, SharedBadgeStyle};
use fern_core::widget::{LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::TextStyleRole;

use crate::primitives::TextWidget;

/// A pill-shaped label for displaying tags, counts, or status.
pub struct Badge {
    label: String,
    color: Option<ColorProp>,
    text_color: Option<ColorProp>,
    /// Per-call override for the pill chrome.
    style_override: Option<SharedBadgeStyle>,
    root_child_id: Option<WidgetId>,
}

impl Badge {
    pub fn new(label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        Self {
            label: ls.resolve_now(),
            color: None,
            text_color: None,
            style_override: None,
            root_child_id: None,
        }
    }

    /// Per-call style override for the badge pill chrome. Replaces the
    /// theme-wide default `BadgeStyle` for just this instance.
    pub fn style(mut self, style: impl fern_core::styles::BadgeStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
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
        let theme_signal = ctx.theme_signal();

        // Default text color: `status_info_fg` via a derived signal so
        // theme changes still propagate. Callers override with
        // `.text_color(...)`. The pill background default
        // (`AccentSubtle`) lives in the recipe; `.color(...)` reaches
        // the style as `background_override`.
        let text: ColorProp = self.text_color.take().unwrap_or_else(|| {
            ColorProp::Bound(theme_signal.map(|t| t.colors.status_info_fg))
        });

        let text_widget = TextWidget::new_literal(&self.label)
            .style(TextStyleRole::Tiny)
            .color(text)
            .single_line()
            .a11y_hidden();
        let content = ctx.add(text_widget);

        // The pill chrome (rounded background + padding inset) is owned
        // by the active `BadgeStyle`.
        let style: SharedBadgeStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.badge.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeBadgeStyle::default()));
        let root = style.make_body(
            &BadgeStyleConfig {
                content,
                background_override: self.color.take(),
            },
            ctx,
        );
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        if let Some(root) = self.root_child_id
            && let Some(size) = ctx.child_size(root, proposal)
        {
            return (size).into();
        }
        proposal.resolve(0.0, 0.0).into()
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
    use fern_core::Theme;

    #[test]
    fn badge_builds_and_renders() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let badge = tree.add(Badge::new_literal("New"));
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let b = tree.bounds(badge);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    #[test]
    fn badge_accessibility() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let badge = tree.add(Badge::new_literal("3"));
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let info = tree.accessibility_node(badge);
        assert_eq!(info.role(), fern_core::accesskit::Role::Label);
        assert_eq!(info.name(), Some("3"));
    }
}
