// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TableView<T>` — generic, virtualized, accessible tabular widget.
//!
//! Built atop the [`ListModel<T>`](bastyde_data::ListModel) /
//! [`ListDataSource`] data layer in
//! `bastyde-data` and the `bastyde-tokens` `TableStyle`. Mirrors Qt's
//! `QTableView`, SwiftUI's `Table`, and JavaFX's `TableView`.
//! The core skeleton: single body pane, row-virtualized with alternating
//! backgrounds, grid lines, `Role::Table > Role::Row > Role::Cell`
//! accessibility, multi-row selection, and an empty-state slot. Headers,
//! sort, filter, resize, reorder, pinning, cell selection, and editing are
//! also included. Row heights come in three modes: uniform (`row_height`,
//! the default fast path), exact per-row callback (`row_height_fn`), and
//! auto-measured (`auto_row_height` — rows grow to their tallest cell,
//! height-for-width). See docs/table-view.md "Row heights".

pub mod a11y;
pub mod body;
pub mod body_pane;
pub mod column;
pub mod filter;
pub mod header;
pub mod keyboard;
pub mod layout;
pub mod row_navigator;
pub mod selection;
#[cfg(test)]
mod tests;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use bastyde_canvas::{Canvas, Point, Rect, Size, SizeProposal};

use bastyde_core::ObserverHandle;
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_data::{
    DataChange, DropPosition, DropResponse, ItemKey, KeyedSelectionModel, ListDataSource, ListModel,
    SelectionModel,
};
use bastyde_i18n::LocalizedString;
use bastyde_tokens::{BorderRole, Easing, SurfaceRole};

use crate::styles::recipe_table_style as cp;

use crate::common::row_metrics::{HeightSource, RowMetrics, SharedRowMetrics};
use crate::common::scroll::OverscrollBehavior;
use crate::data_views::{RowDrag, RowSelection, flat_insertion_target};
use crate::list_source::DndLazy;
use crate::scroll_area::ScrollBarMode;
use crate::scroll_bar::{ScrollBar, ScrollBarOrientation, ScrollBarVisual};

pub use self::column::{
    Alignment, CellContext, Column, ColumnContext, ColumnResizePolicy, ColumnWidth, EditTrigger,
    GridLines, PinnedSide, TabTraversal, TruncationPolicy,
};
pub use self::selection::{CellSelectionModel, TableSelectionMode};
pub use bastyde_data::SortDirection;

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

/// Generic, virtualized, accessible table.
///
/// See module docs for the feature roadmap.
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
    edit_trigger: EditTrigger,
    /// User callback invoked when an edit trigger fires on the focused
    /// cell.
    #[allow(clippy::type_complexity)]
    on_cell_edit_request: Option<Rc<dyn Fn(usize, &str, &mut bastyde_core::widget::EventContext)>>,
    /// Per-column filter text. Updated by filter affordances in the
    /// header, by `set_filter` / `clear_filters`, and by
    /// downstream consumers binding it (e.g., `SortFilterListModel`).
    filters_signal: Signal<HashMap<String, String>>,
    /// User callback invoked on every row activation (Enter on the
    /// focused row).
    #[allow(clippy::type_complexity)]
    on_row_activate: Option<Rc<dyn Fn(usize, &mut bastyde_core::widget::EventContext)>>,
    reorderable_rows: bool,
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
    /// Counts of (leading-pinned, middle, trailing-pinned) columns —
    /// used by paint to draw pane dividers and by the drop-zone math
    /// to classify a drop position.
    pane_boundaries: Rc<RefCell<PaneBoundaries>>,
    viewport_height: Rc<Cell<f32>>,
    /// Width of the header strip (= the column band) snapshotted by
    /// `place_children`. The reorder-drop handler needs it to mirror the
    /// drop x under RTL, where the column content is right-anchored in
    /// the band (`local.x` is measured from the strip's physical left).
    header_strip_width: Rc<Cell<f32>>,

    // Header-cell shared state — tracked across the table so the
    // pointer-capture'd resize delivers PointerMove events back to the
    // active HeaderCell.
    resize_state: header::ResizeStateHandle,

    /// Stable id used by the column-reorder drag payload to disambiguate
    /// inter-table drops.
    table_id: usize,
}

impl<T: 'static> TableView<T> {
    /// Wrap a `ListModel<T>`.
    pub fn new(model: ListModel<T>) -> Self {
        let dnd = DndLazy::from_source(Rc::new(model.clone()));
        let (len_fn, with_item_fn, observe_fn, first_changed_fn) = erase_list_model(model);
        Self::create(len_fn, with_item_fn, observe_fn, first_changed_fn, dnd)
    }

    /// Wrap any `ListDataSource<Item = T>` (e.g. a
    /// [`SortFilterListModel<T>`](bastyde_data::SortFilterListModel)).
    ///
    /// The source owns DnD validation (`can_accept` / `accept_drop`) and
    /// lazy windowing (`row_state` / `request_window` / `fetch_more`); a
    /// read-only source leaves the defaults inert.
    pub fn from_source<S: ListDataSource<Item = T>>(source: S) -> Self {
        let s = Rc::new(source);
        let dnd = DndLazy::from_source(s.clone());
        let (len_fn, with_item_fn, observe_fn, first_changed_fn) =
            erase_data_source::<S, T>(s);
        Self::create(len_fn, with_item_fn, observe_fn, first_changed_fn, dnd)
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
        let (len_fn, with_item_fn, observe_fn, first_changed_fn) =
            erase_data_source::<S, T>(s);
        let mut view = Self::create(len_fn, with_item_fn, observe_fn, first_changed_fn, dnd);
        view.row_selection = Some(row_selection);
        view
    }

    fn create(
        len_fn: LenFn,
        with_item_fn: WithItemFn<T>,
        observe_fn: ObserveFn,
        first_changed_fn: FirstChangedFn,
        dnd: DndLazy,
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
            sort_signal: Signal::new(None),
            column_widths_signal: Signal::new(HashMap::new()),
            column_order_signal: Signal::new(Vec::new()),
            column_pinning_signal: Signal::new(HashMap::new()),
            focused_cell: Signal::new(None),
            type_ahead_label: None,
            type_ahead_timeout: crate::common::type_ahead::DEFAULT_TYPE_AHEAD_TIMEOUT,
            type_ahead: crate::common::type_ahead::TypeAheadState::new(),
            // Replaced at build with the live tree signals; the defaults are
            // only the pre-build values (treat as focused, pointer modality).
            view_focused: Signal::new(true),
            focus_visible: Signal::new(false),
            tab_traversal: TabTraversal::default(),
            editing_cell: Signal::new(None),
            edit_trigger: EditTrigger::default(),
            on_cell_edit_request: None,
            filters_signal: Signal::new(HashMap::new()),
            on_row_activate: None,
            reorderable_rows: false,
            drop_feedback: Signal::new(None),
            activate_on: crate::data_views::ActivateOn::default(),
            header_row_id: None,
            body_pane_id: None,
            scrollbar_id: None,
            empty_id: None,
            pane_version: Signal::new(0_u64),
            pane_built_start: Rc::new(Cell::new(0)),
            pane_built_end: Rc::new(Cell::new(0)),
            pane_total_refresh: Signal::new(0_u64),
            column_widths: Rc::new(RefCell::new(Vec::new())),
            display_indices: Rc::new(RefCell::new(Vec::new())),
            pane_boundaries: Rc::new(RefCell::new(PaneBoundaries::default())),
            viewport_height: Rc::new(Cell::new(600.0)),
            header_strip_width: Rc::new(Cell::new(0.0)),
            resize_state: Rc::new(std::cell::RefCell::new(None)),
            table_id,
        }
    }

    // ── Builder ────────────────────────────────────────────────────────

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
    /// On an editable column whose [`EditTrigger`] is type-to-edit, typing
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

    pub fn add_column(mut self, col: Column<T>) -> Self {
        self.columns.push(col);
        self
    }

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

    pub fn header_height(mut self, height: f32) -> Self {
        self.header_height = Some(height);
        self
    }

    pub fn show_header(mut self, visible: bool) -> Self {
        self.show_header = visible;
        self
    }

    pub fn column_resize_policy(mut self, policy: ColumnResizePolicy) -> Self {
        self.column_resize_policy = policy;
        self
    }

    pub fn tab_traversal(mut self, mode: TabTraversal) -> Self {
        self.tab_traversal = mode;
        self
    }

    pub fn edit_trigger(mut self, trigger: EditTrigger) -> Self {
        self.edit_trigger = trigger;
        self
    }

    /// Hook fired by the keyboard handler when an edit trigger fires
    /// on the focused cell. Receives `(row_index, col_id, ctx)`.
    pub fn on_cell_edit_request(
        mut self,
        f: impl Fn(usize, &str, &mut bastyde_core::widget::EventContext) + 'static,
    ) -> Self {
        self.on_cell_edit_request = Some(Rc::new(f));
        self
    }

    /// Hook fired when the user presses Enter on the focused row.
    pub fn on_row_activate(
        mut self,
        f: impl Fn(usize, &mut bastyde_core::widget::EventContext) + 'static,
    ) -> Self {
        self.on_row_activate = Some(Rc::new(f));
        self
    }

    /// Enable drag-to-reorder of rows (pointer drag + keyboard
    /// Alt+ArrowUp/Down).
    ///
    /// The move is routed through the backing source's `accept_drop`: a
    /// `ListModel` reorders in place, an external source routes the move to
    /// its store. Per-hover the source's `can_accept` decides whether the
    /// drop is allowed — a forbidden position shows no insertion line and
    /// the drop is refused. A row may also be forbidden from dragging at
    /// all (the source's `drag` gate). Cross-table / external drops arrive
    /// at `accept_drop` as `DragSource::Foreign`; a bare `ListModel`
    /// rejects them, an external source decides.
    pub fn reorderable_rows(mut self, enabled: bool) -> Self {
        self.reorderable_rows = enabled;
        self
    }

    /// Choose single- vs double-click activation for `on_row_activate` (default
    /// [`ActivateOn::DoubleClick`](crate::ActivateOn)). Enter/Space activates in
    /// either mode.
    pub fn activate_on(mut self, mode: crate::data_views::ActivateOn) -> Self {
        self.activate_on = mode;
        self
    }

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

    pub fn cell_selection(mut self, sel: CellSelectionModel) -> Self {
        self.cell_selection = Some(sel);
        self
    }

    pub fn alternating_rows(mut self, enabled: bool) -> Self {
        self.alternating_rows = enabled;
        self
    }

    pub fn grid_lines(mut self, kind: GridLines) -> Self {
        self.grid_lines = kind;
        self
    }

    pub fn a11y_label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.a11y_label = Some(label.into());
        self
    }

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

    pub fn scroll_y_signal(&self) -> &Signal<f32> {
        &self.scroll_y
    }

    pub fn max_scroll_y_signal(&self) -> &Signal<f32> {
        &self.max_scroll_y
    }

    pub fn viewport_ratio_y_signal(&self) -> &Signal<f32> {
        &self.viewport_ratio_y
    }

    /// Active sort: `Some((col_id, dir))` or `None` when unsorted.
    /// Mutated by header clicks (cycle: None → Asc → Desc → None) and by
    /// [`set_sort`](Self::set_sort) / [`clear_sort`](Self::clear_sort).
    /// Bind a [`SortFilterListModel`](bastyde_data::SortFilterListModel) to
    /// drive a re-sort of the underlying data:
    ///
    /// ```ignore
    /// let proxy = SortFilterListModel::new(model)
    ///     .with_comparator("name", |a, b| a.name.cmp(&b.name));
    /// proxy.bind_sort_signal(table.sort_signal().clone());
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

    pub fn clear_focused_cell(&self) {
        self.focused_cell.set(None);
    }

    /// Cell currently in edit mode, or `None` when no editor is open.
    /// Cell delegates inspect this via `CellContext::is_editing` and
    /// swap in an editor widget when matched.
    pub fn editing_cell_signal(&self) -> &Signal<Option<(usize, usize)>> {
        &self.editing_cell
    }

    /// Begin editing the cell `(row, col_id)`. Resolves `col_id` to the
    /// current display position and sets `editing_cell`. Silently
    /// no-ops if `col_id` doesn't exist.
    pub fn begin_edit(&self, row: usize, col_id: &str) {
        if let Some((display_pos, _)) = self
            .columns
            .iter()
            .enumerate()
            .find(|(_, c)| c.id == col_id)
        {
            // Find the display position from the live display order.
            let display = self.display_indices.borrow();
            if let Some(pos) = display.iter().position(|&i| i == display_pos) {
                self.editing_cell.set(Some((row, pos)));
            }
        }
    }

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
    /// proxy.bind_filters_signal(table.filters_signal().clone());
    /// ```
    pub fn filters_signal(&self) -> &Signal<HashMap<String, String>> {
        &self.filters_signal
    }

    pub fn set_filter(&self, col_id: &str, text: &str) {
        let mut m = self.filters_signal.get();
        if text.is_empty() {
            m.remove(col_id);
        } else {
            m.insert(col_id.to_string(), text.to_string());
        }
        self.filters_signal.set(m);
    }

    pub fn clear_filters(&self) {
        self.filters_signal.set(HashMap::new());
    }

    // ── Imperative API ─────────────────────────────────────────────────

    /// Scroll so that `row` is aligned to the top of the viewport.
    pub fn scroll_to_row(&self, row: usize) {
        let target = self.row_metrics.borrow_mut().row_top(row);
        let max = self.max_scroll_y.get();
        self.scroll_y.set(target.clamp(0.0, max));
    }

    /// Set the active sort imperatively. Equivalent to writing to
    /// [`sort_signal`](Self::sort_signal) directly.
    pub fn set_sort(&self, col_id: Option<&str>, dir: SortDirection) {
        let next = col_id.map(|c| (c.to_string(), dir));
        self.sort_signal.set(next);
    }

    /// Clear the active sort.
    pub fn clear_sort(&self) {
        self.sort_signal.set(None);
    }

    /// Set or remove a single column's user-resized width override.
    /// A non-positive `width` removes the entry (the column reverts to
    /// its declared width policy).
    pub fn set_column_width(&self, col_id: &str, width: f32) {
        let mut m = self.column_widths_signal.get();
        if width.is_finite() && width > 0.0 {
            m.insert(col_id.to_string(), width);
        } else {
            m.remove(col_id);
        }
        self.column_widths_signal.set(m);
    }

    /// Replace the full width-override map (typically used to restore
    /// a persisted layout).
    pub fn set_column_widths(&self, widths: HashMap<String, f32>) {
        self.column_widths_signal.set(widths);
    }

    /// Replace the column-order list. Ids not declared on this table
    /// are silently dropped on the next layout pass.
    pub fn set_column_order(&self, order: Vec<String>) {
        self.column_order_signal.set(order);
    }

    /// Pin or unpin a single column.
    pub fn set_column_pinning(&self, col_id: &str, side: PinnedSide) {
        let mut m = self.column_pinning_signal.get();
        if matches!(side, PinnedSide::None) {
            m.remove(col_id);
        } else {
            m.insert(col_id.to_string(), side);
        }
        self.column_pinning_signal.set(m);
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

    /// Scroll the minimum distance needed to make `row` visible.
    pub fn ensure_row_visible(&self, row: usize) {
        let scroll = self.scroll_y.get();
        let new_scroll = self.row_metrics.borrow_mut().scroll_for_ensure_visible(
            row,
            scroll,
            self.viewport_height.get(),
            self.max_scroll_y.get(),
        );
        if (new_scroll - scroll).abs() > f32::EPSILON {
            self.scroll_y.set(new_scroll);
        }
    }

    // ── Internals ──────────────────────────────────────────────────────

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

impl<T: 'static> Widget for TableView<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let row_h = self.effective_row_height();
        let header_h = self.effective_header_height();

        // Version signal — bumps drive a rebuild.
        let version = ctx.signal(0_u64);
        version.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        // Scroll-y at Relayout: place_children re-runs without rebuild.
        self.scroll_y.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );
        ctx.register_animated_signal(&self.scroll_y);

        // Row-drop insertion indicator at RepaintOnly so on_drag_hover /
        // on_drag_leave `set(...)` calls dirty paint without a rebuild.
        self.drop_feedback.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );

        // Pane → root total refresh (auto-measure mode): re-place this
        // root when the body pane's measurements changed the content
        // total, so `max_scroll_y` / the thumb ratio pick up the
        // corrected value.
        self.pane_total_refresh.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );

        // Column width overrides: any change re-runs place_children
        // (which calls ColumnSolver with the latest map). No rebuild
        // needed — widths flow through `column_widths` Rc into rows.
        self.column_widths_signal.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );

        // Column order + pinning: changes require a rebuild because the
        // header cells and row cells must be re-emitted in the new order
        // (each cell captures its display-position-based 1-based index).
        let v_for_order = version.clone();
        let order_ver = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.column_order_signal, move |_| {
            let next = order_ver.get() + 1;
            order_ver.set(next);
            v_for_order.set(next);
        });
        let v_for_pin = version.clone();
        let pin_ver = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.column_pinning_signal, move |_| {
            let next = pin_ver.get() + 1;
            pin_ver.set(next);
            v_for_pin.set(next);
        });
        let v_for_edit = version.clone();
        let edit_ver = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.editing_cell, move |_| {
            let next = edit_ver.get() + 1;
            edit_ver.set(next);
            v_for_edit.set(next);
        });
        let v_for_filter = version.clone();
        let filter_ver = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.filters_signal, move |_| {
            let next = filter_ver.get() + 1;
            filter_ver.set(next);
            v_for_filter.set(next);
        });

        // Sort signal: a change requires a rebuild because each header
        // cell's chevron child is added/removed conditionally and the
        // AccessKit `set_sort_direction` is captured at build time.
        let v_for_sort = version.clone();
        let sort_ver = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.sort_signal, move |_| {
            let next = sort_ver.get() + 1;
            sort_ver.set(next);
            v_for_sort.set(next);
        });

        // Observe model changes -> bump version.
        let v_for_data = version.clone();
        let data_ver = Rc::new(Cell::new(0_u64));
        let upstream = (self.observe_fn)(Box::new({
            let dv = data_ver.clone();
            let sel_for_adjust = self.row_selection.clone();
            let cell_sel_for_adjust = self.cell_selection.clone();
            let metrics_for_data = self.row_metrics.clone();
            let len_for_data = self.len_fn.clone();
            let first_changed = self.first_changed_fn.clone();
            move |change| {
                // Keep row metrics in step with the data: rows before
                // the first changed index keep their heights, the rest
                // re-derive. A `SortFilterListModel` source collapses
                // everything to `Reset` — its real divergence comes
                // through the side-channel, which is what lets an
                // append keep the measured prefix.
                let divergence = match change {
                    DataChange::ItemsInserted { range } | DataChange::ItemsRemoved { range } => {
                        Some(range.start)
                    }
                    DataChange::ItemUpdated { index } => Some(*index),
                    DataChange::ItemsMoved { from, to, .. } => Some((*from).min(*to)),
                    DataChange::WindowLoaded { range } => Some(range.start),
                    DataChange::Reset => (first_changed)(),
                };
                metrics_for_data
                    .borrow_mut()
                    .apply_divergence(divergence, (len_for_data)());
                // Keep row selection in step: index-shift (index model) or
                // prune orphaned keys (keyed model). Cell selection (always
                // index-based) is adjusted separately below.
                if let Some(ref rs) = sel_for_adjust {
                    rs.on_data_change(change);
                }
                if let Some(ref s) = cell_sel_for_adjust {
                    match change {
                        DataChange::ItemsInserted { range } => {
                            s.adjust_for_row_insert(range.start, range.end - range.start);
                        }
                        DataChange::ItemsRemoved { range } => {
                            s.adjust_for_row_remove(range.start, range.end - range.start);
                        }
                        DataChange::ItemsMoved { from, to, count } => {
                            s.adjust_for_row_move(*from, *to, *count);
                        }
                        DataChange::Reset => s.clear(),
                        _ => {}
                    }
                }
                let next = dv.get() + 1;
                dv.set(next);
                v_for_data.set(next);
            }
        }));
        ctx.own_handle(upstream);

        // Observe selection changes -> bump version (rebuild updates the
        // `is_selected` arg passed to cell delegates).
        if let Some(ref rs) = self.row_selection {
            let v_for_sel = version.clone();
            let sel_ver = Rc::new(Cell::new(0_u64));
            let handle = rs.observe_for_rebuild(move || {
                let next = sel_ver.get() + 1;
                sel_ver.set(next);
                v_for_sel.set(next);
            });
            ctx.own_handle(handle);
        }
        if let Some(ref cs) = self.cell_selection {
            let v_for_csel = version.clone();
            let csel_ver = Rc::new(Cell::new(0_u64));
            ctx.effect(&cs.selection_signal(), move |_| {
                let next = csel_ver.get() + 1;
                csel_ver.set(next);
                v_for_csel.set(next);
            });
        }

        // Observe scroll position — only rebuild when visible range exits
        // the buffered window. The Relayout binding above handles
        // intra-buffer scrolls without a rebuild.
        let vp_h = self.viewport_height.clone();
        let len_for_scroll = self.len_fn.clone();
        let (built_start, built_end) = self.visible_range();
        let prev_built_start = Rc::new(Cell::new(built_start));
        let prev_built_end = Rc::new(Cell::new(built_end));
        let v_for_scroll = version.clone();
        let scroll_ver = Rc::new(Cell::new(0_u64));
        let scroll_handle = self.scroll_y.observe({
            let pbs = prev_built_start.clone();
            let pbe = prev_built_end.clone();
            let sv = scroll_ver.clone();
            let metrics = self.row_metrics.clone();
            move |y| {
                let count = (len_for_scroll)();
                let (visible_start, visible_end) =
                    metrics.borrow_mut().visible_range(*y, vp_h.get(), count, 0);
                if visible_start < pbs.get() || visible_end > pbe.get() {
                    let new_start = visible_start.saturating_sub(BUFFER_ROWS);
                    let new_end = (visible_end + BUFFER_ROWS).min(count);
                    pbs.set(new_start);
                    pbe.set(new_end);
                    let next = sv.get() + 1;
                    sv.set(next);
                    v_for_scroll.set(next);
                }
            }
        });
        ctx.own_handle(scroll_handle);

        // Compute display order eagerly — the keyboard handler needs
        // the column count, and the header / body builds below also
        // need it. We re-write `self.display_indices` here; later
        // build steps read it.
        let display_indices_now = self.display_order();
        *self.display_indices.borrow_mut() = display_indices_now.clone();

        // Self handlers: scroll wheel + keyboard + clip + focusable.
        let scroll_y_for_wheel = self.scroll_y.clone();
        let max_scroll_for_wheel = self.max_scroll_y.clone();
        let line_height = row_h;
        let overscroll_behavior = self.overscroll_behavior;
        let smooth_scrolling = self.smooth_scrolling;
        let smooth_scroll_duration = self.smooth_scroll_duration;

        // Bind focused_cell at RepaintOnly — its update redraws the
        // focus ring without rebuilding the row tree.
        self.focused_cell.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );

        // Focus-aware selection + modality-gated focus ring. `begin_view_focus`
        // keys the scope signal on this root id directly — the same id the body
        // pane uses for its row scope (`drag_anchor = ctx.self_id()`), and
        // independent of the arena focusable flag (not yet wired here). A plain
        // `view_focus_active()` here would find no focusable ancestor and fall
        // back to the constant-`true` "outside any scope" signal — `true`
        // whenever ANY widget holds focus, lighting every table's ring at once.
        // The signal is `true` whenever the table or any descendant holds focus,
        // so the selection band dims to `SelectedInactive` on focus-out. Pop
        // straight back; the body pane re-pushes the same cached signal.
        // `focus_visible` gates the cell ring to keyboard navigation. Both bound
        // `RepaintOnly`: a focus/modality change redraws without a rebuild.
        self.view_focused = ctx.begin_view_focus();
        ctx.end_view_focus();
        self.focus_visible = ctx.focus_visible();
        self.view_focused.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        self.focus_visible.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );

        // Build the navigator + key handler. The keyboard module is
        // generic over RowNavigator so TreeTableView can plug in its own
        // tree-aware navigator.
        let navigator: Rc<dyn row_navigator::RowNavigator> =
            Rc::new(row_navigator::FlatNavigator::new(self.len_fn.clone()));
        // display_col_to_id resolves a display position back to its
        // column id, so the keyboard module doesn't need a `Column<T>`
        // reference. Snapshotted at build; rebuilds re-issue this.
        let column_ids_in_display_order: Vec<String> = display_indices_now
            .iter()
            .map(|&i| self.columns[i].id.clone())
            .collect();
        let display_col_to_id: Rc<dyn Fn(usize) -> Option<String>> = {
            let ids = column_ids_in_display_order;
            Rc::new(move |pos| ids.get(pos).cloned())
        };
        let display_col_editable: Rc<dyn Fn(usize) -> bool> = {
            let editable_in_display_order: Vec<bool> = display_indices_now
                .iter()
                .map(|&i| self.columns[i].editable)
                .collect();
            Rc::new(move |pos| editable_in_display_order.get(pos).copied().unwrap_or(false))
        };

        // Type-ahead label resolver (row -> Some(text)) built from the user's
        // `Fn(&T) -> String` + the side-effect source read: the closure only
        // fires for a resident row, so unloaded (lazy) rows resolve to `None`
        // and the search skips them.
        let type_ahead_label: Option<Rc<dyn Fn(usize) -> Option<String>>> =
            self.type_ahead_label.clone().map(|user| {
                let with_item = self.with_item_fn.clone();
                Rc::new(move |i: usize| {
                    let out = std::cell::RefCell::new(None);
                    (with_item)(i, &|item| {
                        *out.borrow_mut() = Some(user(item));
                    });
                    out.into_inner()
                }) as Rc<dyn Fn(usize) -> Option<String>>
            });

        let key_cfg = keyboard::KeyHandlerConfig {
            navigator,
            col_count: display_indices_now.len().max(1),
            focused_cell: self.focused_cell.clone(),
            selection_mode: self.selection_mode,
            selection: self.row_selection.clone(),
            cell_selection: self.cell_selection.clone(),
            scroll_y: self.scroll_y.clone(),
            max_scroll_y: self.max_scroll_y.clone(),
            viewport_height: self.viewport_height.clone(),
            row_metrics: self.row_metrics.clone(),
            tab_traversal: self.tab_traversal,
            editing_cell: self.editing_cell.clone(),
            edit_trigger: self.edit_trigger,
            display_col_to_id,
            display_col_editable,
            on_cell_edit_request: self.on_cell_edit_request.clone(),
            on_row_activate: self.on_row_activate.clone(),
            type_ahead: self.type_ahead.clone(),
            type_ahead_label,
            type_ahead_timeout: self.type_ahead_timeout,
        };

        // Row DnD is owned by the backing source. The view computes the
        // geometric (target_row, position) and asks the source: `can_accept`
        // on hover gates the insertion line (forbidden → no affordance),
        // `accept_drop` on release commits the move (in-place for a
        // `ListModel`, routed for an external source). Same-view reorders and
        // foreign / cross-table drops both flow through `accept_drop` — the
        // erased closures recover SameView-vs-Foreign from the payload.
        let view_id = self.table_id;
        let can_accept_hover = self.dnd.can_accept_fn.clone();
        let scroll_for_hover = self.scroll_y.clone();
        let metrics_for_hover = self.row_metrics.clone();
        let len_for_hover = self.len_fn.clone();
        let header_h_for_hover = header_h;
        let band_width_for_hover = self.header_strip_width.clone();
        let feedback_for_hover = self.drop_feedback.clone();

        let accept_drop_for_drop = self.dnd.accept_drop_fn.clone();
        let scroll_y_for_drop = self.scroll_y.clone();
        let header_h_for_drop = header_h;
        let metrics_for_drop = self.row_metrics.clone();
        let len_fn_for_drop = self.len_fn.clone();
        let feedback_for_drop = self.drop_feedback.clone();

        let feedback_for_leave = self.drop_feedback.clone();
        let scroll_for_tick = self.scroll_y.clone();
        let max_scroll_for_tick = self.max_scroll_y.clone();
        let viewport_for_tick = self.viewport_height.clone();
        let header_h_for_tick = header_h;

        // Alt+Arrow reorder wraps the shared key handler: the move is a
        // synthetic same-view `RowDrag` through the source's `accept_drop`,
        // so it travels exactly the pointer-drop path. Every other key falls
        // through to the shared navigator (cell/row movement, edit, etc.).
        let mut shared_key = keyboard::build_key_handler(key_cfg);
        let reorderable_kbd = self.reorderable_rows;
        let accept_drop_kbd = self.dnd.accept_drop_fn.clone();
        let focused_kbd = self.focused_cell.clone();
        let sel_kbd = self.row_selection.clone();
        let len_kbd = self.len_fn.clone();
        let key_handler = move |event: &bastyde_core::event::WidgetEvent,
                                ctx: &mut bastyde_core::widget::EventContext|
              -> bastyde_core::event::EventResponse {
            use bastyde_core::event::{EventResponse, Key, WidgetEvent};
            if reorderable_kbd
                && let WidgetEvent::KeyDown { key, modifiers, .. } = event
                && modifiers.alt()
            {
                let count = (len_kbd)();
                if count > 0 {
                    let cur = focused_kbd.get().map(|(r, _)| r).or_else(|| {
                        sel_kbd
                            .as_ref()
                            .and_then(|s| s.selected_indices().first().copied())
                    });
                    if let Some(idx) = cur {
                        let mv = match key {
                            Key::ArrowUp if idx > 0 => {
                                Some((idx - 1, DropPosition::Before, idx - 1))
                            }
                            Key::ArrowDown if idx + 1 < count => {
                                Some((idx + 1, DropPosition::After, idx + 1))
                            }
                            _ => None,
                        };
                        if let Some((target, position, dest)) = mv {
                            let payload =
                                bastyde_core::drag_payload::DragPayload::typed(RowDrag {
                                    source_index: idx,
                                    source_view_id: view_id,
                                });
                            if (accept_drop_kbd)(&payload, target, position, view_id) {
                                if let Some(ref s) = sel_kbd {
                                    s.select(dest);
                                }
                                let col = focused_kbd.get().map(|(_, c)| c).unwrap_or(0);
                                focused_kbd.set(Some((dest, col)));
                            }
                            return EventResponse::Handled;
                        }
                    }
                }
            }
            shared_key(event, ctx)
        };

        let handlers = HandlerSet::new()
            .on_scroll(move |event, _ctx| match event {
                bastyde_core::event::WidgetEvent::Scroll { delta, .. } => {
                    let dy = match delta {
                        bastyde_core::event::ScrollDelta::Lines { y, .. } => y * line_height,
                        bastyde_core::event::ScrollDelta::Pixels { y, .. } => *y,
                    };
                    let current = scroll_y_for_wheel.get();
                    let max = max_scroll_for_wheel.get();
                    // Base off the animation target (not the rendered offset)
                    // so a mid-fling boundary correctly chains and successive
                    // notches accumulate instead of restarting from the
                    // partway-animated position.
                    let base = scroll_y_for_wheel.animation_target().unwrap_or(current);
                    let (new_y, moved) = crate::common::scroll::scroll_clamp_axis(base, dy, max);
                    if moved {
                        if smooth_scrolling {
                            scroll_y_for_wheel.animate_to(
                                new_y,
                                smooth_scroll_duration,
                                Easing::EaseOut,
                            );
                        } else {
                            scroll_y_for_wheel.set(new_y);
                        }
                    }
                    // Chain to an ancestor scrollable when fully clamped
                    // (unless Contain), otherwise consume.
                    crate::common::scroll::scroll_response(
                        moved,
                        overscroll_behavior == OverscrollBehavior::Contain,
                    )
                }
                _ => bastyde_core::event::EventResponse::Ignored,
            })
            .on_key(key_handler)
            .on_drag_hover(move |payload, position, _ctx| {
                // Column reorder is handled by the header strip; only
                // row-level drops (same-view `RowDrag` or a foreign payload
                // the source accepts) get an insertion line here.
                if payload.has_typed::<ColumnReorderDragData>() {
                    feedback_for_hover.set(None);
                    return bastyde_core::DropFeedback::NoFeedback;
                }
                let body_y = position.y - header_h_for_hover;
                let scroll = scroll_for_hover.get();
                let content_y = body_y + scroll;
                let len = (len_for_hover)();
                let (ins, line_y) = {
                    let mut m = metrics_for_hover.borrow_mut();
                    m.resize(len);
                    let ins = m.insertion_index(content_y);
                    (ins, m.row_top(ins) - scroll)
                };
                let width = band_width_for_hover.get();
                // Source-owned validation: paint the line only when the
                // source does not reject the hovered position.
                let allowed = flat_insertion_target(ins, len).is_some_and(|(target, pos)| {
                    !matches!(
                        (can_accept_hover)(payload, target, pos, view_id),
                        DropResponse::Reject
                    )
                });
                if allowed {
                    feedback_for_hover.set(Some((line_y, width)));
                    bastyde_core::DropFeedback::InsertionLine { y: line_y, width }
                } else {
                    feedback_for_hover.set(None);
                    bastyde_core::DropFeedback::NoFeedback
                }
            })
            .on_drop(move |payload, position, _ctx| {
                feedback_for_drop.set(None);
                if payload.has_typed::<ColumnReorderDragData>() {
                    return false;
                }
                let body_y = position.y - header_h_for_drop;
                let scroll = scroll_y_for_drop.get();
                let content_y = body_y + scroll;
                let len = (len_fn_for_drop)();
                let ins = {
                    let mut m = metrics_for_drop.borrow_mut();
                    m.resize(len);
                    m.insertion_index(content_y)
                };
                match flat_insertion_target(ins, len) {
                    Some((target, position_kind)) => {
                        (accept_drop_for_drop)(&payload, target, position_kind, view_id)
                    }
                    None => false,
                }
            })
            .on_drag_leave(move |_ctx| {
                feedback_for_leave.set(None);
            })
            .on_drag_tick(move |pos, _ctx| {
                // Auto-scroll when the pointer lingers within 32 px of the
                // body band's top/bottom edge during a drag (body-relative
                // so the header doesn't count as the top edge).
                const EDGE: f32 = 32.0;
                const MAX_VELOCITY: f32 = 12.0;
                let body_h = (viewport_for_tick.get() - header_h_for_tick).max(0.0);
                let y = pos.y - header_h_for_tick;
                let above = (EDGE - y).max(0.0);
                let below = (y - (body_h - EDGE)).max(0.0);
                let delta = if above > 0.0 {
                    -(above / EDGE) * MAX_VELOCITY
                } else if below > 0.0 {
                    (below / EDGE) * MAX_VELOCITY
                } else {
                    0.0
                };
                if delta.abs() > 0.01 {
                    let max = max_scroll_for_tick.get();
                    let new_y = (scroll_for_tick.get() + delta).clamp(0.0, max);
                    scroll_for_tick.set(new_y);
                }
            })
            .clips_children(true)
            .focusable(true);
        ctx.apply_self_handlers(handlers);

        // ── Build children ────────────────────────────────────────────
        self.header_row_id = None;
        self.body_pane_id = None;
        self.scrollbar_id = None;
        self.empty_id = None;

        // Display order was already computed above (before the
        // keyboard handler was wired); pull it back into a local for
        // the header / body loops.
        let display_indices = display_indices_now;

        // Header strip: build first so it sits above the body in the
        // child order (place_children iterates in this order).
        if self.show_header {
            let mut cell_ids: Vec<WidgetId> = Vec::with_capacity(display_indices.len());
            let active_sort = self.sort_signal.get();
            for (display_pos, &col_idx) in display_indices.iter().enumerate() {
                let col = &self.columns[col_idx];
                let current_sort = active_sort
                    .as_ref()
                    .and_then(|(id, dir)| if id == &col.id { Some(*dir) } else { None });
                // Filter zone width: indicator glyph + a small horizontal
                // padding for tap tolerance. Mirrors the layout of the
                // HStack inside HeaderCell::build.
                let filter_zone_width = cp::FILTER_INDICATOR_SIZE + cp::CELL_PADDING_HORIZONTAL;
                let cell = header::HeaderCell::new(
                    col.id.clone(),
                    col.header_label.resolve_now(),
                    display_pos + 1,
                    col.sortable,
                    col.resizable,
                    col.reorderable,
                    cp::RESIZE_HANDLE_WIDTH,
                    current_sort,
                    self.sort_signal.clone(),
                    self.column_widths_signal.clone(),
                    self.column_widths.clone(),
                    display_pos,
                    self.column_resize_policy,
                    self.resize_state.clone(),
                    self.table_id,
                    col.filterable,
                    filter_zone_width,
                    self.filters_signal.clone(),
                );
                cell_ids.push(ctx.add(cell));
            }
            let header_row = header::HeaderRow::new(
                cell_ids,
                self.column_widths.clone(),
                cp::GRID_LINE_THICKNESS,
            );
            // Wire reorder drag-target handlers on the header strip.
            let header_row_id = ctx.add(header_row);
            attach_header_reorder_handlers(
                ctx,
                header_row_id,
                self.table_id,
                self.column_widths.clone(),
                self.display_indices.clone(),
                self.pane_boundaries.clone(),
                self.column_order_signal.clone(),
                self.column_pinning_signal.clone(),
                self.columns.iter().map(|c| c.id.clone()).collect(),
                self.header_strip_width.clone(),
            );
            self.header_row_id = Some(header_row_id);
        }

        let row_count = (self.len_fn)();

        // Lazy: nudge the source to load the realized window, and fetch the
        // next page as the viewport nears the end (append-only sources). A
        // fully-resident source leaves these inert.
        let (vis_start, vis_end) = self.visible_range();
        (self.dnd.request_window_fn)(vis_start..vis_end);
        if (self.dnd.can_fetch_more_fn)() && vis_end + BUFFER_ROWS >= row_count {
            (self.dnd.fetch_more_fn)();
        }

        if row_count == 0 {
            // Empty state.
            if let Some(ref f) = self.empty_view {
                let id = ctx.add_boxed(f());
                self.empty_id = Some(id);
            }
        } else {
            // Hoist the row pane into its own widget so that
            // scroll-buffer-exit rebuilds (which happen mid-thumb-drag
            // when the user scrolls past the buffered range) target a
            // sibling of the scrollbar rather than the scrollbar's
            // ancestor. Rebuilding the ancestor would be deferred by
            // the framework (to preserve the captured drag), leaving
            // the body empty until the user released the thumb.
            let pane = body_pane::BodyPane::<T> {
                len_fn: self.len_fn.clone(),
                with_item_fn: self.with_item_fn.clone(),
                drag_fn: self.dnd.drag_fn.clone(),
                row_state_fn: self.dnd.row_state_fn.clone(),
                columns: self.columns.clone(),
                display_indices: self.display_indices.clone(),
                column_widths: self.column_widths.clone(),
                row_metrics: self.row_metrics.clone(),
                selection_mode: self.selection_mode,
                selection: self.row_selection.clone(),
                cell_selection: self.cell_selection.clone(),
                scroll_y: self.scroll_y.clone(),
                viewport_height: self.viewport_height.clone(),
                editing_cell: self.editing_cell.clone(),
                focused_cell: self.focused_cell.clone(),
                reorderable_rows: self.reorderable_rows,
                view_id: self.table_id,
                drag_anchor: ctx.self_id(),
                on_row_activate: self.on_row_activate.clone(),
                activate_on: self.activate_on,
                version: self.pane_version.clone(),
                prev_built_start: self.pane_built_start.clone(),
                prev_built_end: self.pane_built_end.clone(),
                total_refresh: self.pane_total_refresh.clone(),
                row_entries: Vec::new(),
            };
            self.body_pane_id = Some(ctx.add(pane));
        }

        // Scrollbar (single internal vertical bar).
        if self.show_internal_scrollbars {
            let sb = ScrollBar::new(
                ScrollBarOrientation::Vertical,
                self.scroll_y.clone(),
                self.max_scroll_y.clone(),
                self.viewport_ratio_y.clone(),
            )
            .visual(match self.scroll_bar_style {
                ScrollBarMode::Permanent => ScrollBarVisual::Permanent,
                ScrollBarMode::Overlay => ScrollBarVisual::Overlay,
                ScrollBarMode::Thin => ScrollBarVisual::Thin,
            });
            self.scrollbar_id = Some(ctx.add(sb));
        }

        // Z-order: body rows first, then empty/scrollbar, then header
        // last. The header band overlaps the top of the body region
        // when `scroll_y > 0` (rows positioned at `body_origin_y +
        // row_idx * row_h - scroll_y` can extend above
        // `body_origin_y` on overscroll). Painting the header last
        // means it sits on top of any row that bleeds into the
        // header band — without this fix, scrolled-out rows would
        // visibly draw over the header label.
        let mut children: Vec<WidgetId> = Vec::new();
        if let Some(id) = self.body_pane_id {
            children.push(id);
        }
        if let Some(id) = self.empty_id {
            children.push(id);
        }
        if let Some(id) = self.scrollbar_id {
            children.push(id);
        }
        if let Some(id) = self.header_row_id {
            children.push(id);
        }
        // Suppress the unused-binding warning on header_h while the
        // value is consumed by `place_children` via the same helper.
        let _ = header_h;
        children
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let width = proposal.width.unwrap_or(400.0);
        let height = proposal.height.unwrap_or(300.0);
        self.viewport_height.set(height);
        Size::new(width, height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        if children.is_empty() {
            return;
        }
        let rtl = ctx.is_rtl();
        let header_h = self.effective_header_height();
        let body_height = (bounds.height - header_h).max(0.0);

        // Parent-before-child layout order means this runs before the
        // body pane's measure pass — in auto-measure mode the scrollbar
        // totals settle one frame after a measurement change.
        let total_height = self.total_content_height();
        let max_y = (total_height - body_height).max(0.0);
        self.max_scroll_y.set(max_y);
        let ratio = if total_height > 0.0 {
            (body_height / total_height).clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.viewport_ratio_y.set(ratio);
        self.clamp_scroll();

        let needs_scrollbar = self.show_internal_scrollbars && total_height > body_height + 0.5;
        // Permanent reserves a column for the bar; Overlay / Thin float
        // over the content, so rows span the full width.
        let reserves_bar =
            needs_scrollbar && self.scroll_bar_style == ScrollBarMode::Permanent;
        let body_width = if reserves_bar {
            (bounds.width - SCROLLBAR_THICKNESS).max(0.0)
        } else {
            bounds.width
        };
        // Under RTL the vertical scrollbar moves to the physical left
        // (matching `ScrollArea`), so the body/header band shifts right
        // by its thickness. `band_left` is the shared origin for the
        // body pane, empty state, and header; `scrollbar_x` is the
        // scrollbar's own physical x. The paint pass derives the same
        // content region from these conventions so the two never drift.
        let band_left = if rtl && reserves_bar {
            bounds.x + SCROLLBAR_THICKNESS
        } else {
            bounds.x
        };
        let scrollbar_x = if rtl {
            bounds.x
        } else {
            bounds.x + bounds.width - SCROLLBAR_THICKNESS
        };
        // The header strip spans the band; snapshot its width for the
        // reorder-drop handler's RTL mirror.
        self.header_strip_width.set(body_width);

        // Resolve column widths in display order, honoring any
        // user-resize overrides from `column_widths_signal`.
        let overrides = self.column_widths_signal.get();
        let display = self.display_indices.borrow().clone();
        let widths = layout::ColumnSolver::resolve_in_order(
            &self.columns,
            &display,
            body_width,
            cp::MIN_COLUMN_WIDTH_DEFAULT,
            &overrides,
        );
        *self.column_widths.borrow_mut() = widths;
        let body_origin_y = bounds.y + header_h;

        let mut next = 0;

        // BodyPane fills the body region. It positions its rows
        // internally using its own scroll signal and clips them to
        // its own bounds.
        if self.body_pane_id.is_some() {
            if let Some(child) = children.get_mut(next) {
                child.origin = Point::new(band_left, body_origin_y);
                child.size = Size::new(body_width, body_height);
            }
            next += 1;
        }

        // Empty-state child fills the body region (below the header).
        if self.empty_id.is_some() {
            if let Some(child) = children.get_mut(next) {
                child.origin = Point::new(band_left, body_origin_y);
                child.size = Size::new(body_width, body_height);
            }
            next += 1;
        }

        // Scrollbar — alongside the body, below the header. Physical
        // left under RTL, physical right under LTR.
        if self.scrollbar_id.is_some() {
            if let Some(child) = children.get_mut(next) {
                if needs_scrollbar {
                    child.origin = Point::new(scrollbar_x, body_origin_y);
                    child.size = Size::new(SCROLLBAR_THICKNESS, body_height);
                } else {
                    child.origin = bounds.origin();
                    child.size = Size::ZERO;
                }
            }
            next += 1;
        }

        // Header strip last — placed at top y but emitted last so paint
        // z-order draws it above any overscrolled body rows.
        if self.header_row_id.is_some()
            && let Some(child) = children.get_mut(next)
        {
            child.origin = Point::new(band_left, bounds.y);
            child.size = Size::new(body_width, header_h);
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let header_h = self.effective_header_height();
        let colors = &ctx.theme.colors;

        let scroll_y = self.scroll_y.get();
        let body_origin_y = bounds.y + header_h;
        let body_height = (bounds.height - header_h).max(0.0);
        let widths = self.column_widths.borrow();
        let body_width = widths.iter().sum::<f32>();
        let body_width_for_paint = if body_width > 0.0 {
            body_width.min(bounds.width)
        } else {
            bounds.width
        };
        // Physical left edge of the column content. Under RTL the band is
        // right-aligned within `bounds` (the scrollbar took the left), so
        // content runs from `bounds.right() - body_width` leftward —
        // exactly where `place_children` reverse-placed the cells.
        let rtl = ctx.layout_direction == bastyde_core::environment::LayoutDirection::RightToLeft;
        let content_left = if rtl {
            bounds.x + bounds.width - body_width_for_paint
        } else {
            bounds.x
        };

        // Visible row window for the paint passes — offset-table-driven
        // so variable heights paint correctly. One metrics borrow per
        // pass; nothing inside re-enters the metrics.
        let row_count = (self.len_fn)();
        let (first_visible, last_visible) =
            self.row_metrics
                .borrow_mut()
                .visible_range(scroll_y, body_height, row_count, 0);

        // Clip the root-painted row decorations (alt-row stripes,
        // selection bands, grid lines, focus ring) to the body band.
        // `clips_children` only clips child WIDGETS — this widget's own
        // paint would otherwise bleed past the table's bottom edge for
        // the partially visible last row (its stripe/grid-line rect
        // spans the full row height).
        canvas.set_clip(Rect::new(
            content_left,
            body_origin_y,
            body_width_for_paint,
            body_height,
        ));

        // Alt-row backgrounds — paint odd visible rows. Parity keys on
        // the row index, not on y, so stripes stay stable under
        // variable heights.
        if self.alternating_rows {
            let mut m = self.row_metrics.borrow_mut();
            for row_idx in first_visible..last_visible {
                if row_idx % 2 == 1 {
                    let y = body_origin_y + m.row_top(row_idx) - scroll_y;
                    let h = m.row_height(row_idx);
                    let rect = Rect::new(content_left, y, body_width_for_paint, h);
                    canvas.fill_rect(rect, SurfaceRole::AltRow.resolve(colors));
                }
            }
        }

        // Selection highlights — row selection modes only.
        if let Some(ref sel) = self.row_selection
            && matches!(
                self.selection_mode,
                TableSelectionMode::SingleRow | TableSelectionMode::MultiRow
            )
        {
            // Focus-aware: active `Selected` while the table holds keyboard
            // focus, muted `SelectedInactive` once focus moves elsewhere.
            let bg = if self.view_focused.get() {
                SurfaceRole::Selected.resolve(colors)
            } else {
                SurfaceRole::SelectedInactive.resolve(colors)
            };
            let mut m = self.row_metrics.borrow_mut();
            for row_idx in sel.selected_indices() {
                let y = body_origin_y + m.row_top(row_idx) - scroll_y;
                let h = m.row_height(row_idx);
                if y + h < body_origin_y || y > body_origin_y + body_height {
                    continue;
                }
                let rect = Rect::new(content_left, y, body_width_for_paint, h);
                canvas.fill_rect(rect, bg);
            }
        }

        // Grid lines.
        let line_color = BorderRole::Divider.resolve(colors);
        let line_w = cp::GRID_LINE_THICKNESS.max(1.0);

        if matches!(self.grid_lines, GridLines::Horizontal | GridLines::Both) {
            let mut m = self.row_metrics.borrow_mut();
            for row_idx in first_visible..last_visible {
                let bottom = m.row_top(row_idx) + m.row_height(row_idx);
                let y = body_origin_y + bottom - scroll_y - line_w;
                let rect = Rect::new(content_left, y, body_width_for_paint, line_w);
                canvas.fill_rect(rect, line_color);
            }
        }

        if matches!(self.grid_lines, GridLines::Vertical | GridLines::Both) {
            let content_right = content_left + body_width_for_paint;
            if rtl {
                // Columns run right-to-left: accumulate from the right
                // edge and draw the divider at each column's (physical)
                // left boundary, skipping the outermost edge.
                let mut x = content_right;
                for &w in widths.iter() {
                    x -= w;
                    if x > content_left + 0.5 {
                        let rect = Rect::new(x, body_origin_y, line_w, body_height);
                        canvas.fill_rect(rect, line_color);
                    }
                }
            } else {
                let mut x = content_left;
                for &w in widths.iter() {
                    x += w;
                    if x < content_right - 0.5 {
                        let rect = Rect::new(x - line_w, body_origin_y, line_w, body_height);
                        canvas.fill_rect(rect, line_color);
                    }
                }
            }
        }

        // Focus ring on the currently-focused cell — keyboard-only
        // (`:focus-visible`) and only while the table itself holds focus, so a
        // mouse click never leaves a ring and an unfocused table shows none.
        if self.view_focused.get()
            && self.focus_visible.get()
            && let Some((focus_row, focus_col)) = self.focused_cell.get()
            && focus_col < widths.len()
        {
            let mut x_off = 0.0_f32;
            for &w in widths.iter().take(focus_col) {
                x_off += w;
            }
            let cell_w = widths[focus_col];
            let (focus_top, focus_h) = {
                let mut m = self.row_metrics.borrow_mut();
                (m.row_top(focus_row), m.row_height(focus_row))
            };
            let y = body_origin_y + focus_top - scroll_y;
            if y + focus_h >= body_origin_y && y <= body_origin_y + body_height {
                let inset = cp::FOCUS_RING_INSET;
                let stroke = cp::GRID_LINE_THICKNESS.max(1.5);
                let ring_color = BorderRole::Focused.resolve(colors);
                // `x_off` is the leading-side offset (sum of widths before
                // the focused column). Under RTL that offset is measured
                // from the right edge of the content band.
                let rx = if rtl {
                    content_left + body_width_for_paint - x_off - cell_w + inset
                } else {
                    content_left + x_off + inset
                };
                let ry = y + inset;
                let rw = (cell_w - inset * 2.0).max(0.0);
                let rh = (focus_h - inset * 2.0).max(0.0);
                // Top
                canvas.fill_rect(Rect::new(rx, ry, rw, stroke), ring_color);
                // Bottom
                canvas.fill_rect(Rect::new(rx, ry + rh - stroke, rw, stroke), ring_color);
                // Left
                canvas.fill_rect(Rect::new(rx, ry, stroke, rh), ring_color);
                // Right
                canvas.fill_rect(Rect::new(rx + rw - stroke, ry, stroke, rh), ring_color);
            }
        }

        // Row-drop insertion indicator (source-accepted positions only —
        // a forbidden hover clears the signal, so no line shows). `y` is
        // stored body-local; the band clip is already active.
        if let Some((y, _width)) = self.drop_feedback.get() {
            let line_color = BorderRole::Focused.resolve(colors);
            let thickness = 2.0_f32;
            let line_y = body_origin_y + y - thickness * 0.5;
            canvas.fill_rect(
                Rect::new(content_left, line_y, body_width_for_paint, thickness),
                line_color,
            );
        }

        canvas.clear_clip();

        // Container focus ring — the table holds keyboard focus but nothing
        // indicates where: no current cell (no cell ring) and no selection (no
        // band). Outline the whole view so Tab has a visible landing point
        // before the user navigates (mirrors TreeView / ListView).
        let nothing_indicated = self.focused_cell.get().is_none()
            && self
                .row_selection
                .as_ref()
                .map_or(true, |s| s.selected_indices().is_empty())
            && self.cell_selection.as_ref().map_or(true, |s| s.count() == 0);
        if self.view_focused.get() && self.focus_visible.get() && nothing_indicated {
            let inset = 1.0_f32;
            let rect = Rect::new(
                bounds.x + inset,
                bounds.y + inset,
                (bounds.width - inset * 2.0).max(0.0),
                (bounds.height - inset * 2.0).max(0.0),
            );
            canvas.stroke_rect(rect, BorderRole::Focused.resolve(colors), 1.5);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Table);
        if let Some(ref label) = self.a11y_label {
            builder.set_name(label.resolve_now());
        }
        // AccessKit's `row_count` includes the header row when present —
        // matches ARIA `aria-rowcount` semantics.
        let row_count = (self.len_fn)() + if self.show_header { 1 } else { 0 };
        let col_count = self.columns.len();
        let n = builder.inner_mut();
        n.set_row_count(row_count);
        n.set_column_count(col_count);
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn children(&self) -> Vec<WidgetId> {
        // Same order as `build()` — body pane first, header last so
        // it paints on top of any overscrolled rows.
        let mut out: Vec<WidgetId> = Vec::new();
        if let Some(id) = self.body_pane_id {
            out.push(id);
        }
        if let Some(id) = self.empty_id {
            out.push(id);
        }
        if let Some(id) = self.scrollbar_id {
            out.push(id);
        }
        if let Some(id) = self.header_row_id {
            out.push(id);
        }
        out
    }

    fn clips_children(&self) -> bool {
        true
    }
}

// ── Reorder drag-target plumbing ───────────────────────────────────────────

/// Attach `on_drag_hover` and `on_drop` to the header strip so reorder
/// drags from any cell of *this* table can be classified into a pane
/// (Leading / None / Trailing) and an insertion index.
///
/// Inter-table drops are rejected by matching `source_table_id`.
#[allow(clippy::too_many_arguments)]
fn attach_header_reorder_handlers(
    ctx: &mut BuildContext,
    header_row_id: WidgetId,
    source_table_id: usize,
    column_widths: Rc<RefCell<Vec<f32>>>,
    display_indices: Rc<RefCell<Vec<usize>>>,
    pane_boundaries: Rc<RefCell<PaneBoundaries>>,
    column_order_signal: Signal<Vec<String>>,
    column_pinning_signal: Signal<HashMap<String, PinnedSide>>,
    column_ids: Vec<String>,
    header_strip_width: Rc<Cell<f32>>,
) {
    let widths_for_drop = column_widths.clone();
    let display_for_drop = display_indices.clone();
    let panes_for_drop = pane_boundaries.clone();
    let order_for_drop = column_order_signal.clone();
    let pinning_for_drop = column_pinning_signal.clone();
    let ids_for_drop = column_ids;
    let strip_width_for_drop = header_strip_width;

    ctx.apply_handlers(
        header_row_id,
        HandlerSet::new()
            .on_drag_hover(|payload, _position, _ctx| {
                if payload.has_typed::<ColumnReorderDragData>() {
                    bastyde_core::DropFeedback::HighlightRect {
                        rect: bastyde_canvas::Rect::ZERO,
                        color: bastyde_tokens::Color::TRANSPARENT,
                    }
                } else {
                    bastyde_core::DropFeedback::NoFeedback
                }
            })
            .on_drop(move |mut payload, position, ctx| {
                let drag = match payload.take_typed::<ColumnReorderDragData>() {
                    Some(d) => d,
                    None => return false,
                };
                if drag.source_table_id != source_table_id {
                    return false;
                }
                let widths = widths_for_drop.borrow().clone();
                let display = display_for_drop.borrow().clone();
                let panes = *panes_for_drop.borrow();
                let total = display.len();
                if total == 0 {
                    return false;
                }

                // `position` is local to the header strip (origin at its
                // physical-left edge). Under RTL the columns are placed in
                // display order from the strip's right edge leftward, so
                // mirror the drop x against the strip width before running
                // the left-to-right scan. (A drop in any non-content dead
                // space then maps past the last column → append, matching
                // LTR's trailing-end behaviour.)
                let drop_x = if ctx.is_rtl() {
                    strip_width_for_drop.get() - position.x
                } else {
                    position.x
                };

                // Compute insertion index in display order: find the
                // first column whose midpoint exceeds the (mirrored) x.
                let mut x = 0.0;
                let mut insertion_display_idx = total;
                for (i, w) in widths.iter().enumerate() {
                    let mid = x + w * 0.5;
                    if drop_x < mid {
                        insertion_display_idx = i;
                        break;
                    }
                    x += w;
                }

                // Classify the drop position into a pane.
                let new_pinning = if insertion_display_idx <= panes.leading_count {
                    PinnedSide::Leading
                } else if insertion_display_idx >= panes.middle_end {
                    PinnedSide::Trailing
                } else {
                    PinnedSide::None
                };

                // Update pinning override (record only when it deviates
                // from None, which is the framework default).
                let mut pin_map = pinning_for_drop.get();
                match new_pinning {
                    PinnedSide::None => {
                        pin_map.remove(&drag.col_id);
                    }
                    other => {
                        pin_map.insert(drag.col_id.clone(), other);
                    }
                }
                pinning_for_drop.set(pin_map);

                // Rebuild the column-order list to reflect the drop.
                let mut new_order: Vec<String> =
                    display.iter().map(|&i| ids_for_drop[i].clone()).collect();
                let from_pos = new_order.iter().position(|id| id == &drag.col_id);
                if let Some(from) = from_pos {
                    let item = new_order.remove(from);
                    let to = if from < insertion_display_idx {
                        insertion_display_idx.saturating_sub(1)
                    } else {
                        insertion_display_idx
                    };
                    let to = to.min(new_order.len());
                    new_order.insert(to, item);
                    order_for_drop.set(new_order);
                }
                true
            }),
    );
}
