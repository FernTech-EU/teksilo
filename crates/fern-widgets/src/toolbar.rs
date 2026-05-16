//! Toolbar — a compact horizontal container for action buttons.

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::widget::{LayoutContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

use crate::Panel;
use crate::primitives::HStack;

/// Toolbar design tokens — relocated from `theme.components.toolbar`
/// in Stage G of the styling migration.
pub const TOOLBAR_HEIGHT_COMPACT: f32 = 30.0;
pub const TOOLBAR_HEIGHT_DEFAULT: f32 = 40.0;
pub const TOOLBAR_BUTTON_SIZE_COMPACT: f32 = 22.0;
pub const TOOLBAR_BUTTON_SIZE_DEFAULT: f32 = 30.0;
pub const TOOLBAR_ICON_SIZE: f32 = 16.0;
pub const TOOLBAR_SEPARATOR_WIDTH: f32 = 1.0;
pub const TOOLBAR_SEPARATOR_INSET: f32 = 4.0;

/// A compact horizontal container for toolbar actions.
pub struct Toolbar {
    pending: Vec<PendingChild>,
    child_ids: Vec<WidgetId>,
    root_child_id: Option<WidgetId>,
    label: Option<String>,
}

impl Toolbar {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            child_ids: Vec::new(),
            root_child_id: None,
            label: None,
        }
    }

    /// Add an inline child widget (deferred insertion).
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending.push(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Add a pre-registered child widget by ID.
    pub fn add_child(mut self, id: WidgetId) -> Self {
        self.pending.push(PendingChild::Id(id));
        self
    }

    /// Override the toolbar's accessible name. Default is the localised
    /// "Toolbar" string from the framework bundle. Use this when a window
    /// has multiple toolbars that need distinguishing ("Formatting",
    /// "Drawing", etc.).
    pub fn label(mut self, label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `label(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn label_literal(self, label: impl Into<String>) -> Self {
        self.label(fern_i18n::LocalizedString::literal(label))
    }
}

impl Default for Toolbar {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Toolbar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Toolbar").finish()
    }
}

impl Widget for Toolbar {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let _ = ctx.theme_signal();
        let spacing = TOOLBAR_SEPARATOR_INSET;
        let padding_signal = fern_core::signal::Signal::new(TOOLBAR_SEPARATOR_INSET);

        // Resolve pending children
        let pending = std::mem::take(&mut self.pending);
        if !pending.is_empty() {
            self.child_ids = pending
                .into_iter()
                .map(|child| match child {
                    PendingChild::Id(id) => id,
                    PendingChild::Deferred(w) => ctx.add_boxed(w),
                })
                .collect();
        }

        let mut row = HStack::new().spacing(spacing);
        for &id in &self.child_ids {
            row = row.add_child(id);
        }

        let row_id = ctx.add(row);
        let root = ctx.add(
            Panel::new()
                .padding(padding_signal)
                .a11y_presentational()
                .child_id(row_id),
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
        builder.set_role(fern_core::accesskit::Role::Toolbar);
        let name = self
            .label
            .clone()
            .unwrap_or_else(|| fern_i18n::tr_widget!(a11y_toolbar_name()).resolve_now());
        builder.set_name(name);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;

    #[test]
    fn toolbar_builds() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let tb = tree.add(Toolbar::new());
        tree.layout(SizeProposal::exact(400.0, 50.0));
        let b = tree.bounds(tb);
        assert!(b.width > 0.0);
    }

    #[test]
    fn toolbar_with_children() {
        use crate::Button;
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let btn = tree.add(Button::new_literal("Action"));
        let tb = tree.add(Toolbar::new().add_child(btn));
        tree.layout(SizeProposal::exact(400.0, 50.0));
        let b = tree.bounds(tb);
        assert!(b.width > 0.0);
    }

    #[test]
    fn toolbar_accessibility() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let tb = tree.add(Toolbar::new());
        tree.layout(SizeProposal::exact(400.0, 50.0));
        let info = tree.accessibility_node(tb);
        assert_eq!(info.role(), fern_core::accesskit::Role::Toolbar);
        assert!(
            info.name().is_some(),
            "toolbar should carry a default a11y name"
        );
    }

    #[test]
    fn toolbar_custom_label_overrides_default() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let tb = tree.add(Toolbar::new().label_literal("Formatting"));
        tree.layout(SizeProposal::exact(400.0, 50.0));
        let info = tree.accessibility_node(tb);
        assert_eq!(info.name(), Some("Formatting"));
    }

    #[test]
    fn toolbar_has_no_group_wrapper_in_a11y_tree() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let _tb = tree.add(Toolbar::new());
        tree.layout(SizeProposal::exact(400.0, 50.0));
        let update = tree.sync_accessibility();
        // Panel wrapper is marked presentational, so the only non-
        // Window container in the tree should be the Toolbar itself.
        let groups: Vec<_> = update
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == fern_core::accesskit::Role::Group)
            .collect();
        assert!(
            groups.is_empty(),
            "expected no Role::Group wrapper under Toolbar, got {}",
            groups.len()
        );
    }
}
