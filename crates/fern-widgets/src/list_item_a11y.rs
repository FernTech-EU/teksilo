//! Thin accessibility wrapper for list/tree item widgets.
//!
//! Wraps a delegate-created widget with the correct AccessKit role
//! and positional properties (position_in_set, size_of_set, level, expanded).

use fern_canvas::{Rect, SizeProposal};

use fern_core::accessibility::AccessNodeBuilder;
use fern_core::widget::{LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

/// Wrapper that sets Role::ListBoxOption with selection state.
///
/// ListView is interactive (keyboard navigation, selection) so the correct
/// ARIA container role is `listbox` and items are `option`, not the
/// non-interactive `list`/`listitem` pair.
#[derive(Debug)]
pub(crate) struct ListItemWrapper {
    child: WidgetId,
    selected: bool,
}

impl ListItemWrapper {
    pub fn new(child: WidgetId, selected: bool) -> Self {
        Self { child, selected }
    }
}

impl Widget for ListItemWrapper {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        ctx.child_size(self.child, proposal)
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::ListBoxOption);
        builder.set_selected(self.selected);
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}

/// Wrapper that sets Role::TreeItem with level, expanded state, and selection.
#[derive(Debug)]
pub(crate) struct TreeItemWrapper {
    child: WidgetId,
    level: usize,    // 1-based
    position: usize, // 1-based within sibling group
    total_siblings: usize,
    expanded: Option<bool>, // None if leaf
    selected: bool,
}

impl TreeItemWrapper {
    pub fn new(
        child: WidgetId,
        level_1based: usize,
        position_1based: usize,
        total_siblings: usize,
        expanded: Option<bool>,
        selected: bool,
    ) -> Self {
        Self {
            child,
            level: level_1based,
            position: position_1based,
            total_siblings,
            expanded,
            selected,
        }
    }
}

impl Widget for TreeItemWrapper {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        ctx.child_size(self.child, proposal)
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::TreeItem);
        builder.inner_mut().set_level(self.level);
        builder.inner_mut().set_position_in_set(self.position);
        builder.inner_mut().set_size_of_set(self.total_siblings);
        if let Some(expanded) = self.expanded {
            builder.set_expanded(expanded);
        }
        builder.set_selected(self.selected);
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}
