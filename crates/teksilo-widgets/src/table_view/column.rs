// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Column descriptor + supporting enums for `TableView` and `TreeTableView`.
//!
//! Columns are declared once per table; the table consumes a `Vec<Column<T>>`
//! and shares it with its body subtree. The cell delegate is `Rc`-erased so a
//! `Column<T>` is cheap to clone for any internal pane that needs its own
//! copy.

use std::rc::Rc;

use teksilo_core::widget::Widget;
use teksilo_data::SortDirection;
use teksilo_i18n::LocalizedString;

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

/// Which gestures open a cell editor — a **set**, composed with `|`, after
/// Qt's `QAbstractItemView::EditTriggers`.
///
/// A set rather than an enum of named combinations, because the combinations
/// are the caller's to choose: "one click" and "F2 or one click" are ordinary
/// requests that a closed enum of `F2 / F2OrType / F2OrTypeOrDoubleClick /
/// DoubleClick / None` could not express at all.
///
/// Set table-wide with [`TableView::edit_triggers`](crate::TableView::edit_triggers)
/// / [`TreeTableView::edit_triggers`](crate::TreeTableView::edit_triggers), and
/// per column with [`Column::edit_triggers`] — the column wins where it sets
/// one. Only cells of an [`editable`](Column::editable) column ever open an
/// editor, whatever the triggers say; the two are the same split Qt makes
/// between a view's `editTriggers` and an item's `ItemIsEditable`.
///
/// **`SINGLE_CLICK` claims the press.** A cell that edits on one click does not
/// also select its row — the same trade any interactive cell content already
/// makes, and the reason it is per column: put it on the columns that are
/// nothing but a value, and leave the row's own column alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditTriggers(u8);

impl EditTriggers {
    /// Editing is never opened by the view. Cells of an editable column still
    /// render normally; nothing reaches `on_cell_edit_request`.
    pub const NONE: Self = Self(0);
    /// **F2** on the focused cell.
    pub const F2: Self = Self(1 << 0);
    /// Any printable character typed on the focused cell. Note that the
    /// keystroke that opens the editor is **not** delivered into it — the
    /// editor does not exist until the next build — so this reads as "F2 with
    /// an extra key", and it shadows type-ahead on every editable column.
    pub const ANY_KEY: Self = Self(1 << 1);
    /// A single click on the cell. Claims the press, so that cell no longer
    /// selects its row.
    pub const SINGLE_CLICK: Self = Self(1 << 2);
    /// A double click on the cell. It takes the gesture from row activation on
    /// **this column** — a column that edits on double-click must not also open
    /// its row on the same click — while every other column still activates.
    pub const DOUBLE_CLICK: Self = Self(1 << 3);
    /// Every trigger at once.
    pub const ALL: Self = Self(0b0000_1111);

    /// `true` when every trigger in `other` is present.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// `true` when nothing opens an editor.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}

impl Default for EditTriggers {
    /// `F2 | ANY_KEY | DOUBLE_CLICK` — what the old
    /// `EditTriggers::F2OrTypeOrDoubleClick` default named. (It only ever
    /// delivered the first two: the click arm had no implementation anywhere.)
    fn default() -> Self {
        Self::F2.union(Self::ANY_KEY).union(Self::DOUBLE_CLICK)
    }
}

impl std::ops::BitOr for EditTriggers {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl std::ops::BitOrAssign for EditTriggers {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

impl std::ops::BitAnd for EditTriggers {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        self.intersection(rhs)
    }
}

/// Tab / Shift-Tab traversal policy across cells of a row.
///
/// Regardless of the policy, **Ctrl+Tab / Ctrl+Shift+Tab always move focus
/// out of the table** to the next / previous focusable widget — the reliable
/// escape from `CellsThenRows`, so keyboard focus is never trapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabTraversal {
    /// Tab moves to the next cell within the row, then wraps to the first
    /// cell of the next row. **Default.** (Ctrl+Tab leaves the table.)
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
    /// `TreeTableView` only — depth of the row in the hierarchy. `None` for
    /// flat tables.
    pub depth: Option<usize>,
    /// `TreeTableView` only — true on the column hosting the twist arrow.
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
    /// Per-column override of the view's [`EditTriggers`]; `None` inherits.
    pub(crate) edit_triggers: Option<EditTriggers>,
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
            edit_triggers: None,
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

    /// Override the view's [`EditTriggers`] for this column alone.
    ///
    /// The reason the set is not only table-wide: a table's columns rarely
    /// want the same gesture. A tree column has to keep click-to-select and
    /// double-click-to-open, while the plain value columns beside it are
    /// exactly where one click to edit belongs. Unset columns inherit the
    /// view's set.
    pub fn edit_triggers(mut self, triggers: EditTriggers) -> Self {
        self.edit_triggers = Some(triggers);
        self
    }

    /// The triggers in force for this column, given the view's set — the
    /// question the body pane and the key handler both ask, and the one an
    /// application's own tests want to ask about their column set.
    ///
    /// A non-editable column never opens an editor, whatever either says.
    pub fn effective_edit_triggers(&self, view: EditTriggers) -> EditTriggers {
        if !self.editable {
            return EditTriggers::NONE;
        }
        self.edit_triggers.unwrap_or(view)
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
            edit_triggers: self.edit_triggers,
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
