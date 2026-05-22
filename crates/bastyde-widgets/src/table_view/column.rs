//! Column descriptor + supporting enums for `TableView` and `TreeTable`.
//!
//! Columns are declared once per table; the table consumes a `Vec<Column<T>>`
//! and shares it with its body subtree. The cell delegate is `Rc`-erased so a
//! `Column<T>` is cheap to clone for any internal pane that needs its own
//! copy.

use std::rc::Rc;

use bastyde_core::widget::Widget;
use bastyde_data::SortDirection;
use bastyde_i18n::LocalizedString;

/// How a column's width is determined during layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColumnWidth {
    /// Exact pixel width. Clamped by `min_width` / `max_width`.
    Fixed(f32),
    /// Share of the leftover space proportional to the flex factor —
    /// behaves like CSS `flex-grow`. The factor must be `> 0.0`.
    Flex(f32),
    /// Intrinsic content width (currently approximated by the table's
    /// `min_column_width_default` token; refined to probe the
    /// header label and visible cells).
    Auto,
}

impl Default for ColumnWidth {
    fn default() -> Self {
        ColumnWidth::Flex(1.0)
    }
}

/// Whether a column is pinned to one side of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PinnedSide {
    /// Pinned against the leading edge — stays visible during horizontal
    /// scroll.
    Leading,
    /// Not pinned — scrolls horizontally with the body.
    #[default]
    None,
    /// Pinned against the trailing edge.
    Trailing,
}

/// Horizontal alignment of a cell's content within its column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Alignment {
    #[default]
    Leading,
    Center,
    Trailing,
}

/// Strategy when a cell's text overflows its column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TruncationPolicy {
    /// `…`-elide the trailing portion. **Default.**
    #[default]
    Ellipsis,
    /// Don't truncate; let the cell content draw beyond the column edge
    /// (the body pane's clip will hide it).
    None,
    /// Fade the trailing portion — gradient mask.
    Fade,
}

/// Whether the table draws grid lines between rows / columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GridLines {
    #[default]
    None,
    Horizontal,
    Vertical,
    Both,
}

/// Whether column resize commits the new width on every drag tick (`Live`)
/// or only on `Ended` (`OnRelease`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColumnResizePolicy {
    #[default]
    Live,
    OnRelease,
}

/// Triggers that cause the table to fire `on_cell_edit_request` on the
/// focused cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditTrigger {
    /// All three triggers active. **Default.**
    #[default]
    F2OrTypeOrDoubleClick,
    F2,
    F2OrType,
    DoubleClick,
    /// Editing disabled — the table does not fire `on_cell_edit_request`.
    None,
}

/// Tab / Shift-Tab traversal policy across cells of a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabTraversal {
    /// Tab moves to the next cell within the row, then wraps to the first
    /// cell of the next row. **Default.**
    #[default]
    CellsThenRows,
    /// Tab leaves the table once the focused cell is reached at the row
    /// boundary; the focus owner is whatever follows the table in tab
    /// order.
    OutOfTable,
}

/// Per-cell context handed to a column's cell delegate during build.
#[derive(Debug, Clone)]
pub struct CellContext {
    /// Row index in the visible-list space (post sort/filter).
    pub row_index: usize,
    /// Column id (slug supplied at construction).
    pub col_id: String,
    /// Visible-column index, starting at 0 in display order.
    pub col_index: usize,
    /// Whether this row (or this specific cell, in cell-selection mode) is
    /// part of the current selection.
    pub is_selected: bool,
    /// Whether this cell currently carries the keyboard focus.
    pub is_focused: bool,
    /// Whether the pointer is hovering this cell.
    pub is_hovered: bool,
    /// Whether `editing_cell_signal` matches this cell.
    pub is_editing: bool,
    /// `TreeTable` only — depth of the row in the hierarchy. `None` for
    /// flat tables.
    pub depth: Option<usize>,
    /// `TreeTable` only — true on the column hosting the twist arrow.
    pub is_tree_column: bool,
}

/// Per-column-header context handed to a column's header delegate.
#[derive(Debug, Clone)]
pub struct ColumnContext {
    pub col_id: String,
    pub col_index: usize,
    /// Active sort direction if this column is the current sort column.
    pub sort: Option<SortDirection>,
    /// Current filter text (empty = no filter).
    pub filter_text: String,
    pub is_hovered: bool,
}

/// Single column declaration. Column ids must be **stable, unique strings**
/// — they're the persistence key for sort, filter, width, and ordering.
pub struct Column<T: 'static> {
    pub(crate) id: String,
    pub(crate) header_label: LocalizedString,
    pub(crate) width: ColumnWidth,
    pub(crate) min_width: Option<f32>,
    pub(crate) max_width: Option<f32>,
    pub(crate) alignment: Alignment,
    pub(crate) resizable: bool,
    pub(crate) reorderable: bool,
    pub(crate) sortable: bool,
    pub(crate) filterable: bool,
    pub(crate) editable: bool,
    pub(crate) pinned: PinnedSide,
    pub(crate) truncation: TruncationPolicy,
    pub(crate) cell: Rc<dyn Fn(&T, &CellContext) -> Box<dyn Widget>>,
    pub(crate) header_override: Option<Rc<dyn Fn(&ColumnContext) -> Box<dyn Widget>>>,
}

impl<T: 'static> Column<T> {
    /// Create a column with a stable id, a localized header label, and a
    /// cell builder that takes `&T` plus a [`CellContext`] and returns a
    /// boxed widget.
    pub fn new(
        id: impl Into<String>,
        header: impl Into<LocalizedString>,
        cell: impl Fn(&T, &CellContext) -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            header_label: header.into(),
            width: ColumnWidth::default(),
            min_width: None,
            max_width: None,
            alignment: Alignment::default(),
            resizable: true,
            reorderable: true,
            sortable: false,
            filterable: false,
            editable: false,
            pinned: PinnedSide::None,
            truncation: TruncationPolicy::default(),
            cell: Rc::new(cell),
            header_override: None,
        }
    }

    pub fn width(mut self, w: ColumnWidth) -> Self {
        self.width = w;
        self
    }

    pub fn min_width(mut self, px: f32) -> Self {
        self.min_width = Some(px);
        self
    }

    pub fn max_width(mut self, px: f32) -> Self {
        self.max_width = Some(px);
        self
    }

    pub fn alignment(mut self, a: Alignment) -> Self {
        self.alignment = a;
        self
    }

    pub fn resizable(mut self, b: bool) -> Self {
        self.resizable = b;
        self
    }

    pub fn reorderable(mut self, b: bool) -> Self {
        self.reorderable = b;
        self
    }

    pub fn sortable(mut self, b: bool) -> Self {
        self.sortable = b;
        self
    }

    pub fn filterable(mut self, b: bool) -> Self {
        self.filterable = b;
        self
    }

    /// Mark the column as editable. Default `false`. F2 / type-to-edit
    /// only enter edit mode on cells of editable columns; the
    /// `on_cell_edit_request` hook also fires only for these. Cells of
    /// non-editable columns continue to render their static delegate
    /// regardless of `editing_cell`.
    pub fn editable(mut self, b: bool) -> Self {
        self.editable = b;
        self
    }

    pub fn pinned(mut self, side: PinnedSide) -> Self {
        self.pinned = side;
        self
    }

    pub fn truncation(mut self, p: TruncationPolicy) -> Self {
        self.truncation = p;
        self
    }

    /// Override the default header rendering (label + sort/filter
    /// indicators). The closure receives a [`ColumnContext`] reflecting
    /// the current sort/filter state.
    pub fn header_override(
        mut self,
        f: impl Fn(&ColumnContext) -> Box<dyn Widget> + 'static,
    ) -> Self {
        self.header_override = Some(Rc::new(f));
        self
    }

    /// Stable column id (the persistence key for sort, filter, width,
    /// and ordering signals).
    pub fn id(&self) -> &str {
        &self.id
    }
}

impl<T: 'static> Clone for Column<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            header_label: self.header_label.clone(),
            width: self.width,
            min_width: self.min_width,
            max_width: self.max_width,
            alignment: self.alignment,
            resizable: self.resizable,
            reorderable: self.reorderable,
            sortable: self.sortable,
            filterable: self.filterable,
            editable: self.editable,
            pinned: self.pinned,
            truncation: self.truncation,
            cell: self.cell.clone(),
            header_override: self.header_override.clone(),
        }
    }
}

impl<T: 'static> std::fmt::Debug for Column<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Column")
            .field("id", &self.id)
            .field("width", &self.width)
            .field("alignment", &self.alignment)
            .field("pinned", &self.pinned)
            .finish()
    }
}
