// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Thin accessibility wrapper for list/tree item widgets.
//!
//! Wraps a delegate-created widget with the correct AccessKit role
//! and positional properties (position_in_set, size_of_set, level, expanded),
//! and advertises the actions an assistive-tech client (or the automation
//! bridge) drives a row with — `Click`, `ScrollIntoView`, and
//! `Expand`/`Collapse` on a tree branch. The handlers behind those actions are
//! installed by the owning pane (`ListBodyPane` / `TreeViewBodyPane`), which is
//! the only place that can reach the selection model and the row source.
//!
//! The wrapper carries no *name*: the delegate's row widget owns the label, one
//! node further down. `WidgetTree`'s accessibility walk copies it up
//! (name-from-content, as ARIA specifies for `option` / `treeitem`), so the
//! emitted row node carries role, state and name together.

use teksilo_canvas::{Rect, SizeProposal};

use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::widget::{LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;

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
    ) -> teksilo_core::widget::LayoutResponse {
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
        builder.set_role(teksilo_core::accesskit::Role::ListBoxOption);
        builder.set_selected(self.selected);
        // A row is a real AT target: a screen reader's "activate" and an
        // automation `invoke_action(row, "click")` both arrive as
        // `Action::Click`, and `ScrollIntoView` is the only way to reach a row
        // the virtualizer has parked outside the viewport (its bounds are
        // content coordinates, so a synthetic pointer click at them lands
        // nowhere). `ListBodyPane::build` installs the matching handlers —
        // an advertised action with no handler is worse than no action at all,
        // since the caller gets a success reply for a no-op, so the two sites
        // must always be edited together.
        //
        // `Focus` is deliberately NOT advertised: the *container* is the
        // focusable node, and `WidgetEvent::AccessAction`'s Focus arm is
        // intercepted by the tree before any widget sees it, so advertising it
        // here would move focus off the view's root and kill arrow navigation.
        builder.add_action(teksilo_core::accesskit::Action::Click);
        builder.add_action(teksilo_core::accesskit::Action::ScrollIntoView);
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
    ) -> teksilo_core::widget::LayoutResponse {
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
        builder.set_role(teksilo_core::accesskit::Role::TreeItem);
        builder.set_level(self.level);
        builder.set_position_in_set(self.position);
        builder.inner_mut().set_size_of_set(self.total_siblings);
        if let Some(expanded) = self.expanded {
            builder.set_expanded(expanded);
        }
        builder.set_selected(self.selected);
        // See `ListItemWrapper::accessibility` for why `Click` /
        // `ScrollIntoView` are advertised and `Focus` is not. The handlers live
        // in `TreeViewBodyPane::build`.
        builder.add_action(teksilo_core::accesskit::Action::Click);
        builder.add_action(teksilo_core::accesskit::Action::ScrollIntoView);
        // Only the direction that would actually change something: a collapsed
        // branch advertises `Expand`, an expanded one `Collapse`. A leaf
        // (`expanded == None`) advertises neither. Without this a caller
        // driving a tree has no way to open a section at all when the delegate
        // owns the chevron (`row_click_expands(false)`) — the chevron is a
        // nameless 16 px hit target it can only find by guessing at pixels.
        match self.expanded {
            Some(true) => builder.add_action(teksilo_core::accesskit::Action::Collapse),
            Some(false) => builder.add_action(teksilo_core::accesskit::Action::Expand),
            None => {}
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}
