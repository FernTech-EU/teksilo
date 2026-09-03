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

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Rect, SizeProposal};

use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;

use crate::data_views::RowSelection;

/// Builds a row's own widget, and returns its id.
///
/// Called with the row's selectedness at the moment of the build, because
/// that is an argument the delegate takes. Erases the source's item type, so
/// the wrapper does not have to be generic over it: the owning pane closes
/// over the delegate, the data source and the tooltip resolvers, and this is
/// what is left of them.
///
/// `None` means the row had nothing to show after all.
pub(crate) type RowBody = Rc<dyn Fn(&mut BuildContext, bool) -> Option<WidgetId>>;

/// Wrapper that sets Role::ListBoxOption with selection state.
///
/// ListView is interactive (keyboard navigation, selection) so the correct
/// ARIA container role is `listbox` and items are `option`, not the
/// non-interactive `list`/`listitem` pair.
///
/// # The rebuild boundary for a selection move
///
/// The wrapper builds the delegate's row widget itself rather than being
/// handed one, and watches the selection for its own row only. A selection
/// move flips exactly two rows: the one that lost it and the one that gained
/// it. Those two rebuild; every other row keeps the widget, the arena node
/// and the AccessKit node id it already had.
///
/// The owning pane used to do this instead, by rebuilding itself whenever the
/// selection changed anywhere. That replaced every realized row on every
/// arrow press. It cost the scroll offset, the keyboard anchor, and the
/// identity of the node the container nominates as its `active_descendant` —
/// a screen reader was being pointed at a node it had never seen, one
/// keystroke after the node it had been told about ceased to exist.
///
/// Note what stays with the pane: every per-row *handler* is applied to this
/// wrapper's id, not to the row widget inside it. Those handlers survive a
/// rebuild of the wrapper's children, which is the only thing that happens
/// here. They must not be applied a second time — `HandlerSet::merge` chains
/// handlers rather than replacing them, so a row whose handlers were
/// reapplied would act on one press twice.
pub(crate) struct ListItemWrapper {
    body: RowBody,
    /// The selection to watch, and this row's index in it.
    selection: Option<RowSelection>,
    /// Model index. The screen reader says "row 147", so the position it
    /// publishes is `index + 1` in the whole model, never a position within
    /// the realized window, which would restart at 1 on every scroll.
    index: usize,
    /// Rebuild trigger, bumped only when *this* row's selectedness flips.
    version: Signal<u64>,

    // Build state.
    selected: bool,
    child: Option<WidgetId>,
}

impl ListItemWrapper {
    pub fn new(body: RowBody, selection: Option<RowSelection>, index: usize) -> Self {
        Self {
            body,
            selection,
            index,
            version: Signal::new(0),
            selected: false,
            child: None,
        }
    }
}

impl std::fmt::Debug for ListItemWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListItemWrapper")
            .field("index", &self.index)
            .field("selected", &self.selected)
            .finish_non_exhaustive()
    }
}

impl Widget for ListItemWrapper {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // A persistent field rather than `ctx.signal`, so the observer
        // installed below survives into the build it triggers.
        self.version
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        let selected = self
            .selection
            .as_ref()
            .map(|s| s.is_selected(self.index))
            .unwrap_or(false);
        self.selected = selected;

        if let Some(ref selection) = self.selection {
            let watched = selection.clone();
            let index = self.index;
            let version = self.version.clone();
            // Seeded with what this build is about to draw, so the first
            // notification after it compares against the truth on screen.
            let last = Cell::new(selected);
            let handle = selection.observe_for_rebuild(move || {
                let now = watched.is_selected(index);
                // Every row hears every selection change. Only the two whose
                // own state moved may rebuild; the rest return here, which is
                // the whole point of watching per row.
                if now != last.get() {
                    last.set(now);
                    version.set(version.get() + 1);
                }
            });
            ctx.own_handle(handle);
        }

        self.child = (self.body)(ctx, selected);
        self.child.into_iter().collect()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        self.child
            .and_then(|child| ctx.child_size(child, proposal))
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
        builder.set_position_in_set(self.index + 1);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
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
