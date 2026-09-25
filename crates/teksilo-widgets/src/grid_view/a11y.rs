// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Accessibility wrappers for `GridView` tiles.
//!
//! Each realized tile is wrapped in a [`TileA11y`] node carrying
//! `Role::GridCell` plus the ARIA grid coordinates (row/column index and
//! position-in-set). The container itself emits `Role::Grid` with the
//! *logical* row/column totals and the `size_of_set` its tiles resolve
//! upward to (see `GridView::accessibility`), so screen readers announce
//! "row R, column C, N of M".

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

/// Builds a tile's own widget, and returns its id.
///
/// Called with the tile's selectedness at the moment of the build, because
/// `TileContext::is_selected` hands it to the delegate. Erases the source's
/// item type: the pane closes over the delegate and the data source, and this
/// is what is left of them. `None` means the tile had nothing to show after
/// all.
pub(crate) type TileBody = Rc<dyn Fn(&mut BuildContext, bool) -> Option<WidgetId>>;

/// Wraps a tile's delegate widget with `Role::GridCell` + grid coordinates.
///
/// 1-based `row_index` / `col_index` follow the ARIA convention. The position
/// in set (`aria-posinset`) is `index + 1`, the flat index in the *logical*
/// set, not in the realized window, so virtualization stays invisible to
/// assistive tech. Its `aria-setsize` half is not here: AccessKit reads a set
/// size from the container, so `GridView`'s own `Role::Grid` node publishes
/// it.
///
/// # The rebuild boundary for a selection change
///
/// The wrapper builds the delegate's widget itself and watches the selection
/// for its own tile only, as `ListItemWrapper` does for a list row. A change
/// of selection rebuilds the tiles whose selectedness it flipped, below this
/// node; every tile keeps the node id the grid names as its active
/// descendant. The pane used to rebuild every realized tile instead, so the
/// tile under the cursor came back as a node the platform had never seen:
/// each adapter reported a focus change to it, and Orca stopped what it was
/// saying, the count just announced included, to read the tile again. And the
/// tile's `selected` state never changed on a node a reader knew, so a toggle
/// was never heard as one.
///
/// The per-tile handlers the pane applies to this node's id survive the
/// rebuild of its children, which is all that happens here. They must not be
/// applied a second time: `HandlerSet::merge` chains handlers.
pub(crate) struct TileA11y {
    body: TileBody,
    /// The selection to watch, and this tile's flat model index in it.
    selection: Option<RowSelection>,
    index: usize,
    row_index: usize, // 1-based
    col_index: usize, // 1-based
    /// Concise per-item name (`GridView::tile_a11y_label`); `None` leaves the
    /// cell's name to its contents.
    name: Option<String>,
    /// Rebuild trigger, bumped only when *this* tile's selectedness flips.
    version: Signal<u64>,

    // Build state.
    selected: bool,
    child: Option<WidgetId>,
}

impl TileA11y {
    pub(crate) fn new(
        body: TileBody,
        selection: Option<RowSelection>,
        index: usize,
        row_index_1based: usize,
        col_index_1based: usize,
        name: Option<String>,
    ) -> Self {
        Self {
            body,
            selection,
            index,
            row_index: row_index_1based,
            col_index: col_index_1based,
            name,
            version: Signal::new(0),
            selected: false,
            child: None,
        }
    }
}

impl std::fmt::Debug for TileA11y {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TileA11y")
            .field("index", &self.index)
            .field("selected", &self.selected)
            .finish_non_exhaustive()
    }
}

impl Widget for TileA11y {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // A persistent field rather than `ctx.signal`, so the observer
        // installed below survives into the build it triggers.
        self.version
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        let selected = self
            .selection
            .as_ref()
            .is_some_and(|s| s.is_selected(self.index));
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
        builder.set_role(teksilo_core::accesskit::Role::GridCell);
        if let Some(name) = &self.name {
            builder.set_name(name.clone());
        }
        builder.set_selected(self.selected);
        builder.set_position_in_set(self.index + 1);
        // The "of N" half lives on the grid's own `Role::Grid` node, beside
        // the row and column counts.
        builder.set_row_index(self.row_index);
        builder.set_column_index(self.col_index);
        builder.add_action(teksilo_core::accesskit::Action::Click);
        // No `Action::Focus`: a tile is reached through the grid's active
        // descendant and takes no keys of its own. The dispatcher services
        // `Focus` itself, moving keyboard focus onto the node named, focusable
        // or not, so a screen reader's focus request on a tile (UIA
        // `SetFocus`, AT-SPI `grab_focus`, VoiceOver's keyboard focus following
        // its cursor) took focus off the grid while the grid's cursor stayed
        // where it was: Enter opened the cursor's tile rather than the one the
        // reader was on, and the next key that rebuilt the tiles dropped focus
        // onto the window. An assistive `Focus` on a tile is now reported
        // unhandled and moves nothing; `Click` chooses the tile and moves the
        // cursor to it. `ListItemWrapper` and a calendar day do the same.
        // Advertised, not just handled: every adapter gates its scroll
        // pattern on the node *supporting* the action (UIA's
        // `IScrollItemProvider`, AppKit's `accessibilityScrollToVisible`,
        // AT-SPI's `ScrollTo`), so a tile whose pane installs the handler
        // through `on_access_action` alone is still unreachable to a real
        // screen reader. The handler itself lives on the body pane.
        builder.add_action(teksilo_core::accesskit::Action::ScrollIntoView);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }
}

/// Wraps a section header with `Role::RowHeader` and its row position.
#[derive(Debug)]
pub(crate) struct SectionHeaderA11y {
    child: WidgetId,
    title: String,
}

impl SectionHeaderA11y {
    pub(crate) fn new(child: WidgetId, title: String) -> Self {
        Self { child, title }
    }
}

impl Widget for SectionHeaderA11y {
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
        builder.set_role(teksilo_core::accesskit::Role::RowHeader);
        builder.set_name(self.title.clone());
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}
