// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Badge — a pill-shaped label for tags, status indicators, and counts.
//!
//! `Badge` renders a short piece of text inside a rounded-pill container.
//! Common uses include tag chips on list items, unread-count bubbles in
//! navigation rails, and severity labels in alert rows. The pill chrome
//! (corner radius, padding, surface tint) is driven by the active
//! `BadgeStyle`; callers may swap it per-instance (`.style(...)`) or
//! theme-wide via `theme.style_slots.badge`.
//!
//! ## When to use
//!
//! - Inline chip that annotates another widget (version tag, "NEW" label).
//! - Standalone count indicator; pair with `SeverityBadge` for icon-backed
//!   status glyphs.
//!
//! ## Accessibility
//!
//! Announces as `Role::Label` with its resolved text as the AT name.
//! The inner `TextWidget` is hidden from AT to avoid double-announcement.
//!
//! ```rust
//! # use bastyde_widgets::Badge;
//! # use bastyde_i18n::lit;
//! # use bastyde_tokens::Color;
//! let _badge = Badge::new(lit!("NEW"))
//!     .background(Color::new(0.2, 0.6, 1.0, 1.0));
//! ```

use std::rc::Rc;

use bastyde_canvas::{Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::styles::{BadgeStyleConfig, SharedBadgeStyle};
use bastyde_core::widget::{LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::TextStyleRole;

use crate::primitives::TextWidget;
use bastyde_i18n::LocalizedString;

/// A pill-shaped label for displaying tags, counts, or status.
pub struct Badge {
    label: LocalizedString,
    background: Option<ColorProp>,
    text_role: Option<ColorProp>,
    /// Per-call override for the label's text style (font, size, weight).
    /// `None` ⇒ the default `TextStyleRole::Tiny`.
    text_style: Option<bastyde_core::color_prop::TextStyleProp>,
    /// Per-call override for the pill chrome.
    style_override: Option<SharedBadgeStyle>,
    root_child_id: Option<WidgetId>,
    /// Optional plain tooltip text shown after a hover delay. Mutually exclusive
    /// with the rich / composite slots — every setter clears the other two so
    /// the last call wins.
    tooltip_text: Option<LocalizedString>,
    /// Optional rich tooltip source (registry key or inline content).
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Optional composite tooltip body (arbitrary widget tree).
    composite_tooltip_content: Option<Box<dyn Widget>>,
}

impl Badge {
    /// Construct a badge with the given label text.
    pub fn new(label: impl Into<LocalizedString>) -> Self {
        Self {
            label: label.into(),
            background: None,
            text_role: None,
            text_style: None,
            style_override: None,
            root_child_id: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
        }
    }

    /// Per-call style override for the badge pill chrome. Replaces the
    /// theme-wide default `BadgeStyle` for just this instance.
    pub fn style(mut self, style: impl bastyde_core::styles::BadgeStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Override the badge background. Accepts `Color`, a
    /// [`SurfaceRole`](bastyde_tokens::SurfaceRole) / [`TextRole`](bastyde_tokens::TextRole),
    /// or a `Signal<Color>`. Default (unset) is `SurfaceRole::AccentSubtle`.
    pub fn background(mut self, color: impl Into<ColorProp>) -> Self {
        self.background = Some(color.into());
        self
    }

    /// Override the badge text color. Accepts `Color`, a role, or a signal.
    /// Default (unset) is the theme's `status_info_fg`.
    pub fn text_role(mut self, color: impl Into<ColorProp>) -> Self {
        self.text_role = Some(color.into());
        self
    }

    /// Override the label's text style (font, size, weight). Accepts a
    /// `TextStyleRole`, a `TextStyle`, or a `Signal` of either. Default
    /// (unset) is `TextStyleRole::Tiny`.
    pub fn text_style(mut self, style: impl Into<bastyde_core::color_prop::TextStyleProp>) -> Self {
        self.text_style = Some(style.into());
        self
    }

    /// Attach a plain single-line tooltip shown after a hover delay.
    ///
    /// Mutually exclusive with [`rich_tooltip`](Self::rich_tooltip),
    /// [`rich_tooltip_content`](Self::rich_tooltip_content), and
    /// [`composite_tooltip`](Self::composite_tooltip) — the last setter called wins.
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip identified by a registry key.
    ///
    /// Mutually exclusive with [`tooltip`](Self::tooltip),
    /// [`rich_tooltip_content`](Self::rich_tooltip_content), and
    /// [`composite_tooltip`](Self::composite_tooltip) — the last setter called wins.
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip from inline [`TooltipContent`](crate::tooltip::TooltipContent).
    ///
    /// Mutually exclusive with [`tooltip`](Self::tooltip),
    /// [`rich_tooltip`](Self::rich_tooltip), and
    /// [`composite_tooltip`](Self::composite_tooltip) — the last setter called wins.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip with an arbitrary widget tree body.
    ///
    /// Mutually exclusive with [`tooltip`](Self::tooltip),
    /// [`rich_tooltip`](Self::rich_tooltip), and
    /// [`rich_tooltip_content`](Self::rich_tooltip_content) — the last setter called wins.
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
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
        // `.text_role(...)`. The pill background default
        // (`AccentSubtle`) lives in the recipe; `.background(...)` reaches
        // the style as `background_override`.
        let text: ColorProp = self
            .text_role
            .take()
            .unwrap_or_else(|| ColorProp::Bound(theme_signal.map(|t| t.colors.status_info_fg)));

        let mut text_widget = TextWidget::new(self.label.clone())
            .color(text)
            .single_line()
            .a11y_hidden();
        text_widget = match &self.text_style {
            Some(style) => text_widget.style(style.clone()),
            None => text_widget.style(TextStyleRole::Tiny),
        };
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
                background_override: self.background.take(),
            },
            ctx,
        );
        self.root_child_id = Some(root);

        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, root, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, root, source, delay);
        } else if let Some(text) = self.tooltip_text.clone() {
            let tooltip_widget = crate::tooltip::TooltipWidget::new(text);
            let tooltip_id = ctx.add(tooltip_widget);
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip(root, tooltip_id, delay);
        }

        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // Rigid: size to content, no shrink (see Button's note).
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
            child.origin = bastyde_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Label);
        builder.set_name(self.label.resolve_now());
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_i18n::lit;

    #[test]
    fn badge_builds_and_renders() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let badge = tree.add(Badge::new(lit!("New")));
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let b = tree.bounds(badge);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    #[test]
    fn badge_accessibility() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let badge = tree.add(Badge::new(lit!("3")));
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let info = tree.accessibility_node(badge);
        assert_eq!(info.role(), bastyde_core::accesskit::Role::Label);
        assert_eq!(info.name(), Some("3"));
    }

    #[test]
    fn tooltip_appears_on_hover() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(Badge::new(lit!("New")).tooltip(lit!("Tip")));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        tree.pointer_move(tree.bounds(id).center());
        tree.advance_time(std::time::Duration::from_secs(1));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "tooltip should appear on hover"
        );
        assert!(tree.find_by_label("Tip").is_some());
    }
}
