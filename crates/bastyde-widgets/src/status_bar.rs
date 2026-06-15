// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! StatusBar — a horizontal bar at the bottom for status information.

use bastyde_canvas::{Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::widget::{LayoutContext, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

use crate::Panel;
use crate::primitives::HStack;
use bastyde_tokens::SurfaceRole;

/// StatusBar design tokens.
pub const STATUS_BAR_HEIGHT: f32 = 22.0;
pub const STATUS_BAR_PADDING_HORIZONTAL: f32 = 8.0;
pub const STATUS_BAR_ITEM_GAP: f32 = 2.0;

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
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
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
        let _ = ctx.theme_signal();
        let spacing = STATUS_BAR_ITEM_GAP;

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
                .background(SurfaceRole::Sunken)
                .padding(spacing)
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
    ) -> bastyde_core::widget::LayoutResponse {
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
        builder.set_role(bastyde_core::accesskit::Role::Status);
        builder.set_name(bastyde_i18n::tr_widget!(a11y_status_bar_name()).resolve_now());
        builder.set_live(bastyde_core::accesskit::Live::Polite);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::widget_tree::WidgetTree;

    #[test]
    fn status_bar_builds() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sb = tree.add(StatusBar::new());
        tree.layout(SizeProposal::exact(400.0, 50.0));
        let b = tree.bounds(sb);
        assert!(b.width > 0.0);
    }

    #[test]
    fn status_bar_accessibility() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sb = tree.add(StatusBar::new());
        tree.layout(SizeProposal::exact(400.0, 50.0));
        let info = tree.accessibility_node(sb);
        assert_eq!(info.role(), bastyde_core::accesskit::Role::Status);
        assert_eq!(info.name(), Some("Status"));
    }

    #[test]
    fn status_bar_tree_has_polite_live_region_and_no_group_wrapper() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sb = tree.add(StatusBar::new());
        tree.layout(SizeProposal::exact(400.0, 50.0));
        let update = tree.sync_accessibility();
        let sb_nid = bastyde_core::accessibility::widget_id_to_node_id(sb);
        let sb_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == sb_nid)
            .map(|(_, n)| n)
            .expect("status bar node in tree");
        assert_eq!(sb_node.live(), Some(bastyde_core::accesskit::Live::Polite));
        // Panel wrapper should be hidden so StatusBar → HStack directly,
        // no intermediate Role::Group node.
        let groups: Vec<_> = update
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == bastyde_core::accesskit::Role::Group)
            .collect();
        assert!(
            groups.is_empty(),
            "expected no Role::Group wrapper under StatusBar, got {}",
            groups.len()
        );
    }
}
