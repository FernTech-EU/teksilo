// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Thin accessibility wrapper for list/tree item widgets.
//!
//! Wraps a delegate-created widget with the correct AccessKit role
//! and positional properties (position_in_set, level, expanded),
//! and advertises the actions an assistive-tech client (or the automation
//! bridge) drives a row with — `Click`, `ScrollIntoView`, and
//! `Expand`/`Collapse` on a tree branch. The handlers behind those actions are
//! installed by the owning pane (`ListBodyPane` / `TreeViewBodyPane`), which is
//! the only place that can reach the selection model and the row source.
//!
//! The matching `size_of_set` is a container property in AccessKit, not a
//! per-item one: `ListView` publishes it on the `Role::ListBox` node its rows
//! hang from, and a flattened tree publishes none at all (`TreeItemWrapper`'s
//! `accessibility` says why).
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
    /// 1-based position in the whole model, not in the realized window: the
    /// screen reader says "row 147", so the number has to be the model's.
    position: usize,
}

impl ListItemWrapper {
    pub fn new(child: WidgetId, selected: bool, position_1based: usize) -> Self {
        Self {
            child,
            selected,
            position: position_1based,
        }
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
        // "row 147". The matching "of 200" lives on the `Role::ListBox`
        // container: AccessKit's `size_of_set` belongs there, unlike ARIA's
        // per-item `aria-setsize`, and `size_of_set_from_container` walks up
        // from an item to find it. A `ListView` announced neither until now.
        builder.set_position_in_set(self.position);
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}

/// Wrapper that sets Role::TreeItem with level, expanded state, and selection.
#[derive(Debug)]
pub(crate) struct TreeItemWrapper {
    child: WidgetId,
    level: usize,           // 1-based
    position: usize,        // 1-based within sibling group
    expanded: Option<bool>, // None if leaf
    selected: bool,
}

impl TreeItemWrapper {
    pub fn new(
        child: WidgetId,
        level_1based: usize,
        position_1based: usize,
        expanded: Option<bool>,
        selected: bool,
    ) -> Self {
        Self {
            child,
            level: level_1based,
            position: position_1based,
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
        // No `size_of_set` here, and none on the container either.
        //
        // AccessKit resolves an item's set size by walking *up* from it
        // (`size_of_set_from_container`), so the only value a flattened tree
        // could publish is one shared by every row at every depth — which is
        // not what "the 2nd of 5 siblings" means. Expressing it correctly needs
        // a real `Role::Group` node per expanded branch, between the container
        // and its rows, and that changes the AT tree shape for every tree
        // widget. Writing the number on this node instead is dead: no adapter
        // on any platform reads it, and leaving it there makes a missing
        // feature look like a working one.
        //
        // The level and the expanded state still carry the hierarchy, and
        // `position_in_set` still says which sibling this is.

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
