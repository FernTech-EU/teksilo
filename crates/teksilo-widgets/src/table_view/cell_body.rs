// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The rebuild boundary of one table cell's delegate output.
//!
//! A cell delegate is handed `is_selected` and `is_focused` at build, so a
//! selection or cursor move has to build the cell's widget again. The panes of
//! `TableView` and `TreeTableView` used to do that by rebuilding themselves:
//! every realized row and cell, their AccessKit nodes and the handlers on them
//! were replaced on every click. The first click of a double click selects the
//! row, so the second click landed on a node that had not seen the first one,
//! and a row or cell that the first click selected could never be opened or
//! edited by a double click.
//!
//! [`CellBody`] sits between the cell's `CellA11y` node (which carries the
//! cell's handlers and accessibility, and is never rebuilt by a selection) and
//! the widget the delegate returns. It watches the selection, the cell
//! selection and the cell cursor, and rebuilds alone when *its own* flags
//! change: a selection move re-runs the delegate for the cells whose state it
//! changed and for no other.
//!
//! It publishes no accessibility node of its own (a property-less generic
//! container, which the walker prunes) and is layout-transparent.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;

use crate::data_views::RowSelection;

/// The two flags a cell delegate reads at build that a selection or cursor
/// move can change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellFlags {
    pub selected: bool,
    pub focused: bool,
}

/// Builds a cell's widget for the given flags, and returns its id. `None`
/// when the cell has nothing to show.
pub(crate) type CellBuild = Rc<dyn Fn(&mut BuildContext, CellFlags) -> Option<WidgetId>>;

/// What one cell watches, and how it reads its own flags from it.
#[derive(Clone)]
pub(crate) struct CellWatch {
    pub row_selection: Option<RowSelection>,
    pub cell_selection: Option<Signal<BTreeSet<(usize, usize)>>>,
    pub focused_cell: Signal<Option<(usize, usize)>>,
    /// This cell's flags, read live from the models above.
    pub flags: Rc<dyn Fn() -> CellFlags>,
    /// Whether this cell is the one being edited. Its delegate output holds
    /// the editor (and whatever the user typed into it), so a flag change
    /// does not rebuild it: the edit's end rebuilds the pane, which builds
    /// the cell again with fresh flags.
    pub editing: Rc<dyn Fn() -> bool>,
}

/// See the module docs.
pub(crate) struct CellBody {
    build: CellBuild,
    watch: CellWatch,
    /// Rebuild trigger, bumped only when this cell's own flags change.
    version: Signal<u64>,
    /// The widget the pane built for the first build (so the pane can skip a
    /// cell with nothing to show). Adopted by the first build.
    first: Option<WidgetId>,
    child: Option<WidgetId>,
}

impl CellBody {
    /// `first` is the widget the pane built with `build` for the flags
    /// `watch.flags` reads now.
    pub(crate) fn new(build: CellBuild, watch: CellWatch, first: WidgetId) -> Self {
        Self {
            build,
            watch,
            version: Signal::new(0),
            first: Some(first),
            child: None,
        }
    }
}

impl std::fmt::Debug for CellBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CellBody")
            .field("child", &self.child)
            .finish_non_exhaustive()
    }
}

impl Widget for CellBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // A persistent field rather than `ctx.signal`, so the observers
        // installed below survive into the build they trigger.
        self.version
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        let flags = (self.watch.flags)();
        let check: Rc<dyn Fn()> = {
            let read = self.watch.flags.clone();
            let editing = self.watch.editing.clone();
            let version = self.version.clone();
            // Seeded with what this build is about to draw.
            let last = Cell::new(flags);
            Rc::new(move || {
                let now = read();
                if now != last.get() {
                    last.set(now);
                    if !editing() {
                        version.set(version.get() + 1);
                    }
                }
            })
        };
        if let Some(ref selection) = self.watch.row_selection {
            let check = check.clone();
            ctx.own_handle(selection.observe_for_rebuild(move || check()));
        }
        if let Some(ref cells) = self.watch.cell_selection {
            let check = check.clone();
            ctx.effect(cells, move |_| check());
        }
        ctx.effect(&self.watch.focused_cell, move |_| check());

        self.child = match self.first.take() {
            Some(first) => Some(first),
            None => (self.build)(ctx, flags),
        };
        self.child.into_iter().collect()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // The full response, not just the size: in a tree table this sits
        // inside the tree column's chrome, where a shrinkable delegate (an
        // ellipsizing label) has to keep its shrink weight.
        self.child
            .and_then(|child| ctx.child_layout_response(child, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into())
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

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }
}
