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

use teksilo_canvas::{Rect, SizeProposal};

use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::widget::{LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;

/// Wraps a tile's delegate widget with `Role::GridCell` + grid coordinates.
///
/// 1-based `row_index` / `col_index` follow the ARIA convention. `position`
/// (`aria-posinset`) is the flat 1-based index in the *logical* set, not in
/// the realized window, so virtualization stays invisible to assistive tech.
/// Its `aria-setsize` half is not here: AccessKit reads a set size from the
/// container, so `GridView`'s own `Role::Grid` node publishes it.
#[derive(Debug)]
pub(crate) struct TileA11y {
    child: WidgetId,
    row_index: usize, // 1-based
    col_index: usize, // 1-based
    position: usize,  // 1-based flat index
    selected: bool,
    /// Concise per-item name (`GridView::tile_a11y_label`); `None` leaves the
    /// cell's name to its contents.
    name: Option<String>,
}

impl TileA11y {
    pub(crate) fn new(
        child: WidgetId,
        row_index_1based: usize,
        col_index_1based: usize,
        position_1based: usize,
        selected: bool,
        name: Option<String>,
    ) -> Self {
        Self {
            child,
            row_index: row_index_1based,
            col_index: col_index_1based,
            position: position_1based,
            selected,
            name,
        }
    }
}

impl Widget for TileA11y {
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
        builder.set_role(teksilo_core::accesskit::Role::GridCell);
        if let Some(name) = &self.name {
            builder.set_name(name.clone());
        }
        builder.set_selected(self.selected);
        builder.set_position_in_set(self.position);
        // The "of N" half lives on the grid's own `Role::Grid` node, beside
        // the row and column counts.
        builder.set_row_index(self.row_index);
        builder.set_column_index(self.col_index);
        builder.add_action(teksilo_core::accesskit::Action::Click);
        builder.add_action(teksilo_core::accesskit::Action::Focus);
        // Advertised, not just handled: every adapter gates its scroll
        // pattern on the node *supporting* the action (UIA's
        // `IScrollItemProvider`, AppKit's `accessibilityScrollToVisible`,
        // AT-SPI's `ScrollTo`), so a tile whose pane installs the handler
        // through `on_access_action` alone is still unreachable to a real
        // screen reader. The handler itself lives on the body pane.
        builder.add_action(teksilo_core::accesskit::Action::ScrollIntoView);
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
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
