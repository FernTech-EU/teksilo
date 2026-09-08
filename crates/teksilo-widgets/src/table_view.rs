// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TableView<T>` — generic, virtualized, accessible tabular widget.
//!
//! Built atop the [`ListModel<T>`](teksilo_data::ListModel) /
//! [`ListDataSource`] data layer in
//! `teksilo-data` and the `teksilo-tokens` `TableStyle`. Mirrors Qt's
//! `QTableView`, SwiftUI's `Table`, and JavaFX's `TableView`.
//! The core skeleton: single body pane, row-virtualized with alternating
//! backgrounds, grid lines, `Role::Table > Role::Row > Role::Cell`
//! accessibility, multi-row selection, and an empty-state slot. Headers,
//! sort, filter, resize, reorder, pinning, cell selection, and editing are
//! also included. Row heights come in three modes: uniform (`row_height`,
//! the default fast path), exact per-row callback (`row_height_fn`), and
//! auto-measured (`auto_row_height` — rows grow to their tallest cell,
//! height-for-width). See docs/table-view.md "Row heights".
//!
//! ```ignore
//! use teksilo_data::ListModel;
//! use teksilo_widgets::table_view::{Column, ColumnWidth, TableView};
//! use teksilo_i18n::lit;
//!
//! struct Person { name: String, age: u32 }
//!
//! let model: ListModel<Person> = ListModel::new();
//! let _table = TableView::new(model)
//!     .add_column(Column::new("name", ColumnWidth::Flex(1.0))
//!         .label(lit!("Name"))
//!         .cell(|p: &Person, _cx| Box::new(
//!             teksilo_widgets::primitives::TextWidget::new(
//!                 teksilo_i18n::lit!(p.name.clone())
//!             )
//!         )))
//!     .add_column(Column::new("age", ColumnWidth::Fixed(60.0))
//!         .label(lit!("Age"))
//!         .cell(|p: &Person, _cx| Box::new(
//!             teksilo_widgets::primitives::TextWidget::new(
//!                 teksilo_i18n::lit!(p.age.to_string())
//!             )
//!         )))
//!     .alternating_rows(true)
//!     .row_height(32.0);
//! ```
//!
//! ## Pan to scroll
//!
//! The view installs [`common::scrollable::ScrollableBehavior`](crate::common::scrollable::ScrollableBehavior),
//! which gives it the shared wheel arithmetic, a finger's pan and the
//! `PanClaim` that puts it on a pan's claimant chain. A pan scrolls it, the
//! release coasts, and a pan it cannot absorb hands the **whole** event to the
//! container outside — never a residual. A pan that starts on a row scrolls
//! rather than activating it or collapsing a multi-selection onto it. Both axes
//! are claimed. Shift+wheel still
//! scrolls the columns, and a finger's pan is never remapped by a held Shift:
//! the remap is a wheel convention, and turning a drag sideways is not what
//! the hand asked for.

pub mod a11y;
pub mod body;
pub mod body_pane;
pub mod column;
pub mod filter;
pub mod header;
pub mod imperative;
pub mod keyboard;
pub mod layout;
pub mod row_navigator;
pub mod selection;
mod widget_impl;

#[cfg(test)]
mod tests;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};

use teksilo_core::ObserverHandle;
use teksilo_core::accessibility::{AccessNodeBuilder, widget_id_to_node_id};
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::kinetic::KineticScroller;
use teksilo_core::pointer::touch_action::PanAxes;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget::{LayoutContext, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_data::{
    DataChange, DropPosition, DropResponse, ItemKey, KeyedSelectionModel, ListDataSource,
    ListModel, SelectionModel,
};
use teksilo_i18n::LocalizedString;
use teksilo_tokens::{BorderRole, OverscrollStyle, SurfaceRole};

use crate::styles::recipe_table_style as cp;

use crate::common::row_metrics::{HeightSource, RowMetrics, SharedRowMetrics};
use crate::common::scroll::OverscrollBehavior;
use crate::data_views::{
    DragTransferMode, RowDragData, RowSelection, ViewId, ViewKind, flat_insertion_target,
};
use crate::list_source::DndLazy;
use crate::scroll_area::ScrollBarMode;
use crate::scroll_bar::{ScrollBar, ScrollBarOrientation, ScrollBarVisual};

pub use self::column::{
    Alignment, CellContext, Column, ColumnContext, ColumnResizePolicy, ColumnWidth, EditTriggers,
    GridLines, PinnedSide, TabTraversal, TruncationPolicy,
};
pub use self::selection::{CellSelectionModel, TableSelectionMode};
pub use teksilo_data::SortDirection;

const BUFFER_ROWS: usize = 5;
const SCROLLBAR_THICKNESS: f32 = 12.0;

/// Pane partition produced by [`TableView::display_order`].
///
/// `leading_count` columns sit in the leading-pinned region, the next
/// `middle_end - leading_count` columns sit in the middle (scrollable
/// in future phases) region, and the remainder are trailing-pinned.
/// All counts are positions inside the display-order vector.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PaneBoundaries {
    pub leading_count: usize,
    pub middle_end: usize,
}

impl PaneBoundaries {
    pub(crate) fn new(leading_count: usize, middle_end: usize) -> Self {
        Self {
            leading_count,
            middle_end,
        }
    }
}

/// Drag payload for column reorder. Carried via `DragPayload::typed`.
#[derive(Debug, Clone)]
pub(crate) struct ColumnReorderDragData {
    pub col_id: String,
    /// Stable id of the source TableView, so dropping into a sibling
    /// table is rejected by the on_drop matcher.
    pub source_table_id: usize,
}

// ── Source erasure ─────────────────────────────────────────────────────────

type LenFn = Rc<dyn Fn() -> usize>;
type WithItemFn<T> = Rc<dyn Fn(usize, &dyn Fn(&T))>;
type ObserveFn = Rc<dyn Fn(Box<dyn Fn(&DataChange)>) -> ObserverHandle>;
/// Divergence side-channel for `DataChange::Reset`-emitting proxies
/// (`ListDataSource::first_changed_index`). Raw `ListModel`s report
/// `None` — their observers already get fine-grained variants.
type FirstChangedFn = Rc<dyn Fn() -> Option<usize>>;

/// The multi-cell read erasure. `TableView` reads each row's item once
/// per cell (each column's `cell` delegate), so it keeps the side-effect
/// `with_item_fn` form rather than `ListSource`'s single-widget reader.
/// The DnD + lazy protocol is shared from `DndLazy` (built separately in
/// the constructors). Returned alongside the `Rc<S>` source so the caller
/// can build a `DndLazy` from the same handle without re-wrapping.
fn erase_list_model<T: 'static>(
    model: ListModel<T>,
) -> (LenFn, WithItemFn<T>, ObserveFn, FirstChangedFn) {
    let m_len = model.clone();
    let m_read = model.clone();
    let m_obs = model;
    let len_fn: LenFn = Rc::new(move || m_len.len());
    let with_item_fn: WithItemFn<T> = Rc::new(move |idx, f| {
        m_read.with_item(idx, |item| f(item));
    });
    let observe_fn: ObserveFn =
        Rc::new(move |callback| m_obs.observe_changes(move |change| callback(change)));
    (len_fn, with_item_fn, observe_fn, Rc::new(|| None))
}

fn erase_data_source<S: ListDataSource<Item = T>, T: 'static>(
    s: Rc<S>,
) -> (LenFn, WithItemFn<T>, ObserveFn, FirstChangedFn) {
    let s_len = s.clone();
    let s_read = s.clone();
    let s_obs = s.clone();
    let s_changed = s;
    let len_fn: LenFn = Rc::new(move || s_len.len());
    let with_item_fn: WithItemFn<T> = Rc::new(move |idx, f| {
        s_read.with_item(idx, |item| f(item));
    });
    let observe_fn: ObserveFn =
        Rc::new(move |callback| s_obs.observe_changes(move |change| callback(change)));
    let first_changed_fn: FirstChangedFn = Rc::new(move || s_changed.first_changed_index());
    (len_fn, with_item_fn, observe_fn, first_changed_fn)
}

// `read_item` lived here for the inline body-row build; that loop now
// lives in `BodyPane` which has its own copy. Keeping it removed
// avoids dead-code drift between the two paths.

// ── Public widget ──────────────────────────────────────────────────────────

/// Generic, virtualized, accessible table with sortable / filterable / resizable columns.
///
/// Construct with [`TableView::new`] (from a [`ListModel<T>`](teksilo_data::ListModel))
/// or [`TableView::from_source`] (any [`ListDataSource`]), then chain builder methods
/// to configure columns, row heights, selection, and so on. See module docs for the full
/// feature list and row-height modes.
pub struct TableView<T: 'static> {
    // Source erasure (multi-cell read path; DnD + lazy live in `dnd`).
    len_fn: LenFn,
    with_item_fn: WithItemFn<T>,
    observe_fn: ObserveFn,
    first_changed_fn: FirstChangedFn,
    /// Source-owned DnD validation + lazy windowing, erased from the
    /// backing `ListDataSource`. A `ListModel` reorders in place via its
    /// `accept_drop`; an external source routes the move to its store and
    /// can forbid a drop by returning `DropResponse::Reject` (the view
    /// then paints no insertion line).
    dnd: DndLazy,
    /// Resolve a row index to a movement-proof handle (see `RowAnchor`).
    anchor_fn: Rc<dyn Fn(usize) -> crate::data_views::RowAnchor>,
    /// Anchor for the row with an open cell editor, so the editor follows its
    /// row instead of its index. See `reconcile_editing_row`.
    editing_anchor: Rc<RefCell<Option<crate::data_views::RowAnchor>>>,

    // Configuration
    columns: Vec<Column<T>>,
    row_height: Option<f32>,
    /// Height-mode selection (uniform / exact callback / auto-measure).
    height_source: HeightSource,
    /// Row geometry — shared with `BodyPane` and the keyboard handler.
    row_metrics: SharedRowMetrics,
    header_height: Option<f32>,
    show_header: bool,
    selection_mode: TableSelectionMode,
    /// Row selection — index-based `SelectionModel` or keyed
    /// `KeyedSelectionModel<K>`, unified behind the index-facing facade.
    row_selection: Option<RowSelection>,
    cell_selection: Option<CellSelectionModel>,
    alternating_rows: bool,
    grid_lines: GridLines,
    /// See [`Self::stretch_last_column`].
    stretch_last_column: bool,
    a11y_label: Option<LocalizedString>,
    show_internal_scrollbars: bool,
    empty_view: Option<Rc<dyn Fn() -> Box<dyn Widget>>>,
    column_resize_policy: ColumnResizePolicy,

    /// Animate wheel scrolling instead of snapping to the new offset.
    /// Enabled by default — mirrors `ScrollArea`. Without it, each wheel
    /// notch jumps by `row_height` per delivered line (typically 3),
    /// which reads as a coarse multi-row jump rather than a smooth glide.
    smooth_scrolling: bool,
    /// Duration of the smooth scroll animation.
    smooth_scroll_duration: Duration,

    /// How the scroll bar is displayed. Defaults to `Permanent` — a
    /// layout sibling that reserves its own width. `Overlay` / `Thin`
    /// float over the content instead, like `ScrollArea`.
    scroll_bar_style: ScrollBarMode,

    // Public reactive signals
    scroll_y: Signal<f32>,
    max_scroll_y: Signal<f32>,
    /// Scroll-chaining behavior at the boundary (default `Chain`).
    overscroll_behavior: OverscrollBehavior,
    viewport_ratio_y: Signal<f32>,
    /// Horizontal scroll offset of the Middle (unpinned) pane — see
    /// `PaneBoundaries`. Leading/Trailing-pinned columns never move; the
    /// Middle pane's content shifts by `-scroll_x`.
    scroll_x: Signal<f32>,
    /// Maximum `scroll_x` — `middle_content_width − middle_viewport_width`.
    max_scroll_x: Signal<f32>,
    /// Middle-pane viewport-to-content width ratio, for the horizontal
    /// scroll bar's thumb.
    viewport_ratio_x: Signal<f32>,
    /// This surface's pan physics: the range a finger's pan is clamped to and
    /// the offset it is currently holding. Owned by the view rather than by
    /// the [`ScrollableBehavior`](crate::common::scrollable::ScrollableBehavior)
    /// so it survives a rebuild, and so `place_children` — the only pass that
    /// knows the viewport extent — can publish into it.
    scroller: Rc<RefCell<KineticScroller>>,
    sort_signal: Signal<Option<(String, SortDirection)>>,
    column_widths_signal: Signal<HashMap<String, f32>>,
    /// Column ids in display order. Empty means "use declaration order".
    column_order_signal: Signal<Vec<String>>,
    /// Per-id override for `Column::pinned`. Missing keys mean "use the
    /// declared pinning". The drag-to-reorder UI updates this when a
    /// column crosses a pane boundary.
    column_pinning_signal: Signal<HashMap<String, PinnedSide>>,
    /// Currently keyboard-focused cell `(row_index, display_col)`, or
    /// `None` when no cell is focused.
    focused_cell: Signal<Option<(usize, usize)>>,
    /// The realized `(row index -> row wrapper id)` map, filled by the body
    /// pane each build. Lets this widget's `&self` methods resolve a row index
    /// to a widget without reaching into the pane. Mirrors `ListView::row_map`.
    row_map: Rc<RefCell<Vec<(usize, WidgetId)>>>,
    /// Type-ahead ("type to jump") label extractor — opt-in via
    /// [`type_ahead_label`](Self::type_ahead_label).
    #[allow(clippy::type_complexity)]
    type_ahead_label: Option<Rc<dyn Fn(&T) -> String>>,
    /// Reset window for the type-ahead search term.
    type_ahead_timeout: Duration,
    /// Persistent type-ahead buffer (survives the per-keystroke rebuild).
    type_ahead: Rc<crate::common::type_ahead::TypeAheadState>,
    tab_traversal: TabTraversal,
    /// Cell currently in edit mode, or `None` when no editor is open.
    /// Cell delegates inspect this through `CellContext::is_editing` to
    /// swap in an editor widget.
    editing_cell: Signal<Option<(usize, usize)>>,
    edit_triggers: EditTriggers,
    /// User callback invoked when an edit trigger fires on the focused
    /// cell.
    #[allow(clippy::type_complexity)]
    on_cell_edit_request: Option<Rc<dyn Fn(usize, &str, &mut teksilo_core::widget::EventContext)>>,
    #[allow(clippy::type_complexity)]
    on_cell_edit_dismissed:
        Option<Rc<dyn Fn(usize, &str, &mut teksilo_core::widget::EventContext)>>,
    /// Per-column filter text. Updated by filter affordances in the
    /// header, by `set_filter` / `clear_filters`, and by
    /// downstream consumers binding it (e.g., `SortFilterListModel`).
    filters_signal: Signal<HashMap<String, String>>,
    /// User callback invoked on every row activation (Enter on the
    /// focused row).
    #[allow(clippy::type_complexity)]
    on_row_activate: Option<Rc<dyn Fn(usize, &mut teksilo_core::widget::EventContext)>>,
    reorderable: bool,
    /// Active row-drop insertion indicator `(body_local_y, width)` —
    /// `body_local_y` is measured from the body band top (below the
    /// header). Set by `on_drag_hover` when the source accepts the
    /// hovered position, cleared on leave / drop, read by `paint`.
    /// Reactive (`RepaintOnly`) so a `set(...)` dirties the table.
    drop_feedback: Signal<Option<(f32, f32)>>,

    /// Whether activation is a single or double click (default `DoubleClick`).
    activate_on: crate::data_views::ActivateOn,

    /// `true` while this view — its root or any descendant (e.g. a cell
    /// editor) — holds keyboard focus. Captured at build from
    /// [`BuildContext::view_focus_active`] and bound `RepaintOnly`. Drives
    /// **focus-aware selection**: the selection band paints with the active
    /// `Selected` chrome while focused and the muted `SelectedInactive` chrome
    /// once focus leaves the table — the standard desktop affordance.
    view_focused: Signal<bool>,
    /// Input-modality `:focus-visible` — `true` after keyboard input, `false`
    /// after a pointer press. Gates the cell focus ring so it shows only
    /// during keyboard navigation, never on a mouse click. Bound `RepaintOnly`.
    focus_visible: Signal<bool>,

    // Build state
    header_row_id: Option<WidgetId>,
    body_pane_id: Option<WidgetId>,
    scrollbar_id: Option<WidgetId>,
    /// Horizontal scroll bar along the bottom of the Middle pane only —
    /// built whenever `show_internal_scrollbars` is set, placed/sized (and
    /// hidden at zero size, mirroring the vertical bar) in `place_children`.
    h_scrollbar_id: Option<WidgetId>,
    empty_id: Option<WidgetId>,
    /// Pane-local rebuild trigger + buffered range, owned here so they
    /// survive `TableView` rebuilds (each rebuild constructs a fresh
    /// `BodyPane` struct that inherits these handles).
    pane_version: Signal<u64>,
    pane_built_start: Rc<Cell<usize>>,
    pane_built_end: Rc<Cell<usize>>,
    /// Bumped by the pane when a measure pass changes the content
    /// total; bound at `Relayout` on this root so `max_scroll_y` / the
    /// thumb ratio are recomputed with the corrected total next frame.
    pane_total_refresh: Signal<u64>,

    // Layout state
    /// Resolved widths in **display order** (parallel to
    /// `display_indices`).
    column_widths: Rc<RefCell<Vec<f32>>>,
    /// Display-order indices into `self.columns`. Recomputed each
    /// `build()`; read by `place_children` and `paint`.
    display_indices: Rc<RefCell<Vec<usize>>>,
    /// `(row, display_pos) -> WidgetId` for every cell realized by the
    /// body pane's latest `build()`. Shared with `BodyPane` (the GridView
    /// `tile_map` pattern — two holders across the sibling-of-scrollbar
    /// split): the pane overwrites it wholesale each time it rebuilds, so
    /// a cell that scrolled out of the realized buffer simply isn't in
    /// the map. `accessibility()` reads it to point `active_descendant`
    /// at the keyboard-focused cell's own AT node.
    cell_map: Rc<RefCell<Vec<((usize, usize), WidgetId)>>>,
    /// Counts of (leading-pinned, middle, trailing-pinned) columns —
    /// used by paint to draw pane dividers and by the drop-zone math
    /// to classify a drop position.
    pane_boundaries: Rc<RefCell<PaneBoundaries>>,
    viewport_height: Rc<Cell<f32>>,
    /// Middle-pane viewport width, snapshotted by `place_children` — the
    /// horizontal analogue of `viewport_height`. Read by the keyboard
    /// handler's ensure-column-visible follow.
    middle_viewport_width: Rc<Cell<f32>>,
    /// Set on the first `place_children`. Until then `viewport_height` still
    /// holds its construction placeholder, so viewport-relative imperatives
    /// (`ensure_row_visible`) would scroll against a size that was never real.
    laid_out: Rc<Cell<bool>>,
    /// The row-area's absolute (window) rect (below the header), cached by
    /// `place_children`. Threaded into the keyboard handler so it can chase the
    /// focused row into any *enclosing* scroll area via
    /// [`EventContext::ensure_visible`](teksilo_core::widget::EventContext::ensure_visible).
    body_bounds: Rc<Cell<Rect>>,
    /// Width of the header strip (= the column band) snapshotted by
    /// `place_children`. The reorder-drop handler needs it to mirror the
    /// drop x under RTL, where the column content is right-anchored in
    /// the band (`local.x` is measured from the strip's physical left).
    header_strip_width: Rc<Cell<f32>>,

    // Header-cell shared state — tracked across the table so the
    // pointer-capture'd resize delivers PointerMove events back to the
    // active HeaderCell.
    resize_state: header::ResizeStateHandle,
    /// Display slot of the column under an active resize drag, or `None`.
    /// Shared with every `HeaderCell` so the *target* column shows the
    /// "resizing" chrome — which is not always the cell holding the pointer
    /// capture, since a grip straddles the divider between two cells.
    resize_target: Signal<Option<usize>>,
    /// Window x of the prospective divider while a
    /// [`ColumnResizePolicy::OnRelease`] drag is in flight. Painted as a
    /// guide line by `paint`; `None` at rest. Under `Live` the columns
    /// themselves move, so nothing is published here.
    resize_preview_x: Signal<Option<f32>>,

    /// Stable id used by the column-reorder drag payload to disambiguate
    /// inter-table drops. Unrelated to row DnD — a wholly separate
    /// mechanism (`ColumnReorderDragData` + header handlers).
    table_id: usize,

    /// Stable, kind-tagged ID for this TableView instance's **row** DnD
    /// (identifies its own row reorder vs. a foreign row drop, even across
    /// widget kinds / windows). Distinct from `table_id` above, which only
    /// disambiguates the separate column-reorder mechanism.
    model_id: ViewId,

    /// Cross-widget export / foreign-receive machinery — the builders
    /// (`.exportable`, `.export_external`, `.accept_foreign_rows`,
    /// `.on_rows_received`, `.on_rows_transferred_out`), the drag-start payload
    /// build, and the move-out completion, shared by all four data views.
    export: crate::data_views::RowExport<T>,

    /// Whole-view enabled state, statically or reactively. Forwarded to the
    /// arena via `ctx.enabled_when(self_id, self.enabled.clone())` at build
    /// time; a disabled view greys out and stops accepting focus /
    /// selection / keyboard input (arena-gated).
    enabled: Prop<bool>,
}

/// Build the anchor factory for a keyed source: capture the row's key now,
/// resolve its current index later. Keyless sources fall back to a fixed anchor.
fn anchor_factory<S: ListDataSource<Item = T> + 'static, T: 'static>(
    s: Rc<S>,
) -> Rc<dyn Fn(usize) -> crate::data_views::RowAnchor> {
    Rc::new(move |index| match s.key_at(index) {
        Some(key) => {
            let src = s.clone();
            crate::data_views::RowAnchor::new(Rc::new(move || {
                if src.key_at(index).as_ref() == Some(&key) {
                    return Some(index);
                }
                src.index_of(&key)
            }))
        }
        None => crate::data_views::RowAnchor::fixed(index),
    })
}

impl<T: 'static> TableView<T> {
    /// Wrap a `ListModel<T>`.
    pub fn new(model: ListModel<T>) -> Self {
        let dnd = DndLazy::from_source(Rc::new(model.clone()));
        let (len_fn, with_item_fn, observe_fn, first_changed_fn) = erase_list_model(model);
        // A bare `ListModel` exposes no row identity.
        let anchor_fn = Rc::new(crate::data_views::RowAnchor::fixed) as Rc<dyn Fn(usize) -> _>;
        Self::create(
            len_fn,
            with_item_fn,
            observe_fn,
            first_changed_fn,
            dnd,
            anchor_fn,
        )
    }

    /// Wrap any `ListDataSource<Item = T>` (e.g. a
    /// [`SortFilterListModel<T>`](teksilo_data::SortFilterListModel)).
    ///
    /// The source owns DnD validation (`can_accept` / `accept_drop`) and
    /// lazy windowing (`row_state` / `request_window` / `fetch_more`); a
    /// read-only source leaves the defaults inert.
    pub fn from_source<S: ListDataSource<Item = T>>(source: S) -> Self {
        let s = Rc::new(source);
        let dnd = DndLazy::from_source(s.clone());
        let anchor_fn = anchor_factory::<S, T>(s.clone());
        let (len_fn, with_item_fn, observe_fn, first_changed_fn) = erase_data_source::<S, T>(s);
        Self::create(
            len_fn,
            with_item_fn,
            observe_fn,
            first_changed_fn,
            dnd,
            anchor_fn,
        )
    }

    /// Wrap any `ListDataSource<Item = T>` with **keyed** row selection. The
    /// `KeyedSelectionModel<S::Key>` tracks selection by source identity, so it
    /// survives reorders / filters / lazy window-slides and stays consistent
    /// across two views of the same source. The view stays `TableView<T>` — the
    /// index↔key mapping is captured from the concrete source here. Equivalent
    /// to `from_source(..)` plus an identity-based replacement for
    /// [`selection`](Self::selection).
    pub fn from_source_keyed<S: ListDataSource<Item = T>>(
        source: S,
        keyed: KeyedSelectionModel<S::Key>,
    ) -> Self
    where
        S::Key: ItemKey,
    {
        let s = Rc::new(source);
        let dnd = DndLazy::from_source(s.clone());
        let key_at = {
            let s = s.clone();
            Rc::new(move |i| s.key_at(i)) as Rc<dyn Fn(usize) -> Option<S::Key>>
        };
        let len = {
            let s = s.clone();
            Rc::new(move || s.len()) as Rc<dyn Fn() -> usize>
        };
        let contains = {
            let s = s.clone();
            Rc::new(move |k: &S::Key| (0..s.len()).any(|i| s.key_at(i).as_ref() == Some(k)))
                as Rc<dyn Fn(&S::Key) -> bool>
        };
        let row_selection = RowSelection::from_keyed(keyed, key_at, len, contains);
        let anchor_fn = anchor_factory::<S, T>(s.clone());
        let (len_fn, with_item_fn, observe_fn, first_changed_fn) = erase_data_source::<S, T>(s);
        let mut view = Self::create(
            len_fn,
            with_item_fn,
            observe_fn,
            first_changed_fn,
            dnd,
            anchor_fn,
        );
        view.row_selection = Some(row_selection);
        view
    }

    fn create(
        len_fn: LenFn,
        with_item_fn: WithItemFn<T>,
        observe_fn: ObserveFn,
        first_changed_fn: FirstChangedFn,
        dnd: DndLazy,
        anchor_fn: Rc<dyn Fn(usize) -> crate::data_views::RowAnchor>,
    ) -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
        let table_id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        Self {
            len_fn,
            with_item_fn,
            observe_fn,
            first_changed_fn,
            dnd,
            anchor_fn,
            editing_anchor: Rc::new(RefCell::new(None)),
            columns: Vec::new(),
            row_height: None,
            height_source: HeightSource::Uniform,
            row_metrics: Rc::new(RefCell::new(RowMetrics::uniform(cp::ROW_HEIGHT, 0.0))),
            header_height: None,
            show_header: true,
            selection_mode: TableSelectionMode::default(),
            row_selection: None,
            cell_selection: None,
            alternating_rows: false,
            grid_lines: GridLines::None,
            stretch_last_column: false,
            a11y_label: None,
            show_internal_scrollbars: true,
            empty_view: None,
            column_resize_policy: ColumnResizePolicy::default(),
            smooth_scrolling: true,
            smooth_scroll_duration: Duration::from_millis(150),
            scroll_bar_style: ScrollBarMode::Permanent,
            overscroll_behavior: OverscrollBehavior::default(),
            scroll_y: Signal::new_animated(0.0),
            max_scroll_y: Signal::new(0.0),
            viewport_ratio_y: Signal::new(1.0),
            scroll_x: Signal::new_animated(0.0),
            max_scroll_x: Signal::new(0.0),
            viewport_ratio_x: Signal::new(1.0),
            scroller: Rc::new(RefCell::new(KineticScroller::new(OverscrollStyle::Clamp))),
            sort_signal: Signal::new(None),
            column_widths_signal: Signal::new(HashMap::new()),
            column_order_signal: Signal::new(Vec::new()),
            column_pinning_signal: Signal::new(HashMap::new()),
            focused_cell: Signal::new(None),
            row_map: Rc::new(RefCell::new(Vec::new())),
            type_ahead_label: None,
            type_ahead_timeout: crate::common::type_ahead::DEFAULT_TYPE_AHEAD_TIMEOUT,
            type_ahead: crate::common::type_ahead::TypeAheadState::new(),
            // Replaced at build with the live tree signals; the defaults are
            // only the pre-build values (treat as focused, pointer modality).
            view_focused: Signal::new(true),
            focus_visible: Signal::new(false),
            tab_traversal: TabTraversal::default(),
            editing_cell: Signal::new(None),
            edit_triggers: EditTriggers::default(),
            on_cell_edit_request: None,
            on_cell_edit_dismissed: None,
            filters_signal: Signal::new(HashMap::new()),
            on_row_activate: None,
            reorderable: false,
            drop_feedback: Signal::new(None),
            activate_on: crate::data_views::ActivateOn::default(),
            header_row_id: None,
            body_pane_id: None,
            scrollbar_id: None,
            h_scrollbar_id: None,
            empty_id: None,
            pane_version: Signal::new(0_u64),
            pane_built_start: Rc::new(Cell::new(0)),
            pane_built_end: Rc::new(Cell::new(0)),
            pane_total_refresh: Signal::new(0_u64),
            column_widths: Rc::new(RefCell::new(Vec::new())),
            display_indices: Rc::new(RefCell::new(Vec::new())),
            cell_map: Rc::new(RefCell::new(Vec::new())),
            pane_boundaries: Rc::new(RefCell::new(PaneBoundaries::default())),
            viewport_height: Rc::new(Cell::new(600.0)),
            middle_viewport_width: Rc::new(Cell::new(600.0)),
            laid_out: Rc::new(Cell::new(false)),
            body_bounds: Rc::new(Cell::new(Rect::ZERO)),
            header_strip_width: Rc::new(Cell::new(0.0)),
            resize_state: Rc::new(std::cell::RefCell::new(None)),
            resize_target: Signal::new(None),
            resize_preview_x: Signal::new(None),
            table_id,
            model_id: ViewId::next(ViewKind::Table),
            export: crate::data_views::RowExport::default(),
            enabled: Prop::Static(true),
        }
    }

    // ── Builder ────────────────────────────────────────────────────────

    /// Enable or disable the whole view. A disabled view greys out and stops
    /// accepting focus / selection / keyboard input (arena-gated).
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }

    /// Set the scroll-chaining behavior at the boundary (default
    /// [`OverscrollBehavior::Chain`]; [`Contain`](OverscrollBehavior::Contain)
    /// disables chaining to an ancestor scrollable).
    pub fn overscroll_behavior(mut self, behavior: OverscrollBehavior) -> Self {
        self.overscroll_behavior = behavior;
        self
    }

    /// Enable or disable animated wheel scrolling (enabled by default).
    /// When disabled, wheel events snap immediately to the new offset.
    pub fn smooth_scrolling(mut self, enabled: bool) -> Self {
        self.smooth_scrolling = enabled;
        self
    }

    /// Enable **type-ahead** ("type to jump"): typing a printable character
    /// while the table has keyboard focus jumps the focused row to the next
    /// row whose label starts with the accumulated search term, wrapping
    /// around (Qt `keyboardSearch` / macOS & Windows type-select).
    /// `label(&item)` yields the searchable text for a row; matching is
    /// ASCII-case-insensitive. A pause longer than the
    /// [`type_ahead_timeout`](Self::type_ahead_timeout) starts a fresh term.
    ///
    /// On an editable column whose [`EditTriggers`] is type-to-edit, typing
    /// starts an edit instead — type-ahead applies on non-editable columns
    /// (or when no type-to-edit trigger is configured).
    pub fn type_ahead_label(mut self, label: impl Fn(&T) -> String + 'static) -> Self {
        self.type_ahead_label = Some(Rc::new(label));
        self
    }

    /// Reset window between keystrokes before the type-ahead search term
    /// clears (default 500 ms). A zero duration disables type-ahead.
    pub fn type_ahead_timeout(mut self, timeout: Duration) -> Self {
        self.type_ahead_timeout = timeout;
        self
    }

    /// Duration of the smooth scroll animation (default 150 ms).
    pub fn smooth_scroll_duration(mut self, duration: Duration) -> Self {
        self.smooth_scroll_duration = duration;
        self
    }

    /// How the scroll bar is displayed (default `Permanent`). `Overlay`
    /// and `Thin` float the bar over the content instead of reserving a
    /// layout column for it, mirroring `ScrollArea::scroll_bar_style`.
    pub fn scroll_bar_style(mut self, style: ScrollBarMode) -> Self {
        self.scroll_bar_style = style;
        self
    }

    /// Append a single [`Column<T>`] definition to the table.
    pub fn add_column(mut self, col: Column<T>) -> Self {
        self.columns.push(col);
        self
    }

    /// Append multiple [`Column<T>`] definitions from an iterator.
    pub fn columns(mut self, cols: impl IntoIterator<Item = Column<T>>) -> Self {
        self.columns.extend(cols);
        self
    }

    /// Re-materialize `self.row_metrics` after a height-mode /
    /// row-height builder call.
    fn remake_metrics(&self) {
        *self.row_metrics.borrow_mut() = self
            .height_source
            .make_metrics(self.effective_row_height(), 0.0);
    }

    /// Fixed row height (default: the table style's 28 px) — the
    /// uniform fast path. Mutually exclusive with
    /// [`row_height_fn`](Self::row_height_fn) and
    /// [`auto_row_height`](Self::auto_row_height); the last mode setter
    /// wins.
    pub fn row_height(mut self, height: f32) -> Self {
        self.row_height = Some(height);
        self.height_source = HeightSource::Uniform;
        self.remake_metrics();
        self
    }

    /// Per-row heights from a callback over the visible row index. The
    /// callback must be pure (same index + same data → same height); it
    /// is re-swept from the first changed index on every model change
    /// (a `SortFilterListModel` source reports that index through
    /// `first_changed_index`, so sort/filter/append keep the valid
    /// prefix). No measurement pass runs.
    pub fn row_height_fn(mut self, f: impl Fn(usize) -> f32 + 'static) -> Self {
        self.height_source = HeightSource::Exact(Rc::new(f));
        self.remake_metrics();
        self
    }

    /// Auto-measured row heights: each realized row reports the height
    /// of its tallest cell measured at the cell's column width
    /// (height-for-width), unrealized rows assume `estimated`. Scroll
    /// anchoring keeps content above the viewport stationary as
    /// estimates are corrected; the scrollbar settles one frame after a
    /// measurement change.
    pub fn auto_row_height(mut self, estimated: f32) -> Self {
        self.height_source = HeightSource::Auto { estimated };
        self.remake_metrics();
        self
    }

    /// Override the column header row height in logical pixels. Default: the table style's `HEADER_HEIGHT`.
    pub fn header_height(mut self, height: f32) -> Self {
        self.header_height = Some(height);
        self
    }

    /// Show or hide the column header row. Default: visible.
    pub fn show_header(mut self, visible: bool) -> Self {
        self.show_header = visible;
        self
    }

    /// Set how column widths are redistributed when columns are
    /// added, resized, or the table's own width changes. See
    /// [`ColumnResizePolicy`].
    pub fn column_resize_policy(mut self, policy: ColumnResizePolicy) -> Self {
        self.column_resize_policy = policy;
        self
    }

    /// Control how Tab / Shift+Tab navigate between cells. See
    /// [`TabTraversal`].
    pub fn tab_traversal(mut self, mode: TabTraversal) -> Self {
        self.tab_traversal = mode;
        self
    }

    /// Set which user action opens a cell editor. See [`EditTriggers`].
    pub fn edit_triggers(mut self, trigger: EditTriggers) -> Self {
        self.edit_triggers = trigger;
        self
    }

    /// Hook fired by the keyboard handler when an edit trigger fires
    /// on the focused cell. Receives `(row_index, col_id, ctx)`.
    pub fn on_cell_edit_request(
        mut self,
        f: impl Fn(usize, &str, &mut teksilo_core::widget::EventContext) + 'static,
    ) -> Self {
        self.on_cell_edit_request = Some(Rc::new(f));
        self
    }

    /// Callback invoked when an **open** cell editor should end because the
    /// pointer went somewhere else: a press that lands on any cell other than
    /// the one being edited. Receives the editing cell's flat row index and
    /// column id, so the owner can commit (or discard) whatever is in its
    /// buffer, then clear its own editing state.
    ///
    /// The counterpart of [`on_cell_edit_request`](Self::on_cell_edit_request),
    /// and the view cannot do it alone: the framework owns *which* cell is being
    /// edited, but only the owner knows what an ended edit means — commit,
    /// discard, or refuse a value that will not parse.
    ///
    /// **Why a press and not a focus change.** "The editor lost focus" is the
    /// obvious signal and it cannot be used: a body pane rebuilds constantly —
    /// selection, filtering, scroll, a reload from elsewhere — and every rebuild
    /// destroys and re-creates the open editor, so focus leaves it many times
    /// during an edit the writer never interrupted. A press on another cell is
    /// unambiguous and happens exactly once.
    pub fn on_cell_edit_dismissed(
        mut self,
        f: impl Fn(usize, &str, &mut teksilo_core::widget::EventContext) + 'static,
    ) -> Self {
        self.on_cell_edit_dismissed = Some(Rc::new(f));
        self
    }

    /// Callback invoked when a row is activated: a click or a double click per
    /// [`activate_on`](Self::activate_on), or Enter on the focused row.
    /// Receives the flat row index.
    ///
    /// The pointer half is a **gesture**, so it arbitrates against a pan and
    /// against the reorder drag through the gesture arena: a click activates,
    /// a drag does not.
    pub fn on_row_activate(
        mut self,
        f: impl Fn(usize, &mut teksilo_core::widget::EventContext) + 'static,
    ) -> Self {
        self.on_row_activate = Some(Rc::new(f));
        self
    }

    /// Enable drag-to-reorder of **rows** (pointer drag + keyboard
    /// Alt+ArrowUp/Down). Distinct from
    /// [`Column::reorderable`](crate::Column::reorderable), which reorders
    /// *columns* and defaults to `true`; this defaults to `false`.
    ///
    /// The move is routed through the backing source's `accept_drop`: a
    /// `ListModel` reorders in place, an external source routes the move to
    /// its store. Per-hover the source's `can_accept` decides whether the
    /// drop is allowed — a forbidden position shows no insertion line and
    /// the drop is refused. A row may also be forbidden from dragging at
    /// all (the source's `drag` gate). Cross-table / external drops arrive
    /// at `accept_drop` as `DragSource::Foreign`; a bare `ListModel`
    /// rejects them, an external source decides.
    pub fn reorderable(mut self, enabled: bool) -> Self {
        self.reorderable = enabled;
        self
    }

    /// Renamed to [`reorderable`](Self::reorderable), matching `ListView`,
    /// `GridView`, `TreeView` and `TreeTableView` — this was the only view in
    /// the family spelling it differently.
    #[deprecated(since = "0.6.3", note = "renamed to `reorderable`")]
    pub fn reorderable_rows(self, enabled: bool) -> Self {
        self.reorderable(enabled)
    }

    /// Make rows **droppable outside this view** — on a
    /// [`DropTarget`](crate::DropTarget), another data view, or the OS.
    ///
    /// A dragged row (or the whole selection, when the pressed row is part of a
    /// multi-selection) carries clones of its items in a public
    /// [`RowDragData<T>`](crate::RowDragData), so a foreign receiver can pull
    /// them out with `payload.get_typed::<RowDragData<T>>()` /
    /// `DropTarget::on_drop_typed::<RowDragData<T>>()` — no serialization. This
    /// also makes rows a drag source even without [`reorderable`](Self::reorderable).
    ///
    /// `mode` chooses what happens to the origin rows once a *foreign* target
    /// accepts them: [`DragTransferMode::Move`] removes them (via the source's
    /// `on_drag_out`, or [`on_rows_transferred_out`](Self::on_rows_transferred_out)),
    /// [`DragTransferMode::Copy`] leaves them. A same-view reorder is never a
    /// transfer, so `mode` never affects it. Requires `T: Clone`.
    pub fn exportable(mut self, mode: DragTransferMode) -> Self
    where
        T: Clone,
    {
        self.export.set_exportable(mode);
        self
    }

    /// Additionally advertise the dragged rows as MIME data so they can be
    /// dropped on a [`DropZone`](crate::DropZone) or exported to another
    /// application / window via the OS. `f` maps the dragged items to
    /// `(mime_type, bytes)` pairs (e.g. `text/plain`, `text/uri-list`, an
    /// app-specific `application/x-…`). Implies [`exportable`](Self::exportable)
    /// (defaulting to [`DragTransferMode::Move`] if not already set). Requires
    /// `T: Clone`.
    pub fn export_external(mut self, f: impl Fn(&[T]) -> Vec<(String, Vec<u8>)> + 'static) -> Self
    where
        T: Clone,
    {
        self.export.set_export_external(f);
        self
    }

    /// Override how rows moved out to a foreign target are removed from this
    /// view. Receives the dragged rows' indices (descending-safe) and the live
    /// context. Without this, an [`exportable`](Self::exportable)
    /// [`Move`](DragTransferMode::Move) drag removes them through the source's
    /// `on_drag_out` (works out of the box for a `ListModel`).
    pub fn on_rows_transferred_out(
        mut self,
        f: impl Fn(&[usize], &mut teksilo_core::widget::EventContext) + 'static,
    ) -> Self {
        self.export.set_on_rows_transferred_out(f);
        self
    }

    /// Accept exported rows dropped from a **different** view or source without
    /// writing a custom `ListDataSource`. Pair with
    /// [`on_rows_received`](Self::on_rows_received), which is handed the dropped
    /// items and the insertion index. (Same-view reorder is
    /// [`reorderable`](Self::reorderable); a custom `ListDataSource` can still
    /// accept foreign drops through its `can_accept`/`accept_drop` instead.)
    pub fn accept_foreign_rows(mut self, accept: bool) -> Self {
        self.export.accept_foreign_rows = accept;
        self
    }

    /// Handler for rows accepted via [`accept_foreign_rows`](Self::accept_foreign_rows):
    /// `(items, insertion_index, ctx)`. Insert them into your model at the
    /// index.
    pub fn on_rows_received(
        mut self,
        f: impl Fn(Vec<T>, usize, &mut teksilo_core::widget::EventContext) + 'static,
    ) -> Self {
        self.export.set_on_rows_received(f);
        self
    }

    /// Choose single- vs double-click activation for `on_row_activate` (default
    /// [`ActivateOn::DoubleClick`](crate::ActivateOn)). Enter/Space activates in
    /// either mode.
    pub fn activate_on(mut self, mode: crate::data_views::ActivateOn) -> Self {
        self.activate_on = mode;
        self
    }

    /// Choose the row-selection granularity (None / Single / Multi).
    /// See [`TableSelectionMode`].
    pub fn selection_mode(mut self, mode: TableSelectionMode) -> Self {
        self.selection_mode = mode;
        self
    }

    /// Set the index-based row selection model (positions). For identity-based
    /// selection that survives reorder / filter / window-slide, build the view
    /// with [`from_source_keyed`](Self::from_source_keyed) instead.
    pub fn selection(mut self, sel: SelectionModel) -> Self {
        self.row_selection = Some(RowSelection::from_index(sel));
        self
    }

    /// Install an independent cell-selection model on top of row selection.
    /// See [`CellSelectionModel`].
    pub fn cell_selection(mut self, sel: CellSelectionModel) -> Self {
        self.cell_selection = Some(sel);
        self
    }

    /// Paint every other row with a tinted background. Default: off.
    pub fn alternating_rows(mut self, enabled: bool) -> Self {
        self.alternating_rows = enabled;
        self
    }

    /// Draw horizontal and/or vertical grid lines between cells.
    /// See [`GridLines`].
    pub fn grid_lines(mut self, kind: GridLines) -> Self {
        self.grid_lines = kind;
        self
    }

    /// Let the **last column in display order** take up whatever width the
    /// other columns leave, so the table never ends in a bare strip at its
    /// trailing edge — Qt's `stretchLastSection`, NSTableView's
    /// `lastColumnOnlyAutoresizingStyle`. Default: off.
    ///
    /// Positional, not a property of a column: after a reorder it is the
    /// *new* last column that stretches and the previous one goes back to
    /// its own width. While a column stretches, its declared width is the
    /// floor it grows from, its user-resize override is ignored, and its
    /// trailing grip is disabled (no AccessKit Increment/Decrement either):
    /// any size the user gave it, the stretch would take straight back.
    /// Resizing any *other* column reflows it. Once the other columns
    /// exceed the viewport there is nothing left to stretch into — the
    /// last column sits at its own width and the pane scrolls, as in Qt.
    ///
    /// `Flex` columns already share every spare pixel among themselves, so
    /// a table of `Flex` columns looks the same either way: this is for
    /// pixel-sized tables (`Fixed` / `Auto`, or widths the user has set)
    /// that would otherwise end in a gap.
    pub fn stretch_last_column(mut self, on: bool) -> Self {
        self.stretch_last_column = on;
        self
    }

    /// Provide an accessible label for the table (`aria-label`). Required
    /// when the page hosts more than one table so screen readers can
    /// distinguish them.
    pub fn a11y_label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.a11y_label = Some(label.into());
        self
    }

    /// Show or hide the built-in vertical scroll bar. Default: visible. Set to
    /// `false` when an external scroll bar is wired to [`scroll_y_signal`](Self::scroll_y_signal).
    pub fn show_internal_scrollbars(mut self, show: bool) -> Self {
        self.show_internal_scrollbars = show;
        self
    }

    /// Widget shown when the source is empty.
    pub fn empty_view(mut self, f: impl Fn() -> Box<dyn Widget> + 'static) -> Self {
        self.empty_view = Some(Rc::new(f));
        self
    }

    // ── Public reactive signals ────────────────────────────────────────

    /// Current vertical scroll offset in logical pixels.
    pub fn scroll_y_signal(&self) -> &Signal<f32> {
        &self.scroll_y
    }

    /// Maximum vertical scroll offset — `total_content_height − viewport_height`.
    pub fn max_scroll_y_signal(&self) -> &Signal<f32> {
        &self.max_scroll_y
    }

    /// Viewport-to-content height ratio, used by external scroll bar thumbs.
    pub fn viewport_ratio_y_signal(&self) -> &Signal<f32> {
        &self.viewport_ratio_y
    }

    /// Current horizontal scroll offset of the Middle (unpinned) pane, in
    /// logical pixels. Leading/Trailing-pinned columns are unaffected —
    /// see [`Column::pinned`].
    pub fn scroll_x_signal(&self) -> &Signal<f32> {
        &self.scroll_x
    }

    /// Maximum horizontal scroll offset — `middle_content_width −
    /// middle_viewport_width`.
    pub fn max_scroll_x_signal(&self) -> &Signal<f32> {
        &self.max_scroll_x
    }

    /// Middle-pane viewport-to-content width ratio, used by external
    /// horizontal scroll bar thumbs.
    pub fn viewport_ratio_x_signal(&self) -> &Signal<f32> {
        &self.viewport_ratio_x
    }

    /// Active sort: `Some((col_id, dir))` or `None` when unsorted.
    /// Mutated by header clicks (cycle: None → Asc → Desc → None) and by
    /// [`set_sort`](Self::set_sort) / [`clear_sort`](Self::clear_sort).
    /// Bind a [`SortFilterListModel`](teksilo_data::SortFilterListModel) to
    /// drive a re-sort of the underlying data:
    ///
    /// ```ignore
    /// let proxy = SortFilterListModel::new(model)
    ///     .with_comparator("name", |a, b| a.name.cmp(&b.name));
    /// proxy.sort_signal(table.sort_signal().clone());
    /// ```
    pub fn sort_signal(&self) -> &Signal<Option<(String, SortDirection)>> {
        &self.sort_signal
    }

    /// Map of column id → user-overridden width. A column id appears in
    /// this map only after the user resizes that column; missing keys
    /// mean "use the declared width policy".
    pub fn column_widths_signal(&self) -> &Signal<HashMap<String, f32>> {
        &self.column_widths_signal
    }

    /// Column ids in display order. Updated when the user drags a
    /// header to reorder, or imperatively via
    /// [`set_column_order`](Self::set_column_order). When empty, the
    /// declared order applies. Pinned-side groups (Leading / None /
    /// Trailing) are *always* honored — the entries inside this signal
    /// only re-sort within each group.
    pub fn column_order_signal(&self) -> &Signal<Vec<String>> {
        &self.column_order_signal
    }

    /// Per-id pinning override map. A key here pins the column to that
    /// side; missing keys fall back to the declared `Column::pinned`.
    /// Updated when the user drags a column across a pane boundary.
    pub fn column_pinning_signal(&self) -> &Signal<HashMap<String, PinnedSide>> {
        &self.column_pinning_signal
    }

    /// Currently keyboard-focused cell, as `(row_index, display_col)`,
    /// or `None` when no cell is focused. Mutated by the keyboard
    /// handler (Arrow keys / Tab / Home / End / PgUp / PgDn /
    /// Ctrl-Home / Ctrl-End / Escape) and by direct
    /// [`set_focused_cell`](Self::set_focused_cell) /
    /// [`clear_focused_cell`](Self::clear_focused_cell) calls.
    pub fn focused_cell_signal(&self) -> &Signal<Option<(usize, usize)>> {
        &self.focused_cell
    }

    /// Move the focused cell. Out-of-range values are silently clamped
    /// when the next layout runs.
    pub fn set_focused_cell(&self, row: usize, col: usize) {
        self.focused_cell.set(Some((row, col)));
    }

    /// Remove keyboard focus from any cell (equivalent to pressing Escape).
    pub fn clear_focused_cell(&self) {
        self.focused_cell.set(None);
    }

    /// Cell currently in edit mode, or `None` when no editor is open.
    /// Cell delegates inspect this via `CellContext::is_editing` and
    /// swap in an editor widget when matched.
    pub fn editing_cell_signal(&self) -> &Signal<Option<(usize, usize)>> {
        &self.editing_cell
    }

    /// Begin editing the cell `(row, col_id)`. Silently no-ops if `col_id`
    /// isn't a currently-displayed column, or if `row` is outside the visible
    /// range — an out-of-range target would otherwise strand `editing_cell` on
    /// a row nothing can match.
    ///
    /// Callable **before the view is mounted**, which is the only point at
    /// which a consumer can seed a freshly constructed view with an edit
    /// target it already holds. `display_indices` is a cache `build()` fills,
    /// so a pre-mount call finds it empty; the order is recomputed on demand
    /// in that case rather than resolving against nothing and no-opping for a
    /// third, undocumented reason.
    pub fn begin_edit(&self, row: usize, col_id: &str) {
        let cached = self.display_indices.borrow();
        let recomputed;
        let display: &[usize] = if cached.is_empty() {
            recomputed = self.display_order();
            &recomputed
        } else {
            &cached
        };
        if let Some(target) =
            imperative::resolve_edit_target(row, col_id, &self.columns, display, (self.len_fn)())
        {
            drop(cached);
            self.editing_cell.set(Some(target));
        }
    }

    /// Close the active cell editor without committing (the field's `on_blur` still fires).
    pub fn end_edit(&self) {
        self.editing_cell.set(None);
    }

    /// Per-column filter text. Updated by filter affordances in
    /// header cells and by
    /// [`set_filter`](Self::set_filter) / [`clear_filters`](Self::clear_filters).
    /// Bind a `SortFilterListModel<T>` to drive the upstream data:
    ///
    /// ```ignore
    /// let proxy = SortFilterListModel::new(model)
    ///     .with_predicate("name", |t| {
    ///         let needle = t.to_string();
    ///         Box::new(move |r: &Row| r.name.contains(&needle))
    ///     });
    /// proxy.filters_signal(table.filters_signal().clone());
    /// ```
    pub fn filters_signal(&self) -> &Signal<HashMap<String, String>> {
        &self.filters_signal
    }

    /// Set or clear the filter text for a single column. An empty `text` removes
    /// the entry for `col_id` (same as clearing the filter for that column).
    pub fn set_filter(&self, col_id: &str, text: &str) {
        imperative::set_filter(&self.filters_signal, col_id, text);
    }

    /// Remove all active column filters.
    pub fn clear_filters(&self) {
        imperative::set_if_changed(&self.filters_signal, HashMap::new());
    }

    // ── Imperative API ─────────────────────────────────────────────────

    /// Scroll so that `row` is aligned to the top of the viewport. A no-op
    /// before the first layout pass.
    pub fn scroll_to_row(&self, row: usize) {
        imperative::scroll_to_row(row, &self.row_metrics, &self.scroll_y, &self.max_scroll_y);
    }

    /// Set the active sort imperatively. Equivalent to writing to
    /// [`sort_signal`](Self::sort_signal) directly, except that an unchanged
    /// value neither writes nor notifies — see
    /// [`set_column_widths`](Self::set_column_widths).
    pub fn set_sort(&self, col_id: Option<&str>, dir: SortDirection) {
        let next = col_id.map(|c| (c.to_string(), dir));
        imperative::set_if_changed(&self.sort_signal, next);
    }

    /// Clear the active sort.
    pub fn clear_sort(&self) {
        imperative::set_if_changed(&self.sort_signal, None);
    }

    /// Set or remove a single column's user-resized width override.
    /// A non-positive `width` removes the entry (the column reverts to
    /// its declared width policy).
    pub fn set_column_width(&self, col_id: &str, width: f32) {
        imperative::set_column_width(&self.column_widths_signal, col_id, width);
    }

    /// Replace the full width-override map (typically used to restore
    /// a persisted layout).
    ///
    /// A no-op when the map is unchanged, so the documented
    /// settings-round-trip wiring (see docs/table-view.md, "Persistence")
    /// terminates instead of recursing: `Signal::set` has no equality check of
    /// its own, and a live resize writes a width on every pointer move.
    pub fn set_column_widths(&self, widths: HashMap<String, f32>) {
        imperative::set_column_widths(&self.column_widths_signal, widths);
    }

    /// Replace the column-order list. Ids not declared on this table
    /// are silently dropped on the next layout pass.
    pub fn set_column_order(&self, order: Vec<String>) {
        imperative::set_if_changed(&self.column_order_signal, order);
    }

    /// Pin or unpin a single column.
    pub fn set_column_pinning(&self, col_id: &str, side: PinnedSide) {
        imperative::set_column_pinning(&self.column_pinning_signal, col_id, side);
    }

    /// Effective pinning for a column — `column_pinning_signal` wins
    /// over the declared `Column::pinned`.
    fn effective_pinning(&self, col: &Column<T>) -> PinnedSide {
        self.column_pinning_signal
            .get()
            .get(&col.id)
            .copied()
            .unwrap_or(col.pinned)
    }

    /// Compute the visible column display order: a flat list of indices
    /// into `self.columns`. Columns are partitioned by effective
    /// pinning (Leading first, then None, then Trailing); within each
    /// pane they appear in `column_order_signal` order, with any
    /// columns missing from the signal appended in declaration order.
    fn display_order(&self) -> Vec<usize> {
        let order_signal = self.column_order_signal.get();
        let mut order_map: HashMap<&str, usize> = HashMap::new();
        for (i, id) in order_signal.iter().enumerate() {
            order_map.insert(id.as_str(), i);
        }
        let mut leading: Vec<usize> = Vec::new();
        let mut middle: Vec<usize> = Vec::new();
        let mut trailing: Vec<usize> = Vec::new();
        for (i, col) in self.columns.iter().enumerate() {
            match self.effective_pinning(col) {
                PinnedSide::Leading => leading.push(i),
                PinnedSide::None => middle.push(i),
                PinnedSide::Trailing => trailing.push(i),
            }
        }
        // Sort key: explicit `column_order_signal` positions win (low
        // values); columns missing from the signal fall back to their
        // declaration index, offset by a huge constant so they always
        // sort after any explicitly-ordered column.
        const FALLBACK_BASE: usize = usize::MAX / 2;
        let sort_pane = |bucket: &mut Vec<usize>, cols: &[Column<T>]| {
            bucket.sort_by_key(|&i| {
                order_map
                    .get(cols[i].id.as_str())
                    .copied()
                    .unwrap_or(FALLBACK_BASE + i)
            });
        };
        sort_pane(&mut leading, &self.columns);
        sort_pane(&mut middle, &self.columns);
        sort_pane(&mut trailing, &self.columns);
        let mut out = Vec::with_capacity(leading.len() + middle.len() + trailing.len());
        out.extend(leading);
        let leading_count = out.len();
        out.extend(middle);
        let middle_end = out.len();
        out.extend(trailing);
        // Stash the boundaries so paint / drop-zone math can read them.
        *self.pane_boundaries.borrow_mut() = PaneBoundaries::new(leading_count, middle_end);
        out
    }

    /// Scroll the minimum distance needed to make `row` visible. A no-op
    /// before the first layout pass, when the viewport height is not yet known.
    pub fn ensure_row_visible(&self, row: usize) {
        imperative::ensure_row_visible(
            row,
            &self.row_metrics,
            &self.scroll_y,
            &self.max_scroll_y,
            self.viewport_height.get(),
            self.laid_out.get(),
        );
    }

    // ── Internals ──────────────────────────────────────────────────────

    /// Scroll the row the keyboard cursor is on into view when this table
    /// takes focus.
    ///
    /// Only the rows near the viewport are realized, so on a table taller than
    /// the window the cursor's row frequently has no widget. Everything that
    /// speaks for it then has nothing to speak about: no row node carries
    /// `selected`, the `cell_map` lookup in `accessibility()` finds nothing so
    /// no `active_descendant` is nominated, and a screen reader taking focus
    /// here is told nothing at all. Worse, the first arrow press steps *past*
    /// that row, because the cursor was somewhere the user was never shown.
    ///
    /// The row resolves the way `context_menu_key_target` below resolves it:
    /// the focused cell's row if the user has navigated, else the first
    /// selected row. Both are "the row this table is currently about", and a
    /// session restored into a selection has no focused cell yet.
    ///
    /// **Vertical only, and that is the whole of it here.** The body pane
    /// realizes every display column of a realized row, iterating the full
    /// `display_indices` with no `scroll_x` culling
    /// (`table_view/body_pane.rs:313-443`), so the cell is in `cell_map`
    /// whatever the horizontal offset is. The row is the only axis that can
    /// hide it from the AT tree. A sighted keyboard user can still land with
    /// the cursor's column scrolled off to the side, which would want
    /// `ensure_col_visible` (`table_view/keyboard.rs:483`); that is a private
    /// helper of the key-handler module and out of this change's reach.
    ///
    /// Ensure-visible rather than scroll-to: a row already on screen must not
    /// jump under somebody who can see it.
    ///
    /// The handles are cloned into the effect rather than reaching through
    /// `self`, which the closure cannot borrow.
    fn reveal_current_row_on_focus(&self, ctx: &mut teksilo_core::build_context::BuildContext) {
        let row_metrics = self.row_metrics.clone();
        let scroll_y = self.scroll_y.clone();
        let max_scroll_y = self.max_scroll_y.clone();
        let viewport_height = self.viewport_height.clone();
        let laid_out = self.laid_out.clone();
        let focused_cell = self.focused_cell.clone();
        let selection = self.row_selection.clone();

        ctx.effect(&self.view_focused, move |focused| {
            if !*focused {
                return;
            }
            let Some(row) = focused_cell.get().map(|(row, _col)| row).or_else(|| {
                selection
                    .as_ref()
                    .and_then(|s| s.selected_indices().first().copied())
            }) else {
                return;
            };
            imperative::ensure_row_visible(
                row,
                &row_metrics,
                &scroll_y,
                &max_scroll_y,
                viewport_height.get(),
                laid_out.get(),
            );
        });
    }

    /// The configured row height (override) or the table style's 28 px
    /// fallback. In the non-uniform modes this is the seed estimate;
    /// real geometry lives in `row_metrics`.
    fn effective_row_height(&self) -> f32 {
        self.row_height.unwrap_or(cp::ROW_HEIGHT)
    }

    fn effective_header_height(&self) -> f32 {
        if !self.show_header {
            0.0
        } else {
            self.header_height.unwrap_or(cp::HEADER_HEIGHT)
        }
    }

    fn total_content_height(&self) -> f32 {
        self.row_metrics.borrow_mut().total_height((self.len_fn)())
    }

    fn visible_range(&self) -> (usize, usize) {
        self.row_metrics.borrow_mut().visible_range(
            self.scroll_y.get(),
            self.viewport_height.get(),
            (self.len_fn)(),
            BUFFER_ROWS,
        )
    }

    fn clamp_scroll(&self) {
        let max = self.max_scroll_y.get();
        let current = self.scroll_y.get();
        let clamped = current.clamp(0.0, max);
        if (clamped - current).abs() > 0.001 {
            self.scroll_y.set(clamped);
        }
    }
}

impl<T: 'static> std::fmt::Debug for TableView<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TableView")
            .field("rows", &(self.len_fn)())
            .field("columns", &self.columns.len())
            .field("scroll_y", &self.scroll_y.get())
            .field("selection_mode", &self.selection_mode)
            .field("scroll_bar_style", &self.scroll_bar_style)
            .finish()
    }
}

/// Draw the internal vertical grid-line dividers for one pane band —
/// `slice.len() - 1` lines between adjacent columns, clipped to `rect` so a
/// scrolled Middle-pane line can't bleed past its own viewport into a
/// pinned neighbour. `scroll` is nonzero only for the Middle pane.
///
/// Shared by `TableView`/`TreeTableView`'s `paint()`, which are otherwise
/// near-identical for this decoration.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_pane_dividers(
    canvas: &mut Canvas,
    rect: Rect,
    slice: &[f32],
    scroll: f32,
    rtl: bool,
    color: teksilo_tokens::Color,
    line_w: f32,
) {
    if slice.len() < 2 || rect.width <= 0.0 {
        return;
    }
    canvas.set_clip(rect);
    if rtl {
        let mut x = rect.right() + scroll;
        for &w in &slice[..slice.len() - 1] {
            x -= w;
            canvas.fill_rect(Rect::new(x, rect.y, line_w, rect.height), color);
        }
    } else {
        let mut x = rect.x - scroll;
        for &w in &slice[..slice.len() - 1] {
            x += w;
            canvas.fill_rect(Rect::new(x - line_w, rect.y, line_w, rect.height), color);
        }
    }
    canvas.clear_clip();
}

// Reorder drag-target plumbing (hover + drop on the header strip) lives in
// `header::attach_header_reorder_handlers` — shared with `TreeTableView`,
// which builds its header out of the same `HeaderCell`/`HeaderRow` pair.
