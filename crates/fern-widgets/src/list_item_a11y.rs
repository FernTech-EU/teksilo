//! Thin accessibility wrapper for list/tree item widgets.
//!
//! Wraps a delegate-created widget with the correct AccessKit role
//! and positional properties (position_in_set, size_of_set, level, expanded).

use fern_canvas::{Rect, Size, SizeProposal};

use fern_core::accessibility::AccessNodeBuilder;
use fern_core::widget::{LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

/// Wrapper that sets Role::ListItem with position_in_set and size_of_set.
#[derive(Debug)]
pub(crate) struct ListItemWrapper {
    child: WidgetId,
    position: usize, // 1-based
    total: usize,
}

impl ListItemWrapper {
    pub fn new(child: WidgetId, position_1based: usize, total: usize) -> Self {
        Self {
            child,
            position: position_1based,
            total,
        }
    }
}

impl Widget for ListItemWrapper {
    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        ctx.child_size(self.child, proposal)
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
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
        builder.set_role(fern_core::accesskit::Role::ListItem);
        builder.inner_mut().set_position_in_set(self.position);
        builder.inner_mut().set_size_of_set(self.total);
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}

/// Wrapper that sets Role::TreeItem with level and expanded state.
#[derive(Debug)]
pub(crate) struct TreeItemWrapper {
    child: WidgetId,
    level: usize,           // 1-based
    expanded: Option<bool>, // None if leaf
}

impl TreeItemWrapper {
    pub fn new(child: WidgetId, level_1based: usize, expanded: Option<bool>) -> Self {
        Self {
            child,
            level: level_1based,
            expanded,
        }
    }
}

impl Widget for TreeItemWrapper {
    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        ctx.child_size(self.child, proposal)
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
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
        if let Some(expanded) = self.expanded {
            builder.set_expanded(expanded);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}
