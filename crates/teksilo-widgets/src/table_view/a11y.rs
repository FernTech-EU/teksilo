// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Accessibility wrappers for `TableView` / `TreeTableView`.
//!
//! AccessKit's table semantics work by labelling individual nodes with
//! `Role::Table` / `Role::Row` / `Role::Cell` and stamping each cell with
//! its row/column index. The cell delegate the user supplies typically
//! produces a generic widget (Text, Button, …) that wouldn't carry table
//! semantics by itself, so the body wraps each cell in a thin
//! `CellA11y` node. `TableView`'s row containers themselves
//! (`BodyRow`) carry `Role::Row` and
//! row-index metadata directly. `TreeTableView` wraps each row in a
//! `TreeRowA11y` instead, to declare `set_level` and `set_expanded`, and
//! mirrors the level onto the tree column's cell as well — `CellA11y::
//! with_level` says why the row alone is not heard.
//!
//! These wrappers do not paint or affect layout: they pass the proposed
//! size straight through to their single child and forward all bounds.

use std::rc::Rc;

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::widget::{LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;

use teksilo_data::SortDirection;

/// A flag read at each accessibility walk rather than fixed at build, so the
/// node that publishes it need not be rebuilt when it changes.
#[derive(Clone)]
pub(crate) struct LiveFlag(Rc<dyn Fn() -> bool>);

impl LiveFlag {
    pub(crate) fn new(read: impl Fn() -> bool + 'static) -> Self {
        Self(Rc::new(read))
    }

    pub(crate) fn get(&self) -> bool {
        (self.0)()
    }
}

impl std::fmt::Debug for LiveFlag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("LiveFlag").field(&self.get()).finish()
    }
}

pub(crate) fn live_or(live: &Option<LiveFlag>, fixed: bool) -> bool {
    live.as_ref().map_or(fixed, LiveFlag::get)
}

/// Wrapper that announces a `Role::Cell` (or `Role::RowHeader`) with
/// row/column indices and selection state.
#[derive(Debug)]
pub(crate) struct CellA11y {
    child: WidgetId,
    row_index_1based: usize,
    col_index_1based: usize,
    selected: bool,
    /// When set, the selected state read at each accessibility walk instead
    /// of `selected`: a selection change no longer rebuilds the cell.
    live_selected: Option<LiveFlag>,
    /// When true, emit `Role::RowHeader` instead of `Role::Cell` — used
    /// when the table promotes a column to row-header status.
    is_row_header: bool,
    /// Whether the cell is itself the selectable unit.
    ///
    /// Decides `Role::GridCell` over `Role::Cell`, which is not cosmetic on
    /// Windows: `accesskit_windows` supports UIA's `SelectionItem` pattern for
    /// `GridCell` unconditionally and for `Role::Cell` not at all, so a
    /// selected cell announced as `Cell` never reports `IsSelected`. The two
    /// map identically on macOS (`NSAccessibilityCellRole`) and AT-SPI
    /// (`TableCell`), so this costs nothing there.
    is_grid_cell: bool,
    /// Optional name override (when the cell content isn't textual).
    name: Option<String>,
    /// 1-based hierarchy level, mirrored from the row this cell belongs to.
    ///
    /// `None` on a flat table, and on every column of a tree table but the
    /// one that draws the indent. See `with_level`.
    level_1based: Option<usize>,
    /// Expand state of the row this cell belongs to: `Some(true|false)` on a
    /// branch, `None` on a leaf and on every cell that is not the tree
    /// column's. See `with_expanded`.
    expanded: Option<bool>,
    /// How many columns the cell covers, when it covers more than its own.
    /// See `with_column_span`.
    column_span: Option<usize>,
}

impl CellA11y {
    pub(crate) fn new(
        child: WidgetId,
        row_index_1based: usize,
        col_index_1based: usize,
        selected: bool,
    ) -> Self {
        Self {
            child,
            row_index_1based,
            col_index_1based,
            selected,
            live_selected: None,
            is_row_header: false,
            is_grid_cell: false,
            name: None,
            level_1based: None,
            expanded: None,
            column_span: None,
        }
    }

    /// Announce `Role::GridCell` — the cell is the selectable unit. Named for
    /// the sibling `with_role_row_header`, not `as_*`, which clippy reserves
    /// for by-reference conversions.
    /// Read the selected state live, at each accessibility walk. The view
    /// binds the selection at `AccessibilityOnly` so a change re-walks.
    pub(crate) fn with_live_selected(mut self, selected: LiveFlag) -> Self {
        self.live_selected = Some(selected);
        self
    }

    pub(crate) fn with_grid_cell_role(mut self, is_grid_cell: bool) -> Self {
        self.is_grid_cell = is_grid_cell;
        self
    }

    /// Promote to `Role::RowHeader` (`row_header_column` support).
    #[allow(dead_code)]
    pub(crate) fn with_role_row_header(mut self, is_row_header: bool) -> Self {
        self.is_row_header = is_row_header;
        self
    }

    /// Override the cell's accessible name (`cell_label_fn`).
    #[allow(dead_code)]
    pub(crate) fn with_name(mut self, name: Option<String>) -> Self {
        self.name = name;
        self
    }

    /// Mirror the row's 1-based hierarchy level onto this cell.
    ///
    /// The depth belongs to the *row*, and `TreeRowA11y` publishes it there.
    /// But `TreeTableView` navigates by cell: it nominates the focused *cell*
    /// as the container's `active_descendant`, which leaves the row an
    /// ancestor of the focused node — and NVDA drops `positionInfo_level`
    /// from an ancestor it announces on the way in (`speech.py` clears
    /// `allowProperties["positionInfo_level"]` for
    /// `OutputReason.FOCUSENTERED`). So the depth was published correctly on
    /// the row and spoken nowhere. `TreeView` does not have this problem: its
    /// rows *are* the focused nodes.
    ///
    /// Only the tree column asks for this. It is the column that draws the
    /// indent and the chevron, so it is the one whose announcement matches
    /// what is on screen, and repeating "level 3" on all seven columns of one
    /// row is noise. The price is that arrowing *down* a non-tree column
    /// announces no depth change; the tree column is one arrow away, and
    /// mirroring everywhere to close that gap would charge every horizontal
    /// move for it.
    ///
    /// Windows-only in effect, and not because of this decision: `level` is
    /// the one positional property `accesskit_macos-0.27.0` and
    /// `accesskit_atspi_common-0.20.0` do not expose at all (the latter
    /// publishes `posinset` / `setsize` and no `level`). VoiceOver and Orca
    /// heard nothing from the row either — for them the indent is what
    /// carries depth.
    pub(crate) fn with_level(mut self, level_1based: Option<usize>) -> Self {
        self.level_1based = level_1based;
        self
    }

    /// Mirror the row's expand state onto this cell — same reason as
    /// `with_level`, and one more.
    ///
    /// Announcing it is the first reason: "expanded" is a *state*, and a
    /// screen reader reads the states of the node it is on, which here is the
    /// cell. But a state on an ancestor fails a second way that a level does
    /// not. Toggling a row with ArrowRight moves nothing, so no focus event
    /// follows; all the user has to go on is a property change, and
    /// `accesskit_windows` raises that on the node whose property changed —
    /// the row, which is not the focused element, so NVDA drops it. Opening
    /// and closing a branch was silent.
    ///
    /// The state owes an answer, and already has one. `accesskit_consumer`
    /// derives UIA's ExpandCollapse pattern from the *property*, not from the
    /// action list, so a cell that declares a state will be sent `Expand` and
    /// `Collapse` whether or not anything handles them — the
    /// advertised-and-inert bug the row's own call site fixed. Nothing new is
    /// needed for the cell: an `AccessAction` bubbles, and the row carrying
    /// that pair is on the cell's path to the root. Handlers on the cell as
    /// well would only run `set_expanded_at` twice.
    pub(crate) fn with_expanded(mut self, expanded: Option<bool>) -> Self {
        self.expanded = expanded;
        self
    }

    /// Announce that the cell covers `span` columns (`aria-colspan`), from its
    /// own column index on. `TreeTableView`'s full-width rows are one cell
    /// across every column and say so here.
    ///
    /// Published, not yet heard: none of the adapters this workspace pins
    /// (`accesskit_windows` 0.35, `accesskit_macos` 0.27,
    /// `accesskit_atspi_common` 0.21) reads `column_span`. It is the property
    /// AccessKit defines for exactly this, on the node it belongs to, so an
    /// adapter that learns it needs nothing more from here.
    pub(crate) fn with_column_span(mut self, span: Option<usize>) -> Self {
        self.column_span = span;
        self
    }
}

impl Widget for CellA11y {
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
        builder.set_role(if self.is_row_header {
            teksilo_core::accesskit::Role::RowHeader
        } else if self.is_grid_cell {
            teksilo_core::accesskit::Role::GridCell
        } else {
            teksilo_core::accesskit::Role::Cell
        });
        if let Some(ref name) = self.name {
            builder.set_name(name.clone());
        }
        builder.set_selected(live_or(&self.live_selected, self.selected));
        builder.set_row_index(self.row_index_1based);
        builder.set_column_index(self.col_index_1based);
        // The floor matches `TreeRowA11y`: a caller that computed a depth of
        // 0 lands on the root level, not one above it.
        if let Some(level) = self.level_1based {
            builder.set_level(level.max(1));
        }
        // A leaf declares nothing, which UIA reads as `LeafNode` — the state
        // a cell with no subtree should report, and the reason this is not a
        // plain `bool`.
        if let Some(expanded) = self.expanded {
            builder.set_expanded(expanded);
        }
        if let Some(span) = self.column_span {
            builder.set_column_span(span);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}

/// `TreeTableView`-flavoured row wrapper. Announces `Role::Row` and, in
/// addition to the row index, declares `set_level` (1-based depth),
/// `set_expanded` when the row has children, and `set_position_in_set`
/// among the row's siblings, mirroring what `TreeView`'s `TreeItemWrapper`
/// (`list_item_a11y.rs`) announces, so both widgets say which sibling a
/// row is.
///
/// No `size_of_set` goes with it, here or on the container: AccessKit
/// resolves a set size by walking up from the item, and a flattened tree has
/// no per-branch node to hold one, so neither widget can say "of 5". The
/// comment in `accessibility` below spells out what expressing it would cost.
#[derive(Debug)]
pub(crate) struct TreeRowA11y {
    child: WidgetId,
    row_index_1based: usize,
    /// 1-based hierarchy level (root rows are 1).
    level_1based: usize,
    /// `Some(true|false)` for non-leaf rows; `None` for leaves.
    expanded: Option<bool>,
    selected: bool,
    /// See `CellA11y::live_selected`.
    live_selected: Option<LiveFlag>,
    /// 1-based position among this row's siblings (`TreeSource::sibling_pos`).
    position_in_set: usize,
    /// How many cells the row holds: the set size its cells read (see
    /// `accessibility`).
    cell_count: usize,
    /// The child rows hanging from this one and their set size, filled in
    /// by the pane (see `common::tree_a11y`).
    at: Rc<crate::common::tree_a11y::RowAt>,
}

/// The element id of the group a tree table row's child rows hang from
/// (owned by the row, so it cannot collide with a pane's stand-ins).
const CHILD_ROWS_GROUP: u64 = 0;

impl TreeRowA11y {
    pub(crate) fn new(
        child: WidgetId,
        row_index_1based: usize,
        level_1based: usize,
        expanded: Option<bool>,
        selected: bool,
        position_in_set: usize,
    ) -> Self {
        Self {
            child,
            row_index_1based,
            level_1based,
            expanded,
            selected,
            live_selected: None,
            position_in_set,
            cell_count: 0,
            at: Rc::default(),
        }
    }

    /// The row's cell count and the cell its pane fills with the child rows
    /// hanging from it.
    pub(crate) fn with_rows_below(
        mut self,
        cell_count: usize,
        at: Rc<crate::common::tree_a11y::RowAt>,
    ) -> Self {
        self.cell_count = cell_count;
        self.at = at;
        self
    }

    /// See [`CellA11y::with_live_selected`].
    pub(crate) fn with_live_selected(mut self, selected: LiveFlag) -> Self {
        self.live_selected = Some(selected);
        self
    }
}

impl Widget for TreeRowA11y {
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
        builder.set_role(teksilo_core::accesskit::Role::Row);
        builder.set_selected(live_or(&self.live_selected, self.selected));
        if let Some(exp) = self.expanded {
            builder.set_expanded(exp);
        }
        builder.set_row_index(self.row_index_1based);
        // The floor keeps a caller that computed a depth of 0 on the root
        // level rather than one above it. `set_level` takes the ARIA 1-based
        // number and converts to AccessKit's zero-based property itself.
        builder.set_level(self.level_1based.max(1));
        builder.set_position_in_set(self.position_in_set);
        // AccessKit reads an item's set size from the first node above it
        // that carries one (`size_of_set_from_container`), never from the
        // item itself. This row's "of M" is therefore on the node it hangs
        // from (the pane, the group under its parent row, or a stand-in for
        // an unrealized ancestor), and the number on this node is what its
        // cells read: their count, so a cell never reads the row's children
        // or siblings as its own set.
        if self.cell_count > 0 {
            builder.set_size_of_set(self.cell_count);
        }
        // The child rows (nested shape): after the row's own cells, in a
        // group that carries their set size. The group, not this node, so
        // the cells keep the count above. The walker would push the cells'
        // node and the child rows straight onto this node; the redirect hook
        // claims them, and they are attached here in reading order.
        let kids = self.at.kids.borrow();
        if !kids.is_empty() {
            builder
                .inner_mut()
                .push_child(teksilo_core::accessibility::widget_id_to_node_id(
                    self.child,
                ));
            let group = builder.push_scene_child(
                CHILD_ROWS_GROUP,
                teksilo_core::accessibility::SyntheticKind::TreeChildRows,
                |b| {
                    b.set_role(teksilo_core::accesskit::Role::Group);
                    if let Some(n) = self.at.size_of_set.get() {
                        b.set_size_of_set(n);
                    }
                },
            );
            builder.set_child_bounds_local(group, Rect::new(0.0, 0.0, 0.0, 0.0));
            for &k in kids.iter() {
                builder.attach_scene_child_under(
                    group,
                    teksilo_core::accessibility::widget_id_to_node_id(k),
                );
            }
        }
    }

    fn accessibility_children(&self) -> Option<Vec<WidgetId>> {
        crate::common::tree_a11y::row_accessibility_children([self.child], &self.at)
    }

    fn wants_descendant_redirects(&self) -> bool {
        !self.at.kids.borrow().is_empty()
    }

    fn a11y_redirect_descendant(
        &self,
        self_id: WidgetId,
        descendant: WidgetId,
    ) -> Option<teksilo_core::accesskit::NodeId> {
        let kids = self.at.kids.borrow();
        if kids.is_empty() {
            return None;
        }
        // The row's own cells node: claimed only so the walker does not push
        // it ahead of the group; `accessibility` pushes it onto this node
        // itself. The walker reads only whether a claim was made, so the id
        // returned is this node's own.
        if descendant == self.child {
            return Some(teksilo_core::accessibility::widget_id_to_node_id(self_id));
        }
        kids.contains(&descendant).then(|| {
            teksilo_core::accessibility::synthetic_node_id(
                self_id,
                CHILD_ROWS_GROUP,
                teksilo_core::accessibility::SyntheticKind::TreeChildRows,
            )
        })
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}

/// Header column wrapper — `Role::ColumnHeader` with sort direction
/// when this is the active sort column.
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct ColumnHeaderA11y {
    child: WidgetId,
    col_index_1based: usize,
    name: String,
    sort: Option<SortDirection>,
}

#[allow(dead_code)] // not wired: `HeaderCell` publishes its own node
impl ColumnHeaderA11y {
    pub(crate) fn new(
        child: WidgetId,
        col_index_1based: usize,
        name: impl Into<String>,
        sort: Option<SortDirection>,
    ) -> Self {
        Self {
            child,
            col_index_1based,
            name: name.into(),
            sort,
        }
    }
}

impl Widget for ColumnHeaderA11y {
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
        builder.set_role(teksilo_core::accesskit::Role::ColumnHeader);
        builder.set_name(self.name.clone());
        builder.set_column_index(self.col_index_1based);
        if let Some(dir) = self.sort {
            let ak_dir = match dir {
                SortDirection::Ascending => teksilo_core::accesskit::SortDirection::Ascending,
                SortDirection::Descending => teksilo_core::accesskit::SortDirection::Descending,
            };
            builder.inner_mut().set_sort_direction(ak_dir);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}
