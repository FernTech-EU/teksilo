//! Badge — a pill-shaped label for tags, status indicators, and counts.

use fern_core::accessibility::AccessNodeBuilder;
use fern_core::composite_widget::{BuildContext, CompositeWidget};
use fern_core::event::{EventResponse, WidgetEvent};
use fern_core::state::{Reactive, State};
use fern_core::widget::EventContext;
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius};

use crate::primitives::{Padding, RectWidget, TextWidget, ZStack};

/// A pill-shaped label for displaying tags, counts, or status.
pub struct Badge {
    label: String,
    color: Option<Color>,
    text_color: Option<Color>,
    visible_when_state: Option<Reactive<bool>>,
    enabled_when_state: Option<Reactive<bool>>,
}

impl Badge {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            color: None,
            text_color: None,
            visible_when_state: None,
            enabled_when_state: None,
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

    pub fn visible_when(mut self, state: impl Into<Reactive<bool>>) -> Self {
        self.visible_when_state = Some(state.into());
        self
    }

    pub fn enabled_when(mut self, state: impl Into<Reactive<bool>>) -> Self {
        self.enabled_when_state = Some(state.into());
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

impl CompositeWidget for Badge {
    fn build(&self, ctx: &mut BuildContext) -> WidgetId {
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

        ctx.add(ZStack::new().add_child(bg_id).add_child(padding_id))
    }

    fn event(&mut self, _event: &WidgetEvent, _ctx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Label);
        builder.set_name(&self.label);
    }

    fn take_visible_when(&mut self) -> Option<Reactive<bool>> {
        self.visible_when_state.take()
    }

    fn take_enabled_when(&mut self) -> Option<Reactive<bool>> {
        self.enabled_when_state.take()
    }
}

fern_core::impl_composite_into_widget_tree!(Badge);

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::SizeProposal;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    #[test]
    fn badge_builds_and_renders() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let badge = tree.add_composite(Badge::new("New"));
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let b = tree.bounds(badge);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    #[test]
    fn badge_accessibility() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let badge = tree.add_composite(Badge::new("3"));
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let info = tree.accessibility_node(badge);
        assert_eq!(info.role(), fern_core::accesskit::Role::Label);
        assert_eq!(info.name(), Some("3"));
    }
}
