//! Accessibility wrappers for `TableView` / `TreeTable`.
//!
//! AccessKit's table semantics work by labelling individual nodes with
//! `Role::Table` / `Role::Row` / `Role::Cell` and stamping each cell with
//! its row/column index. The cell delegate the user supplies typically
//! produces a generic widget (Text, Button, …) that wouldn't carry table
//! semantics by itself, so the body wraps each cell in a thin
//! [`CellA11y`] node and each row in a [`RowA11y`] node. `TreeRowA11y` is
//! the tree-flavoured row wrapper used by `TreeTable` — it adds
//! `set_level` and `set_expanded`.
//!
//! These wrappers do not paint or affect layout: they pass the proposed
//! size straight through to their single child and forward all bounds.

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::widget::{LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

use bastyde_data::SortDirection;

/// Wrapper that announces a `Role::Row` with positional metadata for
/// callers that build their row as a single composed widget rather than
/// the column-laid-out [`BodyRow`](crate::table_view::body::BodyRow).
/// Reserved for `TreeTable`'s tree column and row-overrides.
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct RowA11y {
    child: WidgetId,
    /// 1-based row index within the table. Header is row 1; first body row is 2.
    row_index_1based: usize,
    selected: bool,
}

#[allow(dead_code)]
impl RowA11y {
    pub(crate) fn new(child: WidgetId, row_index_1based: usize, selected: bool) -> Self {
        Self {
            child,
            row_index_1based,
            selected,
        }
    }
}

impl Widget for RowA11y {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
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
        builder.set_role(bastyde_core::accesskit::Role::Row);
        builder.set_selected(self.selected);
        builder.inner_mut().set_row_index(self.row_index_1based);
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}

/// Wrapper that announces a `Role::Cell` (or `Role::RowHeader`) with
/// row/column indices and selection state.
#[derive(Debug)]
pub(crate) struct CellA11y {
    child: WidgetId,
    row_index_1based: usize,
    col_index_1based: usize,
    selected: bool,
    /// When true, emit `Role::RowHeader` instead of `Role::Cell` — used
    /// when the table promotes a column to row-header status.
    is_row_header: bool,
    /// Optional name override (when the cell content isn't textual).
    name: Option<String>,
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
            is_row_header: false,
            name: None,
        }
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
}

impl Widget for CellA11y {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
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
            bastyde_core::accesskit::Role::RowHeader
        } else {
            bastyde_core::accesskit::Role::Cell
        });
        if let Some(ref name) = self.name {
            builder.set_name(name.clone());
        }
        builder.set_selected(self.selected);
        let n = builder.inner_mut();
        n.set_row_index(self.row_index_1based);
        n.set_column_index(self.col_index_1based);
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}

/// `TreeTable`-flavoured row wrapper. Like [`RowA11y`] but additionally
/// declares `set_level` (1-based depth) and `set_expanded` when the row
/// has children.
#[derive(Debug)]
pub(crate) struct TreeRowA11y {
    child: WidgetId,
    row_index_1based: usize,
    /// 1-based hierarchy level (root rows are 1).
    level_1based: usize,
    /// `Some(true|false)` for non-leaf rows; `None` for leaves.
    expanded: Option<bool>,
    selected: bool,
}

impl TreeRowA11y {
    pub(crate) fn new(
        child: WidgetId,
        row_index_1based: usize,
        level_1based: usize,
        expanded: Option<bool>,
        selected: bool,
    ) -> Self {
        Self {
            child,
            row_index_1based,
            level_1based,
            expanded,
            selected,
        }
    }
}

impl Widget for TreeRowA11y {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
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
        builder.set_role(bastyde_core::accesskit::Role::Row);
        builder.set_selected(self.selected);
        if let Some(exp) = self.expanded {
            builder.set_expanded(exp);
        }
        let n = builder.inner_mut();
        n.set_row_index(self.row_index_1based);
        // Clamp to 1.. — AccessKit's `set_level` is `usize` but ARIA
        // levels start at 1.
        n.set_level(self.level_1based.max(1));
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

#[allow(dead_code)] // wired up by HeaderRow
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
    ) -> bastyde_core::widget::LayoutResponse {
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
        builder.set_role(bastyde_core::accesskit::Role::ColumnHeader);
        builder.set_name(self.name.clone());
        let n = builder.inner_mut();
        n.set_column_index(self.col_index_1based);
        if let Some(dir) = self.sort {
            let ak_dir = match dir {
                SortDirection::Ascending => bastyde_core::accesskit::SortDirection::Ascending,
                SortDirection::Descending => bastyde_core::accesskit::SortDirection::Descending,
            };
            n.set_sort_direction(ak_dir);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}
