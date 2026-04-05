//! StatusBar — a horizontal bar at the bottom for status information.

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::widget::{IntoWidgetTree, LayoutContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

use crate::primitives::HStack;
use crate::Panel;

/// A status bar for displaying information at the bottom of a window.
pub struct StatusBar {
    pending: Vec<PendingChild>,
    child_ids: Vec<WidgetId>,
    root_child_id: Option<WidgetId>,
}

impl StatusBar {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            child_ids: Vec::new(),
            root_child_id: None,
        }
    }

    /// Add an inline child widget (deferred insertion).
    pub fn child(mut self, widget: impl IntoWidgetTree) -> Self {
        self.pending.push(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Add a pre-registered child widget by ID.
    pub fn add_child(mut self, id: WidgetId) -> Self {
        self.pending.push(PendingChild::Id(id));
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

impl Widget for StatusBar {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let spacing = theme.spacing.xs;

        // Resolve pending children
        let pending = std::mem::take(&mut self.pending);
        if !pending.is_empty() {
            self.child_ids = pending.into_iter().map(|child| match child {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            }).collect();
        }

        let mut row = HStack::new().spacing(spacing);
        for &id in &self.child_ids {
            row = row.add_child(id);
        }

        let row_id = ctx.add(row);
        let root = ctx.add(
            Panel::new()
                .background(theme.colors.surface_tertiary)
                .padding(spacing)
                .set_child(row_id),
        );
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
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
        builder.set_name("Status");
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
    fn status_bar_builds() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let sb = tree.add(StatusBar::new());
        tree.layout(SizeProposal::exact(400.0, 50.0));
        let b = tree.bounds(sb);
        assert!(b.width > 0.0);
    }

    #[test]
    fn status_bar_accessibility() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let sb = tree.add(StatusBar::new());
        tree.layout(SizeProposal::exact(400.0, 50.0));
        let info = tree.accessibility_node(sb);
        assert_eq!(info.name(), Some("Status"));
    }
}
