//! StatusBar — a horizontal bar at the bottom for status information.

use fern_core::accessibility::AccessNodeBuilder;
use fern_core::composite_widget::{BuildContext, CompositeWidget};
use fern_core::event::{EventResponse, WidgetEvent};
use fern_core::state::{Reactive, State};
use fern_core::widget::EventContext;
use fern_core::widget_id::WidgetId;

use crate::primitives::HStack;
use crate::Panel;

/// A status bar for displaying information at the bottom of a window.
///
/// Children must be pre-registered via `add_child(id)`.
pub struct StatusBar {
    child_ids: Vec<WidgetId>,
    visible_when_state: Option<Reactive<bool>>,
    enabled_when_state: Option<Reactive<bool>>,
}

impl StatusBar {
    pub fn new() -> Self {
        Self {
            child_ids: Vec::new(),
            visible_when_state: None,
            enabled_when_state: None,
        }
    }

    /// Add a pre-registered child widget by ID.
    pub fn add_child(mut self, id: WidgetId) -> Self {
        self.child_ids.push(id);
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

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for StatusBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatusBar").finish()
    }
}

impl CompositeWidget for StatusBar {
    fn build(&self, ctx: &mut BuildContext) -> WidgetId {
        let theme = ctx.theme().clone();
        let spacing = theme.spacing.xs;

        let mut row = HStack::new().spacing(spacing);
        for &id in &self.child_ids {
            row = row.add_child(id);
        }

        let row_id = ctx.add(row);
        ctx.add(
            Panel::new()
                .background(theme.colors.surface_tertiary)
                .padding(spacing)
                .set_child(row_id),
        )
    }

    fn event(&mut self, _event: &WidgetEvent, _ctx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
        builder.set_name("Status");
    }

    fn take_visible_when(&mut self) -> Option<Reactive<bool>> {
        self.visible_when_state.take()
    }

    fn take_enabled_when(&mut self) -> Option<Reactive<bool>> {
        self.enabled_when_state.take()
    }
}

fern_core::impl_composite_into_widget_tree!(StatusBar);

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::SizeProposal;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    #[test]
    fn status_bar_builds() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let sb = tree.add_composite(StatusBar::new());
        tree.layout(SizeProposal::exact(400.0, 50.0));
        let b = tree.bounds(sb);
        assert!(b.width > 0.0);
    }

    #[test]
    fn status_bar_accessibility() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let sb = tree.add_composite(StatusBar::new());
        tree.layout(SizeProposal::exact(400.0, 50.0));
        let info = tree.accessibility_node(sb);
        assert_eq!(info.name(), Some("Status"));
    }
}
