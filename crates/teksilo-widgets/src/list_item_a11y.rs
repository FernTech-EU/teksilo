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
//! hang from, and a tree publishes each row's on the node it hangs from: its
//! parent row, or the pane for a root (`TreeItemWrapper`'s `accessibility`
//! and `common::tree_a11y` say how).
//!
//! The wrapper carries no *name*: the delegate's row widget owns the label, one
//! node further down. `WidgetTree`'s accessibility walk copies it up
//! (name-from-content, as ARIA specifies for `option` / `treeitem`), so the
//! emitted row node carries role, state and name together.

use std::cell::{Cell, RefCell};
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
    /// Publish nothing: the row widget inside is the option (see
    /// `ListView::presentational`).
    presentational: bool,
    /// Set when the owning view is in
    /// [`RowKeyboard::Actions`](crate::RowKeyboard::Actions).
    actions: Option<RowActionsSetup>,

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
            presentational: false,
            actions: None,
            selected: false,
            child: None,
        }
    }

    /// Publish nothing of the row, for a view that is
    /// [`presentational`](crate::list_view::ListView::presentational).
    pub fn presentational(mut self, presentational: bool) -> Self {
        self.presentational = presentational;
        self
    }

    /// Make the row a place the keyboard can step into, for a view in
    /// [`RowKeyboard::Actions`](crate::RowKeyboard::Actions).
    pub fn actions(mut self, setup: RowActionsSetup) -> Self {
        self.actions = Some(setup);
        self
    }
}

/// What a row wrapper needs to know about a view in
/// [`RowKeyboard::Actions`](crate::RowKeyboard::Actions).
pub(crate) struct RowActionsSetup {
    /// The key that enters the row, named in the row's description: `F2`, or
    /// `Enter` when the view has no activation handler.
    pub entry_key: teksilo_core::event::Key,
    /// Whether the row's controls can be Tab stops at all: while focus is in
    /// the row the user stepped into. The wrapper installs it as a Tab gate
    /// on itself, which holds the whole row out of the Tab order otherwise.
    pub stop: teksilo_core::signal::Prop<bool>,
    /// Written on every build: the row's controls that can take focus now
    /// (enabled), in Tab order. Empty for a row with nothing to enter.
    pub controls: Rc<RefCell<Vec<WidgetId>>>,
    /// Written when one of the row's controls gains focus: its position in
    /// `controls`. Shared by every row of the view, so it holds the position
    /// in the row focus was last in.
    ///
    /// The hook lives on each control's node, so it is lost when a composite
    /// rebuilds its focusable leaf on its own, and a control enabled after
    /// the row was built has none. The position then stays what it was until
    /// the row is built again.
    pub ordinal: Rc<Cell<usize>>,
}

impl ListItemWrapper {
    /// The description a row in action mode carries: which key enters it,
    /// and that `Escape` comes back. Only a row holding a control says so.
    fn actions_hint(&self) -> Option<String> {
        let setup = self.actions.as_ref()?;
        if setup.controls.borrow().is_empty() {
            return None;
        }
        use crate::keystroke_format::format_keystroke;
        use teksilo_core::event::{Key, Modifiers};
        use teksilo_core::shortcut::KeyStroke;
        let enter = format_keystroke(KeyStroke::new(setup.entry_key, Modifiers::NONE));
        let exit = format_keystroke(KeyStroke::new(Key::Escape, Modifiers::NONE));
        Some(
            teksilo_i18n::tr_widget!(list_view_row_actions_hint(enter = enter, exit = exit))
                .resolve_now(),
        )
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

        if let Some(ref setup) = self.actions {
            // One Tab cycle per row, for the whole life of the mode. While the
            // row is not entered its subtree is out of the Tab order (the
            // `stop` gate), so the scope is empty and Tab skips it. A cycle
            // rather than a pass-through: Tab past the row's last control
            // would otherwise leave the list from the middle of it, and
            // `Escape`, which the row describes, is the way out.
            ctx.set_traversal_scope(teksilo_core::focus::TraversalScopePolicy::Cycle);
            // A gate, not a Tab-stop prop: it holds the whole row out of the
            // Tab order until the user steps in, whatever its controls' own
            // props say. A roving group (a toolbar's current item) carries a
            // prop nearer than any the row could set, and a mere inherited
            // `false` would leave it a stop, so Tab would walk into the row's
            // cycle from the list and never leave. Inside an entered row the
            // controls' own props work as everywhere else.
            ctx.set_tab_gate(ctx.self_id(), setup.stop.clone());
            // Every control that can take focus now, in Tab order, read without
            // touching any of them: a roving item out of the Tab order still
            // has its place, so a position names the same control in the next
            // build of the row.
            let controls = self
                .child
                .map(|child| ctx.focusables_in_tab_order(child))
                .unwrap_or_default();
            for (ordinal, &control) in controls.iter().enumerate() {
                // Which control the user is in, so a rebuild that has to put
                // focus back explicitly puts it on the same one. Focus bubbles,
                // so only the control's own arrival counts.
                let last = setup.ordinal.clone();
                ctx.apply_handlers(
                    control,
                    teksilo_core::widget_builder::HandlerSet::new().on_focus(move |gained, ctx| {
                        if gained && ctx.dispatch_target() == Some(control) {
                            last.set(ordinal);
                        }
                    }),
                );
            }
            *setup.controls.borrow_mut() = controls;
        }

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
        if self.presentational {
            builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
            return;
        }
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
        if let Some(hint) = self.actions_hint() {
            builder.set_description(hint);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }
}

/// Wrapper that sets Role::TreeItem with level, expanded state, and selection.
///
/// The rebuild boundary for a selection move, as in [`ListItemWrapper`]: the
/// wrapper builds its row widget from a [`RowBody`] and rebuilds only when its
/// own row's selectedness flips, so a selection move re-runs the delegate for
/// the two rows whose state changed and every other row keeps its node. The
/// row's handlers live on this wrapper's id, which never changes for a
/// selection move.
pub(crate) struct TreeItemWrapper {
    body: RowBody,
    /// The selection to watch, and this row's flat index in it.
    selection: Option<RowSelection>,
    index: usize,
    /// Rebuild trigger, bumped only when *this* row's selectedness flips.
    version: Signal<u64>,
    /// The row widget the pane already built for the first build, so the
    /// pane can tell a row with nothing to show (no wrapper at all) from one
    /// that has. Adopted by the first build; every later build calls `body`.
    first: Option<WidgetId>,
    /// 1-based depth, or `None` when the row's metadata has not resolved.
    ///
    /// A lazy `TreeDataSource` hands back no metadata for a row still
    /// loading, and a placeholder has no knowable depth or sibling position.
    /// Both are `Option` rather than a defaulted 1 because a screen reader
    /// repeats whatever is published as fact: a row four levels down that
    /// announced "level 1, 1 of …" while its window loaded was not degrading,
    /// it was lying. Absent, they are simply not announced — and the row that
    /// replaces the placeholder announces the truth. The same reasoning the
    /// tree table applies to `CellContext::depth`.
    level: Option<usize>,
    /// 1-based position within the sibling group, or `None` — see `level`.
    position: Option<usize>,
    expanded: Option<bool>, // None if leaf
    /// The child rows hanging from this one and its children's set size,
    /// filled in by the pane (see `common::tree_a11y`).
    at: Rc<crate::common::tree_a11y::RowAt>,

    // Build state.
    selected: bool,
    child: Option<WidgetId>,
}

impl TreeItemWrapper {
    /// `first` is the row widget the pane built with `body` for `selected`,
    /// the selectedness the wrapper reads again at its first build.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        body: RowBody,
        selection: Option<RowSelection>,
        index: usize,
        first: WidgetId,
        level_1based: Option<usize>,
        position_1based: Option<usize>,
        expanded: Option<bool>,
        at: Rc<crate::common::tree_a11y::RowAt>,
    ) -> Self {
        Self {
            body,
            selection,
            index,
            version: Signal::new(0),
            first: Some(first),
            level: level_1based,
            position: position_1based,
            expanded,
            at,
            selected: false,
            child: None,
        }
    }
}

impl std::fmt::Debug for TreeItemWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreeItemWrapper")
            .field("index", &self.index)
            .field("level", &self.level)
            .field("position", &self.position)
            .field("expanded", &self.expanded)
            .field("selected", &self.selected)
            .finish_non_exhaustive()
    }
}

impl Widget for TreeItemWrapper {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
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
            let last = Cell::new(selected);
            let handle = selection.observe_for_rebuild(move || {
                let now = watched.is_selected(index);
                if now != last.get() {
                    last.set(now);
                    version.set(version.get() + 1);
                }
            });
            ctx.own_handle(handle);
        }
        self.child = match self.first.take() {
            Some(first) => Some(first),
            None => (self.body)(ctx, selected),
        };
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
        builder.set_role(teksilo_core::accesskit::Role::TreeItem);
        if let Some(level) = self.level {
            builder.set_level(level);
        }
        if let Some(position) = self.position {
            builder.set_position_in_set(position);
        }
        // The set size on this node is not this row's "of M": AccessKit
        // reads an item's set size from the first node above it that carries
        // one (`size_of_set_from_container`), never from the item itself. So
        // the count published here is this row's number of children, which
        // its child rows read; this row's own "of M" is on the node it hangs
        // from (its parent row, a stand-in for one, or the pane). Only an
        // open branch carries one, and on Windows and Linux every open branch
        // with child rows under it must, or they would read an ancestor's.
        // Known side effect: a node inside this row's own content (a row
        // checkbox, a label) also reads this count as its set size. It
        // publishes no position, so no reader speaks "x of y" for it;
        // TreeTableView avoids it with a group for the child rows, which here
        // would cost NVDA its level count (it stops at the first parent that
        // is not a tree item).
        if let Some(n) = self.at.size_of_set.get() {
            builder.set_size_of_set(n);
        }

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
        self.child.into_iter().collect()
    }

    /// Its own content, then the rows that hang from it (the nested shape,
    /// see `common::tree_a11y`).
    fn accessibility_children(&self) -> Option<Vec<WidgetId>> {
        crate::common::tree_a11y::row_accessibility_children(self.child, &self.at)
    }
}
