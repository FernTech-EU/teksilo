// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! ListView — a virtualized, scrollable list backed by a reactive data model.
//!
//! `ListView<T>` materializes widget subtrees only for the rows currently
//! visible in its viewport (plus a configurable buffer). Scrolling and model
//! changes trigger a localized rebuild that touches only the newly-visible
//! slice, leaving the rest of the tree untouched. The data source is a
//! `ListModel<T>` (in-memory, reactive) or any `ListDataSource<Item = T>`
//! (lazy / external). A delegate closure `(index, &T, selected) -> Box<dyn Widget>`
//! produces each row widget on demand.
//!
//! Row heights come in three modes: **uniform** (`item_height`, the 32 dp
//! default and fastest path), **exact callback** (`item_height_fn` — pure,
//! deterministic per-row sizes), and **auto-measured** (`auto_item_height` —
//! height-for-width measurement with scroll anchoring so content above the
//! viewport stays put while estimates converge).
//!
//! ## When to use
//!
//! - Large or dynamically-loaded lists (thousands of rows) — use `ListView`.
//! - Small, always-all-visible collections — use `Repeater` instead.
//! - Hierarchical data — use `TreeView`.
//! - Multi-column tabular data — use `TableView`.
//!
//! ## Accessibility
//!
//! The widget is `Role::ListBox`; each row is wrapped in
//! `Role::ListBoxOption` with `set_selected` state. Those are the interactive
//! ARIA roles — `listbox` / `option` — not the static `list` / `listitem` pair,
//! because this widget has keyboard navigation and selection.
//!
//! Each row publishes its 1-based `position_in_set` **in the model**, and the
//! container publishes the model's length as `size_of_set`, so a screen reader
//! says "row 147 of 200" rather than counting the realized window. The count
//! sits on the container because AccessKit resolves an item's set size by
//! walking up from it, unlike ARIA's per-item `aria-setsize`.
//!
//! The container is the focusable node and rows deliberately are not, so
//! `set_selected` is the only signal telling assistive technology which row is
//! current — and the row subtree is kept out of the Tab order, so a control the
//! delegate puts in a row (the checkbox `StandardListItem` embeds, most often)
//! never becomes a Tab stop of its own. Such a control publishes a keyboard
//! toggle instead, which `Space` runs. Full keyboard navigation: arrows, Home,
//! End, PageUp, PageDown (each moving the selection, or only the cursor when
//! the accelerator is held), Shift for a range and Ctrl+Shift for an additive
//! one, Space (checks the row when it carries a checkbox, else select/toggle),
//! Enter (activate), Ctrl+A / Ctrl+Shift+A (select all /
//! deselect), Ctrl+Arrow and Ctrl+Space (the disjoint-selection pair),
//! type-ahead (opt-in via `type_ahead_label`), and Shift+F10 or the Menu key
//! for the selected row's context menu. On macOS, Cmd+Down opens the focused
//! row. The chord table and its rationale are in
//! [docs/data-view-keyboard.md](https://github.com/ferntech-eu/teksilo/blob/main/docs/data-view-keyboard.md).
//!
//! ```rust
//! # use teksilo_widgets::ListView;
//! # use teksilo_widgets::primitives::TextWidget;
//! # use teksilo_data::{ListModel, SelectionMode, SelectionModel};
//! # use teksilo_i18n::lit;
//! # struct Item { name: String }
//! # let model: ListModel<Item> = ListModel::from_vec(vec![Item { name: "Alpha".into() }]);
//! # let sel = SelectionModel::new(SelectionMode::Single);
//! let _w = ListView::new(model, |_i, item, _selected| {
//!     Box::new(TextWidget::new(lit!(&item.name)))
//! })
//! .item_height(32.0)
//! .selection(sel);
//! ```

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_tokens::{BorderRole, Easing};

use teksilo_core::DropFeedback;
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::drag_payload::DragPayload;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget::{LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;

use teksilo_data::selection_model::SelectionModel;
use teksilo_data::{ItemKey, KeyedSelectionModel};

use crate::data_views::RowSelection;
use teksilo_data::{DataChange, DropPosition, DropResponse, ListModel};

// Qualified rather than glob-imported: `data_views::ViewKind` is already in
// scope here and means something else (which data view a drag came from).
use crate::common::list_nav;
use crate::common::row_metrics::{HeightSource, RowMetrics, SharedRowMetrics};
use crate::common::scroll::OverscrollBehavior;
use crate::data_views::{DragTransferMode, RowDragData, ViewId, ViewKind, flat_insertion_target};
use crate::list_source::ListSource;
use crate::scroll_area::ScrollBarMode;
use crate::scroll_bar::{ScrollBar, ScrollBarOrientation, ScrollBarVisual};

mod body_pane;

/// Default number of extra items to create above and below the viewport.
const BUFFER_ITEMS: usize = 5;
/// Default item height.
const DEFAULT_ITEM_HEIGHT: f32 = 32.0;
/// Scrollbar thickness.
const SCROLLBAR_THICKNESS: f32 = 12.0;

/// A virtualized scrollable list backed by a [`ListModel<T>`](teksilo_data::ListModel) or `ListDataSource`.
///
/// See the module-level documentation for the full feature overview.
pub struct ListView<T: 'static> {
    source: ListSource<T>,
    delegate: Rc<dyn Fn(usize, &T, bool) -> Box<dyn Widget>>,
    /// Per-row tooltip resolvers. Shared with `TreeView`; see
    /// [`RowTooltips`](crate::data_views::RowTooltips).
    row_tooltips: crate::data_views::RowTooltips<T>,
    item_height: f32,
    spacing: f32,
    /// Height-mode selection (uniform / exact callback / auto-measure).
    height_source: HeightSource,
    /// Row geometry — all virtualization consumers (visible range,
    /// placement, scrollbar totals, ensure-visible, DnD insertion) go
    /// through this. Shared handle: cloned into the scroll observer,
    /// keyboard and DnD closures.
    metrics: SharedRowMetrics,
    /// Row selection — index-based [`SelectionModel`] or keyed
    /// [`KeyedSelectionModel<K>`], unified behind the index-facing facade.
    row_selection: Option<RowSelection>,

    /// Keyboard-focused item index within the list.
    focused_index: Rc<Cell<Option<usize>>>,

    /// Shared (model index → row wrapper id) map, written by the body pane at
    /// the end of every build. Handed out by
    /// [`realized_row_ids`](Self::realized_row_ids) so a host that keeps focus
    /// elsewhere — a command palette whose focus stays in its search field —
    /// can point `active_descendant` at the highlighted row. Mirrors
    /// `GridView`'s `tile_map`.
    row_map: Rc<RefCell<Vec<(usize, WidgetId)>>>,

    /// Type-ahead ("type to jump") label extractor — opt-in via
    /// [`type_ahead_label`](Self::type_ahead_label). When set, typing a
    /// printable character jumps the selection to the next row whose label
    /// starts with the accumulated search term (Qt `keyboardSearch` /
    /// macOS type-select convention).
    type_ahead_label: Option<Rc<dyn Fn(&T) -> String>>,
    /// Reset window for the type-ahead search term.
    type_ahead_timeout: Duration,
    /// Persistent type-ahead buffer — a field (not built in `build`) so the
    /// accumulated term survives the selection-driven rebuild each
    /// keystroke triggers.
    type_ahead: Rc<crate::common::type_ahead::TypeAheadState>,

    /// Enable intra-widget drag reordering + keyboard Alt+Arrow.
    reorderable: bool,

    /// Whether to render an internal vertical scrollbar. When the
    /// caller wants the scrollbar outside the list — e.g. so it
    /// survives ListView rebuilds — this is disabled and the caller
    /// mounts their own, wired through `scroll_y_signal` /
    /// `max_scroll_y_signal` / `viewport_ratio_y_signal`.
    show_scrollbar: bool,

    // Persistent state (survives rebuild)
    scroll_y: Signal<f32>,
    max_scroll_y: Signal<f32>,
    /// Scroll-chaining behavior at the boundary (default `Chain`).
    overscroll_behavior: OverscrollBehavior,
    viewport_ratio_y: Signal<f32>,

    /// Animate wheel scrolling instead of snapping to the new offset.
    /// Enabled by default — mirrors `ScrollArea`.
    smooth_scrolling: bool,
    /// Duration of the smooth scroll animation.
    smooth_scroll_duration: Duration,

    /// How the scroll bar is displayed. Defaults to `Permanent` (reserves
    /// a layout column); `Overlay` / `Thin` float over the content.
    scroll_bar_style: ScrollBarMode,

    /// Active drop feedback (set by on_drag_hover, cleared by on_drag_leave,
    /// read by paint). Reactive Signal — bound at `RepaintOnly` so any
    /// `set(...)` call dirties the ListView for repaint automatically.
    drop_feedback: Signal<Option<(f32, f32)>>, // (y, width) for insertion line
    /// Content width (updated during place_children, used by drag feedback).
    placed_content_width: Rc<Cell<f32>>,

    /// Optional row-activation callback (a click per `activate_on`, or
    /// Enter/Space on the focused row) — distinct from *selection*, which also
    /// moves on arrow navigation.
    on_activate: Option<Rc<dyn Fn(usize, &mut teksilo_core::widget::EventContext)>>,
    /// Whether activation is a single or double click (default `DoubleClick`).
    activate_on: crate::data_views::ActivateOn,

    /// `true` while the view holds keyboard focus (root's inclusive
    /// [`BuildContext::view_focus_active`](teksilo_core::BuildContext::view_focus_active) signal). With `focus_visible`, drives
    /// the **container focus ring** shown when the view is Tab-focused but
    /// nothing is selected. Bound `RepaintOnly`.
    view_focused: Signal<bool>,
    /// Input-modality `:focus-visible` — gates the container ring to keyboard
    /// navigation. Bound `RepaintOnly`.
    focus_visible: Signal<bool>,

    /// Root-level **relayout** trigger. The root's own `place_children` owns
    /// the scrollbar totals (`max_scroll_y`, thumb ratio) and the
    /// content-width decision, none of which its `build` output depends on —
    /// so a data change or a pane measurement that moves the content total
    /// needs a re-place here, not a rebuild. Bumped by the data observer and
    /// by [`body_pane::ListBodyPane::total_refresh`].
    layout_refresh: Signal<u64>,
    /// Root-level **repaint** trigger for the container focus ring, which is
    /// suppressed as soon as anything is selected. Selection changes rebuild
    /// the pane (the delegate's `selected` argument) but must not rebuild the
    /// root — they only change what the root paints.
    paint_refresh: Signal<u64>,

    /// Pane-local rebuild trigger, owned here so it survives pane rebuilds.
    /// Bumped by the root's data observer, and by the pane itself on
    /// scroll-buffer exit, selection change and the post-measure realization
    /// re-check.
    pane_version: Signal<u64>,
    /// Buffered row range materialized by the pane's latest build.
    pane_built_start: Rc<Cell<usize>>,
    pane_built_end: Rc<Cell<usize>>,

    // Set during build
    body_pane_id: Option<WidgetId>,
    scrollbar_id: Option<WidgetId>,
    /// Shared so the on_drag_tick closure sees the current viewport
    /// height when edge-computing its auto-scroll delta. Plain `Cell<f32>`
    /// clones by value, which would leave the tick closure reading the
    /// 600 px default forever.
    viewport_height: Rc<Cell<f32>>,
    /// The ListView's own absolute (window) bounds, cached from
    /// `place_children`. The keyboard handler reads it to build the selected
    /// row's absolute rect and chase it into any *enclosing* scroll area via
    /// [`EventContext::ensure_visible`](teksilo_core::widget::EventContext::ensure_visible).
    /// Rows are not distinct focusable nodes (the view holds focus), so the
    /// framework's focus-driven follow never reveals the selected row in an
    /// outer scroller — this closes that gap.
    viewport_bounds: Rc<Cell<Rect>>,

    /// Stable, kind-tagged ID for this ListView instance (identifies its own
    /// reorder vs. a foreign drop, even across widget kinds / windows).
    model_id: ViewId,

    /// Cross-widget export / foreign-receive machinery — the builders
    /// (`.exportable`, `.export_external`, `.accept_foreign_rows`,
    /// `.on_rows_received`, `.on_rows_transferred_out`), the drag-start payload
    /// build, and the move-out completion, shared by all five data views.
    export: crate::data_views::RowExport<T>,

    /// Whole-view enabled state, statically or reactively. Forwarded to the
    /// arena via `ctx.enabled_when(self_id, self.enabled.clone())` at build
    /// time; `enabled_state` is the single source of truth — a disabled
    /// view greys out and stops accepting focus / selection / keyboard
    /// input (arena-gated).
    enabled: Prop<bool>,
}

impl<T: 'static> ListView<T> {
    /// Create a new ListView backed by a `ListModel<T>`.
    ///
    /// The `delegate` closure receives `(index, &item, selected)` and returns
    /// a boxed widget for that item.
    pub fn new(
        model: ListModel<T>,
        delegate: impl Fn(usize, &T, bool) -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self::create(ListSource::from_model(model), delegate)
    }

    /// Create a ListView backed by a custom `ListDataSource`.
    ///
    /// Use this for large or external datasets that cannot fit in memory.
    /// The source must implement `ListDataSource<Item = T>`.
    pub fn from_source<S: teksilo_data::ListDataSource<Item = T>>(
        source: S,
        delegate: impl Fn(usize, &T, bool) -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self::create(ListSource::from_data_source(source), delegate)
    }

    /// Create a ListView backed by a custom `ListDataSource` with **keyed**
    /// selection. The `KeyedSelectionModel<S::Key>` tracks selection by source
    /// identity, so it survives reorders, filters, lazy window-slides, and
    /// stays consistent across two views of the same source. The view stays
    /// key-less (`ListView<T>`) — the index↔key mapping is captured from the
    /// concrete source here. Mutually exclusive with
    /// [`selection`](Self::selection) (the last one set wins).
    pub fn from_source_keyed<S: teksilo_data::ListDataSource<Item = T>>(
        source: S,
        keyed: KeyedSelectionModel<S::Key>,
        delegate: impl Fn(usize, &T, bool) -> Box<dyn Widget> + 'static,
    ) -> Self
    where
        S::Key: ItemKey,
    {
        let s = Rc::new(source);
        let key_at = {
            let s = s.clone();
            Rc::new(move |i| s.key_at(i)) as Rc<dyn Fn(usize) -> Option<S::Key>>
        };
        let len = {
            let s = s.clone();
            Rc::new(move || s.len()) as Rc<dyn Fn() -> usize>
        };
        // Existence for prune: scan the (cheap, key-only) visible index space —
        // works for lazy sources too, where keys are known before items load.
        let contains = {
            let s = s.clone();
            Rc::new(move |k: &S::Key| (0..s.len()).any(|i| s.key_at(i).as_ref() == Some(k)))
                as Rc<dyn Fn(&S::Key) -> bool>
        };
        let row_selection = RowSelection::from_keyed(keyed, key_at, len, contains);
        let mut view = Self::create(ListSource::from_data_source_rc(s), delegate);
        view.row_selection = Some(row_selection);
        view
    }

    /// Create a ListView backed by a pre-built [`ListSource`]. Crate-
    /// internal entry point for consumers that already own an erased
    /// source (e.g. `ComboBox`'s `ItemSource` bridged through
    /// [`ListSource::from_cloning_accessors`]).
    pub(crate) fn from_list_source(
        source: ListSource<T>,
        delegate: impl Fn(usize, &T, bool) -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self::create(source, delegate)
    }

    fn create(
        source: ListSource<T>,
        delegate: impl Fn(usize, &T, bool) -> Box<dyn Widget> + 'static,
    ) -> Self {
        let model_id = ViewId::next(ViewKind::List);
        Self {
            model_id,
            export: crate::data_views::RowExport::default(),
            source,
            delegate: Rc::new(delegate),
            row_tooltips: Default::default(),
            item_height: DEFAULT_ITEM_HEIGHT,
            spacing: 0.0,
            height_source: HeightSource::Uniform,
            metrics: Rc::new(RefCell::new(RowMetrics::uniform(DEFAULT_ITEM_HEIGHT, 0.0))),
            row_selection: None,
            focused_index: Rc::new(Cell::new(None)),
            row_map: Rc::new(RefCell::new(Vec::new())),
            type_ahead_label: None,
            type_ahead_timeout: crate::common::type_ahead::DEFAULT_TYPE_AHEAD_TIMEOUT,
            type_ahead: crate::common::type_ahead::TypeAheadState::new(),
            reorderable: false,
            show_scrollbar: true,
            drop_feedback: Signal::new(None),
            // Replaced at build with the live tree signals.
            view_focused: Signal::new(false),
            focus_visible: Signal::new(false),
            placed_content_width: Rc::new(Cell::new(0.0)),
            on_activate: None,
            activate_on: crate::data_views::ActivateOn::default(),
            overscroll_behavior: OverscrollBehavior::default(),
            smooth_scrolling: true,
            smooth_scroll_duration: Duration::from_millis(150),
            scroll_bar_style: ScrollBarMode::Permanent,
            scroll_y: Signal::new_animated(0.0),
            max_scroll_y: Signal::new(0.0),
            viewport_ratio_y: Signal::new(1.0),
            layout_refresh: Signal::new(0_u64),
            paint_refresh: Signal::new(0_u64),
            pane_version: Signal::new(0_u64),
            pane_built_start: Rc::new(Cell::new(0)),
            pane_built_end: Rc::new(Cell::new(0)),
            body_pane_id: None,
            scrollbar_id: None,
            viewport_height: Rc::new(Cell::new(600.0)),
            viewport_bounds: Rc::new(Cell::new(Rect::ZERO)),
            enabled: Prop::Static(true),
        }
    }

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
    pub fn smooth_scrolling(mut self, enabled: bool) -> Self {
        self.smooth_scrolling = enabled;
        self
    }

    /// Duration of the smooth scroll animation (default 150 ms).
    pub fn smooth_scroll_duration(mut self, duration: Duration) -> Self {
        self.smooth_scroll_duration = duration;
        self
    }

    /// How the scroll bar is displayed (default `Permanent`). `Overlay`
    /// and `Thin` float the bar over the content instead of reserving a
    /// layout column, mirroring `ScrollArea::scroll_bar_style`.
    pub fn scroll_bar_style(mut self, style: ScrollBarMode) -> Self {
        self.scroll_bar_style = style;
        self
    }

    /// Re-materialize `self.metrics` after a height-mode / item-height /
    /// spacing builder call, keeping the three order-independent.
    fn remake_metrics(&self) {
        *self.metrics.borrow_mut() = self
            .height_source
            .make_metrics(self.item_height, self.spacing);
    }

    /// Set the fixed height per item (default 32.0) — the uniform fast
    /// path. Mutually exclusive with [`item_height_fn`](Self::item_height_fn)
    /// and [`auto_item_height`](Self::auto_item_height); the last mode
    /// setter wins.
    pub fn item_height(mut self, height: f32) -> Self {
        self.item_height = height;
        self.height_source = HeightSource::Uniform;
        self.remake_metrics();
        self
    }

    /// Per-item heights from a callback. The callback must be pure (same
    /// index + same data → same height); it is re-swept from the first
    /// changed index on every model change. No measurement pass runs —
    /// this is the deterministic variable-height path.
    pub fn item_height_fn(mut self, f: impl Fn(usize) -> f32 + 'static) -> Self {
        self.height_source = HeightSource::Exact(Rc::new(f));
        self.remake_metrics();
        self
    }

    /// Auto-measured item heights: each realized row is measured at the
    /// list's content width (height-for-width), unrealized rows assume
    /// `estimated`. Scroll anchoring keeps content above the viewport
    /// stationary as estimates are corrected. `estimated` should be a
    /// typical row height — a wrong estimate only costs realization
    /// churn while measurements settle, never incorrect layout.
    pub fn auto_item_height(mut self, estimated: f32) -> Self {
        self.height_source = HeightSource::Auto { estimated };
        self.remake_metrics();
        self
    }

    /// Set spacing between items (default 0.0).
    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self.remake_metrics();
        self
    }

    /// Set the index-based selection model (positions). For identity-based
    /// selection that survives reorder / filter / window-slide, build the view
    /// with [`from_source_keyed`](Self::from_source_keyed) instead.
    pub fn selection(mut self, sel: SelectionModel) -> Self {
        self.row_selection = Some(RowSelection::from_index(sel));
        self
    }

    /// Keep the row the keyboard is on inside the realized window.
    ///
    /// Only the rows near the viewport are realized, so a current row far from
    /// the scroll offset frequently has **no widget**. Everything that speaks
    /// for it then has nothing to speak about: no node carries `selected`,
    /// [`Self::current_row_widget`] resolves to `None` so no active descendant
    /// is nominated, and a screen reader is told nothing. The first arrow press
    /// steps *past* that row as well, because the cursor was somewhere nobody
    /// was shown.
    ///
    /// Two triggers, and the second is not redundant. Revealing only on focus
    /// misses the common case where the selection is made *from inside* a focus
    /// handler — a list that lands on "whatever is happening now" the first
    /// time it is reached does exactly that, so the reveal would run first,
    /// find nothing selected, and do nothing. Reacting to the selection as well
    /// covers that, and covers any later programmatic selection too.
    ///
    /// `ensure_index_visible` arithmetic rather than `scroll_to_index`: a row
    /// already on screen must not jump under somebody who can see it.
    ///
    /// The handles are cloned into the closures rather than reached through
    /// `self`, which they cannot borrow. A caller could not do this from
    /// outside in any case: the handles are private, and
    /// `with_widget_mut::<ListView<_>>` cannot reach the widget either, since
    /// this type overrides `as_any` and not `as_any_mut`.
    fn reveal_current_row_on_focus(&self, ctx: &mut teksilo_core::build_context::BuildContext) {
        let metrics = self.metrics.clone();
        let scroll_y = self.scroll_y.clone();
        let viewport_height = self.viewport_height.clone();
        let max_scroll_y = self.max_scroll_y.clone();
        let focused_index = self.focused_index.clone();
        let selection = self.row_selection.clone();

        let reveal: Rc<dyn Fn()> = Rc::new(move || {
            let Some(index) = focused_index.get().or_else(|| {
                selection
                    .as_ref()
                    .and_then(|s| s.selected_indices().first().copied())
            }) else {
                return;
            };
            let current = scroll_y.get();
            let target = metrics.borrow_mut().scroll_for_ensure_visible(
                index,
                current,
                viewport_height.get(),
                max_scroll_y.get(),
            );
            if (target - current).abs() > f32::EPSILON {
                scroll_y.set(target);
            }
        });

        let on_focus = reveal.clone();
        ctx.effect(&self.view_focused, move |focused| {
            if *focused {
                on_focus();
            }
        });

        if let Some(sel) = self.row_selection.as_ref() {
            let on_select = reveal;
            let focused = self.view_focused.clone();
            let handle = sel.observe_for_rebuild(move || {
                // Only while this view has focus. A selection driven from
                // elsewhere — a combobox highlighting rows in a list the user
                // is not in — must not scroll the view under them.
                if focused.get() {
                    on_select();
                }
            });
            ctx.own_handle(handle);
        }
    }

    /// A shared handle to the live `(model index → row node id)` map of the
    /// **realized** rows, rewritten at the end of every build.
    ///
    /// The id is the row's `Role::ListBoxOption` wrapper — the node an
    /// `active_descendant` has to point at. Take the handle before moving the
    /// view into the tree; it is populated on the first build.
    ///
    /// This exists for the ARIA combobox / listbox pattern, where keyboard
    /// focus stays on a *text field* while the arrow keys move a highlight
    /// through this list (a command palette, a type-ahead picker). The field's
    /// AT node publishes `active_descendant` pointing here, so a screen reader
    /// announces each row as the highlight moves without focus ever leaving
    /// the input.
    ///
    /// A `ListView` that holds focus itself does **not** need this handle: it
    /// publishes its own `active_descendant` from `accessibility()`, pointing
    /// at whichever row the keyboard is on. It used to publish none, on the
    /// assumption that holding focus was enough, and that assumption is what
    /// made every Teksilo list silent to NVDA.
    ///
    /// Only realized rows are present — a row scrolled outside the
    /// virtualization window has no widget, so look-ups for it return `None`.
    /// Callers should `scroll_to_index` the row they intend to announce.
    pub fn realized_row_ids(&self) -> Rc<RefCell<Vec<(usize, WidgetId)>>> {
        self.row_map.clone()
    }

    /// The realized row the keyboard is on: the navigation cursor when there
    /// is one, else the first selected row.
    ///
    /// `None` when that row is outside the virtualization window, which is the
    /// honest answer — there is no widget for it, so there is no node to point
    /// at and nothing on screen for a menu or an announcement to be about.
    fn current_row_widget(&self) -> Option<WidgetId> {
        let index = self.focused_index.get().or_else(|| {
            self.row_selection
                .as_ref()
                .and_then(|s| s.selected_indices().first().copied())
        })?;
        let map = self.row_map.borrow();
        map.iter()
            .find(|(i, _)| *i == index)
            .map(|(_, widget)| *widget)
    }

    /// Enable intra-widget drag reordering.
    ///
    /// When enabled, rows can be dragged within this ListView to reorder them.
    /// The move is routed through the source's `accept_drop` — a `ListModel`
    /// reorders in place, an external source routes the move to its store. The
    /// hover indicator reflects the source's `can_accept` verdict, so a
    /// forbidden drop shows no insertion line. Keyboard equivalent:
    /// Alt+ArrowUp/Down.
    pub fn reorderable(mut self, enabled: bool) -> Self {
        self.reorderable = enabled;
        self
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
    ///
    /// **Move caveats.** The row is removed only when the drop is accepted by an
    /// in-app target *in the same window* (`DropOutcome::InApp { accepted: true }`)
    /// or the OS reports a genuine move. Shipped OS backends advertise **copy
    /// only**, so a drag exported to another application — or to another window
    /// of the same app — is treated as a *copy*: the origin row is kept and the
    /// receiver must own its own copy semantics. Also, for a `ListModel`-backed
    /// view (whose key *is* the row index) the move-out removes by the indices
    /// captured at drag-start; if a shared handle to the same model is mutated
    /// while the drag is in flight, those indices can point at different rows —
    /// use a keyed source, or [`on_rows_transferred_out`](Self::on_rows_transferred_out)
    /// with your own stable identity, for models that change mid-drag.
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

    /// Set the row-**activation** handler — invoked with the flat row index and
    /// the live [`EventContext`](teksilo_core::widget::EventContext) on a click
    /// (per [`activate_on`](Self::activate_on)) or **Enter** on the focused row.
    /// The context lets the handler open a modal, toast, or dispatch an intent —
    /// matching [`TableView::on_row_activate`](crate::TableView::on_row_activate)
    /// / [`GridView::on_tile_activate`](crate::GridView::on_tile_activate).
    /// Distinct from *selection*: arrow-key navigation and **Space** move /
    /// toggle the selection but do **not** activate.
    pub fn on_activate(
        mut self,
        f: impl Fn(usize, &mut teksilo_core::widget::EventContext) + 'static,
    ) -> Self {
        self.on_activate = Some(Rc::new(f));
        self
    }

    /// Choose single- vs double-click activation (default
    /// [`ActivateOn::DoubleClick`](crate::ActivateOn)). Enter activates in
    /// either mode.
    pub fn activate_on(mut self, mode: crate::data_views::ActivateOn) -> Self {
        self.activate_on = mode;
        self
    }

    /// Enable **type-ahead** ("type to jump"): with this set, typing a
    /// printable character while the list has keyboard focus jumps the
    /// selection to the next row whose label starts with the accumulated
    /// search term, wrapping around (Qt `keyboardSearch` / macOS &
    /// Windows type-select). `label(&item)` yields the searchable text for
    /// a row; matching is ASCII-case-insensitive. A pause longer than the
    /// [`type_ahead_timeout`](Self::type_ahead_timeout) starts a fresh term.
    /// Whether a composite row tooltip offers dwell-to-sticky promotion.
    /// Default `true`.
    ///
    /// Turn it off for a read-only row card: with nothing to reach into there
    /// is nothing to pin, so the countdown indicator would promise an
    /// interaction that does not exist and the surface would outlive the
    /// pointer for no reason.
    pub fn row_tooltip_sticky(mut self, on: bool) -> Self {
        self.row_tooltips.set_composite_sticky(on);
        self
    }

    /// Per-row plain tooltip: one line of text for the row under the pointer.
    ///
    /// The resolver receives the row's flat index and its item; returning
    /// `None` leaves that row without a tip. Mutually exclusive with
    /// [`row_rich_tooltip`](Self::row_rich_tooltip) and
    /// [`row_composite_tooltip`](Self::row_composite_tooltip) — last setter
    /// wins, matching the per-widget tooltip matrix.
    ///
    /// Opens to the row's trailing side, never below it: rows stack
    /// vertically, so a tip below would cover the next row.
    pub fn row_tooltip(
        mut self,
        f: impl Fn(usize, &T) -> Option<teksilo_i18n::LocalizedString> + 'static,
    ) -> Self {
        self.row_tooltips.set_plain(f);
        self
    }

    /// Per-row rich tooltip — a registry key or inline
    /// [`TooltipContent`](crate::tooltip::TooltipContent). See
    /// [`row_tooltip`](Self::row_tooltip) for the shared semantics.
    pub fn row_rich_tooltip(
        mut self,
        f: impl Fn(usize, &T) -> Option<crate::tooltip::RichTooltipSource> + 'static,
    ) -> Self {
        self.row_tooltips.set_rich(f);
        self
    }

    /// Per-row composite tooltip — an arbitrary widget tree describing the row.
    ///
    /// The body is built for every **realized** row (the virtualization window)
    /// and rebuilt with it, so keep the resolver cheap and defer anything
    /// costly to the body's own first paint, which only runs if the tip is
    /// actually shown. See [`row_tooltip`](Self::row_tooltip) for the rest.
    pub fn row_composite_tooltip(
        mut self,
        f: impl Fn(usize, &T) -> Option<Box<dyn Widget>> + 'static,
    ) -> Self {
        self.row_tooltips.set_composite(f);
        self
    }

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

    /// Suppress the internal scroll bar. Use when the caller wants to
    /// mount its own `ScrollBar` outside the ListView (keeping it alive
    /// across rebuilds so a thumb drag isn't torn down when the visible
    /// range shifts past the buffer). The caller is expected to wire
    /// the external bar up to the signals returned by
    /// [`scroll_y_signal`](Self::scroll_y_signal),
    /// [`max_scroll_y_signal`](Self::max_scroll_y_signal) and
    /// [`viewport_ratio_y_signal`](Self::viewport_ratio_y_signal).
    pub fn show_scrollbar(mut self, show: bool) -> Self {
        self.show_scrollbar = show;
        self
    }

    /// Total content height (all items + spacing).
    fn total_content_height(&self) -> f32 {
        self.metrics.borrow_mut().total_height(self.source.len())
    }

    /// Compute the visible range of model indices for the current scroll and viewport.
    fn visible_range(&self) -> (usize, usize) {
        self.metrics.borrow_mut().visible_range(
            self.scroll_y.get(),
            self.viewport_height.get(),
            self.source.len(),
            BUFFER_ITEMS,
        )
    }

    /// The root's children, in the one order `build`, `children` and
    /// `place_children` all rely on: body pane first, scrollbar second.
    /// The pane is always mounted (an empty list realizes zero rows inside
    /// it), so the scrollbar's index only shifts with `show_scrollbar`.
    fn child_ids(&self) -> Vec<WidgetId> {
        [self.body_pane_id, self.scrollbar_id]
            .into_iter()
            .flatten()
            .collect()
    }

    /// Clamp scroll_y to valid range.
    fn clamp_scroll(&self) {
        let max = self.max_scroll_y.get();
        let current = self.scroll_y.get();
        let clamped = current.clamp(0.0, max);
        if (clamped - current).abs() > 0.001 {
            self.scroll_y.set(clamped);
        }
    }

    /// Test-only accessor: the reactive drop-feedback signal. `Some((y, w))`
    /// while a compatible drag hovers, `None` once the drag leaves or ends.
    #[cfg(test)]
    pub(crate) fn drop_feedback_signal(&self) -> &Signal<Option<(f32, f32)>> {
        &self.drop_feedback
    }

    /// The current vertical scroll offset, in logical pixels. Drives the
    /// viewport position and the scroll bar thumb. Exposed so external
    /// logic (e.g. a parent widget implementing custom scroll-into-view)
    /// can read or drive the scroll directly — prefer
    /// [`scroll_to_index`](Self::scroll_to_index) /
    /// [`ensure_index_visible`](Self::ensure_index_visible) when possible.
    pub fn scroll_y_signal(&self) -> &Signal<f32> {
        &self.scroll_y
    }

    /// The maximum scroll offset, `content_height - viewport_height`.
    /// Updated during layout. Exposed for callers that mount their own
    /// external scrollbar via [`show_scrollbar(false)`](Self::show_scrollbar).
    pub fn max_scroll_y_signal(&self) -> &Signal<f32> {
        &self.max_scroll_y
    }

    /// The vertical viewport-to-content ratio (0.0..1.0). Drives the
    /// thumb size on any external scrollbar.
    pub fn viewport_ratio_y_signal(&self) -> &Signal<f32> {
        &self.viewport_ratio_y
    }

    /// Scroll so the given model index is aligned to the top of the
    /// viewport. Clamped to the valid scroll range. Safe to call before
    /// the ListView has been laid out — the clamp will kick in on the
    /// first layout pass.
    pub fn scroll_to_index(&self, index: usize) {
        let target = self.metrics.borrow_mut().row_top(index);
        let max = self.max_scroll_y.get();
        self.scroll_y.set(target.clamp(0.0, max));
    }

    /// Scroll the minimum distance needed to bring the given model
    /// index fully into the viewport. No-op if already visible.
    pub fn ensure_index_visible(&self, index: usize) {
        let scroll = self.scroll_y.get();
        let new_scroll = self.metrics.borrow_mut().scroll_for_ensure_visible(
            index,
            scroll,
            self.viewport_height.get(),
            self.max_scroll_y.get(),
        );
        if (new_scroll - scroll).abs() > f32::EPSILON {
            self.scroll_y.set(new_scroll);
        }
    }
}

impl<T: 'static> std::fmt::Debug for ListView<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListView")
            .field("item_count", &self.source.len())
            .field("item_height", &self.item_height)
            .field("scroll_bar_style", &self.scroll_bar_style)
            .field("scroll_y", &self.scroll_y.get())
            .finish()
    }
}

impl<T: 'static> Widget for ListView<T> {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        // The root builds exactly two children — the body pane and the
        // scrollbar — and neither depends on the data, the selection or the
        // scroll offset. So it declares no `Rebuild`-level binding at all:
        // row realization is the pane's job (see `body_pane`'s module docs
        // for why that separation is load-bearing and not just tidy), and
        // what the root still owns resolves at `Relayout` / `RepaintOnly`.
        let self_id = ctx.self_id();
        ctx.enabled_when(self_id, self.enabled.clone());

        // Scrollbar totals + the content-width decision live in the root's
        // `place_children`; a data change or a pane measurement that moves
        // the content total re-places the root through this.
        self.layout_refresh.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );
        // Container focus ring: painted only while nothing is selected, so a
        // selection change has to reach the root's paint — without rebuilding
        // it and taking the scrollbar down with it.
        self.paint_refresh.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );

        // Bind scroll_y at Relayout so place_children runs on every scroll
        // position change (re-clamps and refreshes the thumb) without a
        // rebuild. The pane holds the matching binding for its rows.
        self.scroll_y.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );

        // Register animated signal for smooth scrolling. Deliberately the
        // ROOT and only the root: the scheduler keys an animation to the
        // widget that registered its signal last and cancels it when that
        // widget rebuilds, so registering from the pane too would make every
        // buffer-exit rebuild abort an in-flight fling.
        ctx.register_animated_signal(&self.scroll_y);

        // Bind drop_feedback at RepaintOnly so `set(...)` calls from
        // on_drag_hover / on_drag_leave dirty the ListView's paint cache
        // without triggering a rebuild.
        self.drop_feedback.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );

        // Focus signals for the container ring (see TreeView). `RepaintOnly` so
        // focus-in/out redraws; selection-emptiness changes arrive on
        // `paint_refresh`. `begin_view_focus` keys the scope signal on this root id directly,
        // independent of the arena focusable flag (not yet wired at this point):
        // a plain `view_focus_active()` would `find_focusable_at_or_above`
        // nothing and fall back to the constant-`true` "outside any scope"
        // signal — lighting the ring whenever ANY other widget takes keyboard
        // focus. Pop straight back; the real row scope below resolves the same
        // cached signal.
        self.view_focused = ctx.begin_view_focus();
        ctx.end_view_focus();
        self.focus_visible = ctx.focus_visible();
        self.reveal_current_row_on_focus(ctx);
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

        // --- Observe model changes ---
        // One observer, root-owned, doing the bookkeeping the pane can't
        // (metrics divergence, selection shift, keyboard cursor) and then
        // fanning out: rebuild the pane (row content changed) and re-place
        // the root (the content total, hence the thumb, changed).
        let pane_version_for_data = self.pane_version.clone();
        let layout_refresh_for_data = self.layout_refresh.clone();
        let data_ver = Rc::new(Cell::new(0_u64));
        let data_handle = (self.source.observe_fn)(Box::new({
            let dv = data_ver.clone();
            let metrics = self.metrics.clone();
            let len_fn = self.source.len_fn.clone();
            let first_changed = self.source.first_changed_fn.clone();
            let row_sel = self.row_selection.clone();
            let focused = self.focused_index.clone();
            move |change| {
                // Keep row metrics in step with the data: rows before
                // the first changed index keep their (seeded or
                // measured) heights, the rest re-derive.
                let divergence = match change {
                    DataChange::ItemsInserted { range } | DataChange::ItemsRemoved { range } => {
                        Some(range.start)
                    }
                    DataChange::ItemUpdated { index } => Some(*index),
                    DataChange::ItemsMoved { from, to, .. } => Some((*from).min(*to)),
                    // A lazy window load makes rows from range.start onward differ.
                    DataChange::WindowLoaded { range } => Some(range.start),
                    // Reset-emitting proxies (SortFilterListModel) expose
                    // their real divergence through the side-channel.
                    DataChange::Reset => (first_changed)(),
                };
                metrics
                    .borrow_mut()
                    .apply_divergence(divergence, (len_fn)());
                // Keep selection in step: index-shift (index model) or prune
                // orphaned keys (keyed model).
                if let Some(ref rs) = row_sel {
                    rs.on_data_change(change);
                }
                // Keep the keyboard-navigation anchor in step too — otherwise
                // it silently points at the wrong row after any insert /
                // remove / move (reachable not just from local edits but
                // from a live watcher pushing in a peer process's write).
                if let Some(current) = focused.get() {
                    focused.set(teksilo_data::data_change::adjust_single_index_for_change(
                        current, change,
                    ));
                }
                let next = dv.get() + 1;
                dv.set(next);
                pane_version_for_data.set(next);
                layout_refresh_for_data.set(next);
            }
        }));
        ctx.own_handle(data_handle);

        // --- Observe selection changes ---
        // The pane runs its own selection observer for the delegate's
        // `selected` argument; the root only needs its container focus ring
        // repainted, since that ring is suppressed once anything is selected.
        if let Some(ref rs) = self.row_selection {
            let paint_refresh_for_sel = self.paint_refresh.clone();
            let sel_ver = Rc::new(Cell::new(0_u64));
            let handle = rs.observe_for_rebuild(move || {
                let next = sel_ver.get() + 1;
                sel_ver.set(next);
                paint_refresh_for_sel.set(next);
            });
            ctx.own_handle(handle);
        }

        // Scroll-buffer exit is deliberately NOT observed here. It rebuilds
        // the body pane and nothing else — the root's own children are
        // unaffected by which rows are realized, and a root rebuild during a
        // scrollbar thumb drag is exactly the one the framework defers.

        // --- Set up scroll event handler + DnD handlers on self ---
        let scroll_y = self.scroll_y.clone();
        let max_scroll = self.max_scroll_y.clone();
        let line_height = self.item_height;
        let overscroll_behavior = self.overscroll_behavior;
        let smooth_scrolling = self.smooth_scrolling;
        let smooth_scroll_duration = self.smooth_scroll_duration;
        let mut handlers = HandlerSet::new()
            .on_scroll(move |event, _ctx| match event {
                teksilo_core::event::WidgetEvent::Scroll { delta, .. } => {
                    let dy = match delta {
                        teksilo_core::event::ScrollDelta::Lines { y, .. } => y * line_height,
                        teksilo_core::event::ScrollDelta::Pixels { y, .. } => *y,
                    };
                    let current = scroll_y.get();
                    let max = max_scroll.get();
                    // Base off the animation target so successive notches
                    // accumulate instead of restarting mid-animation.
                    let base = scroll_y.animation_target().unwrap_or(current);
                    let (new_y, moved) = crate::common::scroll::scroll_clamp_axis(base, dy, max);
                    if moved {
                        if smooth_scrolling {
                            scroll_y.animate_to(new_y, smooth_scroll_duration, Easing::EaseOut);
                        } else {
                            scroll_y.set(new_y);
                        }
                    }
                    // Chain to an ancestor scrollable when fully clamped
                    // (unless Contain), otherwise consume.
                    crate::common::scroll::scroll_response(
                        moved,
                        overscroll_behavior == OverscrollBehavior::Contain,
                    )
                }
                _ => teksilo_core::event::EventResponse::Ignored,
            })
            .clips_children(true)
            .focusable(true);

        // --- Keyboard navigation + Alt+Arrow reorder ---
        {
            let len_for_key = self.source.len_fn.clone();
            let accept_drop_for_key = self.source.dnd.accept_drop_fn.clone();
            let stash_for_key = self.source.dnd.stash_drag_keys_fn.clone();
            let view_id_for_key = self.model_id;
            let sel_for_key = self.row_selection.clone();
            let activate_key = self.on_activate.clone();
            let fi = self.focused_index.clone();
            let reorderable = self.reorderable;
            let scroll_for_nav = self.scroll_y.clone();
            let metrics_for_nav = self.metrics.clone();
            let max_for_nav = self.max_scroll_y.clone();
            let vh_for_nav = self.viewport_height.clone();
            let vb_for_nav = self.viewport_bounds.clone();
            // Type-ahead state + label resolver (reads row text via the
            // source's string accessor, so lazy/unloaded rows are skipped).
            let ta_state = self.type_ahead.clone();
            // Index → realized row id, so `Space` can ask the row whether it
            // publishes a keyboard toggle (a checkbox) before falling back to
            // the selection.
            let row_map_for_key = self.row_map.clone();
            let ta_label = self.type_ahead_label.clone();
            let ta_timeout = self.type_ahead_timeout;
            let with_item_str = self.source.with_item_str_fn.clone();

            handlers = handlers.on_key(move |event, ctx| {
                if let teksilo_core::event::WidgetEvent::KeyDown { key, modifiers, .. } = event {
                    use teksilo_core::event::Key;
                    let count = (len_for_key)();
                    if count == 0 {
                        return teksilo_core::event::EventResponse::Ignored;
                    }

                    // Select all — Ctrl+A, ⌘A on macOS (Multi selection only;
                    // a no-op for Single / None, matching every list control).
                    // With Shift it deselects instead: GTK is the only toolkit
                    // that *mandates* Ctrl+Shift+A, but the ARIA listbox and
                    // tree patterns both sanction an unselect-all, Windows and
                    // Qt simply have none, and adding it takes nothing away.
                    if modifiers.command() && matches!(key, Key::A) {
                        if let Some(ref sel) = sel_for_key
                            && sel.mode() == teksilo_data::SelectionMode::Multi
                        {
                            if modifiers.shift() {
                                sel.clear();
                            } else {
                                sel.select_all(count);
                            }
                            return teksilo_core::event::EventResponse::Handled;
                        }
                        return teksilo_core::event::EventResponse::Ignored;
                    }

                    // macOS reads a couple of chords in a list that the other
                    // desktops spend elsewhere, and both are dead here
                    // otherwise. ⌘↓ opens the row — Finder's "Command–Down
                    // Arrow: Open the selected item", and what VS Code binds as
                    // `list.select`'s macOS secondary. (⌘↑ ascends to the
                    // parent, which a flat list has none of; `TreeView` claims
                    // it.) Off macOS this resolves to `None` and costs nothing.
                    if let Some(alias) = list_nav::mac_alias(*key, *modifiers, ctx.is_rtl()) {
                        if alias == list_nav::MacAlias::Activate {
                            let row = fi
                                .get()
                                .or_else(|| {
                                    sel_for_key
                                        .as_ref()
                                        .and_then(|s| s.selected_indices().first().copied())
                                })
                                .unwrap_or(0)
                                .min(count - 1);
                            if let Some(ref sel) = sel_for_key {
                                sel.select(row);
                            }
                            if let Some(ref cb) = activate_key {
                                cb(row, ctx);
                            }
                            return teksilo_core::event::EventResponse::Handled;
                        }
                        return teksilo_core::event::EventResponse::Ignored;
                    }

                    // Type-ahead: a printable char (no Ctrl/Alt/Super) jumps the
                    // selection to the next row whose label starts with the
                    // accumulated term. Opt-in via `type_ahead_label`.
                    if ta_label.is_some()
                        && !modifiers.ctrl()
                        && !modifiers.alt()
                        && !modifiers.super_key()
                        && let Some(c) = key.to_char()
                    {
                        let current = fi.get().unwrap_or(0).min(count - 1);
                        let label = ta_label.as_ref().unwrap();
                        if let Some(idx) = ta_state.search(c, current, count, ta_timeout, |i| {
                            (with_item_str)(i, &|item| label(item))
                        }) {
                            fi.set(Some(idx));
                            if let Some(ref sel) = sel_for_key {
                                sel.select(idx);
                            }
                            let scroll = scroll_for_nav.get();
                            let new_scroll =
                                metrics_for_nav.borrow_mut().scroll_for_ensure_visible(
                                    idx,
                                    scroll,
                                    vh_for_nav.get(),
                                    max_for_nav.get(),
                                );
                            if (new_scroll - scroll).abs() > f32::EPSILON {
                                scroll_for_nav.set(new_scroll);
                            }
                            crate::common::row_metrics::chase_row_into_outer_view(
                                ctx,
                                &metrics_for_nav,
                                vb_for_nav.get(),
                                idx,
                                new_scroll,
                            );
                            return teksilo_core::event::EventResponse::Handled;
                        }
                        return teksilo_core::event::EventResponse::Ignored;
                    }

                    // Alt+Arrow: reorder via the source's accept_drop (when
                    // reorderable). The move is expressed as a synthetic
                    // same-view RowDragData so it travels exactly the same
                    // source-owned path as a pointer drop.
                    if modifiers.alt() && reorderable {
                        let selected_idx = sel_for_key
                            .as_ref()
                            .and_then(|s| s.selected_indices().first().copied());
                        if let Some(idx) = selected_idx {
                            let mv = match key {
                                teksilo_core::event::Key::ArrowUp if idx > 0 => {
                                    Some((idx - 1, DropPosition::Before, idx - 1))
                                }
                                teksilo_core::event::Key::ArrowDown if idx + 1 < count => {
                                    Some((idx + 1, DropPosition::After, idx + 1))
                                }
                                _ => None,
                            };
                            if let Some((target, position, dest)) = mv {
                                // Synthetic same-view payloads must stash the
                                // dragged row's key at construction — the
                                // accept path resolves identity from the
                                // stash, never from `rows`.
                                (stash_for_key)(&[idx]);
                                let payload = DragPayload::typed(RowDragData::<T> {
                                    source: view_id_for_key,
                                    rows: vec![idx],
                                    items: None,
                                });
                                if (accept_drop_for_key)(
                                    &payload,
                                    target,
                                    position,
                                    view_id_for_key,
                                ) {
                                    if let Some(ref sel) = sel_for_key {
                                        sel.select(dest);
                                    }
                                    fi.set(Some(dest));
                                    // Reveal the moved row (own viewport first,
                                    // then chain to any enclosing scroll area).
                                    let scroll = scroll_for_nav.get();
                                    let new_scroll =
                                        metrics_for_nav.borrow_mut().scroll_for_ensure_visible(
                                            dest,
                                            scroll,
                                            vh_for_nav.get(),
                                            max_for_nav.get(),
                                        );
                                    if (new_scroll - scroll).abs() > f32::EPSILON {
                                        scroll_for_nav.set(new_scroll);
                                    }
                                    crate::common::row_metrics::chase_row_into_outer_view(
                                        ctx,
                                        &metrics_for_nav,
                                        vb_for_nav.get(),
                                        dest,
                                        new_scroll,
                                    );
                                }
                                return teksilo_core::event::EventResponse::Handled;
                            }
                        }
                    }

                    // Navigation keys (no modifiers or with Shift for extend)
                    //
                    // The cursor is `focused_index` once the user has navigated
                    // or clicked; failing that it is the current selection — a
                    // view can be handed a selected row before it is ever
                    // focused (a launcher preselecting the top entry, a dialog
                    // restoring the last choice), and the keyboard must continue
                    // from what the user can see, not from an invisible zero.
                    //
                    // `None` ("no cursor yet") is deliberately NOT the same as
                    // `Some(0)`: from nothing, Down must land ON the first row
                    // and Up on the last one. Stepping to row 1 instead would
                    // silently skip row 0 — the row the user was looking at —
                    // which is what every toolkit (GTK, Qt, macOS, the ARIA
                    // listbox pattern) explicitly avoids.
                    let cursor = fi
                        .get()
                        .or_else(|| {
                            sel_for_key
                                .as_ref()
                                .and_then(|s| s.selected_indices().first().copied())
                        })
                        .map(|i| i.min(count - 1));
                    // Anchor for the keys that need a row to compute *from*
                    // (paging, activation) rather than a direction to step in.
                    let current = cursor.unwrap_or(0);
                    // The edge-and-page family is resolved once, in
                    // `common::list_nav`, so the five data views cannot drift
                    // apart on it again. A flat list has no row to be scoped
                    // to, so `RowFirst` / `RowLast` never arrive here — but a
                    // list's row *is* the collection, so they read the same way
                    // if the view kind is ever widened.
                    let nav = list_nav::nav_chord(*key, *modifiers, list_nav::ViewKind::Linear);
                    let new_idx = if let Some(chord) = nav {
                        Some(match chord.movement {
                            list_nav::NavMove::First | list_nav::NavMove::RowFirst => 0,
                            list_nav::NavMove::Last | list_nav::NavMove::RowLast => count - 1,
                            // Geometry-driven, so variable and auto-measured
                            // heights page by visual distance rather than by a
                            // fixed row count; the ensure-visible below then
                            // scrolls to follow.
                            list_nav::NavMove::Page { down } => {
                                let vh = vh_for_nav.get();
                                let r = {
                                    let mut m = metrics_for_nav.borrow_mut();
                                    m.resize(count);
                                    let target = if down {
                                        m.row_top(current) + vh
                                    } else {
                                        (m.row_top(current) - vh).max(0.0)
                                    };
                                    m.row_at(target)
                                };
                                // Guarantee progress even when one row is
                                // taller than the whole viewport.
                                if r == current && down {
                                    (current + 1).min(count - 1)
                                } else if r == current {
                                    current.saturating_sub(1)
                                } else {
                                    r.min(count - 1)
                                }
                            }
                        })
                    } else {
                        match key {
                            Key::ArrowDown => Some(match cursor {
                                None => 0,
                                Some(c) => (c + 1).min(count - 1),
                            }),
                            Key::ArrowUp => Some(match cursor {
                                None => count - 1,
                                Some(c) => c.saturating_sub(1),
                            }),
                            Key::Enter => {
                                // Enter activates the focused row (open / commit).
                                if let Some(ref sel) = sel_for_key {
                                    sel.select(current);
                                }
                                if let Some(ref cb) = activate_key {
                                    cb(current, ctx);
                                }
                                return teksilo_core::event::EventResponse::Handled;
                            }
                            Key::Space if modifiers.ctrl() => {
                                // Ctrl+Space toggles the focused row's selection —
                                // the keyboard equivalent of Ctrl+click. Distinct
                                // from plain Space below: it always toggles (even
                                // in Single mode, via `SelectionModel::toggle`'s
                                // own Single-mode fallback to `select`), pairing
                                // with Ctrl+Arrow's cursor-only move so a user can
                                // walk the cursor without disturbing the existing
                                // selection, then Ctrl+Space to add rows one at a
                                // time.
                                //
                                // Both halves stay on literal `ctrl()`, macOS
                                // included: ⌘Space is Spotlight and never reaches
                                // an app, and ⌘↑/⌘↓ already mean something else in
                                // a Finder list. This Explorer-style cursor pair
                                // has no ⌘ counterpart, so Control keeps it
                                // reachable and out of the platform's way.
                                if let Some(ref sel) = sel_for_key {
                                    sel.toggle(current);
                                }
                                fi.set(Some(current));
                                return teksilo_core::event::EventResponse::Handled;
                            }
                            Key::Space => {
                                // A row carrying a checkbox reads Space as
                                // "check this" — what Windows does for a
                                // checkbox list view, and what a visible
                                // checkbox looks like it should answer to. The
                                // row's control is out of the Tab order, so
                                // this is its only keyboard route; Ctrl+Space
                                // above keeps toggling the *selection*.
                                //
                                // Rows without a checkbox are unaffected:
                                // there is no published toggle, so Space falls
                                // through to the selection as before.
                                if let Some(row_id) = row_map_for_key
                                    .borrow()
                                    .iter()
                                    .find(|(i, _)| *i == current)
                                    .map(|(_, id)| *id)
                                {
                                    let sel_fallback = sel_for_key.clone();
                                    ctx.row_space_activate(
                                        row_id,
                                        std::rc::Rc::new(move || {
                                            if let Some(ref sel) = sel_fallback {
                                                if sel.mode() == teksilo_data::SelectionMode::Multi
                                                {
                                                    sel.toggle(current);
                                                } else {
                                                    sel.select(current);
                                                }
                                            }
                                        }),
                                    );
                                    fi.set(Some(current));
                                    return teksilo_core::event::EventResponse::Handled;
                                }
                                // Otherwise Space moves/toggles the selection but
                                // does NOT activate — the platform convention
                                // (Enter is the activator). Multi: toggle the
                                // focused row; Single: select it.
                                if let Some(ref sel) = sel_for_key {
                                    if sel.mode() == teksilo_data::SelectionMode::Multi {
                                        sel.toggle(current);
                                    } else {
                                        sel.select(current);
                                    }
                                }
                                fi.set(Some(current));
                                return teksilo_core::event::EventResponse::Handled;
                            }
                            _ => None,
                        }
                    };

                    if let Some(idx) = new_idx {
                        fi.set(Some(idx));
                        // What the chord does to the selection. The edge-and-page
                        // keys carry their own answer from `list_nav`, where the
                        // accelerator means "move the cursor, leave the selection
                        // alone" — the rule GTK4 and Qt both apply to *every*
                        // navigation key.
                        //
                        // The arrows keep reading literal `ctrl()` instead: ⌘↑/⌘↓
                        // already mean something else in a Finder list (see the
                        // Ctrl+Space arm above), so this pair has no ⌘ counterpart
                        // to move to. That asymmetry is deliberate — it is also
                        // what leaves ⌘↑/⌘↓ free for the macOS aliases.
                        let op = match nav {
                            Some(chord) => chord.selection,
                            None if modifiers.ctrl()
                                && !modifiers.shift()
                                && matches!(key, Key::ArrowUp | Key::ArrowDown) =>
                            {
                                list_nav::SelectionOp::Suppress
                            }
                            None if modifiers.shift() => list_nav::SelectionOp::Extend,
                            None => list_nav::SelectionOp::Replace,
                        };
                        if let Some(ref sel) = sel_for_key {
                            match op {
                                list_nav::SelectionOp::Replace => sel.select(idx),
                                list_nav::SelectionOp::Suppress => {}
                                list_nav::SelectionOp::Extend => sel.extend_to(idx),
                                list_nav::SelectionOp::ExtendAdditive => {
                                    sel.extend_to_additive(idx)
                                }
                            }
                        }
                        // Scroll into view — the ListView's own viewport first,
                        // then chain to any enclosing scroll area.
                        let scroll = scroll_for_nav.get();
                        let new_scroll = metrics_for_nav.borrow_mut().scroll_for_ensure_visible(
                            idx,
                            scroll,
                            vh_for_nav.get(),
                            max_for_nav.get(),
                        );
                        if (new_scroll - scroll).abs() > f32::EPSILON {
                            scroll_for_nav.set(new_scroll);
                        }
                        crate::common::row_metrics::chase_row_into_outer_view(
                            ctx,
                            &metrics_for_nav,
                            vb_for_nav.get(),
                            idx,
                            new_scroll,
                        );
                        return teksilo_core::event::EventResponse::Handled;
                    }
                }
                teksilo_core::event::EventResponse::Ignored
            });
        }

        // --- DnD: register self as a drop target when it can reorder OR accept
        // foreign rows. The source's `can_accept` decides per-hover whether the
        // drop is allowed (and a forbidden verdict shows no insertion line). ---
        if self.export.is_drop_target(self.reorderable) {
            let metrics_for_hover = self.metrics.clone();
            let scroll_for_hover = self.scroll_y.clone();
            let len_for_hover = self.source.len_fn.clone();
            let can_accept_for_hover = self.source.dnd.can_accept_fn.clone();
            let my_view_id = self.model_id;

            let feedback_for_hover = self.drop_feedback.clone();
            let width_for_hover = self.placed_content_width.clone();
            let export_for_hover = self.export.clone();
            handlers = handlers.on_drag_hover(move |payload, position, _ctx| {
                let scroll = scroll_for_hover.get().max(0.0);
                let content_y = position.y + scroll;
                let len = (len_for_hover)();
                let (insertion_y, ins) = {
                    let mut m = metrics_for_hover.borrow_mut();
                    m.resize(len);
                    let ins = m.insertion_index(content_y);
                    (m.row_top(ins) - scroll, ins)
                };
                let line_width = width_for_hover.get();
                // Ask the source whether a drop here is allowed; paint the
                // insertion line only when it is. A foreign exported row is
                // allowed when `accept_foreign_rows` is on even though a bare
                // `ListModel`'s `can_accept` rejects the `Foreign` branch.
                let allowed = flat_insertion_target(ins, len).is_some_and(|(target, pos)| {
                    !matches!(
                        (can_accept_for_hover)(payload, target, pos, my_view_id),
                        DropResponse::Reject
                    ) || export_for_hover.accepts_foreign_export(payload, my_view_id)
                });
                if allowed {
                    feedback_for_hover.set(Some((insertion_y, line_width)));
                    DropFeedback::InsertionLine {
                        y: insertion_y,
                        width: line_width,
                    }
                } else {
                    feedback_for_hover.set(None);
                    DropFeedback::NoFeedback
                }
            });

            let len_for_drop = self.source.len_fn.clone();
            let accept_drop_for_drop = self.source.dnd.accept_drop_fn.clone();
            let drop_view_id = self.model_id;
            let scroll_for_drop = self.scroll_y.clone();
            let metrics_for_drop = self.metrics.clone();
            let export_for_drop = self.export.clone();
            let reorderable_for_drop = self.reorderable;

            handlers = handlers.on_drop(move |mut payload, position, ctx| {
                let scroll = scroll_for_drop.get().max(0.0);
                let content_y = position.y + scroll;
                let len = (len_for_drop)();
                let ins = {
                    let mut m = metrics_for_drop.borrow_mut();
                    m.resize(len);
                    m.insertion_index(content_y)
                };
                let is_same_view = payload
                    .get_typed::<RowDragData<T>>()
                    .is_some_and(|rd| rd.source == drop_view_id);
                // A same-view reorder only happens when the view is
                // `reorderable`; a foreign payload is the source's call (a bare
                // ListModel rejects it).
                if (reorderable_for_drop || !is_same_view)
                    && let Some((target, position_kind)) = flat_insertion_target(ins, len)
                    && (accept_drop_for_drop)(&payload, target, position_kind, drop_view_id)
                {
                    if is_same_view {
                        export_for_drop.note_self_reorder();
                    }
                    return true;
                }
                // Otherwise, the shared foreign-receive sugar (peek-before-take).
                export_for_drop.foreign_receive(&mut payload, drop_view_id, ins, ctx)
            });

            // Clear the insertion line whenever the drag leaves this
            // widget — pointer moves to another target, drop completes,
            // Escape cancels, or the source is destroyed.
            let feedback_for_leave = self.drop_feedback.clone();
            handlers = handlers.on_drag_leave(move |_ctx| {
                feedback_for_leave.set(None);
            });

            // Per-frame auto-scroll when the pointer lingers within
            // 32 px of the viewport top or bottom edge during a drag.
            // Linear ramp inside the edge zone, capped at ~12 px/frame
            // so fast-moving fingers still feel responsive but don't
            // rocket past the content.
            let scroll_for_tick = self.scroll_y.clone();
            let max_scroll_for_tick = self.max_scroll_y.clone();
            let viewport_for_tick = self.viewport_height.clone();
            handlers = handlers.on_drag_tick(move |pos, _ctx| {
                const EDGE: f32 = 32.0;
                const MAX_VELOCITY: f32 = 12.0;
                let h = viewport_for_tick.get();
                let above = (EDGE - pos.y).max(0.0);
                let below = (pos.y - (h - EDGE)).max(0.0);
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
            });
        }

        // Export completion (move-out): fires on the drag source — this view's
        // root id, the stable id start_drag was given.
        handlers = self.export.install_completion(handlers);

        ctx.apply_self_handlers(handlers);

        // --- Body pane ---
        // Hoisted into its own widget so that scroll-buffer-exit rebuilds
        // (which happen mid-thumb-drag once the user scrolls past the
        // buffered range) target a SIBLING of the scrollbar rather than the
        // scrollbar's ancestor. Rebuilding the ancestor would be deferred by
        // the framework to preserve the captured drag, leaving the list blank
        // until the user released the thumb. See `body_pane`'s module docs.
        let pane = body_pane::ListBodyPane::<T> {
            source: self.source.clone(),
            delegate: self.delegate.clone(),
            row_tooltips: self.row_tooltips.clone(),
            metrics: self.metrics.clone(),
            row_selection: self.row_selection.clone(),
            focused_index: self.focused_index.clone(),
            row_map: self.row_map.clone(),
            reorderable: self.reorderable,
            export: self.export.clone(),
            on_activate: self.on_activate.clone(),
            activate_on: self.activate_on,
            model_id: self.model_id,
            root_id: self_id,
            scroll_y: self.scroll_y.clone(),
            viewport_height: self.viewport_height.clone(),
            placed_content_width: self.placed_content_width.clone(),
            version: self.pane_version.clone(),
            total_refresh: self.layout_refresh.clone(),
            prev_built_start: self.pane_built_start.clone(),
            prev_built_end: self.pane_built_end.clone(),
            item_entries: Vec::new(),
        };
        self.body_pane_id = Some(ctx.add(pane));

        // --- Create scrollbar ---
        // Skipped when the caller opted out via `show_scrollbar(false)`
        // — they're expected to mount their own, wired through the
        // exposed signal accessors.
        if self.show_scrollbar {
            let scrollbar = ScrollBar::new(
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
            let sb_id = ctx.add(scrollbar);
            self.scrollbar_id = Some(sb_id);
        } else {
            self.scrollbar_id = None;
        }

        self.child_ids()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // The viewport takes whatever the parent offers — but only an
        // allocation is cached for the visible-range computation; a
        // measurement's fallback is not a viewport (`common::viewport`).
        crate::common::viewport::viewport_size(
            proposal,
            &self.viewport_height,
            Size::new(300.0, 200.0),
        )
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Cache our own absolute bounds for the keyboard handler's
        // outer-scroll chase (`ensure_visible`). Done before the empty-children
        // bail so the rect stays fresh even for an empty list that later fills.
        self.viewport_bounds.set(bounds);
        // The allocated height is the authoritative viewport: `build` sizes its
        // realization window from this, and a stale value there costs a
        // permanent rebuild loop (`common::viewport`).
        crate::common::viewport::record_viewport_height(&self.viewport_height, bounds.height);

        if children.is_empty() {
            return;
        }

        let viewport_height = bounds.height;

        // The scrollbar decision uses the pre-measure total: the content
        // width must be known before rows can be measured at it. If a
        // measurement flips the decision, the next frame corrects it.
        let provisional_total = self.total_content_height();
        let needs_internal_scrollbar =
            self.show_scrollbar && provisional_total > viewport_height + 0.5;
        let reserves_bar = self.scroll_bar_style == ScrollBarMode::Permanent;
        let content_width = if needs_internal_scrollbar && reserves_bar {
            (bounds.width - SCROLLBAR_THICKNESS).max(0.0)
        } else {
            bounds.width
        };
        self.placed_content_width.set(content_width);

        // Totals for the scrollbar. In auto-measure mode these are computed
        // BEFORE the pane measures its rows (parent-before-child ordering), so
        // the pane pokes `layout_refresh` when a measurement moves the total
        // and we re-place next frame with the corrected value.
        let total_height = self.total_content_height();
        let max_y = (total_height - viewport_height).max(0.0);
        self.max_scroll_y.set(max_y);
        let ratio = if total_height > 0.0 {
            (viewport_height / total_height).clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.viewport_ratio_y.set(ratio);
        self.clamp_scroll();

        // Two children in a fixed order (see `child_ids`): the body pane
        // fills the content column and positions its own rows; the scrollbar
        // sits alongside it.
        let mut next = 0;
        if self.body_pane_id.is_some() {
            if let Some(child) = children.get_mut(next) {
                child.origin = bounds.origin();
                child.size = Size::new(content_width, bounds.height);
            }
            next += 1;
        }
        if self.scrollbar_id.is_some()
            && let Some(sb_child) = children.get_mut(next)
        {
            if needs_internal_scrollbar {
                sb_child.origin =
                    Point::new(bounds.x + bounds.width - SCROLLBAR_THICKNESS, bounds.y);
                sb_child.size = Size::new(SCROLLBAR_THICKNESS, bounds.height);
            } else {
                sb_child.origin = bounds.origin();
                sb_child.size = Size::ZERO;
            }
        }
    }

    fn paint(
        &self,
        bounds: Rect,
        canvas: &mut teksilo_canvas::Canvas,
        ctx: &teksilo_core::widget::PaintContext,
    ) {
        // Draw insertion line during drag hover. Recipe-driven role +
        // thickness — defaults to BorderRole::Accent / 2 dp; a custom
        // `ListContainerStyle` installed via the theme slot overrides.
        if let Some((y, width)) = self.drop_feedback.get() {
            let recipe = ctx
                .theme
                .style_slots
                .list_container
                .as_ref()
                .map(|s| s.insertion())
                .unwrap_or_default();
            let color = recipe.role.resolve(&ctx.theme.colors);
            let line_y = bounds.y + y;
            let line_x = bounds.x;
            let half = recipe.thickness * 0.5;
            // Own paint isn't covered by `clips_children` — clip so an
            // insertion line at the after-last boundary can't bleed
            // past the widget's bottom edge.
            canvas.set_clip(bounds);
            canvas.fill_rect(
                Rect::new(line_x, line_y - half, width, recipe.thickness),
                color,
            );
            canvas.clear_clip();
        }

        // Container focus ring — keyboard focus landed but nothing is selected,
        // so no row ring shows; outline the whole view (see TreeView).
        let has_selection = self
            .row_selection
            .as_ref()
            .is_some_and(|s| s.has_selection());
        if self.view_focused.get() && self.focus_visible.get() && !has_selection {
            let color = BorderRole::Focused.resolve(&ctx.theme.colors);
            let inset = 1.0_f32;
            let rect = Rect::new(
                bounds.x + inset,
                bounds.y + inset,
                (bounds.width - inset * 2.0).max(0.0),
                (bounds.height - inset * 2.0).max(0.0),
            );
            canvas.stroke_rect(rect, color, 1.5);
        }
    }

    /// The context-menu key opens the *current row's* menu, not the list's.
    ///
    /// A `ListView` is focusable and its rows deliberately are not — the
    /// container owns focus and `set_selected` is what tells assistive
    /// technology which row is current (see `list_item_a11y`). So the
    /// dispatcher's default of "the focused widget" would open the list's own
    /// menu, in the widget family where a per-row menu matters most.
    ///
    /// The row the user means is the keyboard cursor if they have navigated
    /// (`focused_index`), else the first selected row. Both are indices into
    /// the model, and only realized rows have a widget, so a cursor scrolled
    /// outside the virtualization window resolves to nothing and the menu falls
    /// back to the list — which is the right answer, since there is no row on
    /// screen for it to be about.
    fn context_menu_key_target(&self) -> Option<WidgetId> {
        self.current_row_widget()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::ListBox);
        // Whether the selection takes more than one row. A real property on
        // both platforms that have one: UIA's `SelectionCanSelectMultiple`
        // and AT-SPI's multiselectable state. Left unset it reads false, so a
        // multi-select view was telling every screen reader that one row was
        // the most it would ever hold.
        //
        // Gated on the mode, and the gate matters beyond tidiness:
        // `accesskit_windows` picks the event it raises on a selection change
        // from this property (`adapter.rs:189-199`), firing
        // `ElementAddedToSelection` when it is true and `ElementSelected` when
        // it is false. A single-select view publishing `true` would trade the
        // right event for the wrong one.
        if self
            .row_selection
            .as_ref()
            .is_some_and(|selection| selection.mode() == teksilo_data::SelectionMode::Multi)
        {
            builder.set_multiselectable(true);
        }

        // The logical row count, not the realized virtualization window: a
        // 200-row list announces "of 200" even while twenty rows exist as
        // widgets. It belongs here rather than on each row, because
        // `size_of_set_from_container` resolves an item's set size by walking
        // *up* from it — a size written on a row is read by no adapter.
        builder.set_size_of_set(self.source.len());

        // The current row, as the container's active descendant.
        //
        // Keyboard focus stays here, on the list, and the row is marked
        // `selected`. On AT-SPI that is the whole story: Orca announces the
        // selection change. On Windows it is not, because UIA has no
        // active-descendant property at all — what it has is a focused
        // element, and for a list box that element is the item.
        //
        // AccessKit bridges the two in the consumer rather than in each
        // adapter: `accesskit_consumer` resolves the focused node as
        // `focused.active_descendant().unwrap_or(focused)`
        // (`tree.rs:541`) and `accesskit_windows::focus_moved`
        // (`adapter.rs:341-345`) raises `UIA_AutomationFocusChangedEventId` on
        // whatever comes out. So this one property turns every arrow press
        // into the focus change a screen reader announces, and `is_focused`
        // (`consumer node.rs:89-105`) moves from this container to the row,
        // which is what the ARIA listbox pattern says should happen.
        //
        // Without it, arrowing through any Teksilo list is silent to NVDA:
        // there is no focus change to announce, and the selection event that
        // is raised names a row node the pane rebuilt a moment earlier. The
        // mouse still reads rows correctly, because hit-testing does not go
        // through events at all, which is exactly how this hid for so long.
        // Only while this view actually holds focus. A container that does not
        // have focus has no active descendant to speak of, and publishing one
        // anyway puts a second relation in the tree for a client to follow: the
        // combobox pattern (`CommandPalette`) keeps focus on a text field that
        // points at a row in *this* list, and two publishers of the same row is
        // an ambiguity nobody needs to resolve.
        if self.view_focused.get()
            && let Some(row) = self.current_row_widget()
        {
            builder.set_active_descendant(teksilo_core::accessibility::widget_id_to_node_id(row));
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_ids()
    }

    fn clips_children(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_core::widget_tree::WidgetTree;

    /// The realized row wrappers. `ListView`'s own children are the body pane
    /// and the scrollbar (see `body_pane`'s module docs for why the rows sit
    /// one level down), so every test that used to walk `tree.children(lv_id)`
    /// for rows goes through here.
    fn row_ids(tree: &WidgetTree, lv: WidgetId) -> Vec<WidgetId> {
        let kids = tree.children(lv);
        match kids.first() {
            Some(&pane) => tree.children(pane),
            None => Vec::new(),
        }
    }

    /// The internal scrollbar — always the ListView's last child.
    fn scrollbar_of(tree: &WidgetTree, lv: WidgetId) -> WidgetId {
        *tree.children(lv).last().expect("ListView has children")
    }

    #[test]
    fn smooth_scroll_survives_a_body_pane_rebuild() {
        let (mut tree, lv_id, model) = make_list_view(500, 20.0);
        let scroll = {
            tree.layout(SizeProposal::exact(400.0, 200.0));
            let any = tree.widget_as_any(lv_id).unwrap();
            any.downcast_ref::<ListView<usize>>()
                .unwrap()
                .scroll_y_signal()
                .clone()
        };
        crate::common::thumb_drag_test::assert_fling_survives_pane_rebuild(
            &mut tree,
            400.0,
            200.0,
            &scroll,
            "ListView",
            || model.push(9999),
        );
    }

    #[test]
    fn rows_materialize_during_scrollbar_thumb_drag() {
        // The reason `ListBodyPane` exists — see
        // `common::thumb_drag_test`'s module docs for the invariant.
        let (mut tree, lv_id, _model) = make_list_view(500, 20.0);
        crate::common::thumb_drag_test::assert_body_survives_thumb_drag(
            &mut tree,
            lv_id,
            400.0,
            200.0,
            0.0,
            "ListView",
            |t| {
                row_ids(t, lv_id)
                    .into_iter()
                    .filter(|id| {
                        let b = t.bounds(*id);
                        b.height > 1.0 && b.y > -b.height && b.y < 200.0
                    })
                    .count()
            },
        );
    }

    #[derive(Debug)]
    pub(super) struct FixedLeaf(pub f32, pub f32);
    impl Widget for FixedLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> teksilo_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    /// A `ListView` announced neither its position nor its total until now: no
    /// row set `position_in_set`, and no container anywhere in the framework
    /// set `size_of_set`. A screen-reader user arrowing through a 200-row list
    /// heard each row's label and nothing about where they were in it.
    ///
    /// Asked the way a platform adapter asks it — the position off the row, the
    /// total by walking up to the `Role::ListBox` — because that walk is
    /// exactly what a node-level assertion would have missed.
    #[test]
    fn a_row_announces_its_position_out_of_the_whole_model() {
        let (mut tree, lv_id, _model) = make_list_view(200, 20.0);
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let rows = row_ids(&tree, lv_id);
        assert!(!rows.is_empty(), "some rows must be realized");

        let update = tree.sync_accessibility();
        for (i, &row) in rows.iter().enumerate().take(3) {
            crate::a11y_set_semantics::assert_announces(
                &update,
                teksilo_core::accessibility::widget_id_to_node_id(row),
                i + 1,
                200,
                &format!("row {i}"),
            );
        }
    }

    /// The number a row announces is its place in the **model**, not in the
    /// realized window: scroll to row 150 and it must say 151, not 1.
    #[test]
    fn a_scrolled_row_announces_its_model_position_not_its_window_position() {
        let (mut tree, lv_id, _model) = make_list_view(200, 20.0);
        tree.layout(SizeProposal::exact(400.0, 200.0));
        {
            let any = tree.widget_as_any(lv_id).unwrap();
            any.downcast_ref::<ListView<usize>>()
                .unwrap()
                .scroll_y_signal()
                .set(150.0 * 20.0);
        }
        tree.layout(SizeProposal::exact(400.0, 200.0));

        let rows = row_ids(&tree, lv_id);
        assert!(
            !rows.is_empty(),
            "some rows must be realized after scrolling"
        );
        let update = tree.sync_accessibility();
        let (position, size) = crate::a11y_set_semantics::announced_set_position(
            &update,
            teksilo_core::accessibility::widget_id_to_node_id(rows[0]),
        );
        assert_eq!(
            size,
            Some(200),
            "the total is the model's, not the window's"
        );
        assert!(
            position.is_some_and(|p| p > 100),
            "the first realized row after scrolling to the 150th must announce a \
             model position, not a window position; got {position:?}"
        );
    }

    fn make_list_view(count: usize, item_height: f32) -> (WidgetTree, WidgetId, ListModel<usize>) {
        let model = ListModel::from_vec((0..count).collect());
        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model.clone(), move |_i, _item, _selected| {
                Box::new(FixedLeaf(100.0, item_height))
            })
            .item_height(item_height),
        );
        (tree, lv_id, model)
    }

    /// Taking focus reveals the row the keyboard is on, however far down it is.
    ///
    /// Only the rows near the viewport are realized, so a selection made
    /// before the list is looked at — restoring a session, landing on
    /// "whatever is happening now" — usually sits outside that window. Nothing
    /// then speaks for it: no node carries `selected`, no active descendant is
    /// nominated, and a screen reader taking focus here is told nothing. The
    /// first arrow press steps past the row as well, because the cursor was
    /// somewhere nobody was shown.
    ///
    /// Asserted on the accessibility tree, since that is what the failure was
    /// about: the row has to be a node a platform can name.
    #[test]
    fn taking_focus_reveals_the_current_row() {
        use teksilo_data::{SelectionMode, SelectionModel};

        let model = ListModel::from_vec((0..200).collect::<Vec<usize>>());
        let selection = SelectionModel::new(SelectionMode::Single);
        selection.select(150);

        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model.clone(), move |_i, _item, _selected| {
                Box::new(FixedLeaf(100.0, 20.0))
            })
            .item_height(20.0)
            .selection(selection.clone()),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));

        let realized_selected = |tree: &mut WidgetTree| -> Vec<usize> {
            tree.accessibility_tree_snapshot()
                .nodes
                .iter()
                .filter(|(_, node)| node.is_selected() == Some(true))
                .filter_map(|(_, node)| node.position_in_set())
                .collect()
        };

        assert!(
            realized_selected(&mut tree).is_empty(),
            "row 150 starts far outside the realized window, which is the case \
             this is about"
        );

        tree.focus(lv_id);
        tree.layout(SizeProposal::exact(400.0, 200.0));

        assert_eq!(
            realized_selected(&mut tree),
            vec![150],
            "taking focus has to bring the current row into the realized window, \
             or nothing in the tree can be told about it"
        );
    }

    /// And a row already on screen does not jump.
    ///
    /// `ensure_index_visible`, not `scroll_to_index`: somebody who can see the
    /// list must not have it lurch when they click into it.
    #[test]
    fn taking_focus_does_not_move_a_row_already_in_view() {
        use teksilo_data::{SelectionMode, SelectionModel};

        let model = ListModel::from_vec((0..200).collect::<Vec<usize>>());
        let selection = SelectionModel::new(SelectionMode::Single);
        selection.select(2);

        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model.clone(), move |_i, _item, _selected| {
                Box::new(FixedLeaf(100.0, 20.0))
            })
            .item_height(20.0)
            .selection(selection.clone()),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));

        let scroll = {
            let any = tree.widget_as_any(lv_id).unwrap();
            any.downcast_ref::<ListView<usize>>()
                .unwrap()
                .scroll_y_signal()
                .clone()
        };
        let before = scroll.get();

        tree.focus(lv_id);
        tree.layout(SizeProposal::exact(400.0, 200.0));

        assert_eq!(
            scroll.get(),
            before,
            "row 2 is already visible, so the list must not scroll at all"
        );
    }

    /// The focused node, as a platform adapter resolves it, is the current row.
    ///
    /// This is the property that decides whether a screen reader says anything
    /// when the user presses an arrow. Keyboard focus stays on the list, so
    /// there is no focus change for the platform to report unless the list
    /// nominates a row as its active descendant; `accesskit_consumer` then
    /// resolves the focused node through it (`tree.rs:541`) and
    /// `accesskit_windows::focus_moved` raises
    /// `UIA_AutomationFocusChangedEventId` on the row (`adapter.rs:341-345`).
    ///
    /// Asserted through `is_focused`, which is the consumer's own answer and
    /// what both adapters go on, rather than through the raw property: the
    /// property is the mechanism and this is the meaning.
    #[test]
    fn the_current_row_is_what_the_platform_calls_focused() {
        use teksilo_data::{SelectionMode, SelectionModel};

        let model = ListModel::from_vec((0..10).collect::<Vec<usize>>());
        let selection = SelectionModel::new(SelectionMode::Single);
        selection.select(3);

        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model.clone(), move |_i, _item, _selected| {
                Box::new(FixedLeaf(100.0, 20.0))
            })
            .item_height(20.0)
            .selection(selection.clone()),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));
        tree.focus(lv_id);
        tree.layout(SizeProposal::exact(400.0, 200.0));

        // The row a screen reader would be told about, by model index.
        let focused_row = |tree: &mut WidgetTree| -> Option<usize> {
            let snapshot = tree.accessibility_tree_snapshot();
            let consumer = accesskit_consumer::Tree::new(snapshot, true);
            let state = consumer.state();
            let focus = state.focus_id()?;
            let focused = state.node_by_id(focus)?;
            // Exactly what `accesskit_consumer` does before telling an adapter
            // the focus moved (`tree.rs:541`), and what `is_focused` concludes
            // (`node.rs:89-105`). `focus_id` alone is the raw value and still
            // names the container.
            let resolved = focused.active_descendant().unwrap_or(focused);
            // Zero-based in the tree, one-based in the ear.
            resolved.position_in_set()
        };

        assert_eq!(
            focused_row(&mut tree),
            Some(3),
            "with the list focused, the platform's focused node must be the \
             current row and not the list itself"
        );

        selection.select(6);
        tree.layout(SizeProposal::exact(400.0, 200.0));

        assert_eq!(
            focused_row(&mut tree),
            Some(6),
            "and moving the selection must move it, which is the focus change \
             NVDA announces; without it an arrow press is silent"
        );
    }

    /// A list with nothing selected nominates nothing.
    ///
    /// The container stays the focused node, which is correct: there is no row
    /// to be on, and pointing at one would make a screen reader announce a row
    /// the user has not reached.
    #[test]
    fn an_unselected_list_nominates_no_row() {
        let (mut tree, lv_id, _model) = make_list_view(10, 20.0);
        tree.layout(SizeProposal::exact(400.0, 200.0));
        tree.focus(lv_id);
        tree.layout(SizeProposal::exact(400.0, 200.0));

        let snapshot = tree.accessibility_tree_snapshot();
        let consumer = accesskit_consumer::Tree::new(snapshot, true);
        let state = consumer.state();
        let focus = state.focus_id().expect("something has focus");
        let focused = state.node_by_id(focus).expect("the focused node exists");
        let resolved = focused.active_descendant().unwrap_or(focused);
        assert_eq!(
            resolved.role(),
            teksilo_core::accesskit::Role::ListBox,
            "with no selection the list itself is the focused node"
        );
    }

    /// And the row that is now selected says so.
    ///
    /// The pair matters: keeping the widgets would be easy if the selected flag
    /// stopped following the selection, and that would trade a silent screen
    /// reader for a lying one.
    #[test]
    fn the_selected_row_reports_itself_after_a_move() {
        use teksilo_data::{SelectionMode, SelectionModel};

        let model = ListModel::from_vec((0..10).collect::<Vec<usize>>());
        let selection = SelectionModel::new(SelectionMode::Single);
        selection.select(3);

        let mut tree = WidgetTree::new();
        // The id is not needed: this asserts the `selected` flag, which does
        // not depend on the view holding focus.
        let _ = tree.add(
            ListView::new(model.clone(), move |_i, _item, _selected| {
                Box::new(FixedLeaf(100.0, 20.0))
            })
            .item_height(20.0)
            .selection(selection.clone()),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));

        let selected_rows = |tree: &mut WidgetTree| -> Vec<usize> {
            let nodes = tree.accessibility_tree_snapshot().nodes;
            nodes
                .iter()
                .filter(|(_, node)| node.is_selected() == Some(true))
                .filter_map(|(_, node)| node.position_in_set())
                .collect()
        };

        // Raw `position_in_set` is zero-based (AccessKit's convention, one
        // below the ARIA number the adapters speak), so row 4 reads as 3.
        assert_eq!(selected_rows(&mut tree), vec![3]);

        selection.select(4);
        tree.layout(SizeProposal::exact(400.0, 200.0));

        assert_eq!(
            selected_rows(&mut tree),
            vec![4],
            "the flag has to follow the selection, whether or not the widget was \
             rebuilt to carry it"
        );
    }

    #[test]
    fn arrow_nav_resumes_from_the_clicked_row() {
        // Regression: a row click must move the keyboard-navigation cursor
        // (`focused_index`) to the clicked row, so the next Arrow step continues
        // from there — not from the stale keyboard cursor / index 0.
        use teksilo_canvas::Point;
        use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
        use teksilo_data::{SelectionMode, SelectionModel};

        let model = ListModel::from_vec((0..10).collect::<Vec<usize>>());
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel = selection.clone();
        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model, |_i, _item, _sel| Box::new(FixedLeaf(100.0, 20.0)))
                .item_height(20.0)
                .selection(sel),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0)); // 10 rows × 20px all visible
        tree.focus(lv_id);

        // Click row 3 (rows are 20px tall, so y≈70; x past any leading control).
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(50.0, 70.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(50.0, 70.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(
            selection.selected_indices(),
            vec![3],
            "precondition: body click selects row 3"
        );

        // ArrowDown must step to 4 (from the clicked row), not to 1 (from index 0).
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(
            selection.selected_indices(),
            vec![4],
            "ArrowDown after a click resumes from the clicked row (3 → 4)"
        );
    }

    #[test]
    fn focused_index_follows_insert_before_it() {
        // Bug repro: `focused_index` (the keyboard-nav anchor) was never
        // adjusted on any DataChange, so after a peer/insert shifts the
        // rows it silently pointed at the wrong one — the next ArrowDown
        // would resume from a stale position instead of the row the user
        // was actually on.
        use teksilo_canvas::Point;
        use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
        use teksilo_data::{SelectionMode, SelectionModel};

        let model = ListModel::from_vec((0..10).collect::<Vec<usize>>());
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel = selection.clone();
        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model.clone(), |_i, _item, _sel| {
                Box::new(FixedLeaf(100.0, 20.0))
            })
            .item_height(20.0)
            .selection(sel),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.focus(lv_id);

        // Click row 3 — sets both selection and the keyboard-nav anchor to 3.
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(50.0, 70.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(50.0, 70.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(selection.selected_indices(), vec![3], "precondition");

        // A peer-driven reload prepends two rows — row 3 is now row 5.
        model.insert(0, 100);
        model.insert(0, 200);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        // The selection model itself already index-shifts (existing
        // behaviour) — this is just re-confirming the setup, not the fix.
        assert_eq!(
            selection.selected_indices(),
            vec![5],
            "precondition: selection shifts with the inserted rows"
        );

        // If `focused_index` had NOT shifted (the bug), it would still read
        // 3, and ArrowDown would resume from there (→ select 4). With the
        // fix it follows the insert to 5, so ArrowDown resumes from 5 (→ 6).
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(
            selection.selected_indices(),
            vec![6],
            "ArrowDown after a leading insert resumes from the shifted row (5 → 6), \
             not the stale pre-insert one (3 → 4)"
        );
    }

    #[test]
    fn focused_index_dropped_when_its_row_is_removed() {
        // The focused row itself was removed: the anchor must be cleared,
        // not left pointing at whatever now occupies its old slot.
        use teksilo_canvas::Point;
        use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
        use teksilo_data::{SelectionMode, SelectionModel};

        let model = ListModel::from_vec((0..10).collect::<Vec<usize>>());
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel = selection.clone();
        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model.clone(), |_i, _item, _sel| {
                Box::new(FixedLeaf(100.0, 20.0))
            })
            .item_height(20.0)
            .selection(sel),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.focus(lv_id);

        // Click row 3.
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(50.0, 70.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(50.0, 70.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(selection.selected_indices(), vec![3], "precondition");

        // Row 3 itself is removed from under the focused anchor.
        model.remove(3);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        assert!(
            selection.selected_indices().is_empty(),
            "precondition: selection drops the removed row"
        );

        // With `focused_index` cleared (`None`) — and the selection dropped with
        // it, so there is no cursor to fall back on either — the next ArrowDown
        // lands ON row 0. Left un-cleared (the bug), the stale anchor would
        // still read 3 (now clamped to the shrunk list, still in range) and
        // ArrowDown would step to 4 instead.
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(
            selection.selected_indices(),
            vec![0],
            "focused_index was cleared, so nav restarts at the top (row 0), \
             not from the stale removed row's index (3 → 4)"
        );
    }

    #[test]
    fn first_arrow_lands_on_an_end_row_instead_of_skipping_it() {
        // "No cursor yet" is not "cursor on row 0": the very first ArrowDown
        // must select the FIRST row, not step past it to row 1 (which would
        // make the top row unreachable by keyboard until you arrow back up),
        // and the very first ArrowUp must select the LAST row.
        use teksilo_core::event::{Key, Modifiers};
        use teksilo_data::{SelectionMode, SelectionModel};

        for (key, want, what) in [
            (
                Key::ArrowDown,
                0usize,
                "first ArrowDown selects the first row",
            ),
            (Key::ArrowUp, 9usize, "first ArrowUp selects the last row"),
        ] {
            let model = ListModel::from_vec((0..10).collect::<Vec<usize>>());
            let selection = SelectionModel::new(SelectionMode::Single);
            let mut tree = WidgetTree::new();
            let lv_id = tree.add(
                ListView::new(model, |_i, _item, _sel| Box::new(FixedLeaf(100.0, 20.0)))
                    .item_height(20.0)
                    .selection(selection.clone()),
            );
            tree.layout(SizeProposal::exact(400.0, 300.0));
            tree.focus(lv_id);
            assert!(
                selection.selected_indices().is_empty(),
                "precondition: nothing selected, no cursor"
            );

            tree.press_key(key, Modifiers::NONE);
            assert_eq!(selection.selected_indices(), vec![want], "{what}");
        }
    }

    #[test]
    fn keyboard_cursor_starts_from_a_preset_selection() {
        // A view can be handed a selection before it is ever focused (a
        // launcher preselecting the top entry). The first arrow key must
        // continue from that visible row rather than from an invisible zero —
        // otherwise Down on a preselected row 2 would jump backwards to row 0.
        use teksilo_core::event::{Key, Modifiers};
        use teksilo_data::{SelectionMode, SelectionModel};

        let model = ListModel::from_vec((0..10).collect::<Vec<usize>>());
        let selection = SelectionModel::new(SelectionMode::Single);
        selection.select(2);
        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model, |_i, _item, _sel| Box::new(FixedLeaf(100.0, 20.0)))
                .item_height(20.0)
                .selection(selection.clone()),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.focus(lv_id);

        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(
            selection.selected_indices(),
            vec![3],
            "Down from a preselected row 2 continues to 3"
        );
        tree.press_key(Key::ArrowUp, Modifiers::NONE);
        tree.press_key(Key::ArrowUp, Modifiers::NONE);
        assert_eq!(selection.selected_indices(), vec![1], "and Up walks back");
    }

    #[test]
    fn checkbox_press_does_not_select_row() {
        // Regression: pressing an embedded checkbox toggles it but must NOT
        // select the row. The row's select-on-press handler yields to the
        // checkbox's own tap via `ctx.press_claimed_by_interactive_child()`.
        use crate::styles::recipe_standard_item_style as si;
        use teksilo_canvas::Point;
        use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
        use teksilo_data::{SelectionMode, SelectionModel};
        use teksilo_i18n::lit;

        let model = ListModel::from_vec(vec!["alpha", "beta", "gamma"]);
        let checks: Vec<Signal<bool>> = (0..3).map(|_| Signal::new(false)).collect();
        let checks_for_rows = checks.clone();
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel = selection.clone();
        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model, move |i, _item, _selected| {
                Box::new(
                    crate::StandardListItem::new(lit!(format!("row {i}")))
                        .checkbox(checks_for_rows[i].clone()),
                ) as Box<dyn Widget>
            })
            .item_height(40.0)
            .selection(sel),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let rows = row_ids(&tree, lv_id);
        let row0 = tree.bounds(rows[0]);
        let press = |t: &mut WidgetTree, x: f32, y: f32| {
            t.dispatch_event(WidgetEvent::PointerDown {
                position: Point::new(x, y),
                button: PointerButton::Primary,
                modifiers: Modifiers::NONE,
            });
            t.dispatch_event(WidgetEvent::PointerUp {
                position: Point::new(x, y),
                button: PointerButton::Primary,
                modifiers: Modifiers::NONE,
            });
        };

        // Press the embedded checkbox (leading edge): toggles it, must NOT select.
        let cb_x = row0.x
            + si::STANDARD_ITEM_BG_HORIZONTAL_INSET
            + si::STANDARD_ITEM_PADDING_HORIZONTAL
            + 4.0;
        let cb_y = row0.y + row0.height * 0.5;
        press(&mut tree, cb_x, cb_y);
        assert!(checks[0].get(), "checkbox press should toggle the checkbox");
        assert!(
            selection.selected_indices().is_empty(),
            "checkbox press must not select the row (got {:?})",
            selection.selected_indices()
        );

        // Press the row body (far right of the checkbox): selects, no toggle.
        let body_x = row0.x + row0.width * 0.7;
        press(&mut tree, body_x, cb_y);
        assert_eq!(
            selection.selected_indices(),
            vec![0],
            "body press should select row 0"
        );
        assert!(
            checks[0].get(),
            "body press must not toggle the checkbox back"
        );
    }

    #[test]
    fn virtualization_creates_only_visible_items() {
        let (mut tree, lv_id, _model) = make_list_view(10_000, 30.0);
        // Viewport: 300px tall, items 30px each = ~10 visible + 2*5 buffer = ~20
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let children = row_ids(&tree, lv_id);
        // children includes items + 1 scrollbar
        let item_count = children.len() - 1;
        assert!(
            item_count < 30,
            "Expected fewer than 30 items, got {}",
            item_count
        );
        assert!(
            item_count >= 10,
            "Expected at least 10 items, got {}",
            item_count
        );
    }

    #[test]
    fn empty_model_shows_scrollbar_only() {
        let (mut tree, lv_id, _model) = make_list_view(0, 30.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // The pane is mounted even with no data (it is the stable sibling the
        // scrollbar needs), and realizes no rows.
        assert_eq!(tree.children(lv_id).len(), 2, "body pane + scrollbar");
        assert!(
            row_ids(&tree, lv_id).is_empty(),
            "no rows for an empty model"
        );
    }

    #[test]
    fn data_change_triggers_rebuild() {
        let (mut tree, lv_id, model) = make_list_view(5, 30.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let initial_items = row_ids(&tree, lv_id).len(); // minus scrollbar
        assert_eq!(initial_items, 5);

        model.push(99);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let new_items = row_ids(&tree, lv_id).len();
        assert_eq!(new_items, 6);
    }

    #[test]
    fn remove_triggers_rebuild() {
        let (mut tree, lv_id, model) = make_list_view(5, 30.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        assert_eq!(row_ids(&tree, lv_id).len(), 5);

        model.remove(0);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        assert_eq!(row_ids(&tree, lv_id).len(), 4);
    }

    #[test]
    fn items_positioned_correctly() {
        let (mut tree, lv_id, _model) = make_list_view(3, 40.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let children = row_ids(&tree, lv_id);
        // Items should be at y=0, y=40, y=80
        let y0 = tree.bounds(children[0]).y;
        let y1 = tree.bounds(children[1]).y;
        let y2 = tree.bounds(children[2]).y;
        assert!((y0 - 0.0).abs() < 0.01);
        assert!((y1 - 40.0).abs() < 0.01);
        assert!((y2 - 80.0).abs() < 0.01);
    }

    #[test]
    fn items_have_correct_height() {
        let (mut tree, lv_id, _model) = make_list_view(3, 40.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let children = row_ids(&tree, lv_id);
        for i in 0..3 {
            let h = tree.bounds(children[i]).height;
            assert!((h - 40.0).abs() < 0.01, "Item {} height {} != 40.0", i, h);
        }
    }

    #[test]
    fn scrollbar_positioned_on_right_edge() {
        let (mut tree, lv_id, _model) = make_list_view(100, 30.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let sb_bounds = tree.bounds(scrollbar_of(&tree, lv_id));
        // Scrollbar should be at right edge
        assert!(
            (sb_bounds.x - (400.0 - SCROLLBAR_THICKNESS)).abs() < 0.01,
            "Scrollbar x {} != {}",
            sb_bounds.x,
            400.0 - SCROLLBAR_THICKNESS
        );
        assert!((sb_bounds.height - 300.0).abs() < 0.01);
    }

    #[test]
    fn small_list_collapses_scrollbar() {
        let (mut tree, lv_id, _model) = make_list_view(3, 30.0);
        // 3 items * 30px = 90px < 300px viewport
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let sb_bounds = tree.bounds(scrollbar_of(&tree, lv_id));
        assert!(
            sb_bounds.width < 0.01 && sb_bounds.height < 0.01,
            "Scrollbar should be collapsed for small lists"
        );
    }

    #[test]
    fn item_width_leaves_room_for_scrollbar() {
        let (mut tree, lv_id, _model) = make_list_view(100, 30.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let children = row_ids(&tree, lv_id);
        let item_width = tree.bounds(children[0]).width;
        assert!(
            (item_width - (400.0 - SCROLLBAR_THICKNESS)).abs() < 0.01,
            "Item width {} should be {}",
            item_width,
            400.0 - SCROLLBAR_THICKNESS
        );
    }

    #[test]
    fn small_list_items_use_full_width() {
        let (mut tree, lv_id, _model) = make_list_view(3, 30.0);
        // 3 items * 30px = 90px < 300px viewport — no scrollbar needed
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let children = row_ids(&tree, lv_id);
        let item_width = tree.bounds(children[0]).width;
        assert!(
            (item_width - 400.0).abs() < 0.01,
            "Small list item width {} should be full 400.0 (no scrollbar)",
            item_width,
        );
    }

    // --- Selection tests ---

    fn make_selectable_list(
        count: usize,
    ) -> (
        WidgetTree,
        WidgetId,
        ListModel<usize>,
        teksilo_data::SelectionModel,
    ) {
        use teksilo_data::{SelectionMode, SelectionModel};
        let model = ListModel::from_vec((0..count).collect());
        let selection = SelectionModel::new(SelectionMode::Multi);
        let sel_clone = selection.clone();
        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model.clone(), move |_i, _item, _selected| {
                Box::new(FixedLeaf(100.0, 30.0))
            })
            .item_height(30.0)
            .selection(sel_clone),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        (tree, lv_id, model, selection)
    }

    #[test]
    fn click_selects_item() {
        let (mut tree, lv_id, _, selection) = make_selectable_list(5);
        // Click the second item (y = 30..60, center at 45)
        let children = row_ids(&tree, lv_id);
        tree.click(children[1]);
        assert!(selection.is_selected(1), "item 1 should be selected");
        assert!(!selection.is_selected(0), "item 0 should not be selected");
    }

    #[test]
    fn a_multi_select_list_says_so_and_a_single_select_one_does_not() {
        // Left unset the property reads false, so a multi-select list was
        // telling every screen reader that one row was the most it would hold.
        //
        // The negative half is not symmetry for its own sake.
        // `accesskit_windows` chooses the event it raises on a selection change
        // from this property (`adapter.rs:189-199`): `ElementAddedToSelection`
        // when it is true, `ElementSelected` when it is false. A single-select
        // list publishing `true` would trade the right event for the wrong one.
        use teksilo_data::{SelectionMode, SelectionModel};

        let published = |mode: SelectionMode| {
            let model = ListModel::from_vec(vec![1, 2, 3]);
            let mut tree = WidgetTree::new();
            // The view's own id is not needed: the node is found by role,
            // which is how an adapter finds it too.
            tree.add(
                ListView::new(model, move |_i, _item, _selected| {
                    Box::new(FixedLeaf(100.0, 30.0))
                })
                .item_height(30.0)
                .selection(SelectionModel::new(mode)),
            );
            tree.layout(SizeProposal::exact(400.0, 300.0));
            // Read off the real node rather than the `AccessibilityInfo`
            // summary, which carries no such field: the property only exists
            // where an adapter would look for it.
            let snapshot = tree.accessibility_tree_snapshot();
            let consumer = accesskit_consumer::Tree::new(snapshot, true);
            let root = consumer.state().root();
            fn listbox<'a>(
                node: accesskit_consumer::NodeRef<'a>,
            ) -> Option<accesskit_consumer::NodeRef<'a>> {
                if node.role() == teksilo_core::accesskit::Role::ListBox {
                    return Some(node);
                }
                node.children().find_map(listbox)
            }
            listbox(root)
                .expect("the view publishes a ListBox")
                .is_multiselectable()
        };

        assert!(
            published(SelectionMode::Multi),
            "a list that takes more than one row has to say so"
        );
        assert!(
            !published(SelectionMode::Single),
            "and one that does not must not, or Windows raises the wrong event"
        );
    }

    #[test]
    fn moving_the_selection_keeps_every_row_node() {
        // Arrowing down replaced every realized row widget, and with them
        // every AccessKit node id in the list. The `active_descendant` the
        // container publishes then names a node the screen reader has never
        // seen, and the scroll anchor is reset under the user.
        //
        // Only two rows change when the selection moves: the one that lost it
        // and the one that gained it. Every other row is identical, and the
        // wrapper nodes must survive even for those two.
        let (mut tree, lv_id, _model, selection) = make_selectable_list(5);
        selection.select(0);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let before = row_ids(&tree, lv_id);
        assert_eq!(
            before.len(),
            5,
            "all five rows realize in a 300 px viewport"
        );

        selection.select(1);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let after = row_ids(&tree, lv_id);

        assert_eq!(
            before, after,
            "a selection move must not replace the row nodes"
        );
    }

    #[test]
    fn click_replaces_selection() {
        let (mut tree, lv_id, _, selection) = make_selectable_list(5);
        let children = row_ids(&tree, lv_id);
        tree.click(children[0]);
        assert!(selection.is_selected(0));

        tree.click(children[2]);
        assert!(selection.is_selected(2));
        assert!(
            !selection.is_selected(0),
            "previous selection should be cleared"
        );
    }

    #[test]
    fn keyed_selection_tracks_identity_not_index() {
        // from_source_keyed wires a KeyedSelectionModel<S::Key>: a click stores
        // the row's KEY (not its index), proving the index↔key translation.
        use std::rc::Rc;
        use teksilo_core::ObserverHandle;
        use teksilo_data::{KeyedSelectionModel, ListDataSource, SelectionMode};

        struct KeyedSource {
            items: Vec<(u64, usize)>, // (stable key, value)
        }
        impl ListDataSource for KeyedSource {
            type Item = usize;
            type Key = u64;
            fn len(&self) -> usize {
                self.items.len()
            }
            fn with_item<R>(&self, i: usize, f: impl FnOnce(&usize) -> R) -> Option<R> {
                self.items.get(i).map(|(_, v)| f(v))
            }
            fn key_at(&self, i: usize) -> Option<u64> {
                self.items.get(i).map(|(k, _)| *k)
            }
            fn index_of(&self, key: &u64) -> Option<usize> {
                self.items.iter().position(|(k, _)| k == key)
            }
            fn observe_changes(
                &self,
                _f: impl Fn(&teksilo_data::DataChange) + 'static,
            ) -> ObserverHandle {
                ObserverHandle::new(Rc::new(()) as Rc<dyn std::any::Any>, 0, Rc::new(|_| {}))
            }
        }

        let keyed = KeyedSelectionModel::<u64>::new(SelectionMode::Single);
        let source = KeyedSource {
            items: vec![(10, 100), (20, 200), (30, 300)],
        };
        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::from_source_keyed(source, keyed.clone(), |_i, _v, _sel| {
                Box::new(FixedLeaf(100.0, 30.0))
            })
            .item_height(30.0),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Click row 1 → the keyed model stores key 20, not index 1.
        let children = row_ids(&tree, lv_id);
        tree.click(children[1]);
        assert!(keyed.is_selected(&20), "selection is stored by key");
        assert_eq!(keyed.selected_keys(), vec![20]);
        assert!(!keyed.is_selected(&10));
    }

    #[test]
    fn ctrl_click_toggles() {
        use teksilo_core::event::Modifiers;
        let (mut tree, lv_id, _, selection) = make_selectable_list(5);
        let children = row_ids(&tree, lv_id);

        // Select item 0
        tree.click(children[0]);
        assert!(selection.is_selected(0));

        // Ctrl+click item 2 to add it
        let center = tree.bounds(children[2]).center();
        tree.dispatch_event(teksilo_core::event::WidgetEvent::PointerDown {
            position: center,
            button: teksilo_core::event::PointerButton::Primary,
            modifiers: Modifiers::COMMAND,
        });
        tree.dispatch_event(teksilo_core::event::WidgetEvent::PointerUp {
            position: center,
            button: teksilo_core::event::PointerButton::Primary,
            modifiers: Modifiers::COMMAND,
        });

        assert!(selection.is_selected(0), "item 0 should still be selected");
        assert!(selection.is_selected(2), "item 2 should be toggled on");
    }

    #[test]
    fn shift_click_extends_range() {
        use teksilo_core::event::Modifiers;
        let (mut tree, lv_id, _, selection) = make_selectable_list(5);
        let children = row_ids(&tree, lv_id);

        // Select item 1 as anchor
        tree.click(children[1]);
        assert!(
            selection.is_selected(1),
            "item 1 should be selected after plain click"
        );

        // Shift+click item 3 — should extend from anchor (1) to 3
        let center = tree.bounds(children[3]).center();
        tree.dispatch_event(teksilo_core::event::WidgetEvent::PointerDown {
            position: center,
            button: teksilo_core::event::PointerButton::Primary,
            modifiers: Modifiers::SHIFT,
        });

        let selected = selection.selected_indices();
        assert_eq!(
            selected,
            vec![1, 2, 3],
            "Shift+click should select range 1..=3, got {:?}",
            selected
        );
    }

    // --- Scroll boundary tests ---

    #[test]
    fn scroll_changes_visible_items() {
        // 100 items at 30px each. Viewport 300px → ~10 visible at a time.
        let model = ListModel::from_vec((0..100).collect());
        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model.clone(), move |i, _item, _selected| {
                // Encode model index in the leaf width so we can verify which items are visible
                Box::new(FixedLeaf(i as f32, 30.0))
            })
            .item_height(30.0),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Initially: items near index 0 should be visible
        let children = row_ids(&tree, lv_id);
        let first_y = tree.bounds(children[0]).y;
        assert!(
            first_y.abs() < 30.0,
            "First visible item should be near the top, got y={}",
            first_y
        );

        // Scroll down by 1500px (50 items * 30px)
        tree.dispatch_event(teksilo_core::event::WidgetEvent::Scroll {
            delta: teksilo_core::event::ScrollDelta::Pixels { x: 0.0, y: 1500.0 },
            modifiers: Default::default(),
        });
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // After scroll: the first item's Y should be near 0 (scroll offset applied),
        // and crucially it should NOT be the same items as before scroll.
        let children_after = row_ids(&tree, lv_id);
        let item_count_after = children_after.len() - 1;
        assert!(
            item_count_after > 0,
            "Should have visible items after scroll"
        );

        // The first visible item after scrolling 1500px should be positioned
        // near the top of the viewport. Its model position is ~index 50 (1500/30),
        // so its pre-scroll Y would have been 1500. After scroll offset, it's near 0.
        let first_y_after = tree.bounds(children_after[0]).y;
        assert!(
            first_y_after < 300.0,
            "First item should be in viewport after scroll, got y={}",
            first_y_after
        );

        // The pre-scroll first item was at y≈0. After scrolling, the first rendered
        // item should be at a different content position (not the same item).
        // We can verify by checking that the first item's Y is NOT at the same
        // content position as before. Before: item index 0 at y=0.
        // After: the first rendered item's content Y = first_y_after + 1500 ≈ 1500,
        // which corresponds to index ~50. So it's different items.
        // More directly: if we had the same items, their Y would be far outside
        // the viewport (y = 0 - 1500 = -1500), but we see y < 300.
        // This proves the ListView rebuilt with a different visible range.

        // Also verify we still have roughly the right count (not all 100)
        assert!(
            item_count_after < 30,
            "Should still be virtualized after scroll, got {} items",
            item_count_after
        );
    }

    // --- AccessKit tests ---

    #[test]
    fn list_item_has_a11y_role() {
        let (mut tree, lv_id, _model) = make_list_view(3, 30.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // The direct children of ListView are ListItemWrappers (+ scrollbar)
        let children = row_ids(&tree, lv_id);
        let info = tree.accessibility_node(children[0]);
        assert_eq!(
            info.role(),
            teksilo_core::accesskit::Role::ListBoxOption,
            "Item wrapper should have ListBoxOption role"
        );
    }

    // --- Alt+Arrow reorder test ---

    #[test]
    fn alt_arrow_moves_one_step_per_press_across_rebuilds() {
        // Regression for the "moves several lines per press" bug: rebuilds
        // were accumulating on_key handlers via HandlerSet merge semantics,
        // so the Nth Alt+Arrow press fired the reorder N times. Force a few
        // rebuilds (by mutating the selection signal) before pressing the
        // key, then confirm the item moves exactly one position.
        use teksilo_core::event::{Key, Modifiers};
        use teksilo_data::{SelectionMode, SelectionModel};

        let model = ListModel::from_vec(vec![10, 20, 30, 40, 50]);
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel_clone = selection.clone();
        let model_clone = model.clone();

        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model_clone, move |_i, _item, _sel| {
                Box::new(FixedLeaf(100.0, 30.0))
            })
            .item_height(30.0)
            .selection(sel_clone)
            .reorderable(true),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Force several rebuilds by toggling the selection a few times.
        // Each rebuild would previously merge a fresh on_key handler onto the
        // existing chain.
        for i in 0..3 {
            selection.select(i);
            tree.layout(SizeProposal::exact(400.0, 300.0));
        }

        selection.select(0);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        tree.focus(lv_id);
        tree.dispatch_event(teksilo_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowDown,
            modifiers: Modifiers::ALT,
            text: None,
        });

        // Expect a single swap: [10,20,30,40,50] → [20,10,30,40,50].
        assert_eq!(model.with_item(0, |v| *v), Some(20));
        assert_eq!(model.with_item(1, |v| *v), Some(10));
        assert_eq!(model.with_item(2, |v| *v), Some(30));
    }

    /// A 100-row list, ~10 rows to a viewport, focused and ready for keys.
    /// Returns the scroll signal too, so a test can assert that a key moved
    /// the viewport and not just the selection.
    fn keyboard_fixture(
        mode: teksilo_data::SelectionMode,
    ) -> (
        WidgetTree,
        teksilo_data::SelectionModel,
        Signal<f32>,
        SizeProposal,
    ) {
        use teksilo_data::SelectionModel;
        let model = ListModel::from_vec((0..100usize).collect());
        let selection = SelectionModel::new(mode);
        let sel = selection.clone();
        let mut tree = WidgetTree::new();
        let view = ListView::new(model, move |_i, _it, _s| Box::new(FixedLeaf(100.0, 20.0)))
            .item_height(20.0)
            .selection(sel);
        let scroll = view.scroll_y_signal().clone();
        let lv = tree.add(view);
        let p = SizeProposal::exact(400.0, 200.0);
        tree.layout(p);
        tree.focus(lv);
        (tree, selection, scroll, p)
    }

    #[test]
    fn home_and_end_reach_the_ends_of_the_collection() {
        use teksilo_core::event::{Key, Modifiers};
        let (mut tree, selection, _scroll, p) =
            keyboard_fixture(teksilo_data::SelectionMode::Single);
        selection.select(40);

        tree.press_key(Key::End, Modifiers::NONE);
        tree.layout(p);
        assert_eq!(selection.selected_indices(), vec![99]);

        tree.press_key(Key::Home, Modifiers::NONE);
        tree.layout(p);
        assert_eq!(selection.selected_indices(), vec![0]);
    }

    #[test]
    fn home_and_end_scroll_the_row_they_land_on_into_view() {
        use teksilo_core::event::{Key, Modifiers};
        let (mut tree, selection, scroll, p) =
            keyboard_fixture(teksilo_data::SelectionMode::Single);
        selection.select(0);
        tree.layout(p);

        tree.press_key(Key::End, Modifiers::NONE);
        tree.layout(p);
        // 100 rows of 20 dp in a 200 dp viewport: the last row is only visible
        // once the offset reaches the bottom of the content.
        let at_end = scroll.get();
        assert!(
            at_end > 1500.0,
            "End should scroll to the bottom of the content, got {at_end}"
        );

        tree.press_key(Key::Home, Modifiers::NONE);
        tree.layout(p);
        assert!(scroll.get() < 1.0, "Home should scroll back to the top");
    }

    #[test]
    fn the_accelerator_moves_the_cursor_without_disturbing_the_selection() {
        use teksilo_core::event::{Key, Modifiers};
        let (mut tree, selection, _scroll, p) =
            keyboard_fixture(teksilo_data::SelectionMode::Multi);
        selection.select(40);

        // Ctrl/⌘+End walks the cursor to the last row and leaves row 40 picked
        // — the rule GTK4 and Qt apply to every navigation key, and the one
        // Ctrl+Arrow already followed here.
        tree.press_key(Key::End, Modifiers::COMMAND);
        tree.layout(p);
        assert_eq!(selection.selected_indices(), vec![40]);

        tree.press_key(Key::Home, Modifiers::COMMAND);
        tree.layout(p);
        assert_eq!(selection.selected_indices(), vec![40]);

        // The cursor really did move, so a plain Home now selects where it is.
        tree.press_key(Key::PageDown, Modifiers::COMMAND);
        tree.layout(p);
        assert_eq!(selection.selected_indices(), vec![40]);
    }

    #[test]
    fn shift_end_then_shift_home_selects_one_range_not_the_whole_list() {
        use teksilo_core::event::{Key, Modifiers};
        let (mut tree, selection, _scroll, p) =
            keyboard_fixture(teksilo_data::SelectionMode::Multi);
        selection.select(10);

        tree.press_key(Key::End, Modifiers::SHIFT);
        tree.layout(p);
        assert_eq!(selection.count(), 90, "10..=99");

        tree.press_key(Key::Home, Modifiers::SHIFT);
        tree.layout(p);
        assert_eq!(
            selection.selected_indices(),
            (0..=10).collect::<Vec<_>>(),
            "reversing the gesture must shrink the range, not union with it"
        );
    }

    #[test]
    fn ctrl_shift_end_extends_without_losing_an_earlier_pick() {
        use teksilo_core::event::{Key, Modifiers};
        let (mut tree, selection, _scroll, p) =
            keyboard_fixture(teksilo_data::SelectionMode::Multi);
        selection.select(0);
        selection.toggle(95); // a disjoint pick; anchor moves to 95

        tree.press_key(Key::End, Modifiers::COMMAND | Modifiers::SHIFT);
        tree.layout(p);
        let got = selection.selected_indices();
        assert_eq!(got.first().copied(), Some(0), "the earlier pick survives");
        assert_eq!(got.last().copied(), Some(99));
        assert_eq!(got.len(), 6, "0 plus 95..=99");
    }

    #[test]
    fn ctrl_shift_a_deselects_everything() {
        use teksilo_core::event::{Key, Modifiers};
        let (mut tree, selection, _scroll, p) =
            keyboard_fixture(teksilo_data::SelectionMode::Multi);
        tree.press_key(Key::A, Modifiers::COMMAND);
        tree.layout(p);
        assert_eq!(selection.count(), 100);

        tree.press_key(Key::A, Modifiers::COMMAND | Modifiers::SHIFT);
        tree.layout(p);
        assert_eq!(selection.count(), 0);
    }

    #[test]
    fn page_down_up_moves_selection_by_viewport() {
        use teksilo_core::event::{Key, Modifiers};
        use teksilo_data::{SelectionMode, SelectionModel};

        let model = ListModel::from_vec((0..100usize).collect());
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel = selection.clone();
        let mut tree = WidgetTree::new();
        let lv = tree.add(
            ListView::new(model, move |_i, _it, _s| Box::new(FixedLeaf(100.0, 20.0)))
                .item_height(20.0)
                .selection(sel),
        );
        let p = SizeProposal::exact(400.0, 200.0); // ~10 rows visible
        tree.layout(p);
        tree.focus(lv);
        selection.select(0);

        tree.press_key(Key::PageDown, Modifiers::NONE);
        tree.layout(p);
        let after_pgdn = selection.selected_indices()[0];
        assert!(
            after_pgdn >= 8,
            "PageDown should advance ~one viewport of rows, got {after_pgdn}"
        );
        let scroll = with_list_view::<usize, _>(&tree, lv, |v| v.scroll_y_signal().get());
        assert!(scroll > 0.0, "PageDown scrolls to follow, got {scroll}");

        tree.press_key(Key::PageUp, Modifiers::NONE);
        tree.layout(p);
        assert!(
            selection.selected_indices()[0] < after_pgdn,
            "PageUp should move selection back up"
        );
    }

    #[test]
    fn space_toggles_selection_enter_activates() {
        use std::cell::Cell;
        use teksilo_core::event::{Key, Modifiers};
        use teksilo_data::{SelectionMode, SelectionModel};

        let model = ListModel::from_vec((0..5usize).collect());
        let selection = SelectionModel::new(SelectionMode::Multi);
        let sel = selection.clone();
        let activated = Rc::new(Cell::new(None));
        let act = activated.clone();
        let mut tree = WidgetTree::new();
        let lv = tree.add(
            ListView::new(model, move |_i, _it, _s| Box::new(FixedLeaf(100.0, 20.0)))
                .item_height(20.0)
                .selection(sel)
                .on_activate(move |i, _ctx| act.set(Some(i))),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));
        tree.focus(lv);

        // Move the cursor to row 2: the first Down lands ON row 0 (it does not
        // skip it), so it takes three.
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(selection.selected_indices(), vec![2]);
        assert_eq!(activated.get(), None, "arrows never activate");

        // Space toggles the focused row's selection OFF (Multi), no activate.
        tree.press_key(Key::Space, Modifiers::NONE);
        assert!(
            selection.selected_indices().is_empty(),
            "Space toggles row 2 off in Multi mode"
        );
        assert_eq!(activated.get(), None, "Space must NOT activate");

        // Enter activates the focused row (and selects it).
        tree.press_key(Key::Enter, Modifiers::NONE);
        assert_eq!(activated.get(), Some(2), "Enter activates the focused row");
    }

    #[test]
    fn ctrl_a_selects_all_in_multi_mode() {
        use teksilo_core::event::{Key, Modifiers};
        use teksilo_data::{SelectionMode, SelectionModel};

        let model = ListModel::from_vec((0..6usize).collect());
        let selection = SelectionModel::new(SelectionMode::Multi);
        let sel = selection.clone();
        let mut tree = WidgetTree::new();
        let lv = tree.add(
            ListView::new(model, move |_i, _it, _s| Box::new(FixedLeaf(100.0, 20.0)))
                .item_height(20.0)
                .selection(sel),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));
        tree.focus(lv);
        tree.press_key(Key::A, Modifiers::COMMAND);
        assert_eq!(selection.selected_indices().len(), 6, "Ctrl+A selects all");
    }

    #[test]
    fn ctrl_arrow_moves_cursor_without_selecting_in_multi_mode() {
        use teksilo_core::event::{Key, Modifiers};
        use teksilo_data::{SelectionMode, SelectionModel};

        let model = ListModel::from_vec((0..6usize).collect());
        let selection = SelectionModel::new(SelectionMode::Multi);
        let sel = selection.clone();
        let mut tree = WidgetTree::new();
        let lv = tree.add(
            ListView::new(model, move |_i, _it, _s| Box::new(FixedLeaf(100.0, 20.0)))
                .item_height(20.0)
                .selection(sel),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));
        tree.focus(lv);

        // Plain Arrow still selects (the first Down lands ON row 0).
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(selection.selected_indices(), vec![0]);

        // Ctrl+ArrowDown moves the cursor without touching the selection.
        tree.press_key(Key::ArrowDown, Modifiers::CTRL);
        assert_eq!(
            selection.selected_indices(),
            vec![0],
            "Ctrl+ArrowDown must leave the selection unchanged"
        );
        let focused = with_list_view::<usize, _>(&tree, lv, |v| v.focused_index.get());
        assert_eq!(focused, Some(1), "Ctrl+ArrowDown moves the cursor to row 1");

        tree.press_key(Key::ArrowDown, Modifiers::CTRL);
        assert_eq!(selection.selected_indices(), vec![0], "still unchanged");
        let focused = with_list_view::<usize, _>(&tree, lv, |v| v.focused_index.get());
        assert_eq!(focused, Some(2));

        // Ctrl+Space toggles the now-focused row (row 2) on, adding to —
        // not replacing — the existing selection.
        tree.press_key(Key::Space, Modifiers::CTRL);
        assert_eq!(selection.selected_indices(), vec![0, 2]);

        // Ctrl+Space again toggles it back off.
        tree.press_key(Key::Space, Modifiers::CTRL);
        assert_eq!(selection.selected_indices(), vec![0]);

        // Plain Arrow after a Ctrl-cursor move still replaces the
        // selection with the new cursor position (select-follow).
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(selection.selected_indices(), vec![3]);
    }

    #[test]
    fn ctrl_arrow_moves_cursor_without_selecting_in_single_mode() {
        use teksilo_core::event::{Key, Modifiers};
        use teksilo_data::{SelectionMode, SelectionModel};

        let model = ListModel::from_vec((0..6usize).collect());
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel = selection.clone();
        let mut tree = WidgetTree::new();
        let lv = tree.add(
            ListView::new(model, move |_i, _it, _s| Box::new(FixedLeaf(100.0, 20.0)))
                .item_height(20.0)
                .selection(sel),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));
        tree.focus(lv);

        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(selection.selected_indices(), vec![0]);

        tree.press_key(Key::ArrowDown, Modifiers::CTRL);
        assert_eq!(
            selection.selected_indices(),
            vec![0],
            "Ctrl+ArrowDown must not select in Single mode either"
        );
        let focused = with_list_view::<usize, _>(&tree, lv, |v| v.focused_index.get());
        assert_eq!(focused, Some(1));

        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(selection.selected_indices(), vec![2]);
    }

    #[test]
    fn type_ahead_jumps_to_matching_row() {
        use teksilo_core::event::{Key, Modifiers};
        use teksilo_data::{SelectionMode, SelectionModel};

        let model = ListModel::from_vec(vec![
            "Apple".to_string(),
            "Banana".to_string(),
            "Cherry".to_string(),
            "Cranberry".to_string(),
            "Date".to_string(),
        ]);
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel = selection.clone();
        let mut tree = WidgetTree::new();
        let lv = tree.add(
            ListView::new(model, move |_i, _it, _s| Box::new(FixedLeaf(100.0, 20.0)))
                .item_height(20.0)
                .selection(sel)
                .type_ahead_label(|s: &String| s.clone()),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));
        tree.focus(lv);
        selection.select(0);

        // Type 'c' → jumps to "Cherry" (first item after 0 starting with c).
        tree.press_key(Key::C, Modifiers::NONE);
        assert_eq!(selection.selected_indices(), vec![2], "'c' → Cherry");

        // Type 'r' within timeout → buffer "cr" → "Cranberry".
        tree.press_key(Key::R, Modifiers::NONE);
        assert_eq!(selection.selected_indices(), vec![3], "'cr' → Cranberry");
    }

    #[test]
    fn type_ahead_buffer_survives_rebuild() {
        // The persistent-field design under test: each keystroke changes the
        // selection, which schedules a rebuild. Force that rebuild between the
        // two keystrokes; the accumulated buffer ("c" then "cr") must survive,
        // or multi-char search is impossible.
        use teksilo_core::event::{Key, Modifiers};
        use teksilo_data::{SelectionMode, SelectionModel};

        let model = ListModel::from_vec(vec![
            "Apple".to_string(),
            "Cherry".to_string(),
            "Cranberry".to_string(),
        ]);
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel = selection.clone();
        let mut tree = WidgetTree::new();
        let p = SizeProposal::exact(400.0, 200.0);
        let lv = tree.add(
            ListView::new(model, move |_i, _it, _s| Box::new(FixedLeaf(100.0, 20.0)))
                .item_height(20.0)
                .selection(sel)
                .type_ahead_label(|s: &String| s.clone()),
        );
        tree.layout(p);
        tree.focus(lv);
        selection.select(0);

        tree.press_key(Key::C, Modifiers::NONE); // → Cherry (idx 1)
        assert_eq!(selection.selected_indices(), vec![1]);
        tree.layout(p); // <-- the rebuild that would reset a build()-local buffer
        tree.press_key(Key::R, Modifiers::NONE); // "cr" → Cranberry (idx 2)
        assert_eq!(
            selection.selected_indices(),
            vec![2],
            "buffer 'c' must survive the rebuild so 'cr' matches Cranberry"
        );
    }

    #[test]
    fn page_down_on_short_list_jumps_to_last_without_panic() {
        use teksilo_core::event::{Key, Modifiers};
        use teksilo_data::{SelectionMode, SelectionModel};

        // 3 rows in a 200px (~10-row) viewport — content shorter than a page.
        let model = ListModel::from_vec((0..3usize).collect());
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel = selection.clone();
        let mut tree = WidgetTree::new();
        let lv = tree.add(
            ListView::new(model, move |_i, _it, _s| Box::new(FixedLeaf(100.0, 20.0)))
                .item_height(20.0)
                .selection(sel),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));
        tree.focus(lv);
        selection.select(0);
        tree.press_key(Key::PageDown, Modifiers::NONE);
        assert_eq!(selection.selected_indices(), vec![2], "PageDown → last row");
        tree.press_key(Key::PageDown, Modifiers::NONE);
        assert_eq!(selection.selected_indices(), vec![2], "stays at last");
        tree.press_key(Key::PageUp, Modifiers::NONE);
        assert_eq!(selection.selected_indices(), vec![0], "PageUp → first row");
    }

    #[test]
    fn alt_arrow_reorders_item() {
        use teksilo_core::event::{Key, Modifiers};
        use teksilo_data::{SelectionMode, SelectionModel};

        let model = ListModel::from_vec(vec![10, 20, 30, 40, 50]);
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel_clone = selection.clone();
        let model_clone = model.clone();

        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model_clone.clone(), move |_i, _item, _sel| {
                Box::new(FixedLeaf(100.0, 30.0))
            })
            .item_height(30.0)
            .selection(sel_clone)
            .reorderable(true),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Select item at index 2 (value 30)
        selection.select(2);

        // Focus the ListView and press Alt+ArrowDown
        tree.focus(lv_id);
        tree.dispatch_event(teksilo_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowDown,
            modifiers: Modifiers::ALT,
            text: None,
        });

        // Item 30 should now be at index 3
        assert_eq!(model.with_item(3, |v| *v), Some(30));
        assert_eq!(model.with_item(2, |v| *v), Some(40));
    }

    // --- Drag-and-drop integration tests ---

    /// Build a reorderable ListView at the tree root with the given values.
    /// Returns (tree, ListView id, model).
    fn make_reorderable_list(
        values: Vec<usize>,
        item_height: f32,
    ) -> (WidgetTree, WidgetId, ListModel<usize>) {
        let model = ListModel::from_vec(values);
        let model_clone = model.clone();
        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model_clone, move |_i, _item, _sel| {
                Box::new(FixedLeaf(100.0, item_height))
            })
            .item_height(item_height)
            .reorderable(true),
        );
        (tree, lv_id, model)
    }

    /// Run a full drag gesture: PointerDown on source, Move to cross threshold,
    /// Move to target, Up.
    fn drag_item(tree: &mut WidgetTree, from: Point, to: Point) {
        use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: from,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        // Cross drag threshold (default 5px)
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(from.x + 10.0, from.y),
        });
        tree.dispatch_event(WidgetEvent::PointerMove { position: to });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: to,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
    }

    #[test]
    fn drag_reorders_item_downward() {
        let (mut tree, lv_id, model) = make_reorderable_list(vec![10, 20, 30, 40, 50], 30.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Source: item 0 (y=0..30, center y=15). Target: between item 3 and 4
        // (y=120; insertion index = round((120 + 15) / 30) = 4 → after index-shift = 3).
        let children = row_ids(&tree, lv_id);
        let from = tree.bounds(children[0]).center();
        let to = Point::new(from.x, 120.0);
        drag_item(&mut tree, from, to);

        // After move: [20, 30, 40, 10, 50]
        assert_eq!(model.with_item(0, |v| *v), Some(20));
        assert_eq!(model.with_item(3, |v| *v), Some(10));
        assert_eq!(model.with_item(4, |v| *v), Some(50));
    }

    #[test]
    fn drag_reorders_item_upward() {
        let (mut tree, lv_id, model) = make_reorderable_list(vec![10, 20, 30, 40, 50], 30.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Source: item 3 (value 40, y=90..120, center y=105). Target: y=15 (just
        // below top → insertion index 1).
        let children = row_ids(&tree, lv_id);
        let from = tree.bounds(children[3]).center();
        let to = Point::new(from.x, 15.0);
        drag_item(&mut tree, from, to);

        // After move: [10, 40, 20, 30, 50]
        assert_eq!(model.with_item(1, |v| *v), Some(40));
        assert_eq!(model.with_item(2, |v| *v), Some(20));
        assert_eq!(model.with_item(3, |v| *v), Some(30));
    }

    #[test]
    fn reorderable_drag_routes_to_source_accept_drop() {
        // The redesign's core: a reorderable ListView routes the drop to the
        // SOURCE's accept_drop. A source can apply the move to its own store
        // (`ListModel` does) or, for an externally-owned store, capture it and
        // reconcile later. This source captures (from, to) WITHOUT mutating,
        // proving the controlled path with no on_reorder hook.
        use std::cell::RefCell;
        use std::rc::Rc;
        use teksilo_core::ObserverHandle;
        use teksilo_data::{
            DragEligibility, DragSource, DropCommit, DropPosition, DropQuery, DropResponse,
            ListDataSource,
        };

        struct CapturingSource {
            items: Vec<usize>,
            captured: Rc<RefCell<Vec<(usize, usize)>>>,
        }
        impl ListDataSource for CapturingSource {
            type Item = usize;
            type Key = usize;
            fn len(&self) -> usize {
                self.items.len()
            }
            fn with_item<R>(&self, i: usize, f: impl FnOnce(&usize) -> R) -> Option<R> {
                self.items.get(i).map(f)
            }
            fn key_at(&self, i: usize) -> Option<usize> {
                (i < self.items.len()).then_some(i)
            }
            fn observe_changes(
                &self,
                _f: impl Fn(&teksilo_data::DataChange) + 'static,
            ) -> ObserverHandle {
                let inner: Rc<dyn std::any::Any> = Rc::new(());
                ObserverHandle::new(inner, 0, Rc::new(|_| {}))
            }
            fn drag(&self, _k: &usize) -> DragEligibility {
                DragEligibility::CanDrag
            }
            fn can_accept(&self, q: &DropQuery<'_, usize>) -> DropResponse {
                match &q.source {
                    DragSource::SameView { .. } if q.position != DropPosition::Into => {
                        DropResponse::Accept
                    }
                    _ => DropResponse::Reject,
                }
            }
            fn accept_drop(&self, c: DropCommit<'_, usize>) -> bool {
                let DragSource::SameView { key: from } = c.source else {
                    return false;
                };
                let target = c.target;
                let shift = if from < target { 1 } else { 0 };
                let to = match c.position {
                    DropPosition::Before => target.saturating_sub(shift),
                    DropPosition::After => (target + 1).saturating_sub(shift),
                    DropPosition::Into => return false,
                };
                // Controlled: capture the resolved move, do NOT mutate `items`.
                self.captured.borrow_mut().push((from, to));
                true
            }
        }

        let captured: Rc<RefCell<Vec<(usize, usize)>>> = Rc::new(RefCell::new(Vec::new()));
        let source = CapturingSource {
            items: vec![10, 20, 30, 40, 50],
            captured: captured.clone(),
        };
        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::from_source(source, move |_i, _item, _sel| {
                Box::new(FixedLeaf(100.0, 30.0))
            })
            .item_height(30.0)
            .reorderable(true),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Drag item 0 down to y=120 → insertion index 4 → (target 4, Before),
        // which the source resolves to the move (from 0, to 3).
        let children = row_ids(&tree, lv_id);
        let from = tree.bounds(children[0]).center();
        let to = Point::new(from.x, 120.0);
        drag_item(&mut tree, from, to);

        assert_eq!(
            *captured.borrow(),
            vec![(0, 3)],
            "the drop is routed to the source's accept_drop with the resolved move"
        );
    }

    #[test]
    fn drag_emits_items_moved_change() {
        use std::cell::Cell;
        use std::rc::Rc;
        use teksilo_data::DataChange;

        let (mut tree, lv_id, model) = make_reorderable_list(vec![10, 20, 30, 40, 50], 30.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let moved = Rc::new(Cell::new(None::<(usize, usize)>));
        let moved_clone = moved.clone();
        let handle = model.observe_changes(move |change| {
            if let DataChange::ItemsMoved { from, to, .. } = change {
                moved_clone.set(Some((*from, *to)));
            }
        });

        // Drag item 0 down to index 3
        let children = row_ids(&tree, lv_id);
        let from = tree.bounds(children[0]).center();
        let to = Point::new(from.x, 120.0);
        drag_item(&mut tree, from, to);

        assert_eq!(moved.get(), Some((0, 3)));
        drop(handle);
    }

    #[test]
    fn drag_drop_accounts_for_scroll_offset() {
        // 20 items, 30px each (total 600px). Scroll by 60px (2 items) so that
        // item 2 sits at tree y=0.
        let (mut tree, _lv_id, model) = make_reorderable_list((0..20).collect(), 30.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // The Scroll event only dispatches to the hovered or focused widget.
        // Move the pointer over the ListView so it becomes hovered.
        tree.pointer_move(Point::new(50.0, 50.0));
        tree.dispatch_event(teksilo_core::event::WidgetEvent::Scroll {
            delta: teksilo_core::event::ScrollDelta::Pixels { x: 0.0, y: 60.0 },
            modifiers: Default::default(),
        });
        // Wheel scrolling animates; complete it so the offset is the full
        // 60px before the drag math runs.
        tree.tick_animations(std::time::Duration::from_millis(200));
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Drag from tree y=15 (center of item 2) down to tree y=120 (middle
        // of viewport). In the on_drop handler: content_y = 120 + 60 = 180,
        // target_index = (180 + 15) / 30 = 6. Source index = 2, from < to, so
        // adjusted_to = 5. move_item(2, 5) gives [0, 1, 3, 4, 5, 2, 6, ...].
        let from = Point::new(50.0, 15.0);
        let to = Point::new(50.0, 120.0);
        drag_item(&mut tree, from, to);

        assert_eq!(
            model.with_item(5, |v| *v),
            Some(2),
            "Item 2 should land at index 5 after drag with scroll offset"
        );
        assert_eq!(
            model.with_item(2, |v| *v),
            Some(3),
            "Item 3 should shift up to index 2"
        );
    }

    #[test]
    fn click_selects_item_on_reorderable_list_with_selection() {
        // Regression — the user reports that after the recent framework
        // round they can drag but not select. Reproduce the exact combo:
        // a ListView that is BOTH reorderable and selectable, a simple
        // click (PointerDown + PointerUp at the same point, no move),
        // and assert:
        //   1. the SelectionModel signal updates, AND
        //   2. a subsequent rebuild re-invokes the delegate with the new
        //      `selected` flag so the view actually reflects the change.
        use std::cell::Cell;
        use std::rc::Rc;
        use teksilo_data::{SelectionMode, SelectionModel};

        let model = ListModel::from_vec(vec![10, 20, 30, 40, 50]);
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel_clone = selection.clone();
        let model_clone = model.clone();

        // Record which indices were delegated as `selected=true` on each
        // build pass so we can assert post-click rebuild.
        let selected_rebuilds: Rc<std::cell::RefCell<Vec<Vec<usize>>>> =
            Rc::new(std::cell::RefCell::new(Vec::new()));
        let current_pass: Rc<Cell<Vec<usize>>> = Rc::new(Cell::new(Vec::new()));
        let _sr = selected_rebuilds.clone();
        let cp = current_pass.clone();

        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model_clone, move |index, _item, selected| {
                if selected {
                    let mut acc = cp.take();
                    acc.push(index);
                    cp.set(acc);
                }
                Box::new(FixedLeaf(100.0, 30.0))
            })
            .item_height(30.0)
            .selection(sel_clone)
            .reorderable(true),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        selected_rebuilds.borrow_mut().push(current_pass.take());

        // Click item 2.
        let children = row_ids(&tree, lv_id);
        tree.click(children[2]);

        // 1. Selection model updated.
        assert_eq!(selection.selected_indices(), vec![2]);

        // 2. A layout tick after the click must rebuild and deliver the
        //    new selection state to the delegate.
        tree.layout(SizeProposal::exact(400.0, 300.0));
        selected_rebuilds.borrow_mut().push(current_pass.take());

        let passes = selected_rebuilds.borrow().clone();
        assert_eq!(
            passes[0],
            Vec::<usize>::new(),
            "initial build: nothing selected"
        );
        assert_eq!(
            passes[1],
            vec![2],
            "post-click rebuild should deliver selected=true for item 2"
        );
    }

    #[test]
    fn drag_survives_rebuild_triggered_by_selection() {
        // Regression: user clicks a list row (with .selection() set), which
        // fires the selection handler → marks the ListView for rebuild. The
        // same PointerDown also arms the DragRecognizer on the item wrapper
        // and installs pointer capture at that wrapper. When rebuild runs,
        // the OLD wrapper is destroyed and NEW wrappers are created. Without
        // revalidating `pointer_captured_by`, the next PointerMove is routed
        // to the destroyed wrapper id and silently dropped, so the drag
        // gesture never progresses past DragRecognizer::Pending and the user
        // can select but not drag.
        use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
        use teksilo_data::{SelectionMode, SelectionModel};

        let model = ListModel::from_vec(vec![10, 20, 30, 40, 50]);
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel_clone = selection.clone();
        let model_clone = model.clone();

        let mut tree = WidgetTree::new();
        let _lv_id = tree.add(
            ListView::new(model_clone, move |_i, _item, _sel| {
                Box::new(FixedLeaf(100.0, 30.0))
            })
            .item_height(30.0)
            .selection(sel_clone)
            .reorderable(true),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Click-and-drag item 0 down to row 3.
        //
        // PointerDown: fires the selection handler on the wrapper — this
        // trips the selection signal, which dirty-marks the ListView for
        // rebuild. Bubble reaches the wrapper, arms the gesture arena, and
        // captures the pointer at the old wrapper id.
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(50.0, 15.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        // Force the rebuild to run *before* the drag progresses — this is
        // the ordering the real app hits because layout runs between the
        // PointerDown and the first PointerMove. Old wrappers are destroyed
        // here; new ones take their place with different widget ids.
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Cross drag threshold.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(60.0, 15.0),
        });
        // Move to target.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(60.0, 120.0),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(60.0, 120.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        // Item 0 (value 10) should have moved to index 3.
        assert_eq!(
            model.with_item(3, |v| *v),
            Some(10),
            "Drag must complete even after the selection-triggered rebuild \
             destroyed the originally-captured wrapper"
        );
    }

    #[test]
    fn lazy_loading_rows_render_placeholders_and_request_the_window() {
        // A windowed source with nothing resident: every visible row is
        // `Loading`, so the ListView must render placeholder skeletons (not
        // skip the rows) and nudge the source to load the realized window.
        use std::cell::RefCell;
        use std::ops::Range;
        use std::rc::Rc;
        use teksilo_core::ObserverHandle;
        use teksilo_data::{ListDataSource, RowState};

        struct Windowed {
            total: usize,
            requested: Rc<RefCell<Vec<Range<usize>>>>,
        }
        impl ListDataSource for Windowed {
            type Item = usize;
            type Key = usize;
            fn len(&self) -> usize {
                self.total
            }
            fn with_item<R>(&self, _i: usize, _f: impl FnOnce(&usize) -> R) -> Option<R> {
                None // nothing resident yet
            }
            fn key_at(&self, i: usize) -> Option<usize> {
                (i < self.total).then_some(i)
            }
            fn row_state(&self, _i: usize) -> RowState {
                RowState::Loading
            }
            fn request_window(&self, range: Range<usize>) {
                self.requested.borrow_mut().push(range);
            }
            fn observe_changes(
                &self,
                _f: impl Fn(&teksilo_data::DataChange) + 'static,
            ) -> ObserverHandle {
                let inner: Rc<dyn std::any::Any> = Rc::new(());
                ObserverHandle::new(inner, 0, Rc::new(|_| {}))
            }
        }

        let requested = Rc::new(RefCell::new(Vec::new()));
        let source = Windowed {
            total: 1000,
            requested: requested.clone(),
        };
        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::from_source(source, |_i, _item, _sel| Box::new(FixedLeaf(100.0, 30.0)))
                .item_height(30.0),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // 300px / 30px = 10 visible + buffer → the loading rows are realized as
        // placeholder child widgets (children minus the scrollbar), NOT skipped.
        let placeholder_rows = row_ids(&tree, lv_id).len();
        assert!(
            placeholder_rows >= 10,
            "loading rows must render as placeholders, got {placeholder_rows}"
        );
        // And the source was asked to load the realized window.
        assert!(
            !requested.borrow().is_empty(),
            "request_window must be called for the visible range"
        );
    }

    /// Helper: borrow the ListView widget at `id` via the downcast hook
    /// and run a closure against it.
    fn with_list_view<T: 'static, R>(
        tree: &WidgetTree,
        id: WidgetId,
        f: impl FnOnce(&ListView<T>) -> R,
    ) -> R {
        let any = tree.widget_as_any(id).expect("widget exposes as_any");
        let lv = any
            .downcast_ref::<ListView<T>>()
            .expect("widget is a ListView<T>");
        f(lv)
    }

    #[test]
    fn drop_indicator_clears_after_drop() {
        // Regression for the "insertion line lingers after drop" bug —
        // the ListView's drop_feedback Signal must be None once the
        // drag has ended, whether the drop was accepted or not.
        let (mut tree, lv_id, _model) = make_reorderable_list(vec![1, 2, 3, 4, 5], 30.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        drag_item(&mut tree, Point::new(50.0, 15.0), Point::new(50.0, 105.0));

        let feedback =
            with_list_view::<usize, _>(&tree, lv_id, |lv| lv.drop_feedback_signal().get());
        assert!(
            feedback.is_none(),
            "drop_feedback must be cleared by on_drag_leave after drop, got {:?}",
            feedback
        );
    }

    #[test]
    fn drag_spawns_preview_overlay_and_cleans_up() {
        use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

        let (mut tree, _lv_id, _model) = make_reorderable_list(vec![1, 2, 3, 4, 5], 30.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let baseline = tree.overlay_manager().len();

        // PointerDown + threshold-crossing PointerMove starts the drag.
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(50.0, 15.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(60.0, 15.0),
        });

        assert_eq!(
            tree.overlay_manager().len(),
            baseline + 1,
            "Preview overlay should be live during drag"
        );

        // Drop — preview should be dismissed.
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(60.0, 15.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(
            tree.overlay_manager().len(),
            baseline,
            "Preview overlay should be dismissed after drop"
        );
    }

    #[test]
    fn edge_auto_scroll_advances_scroll_y_during_drag() {
        use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

        // 50 items, 30 px each (1500 px of content) in a 300 px viewport.
        let (mut tree, _lv_id, _model) =
            make_reorderable_list((0..50).collect::<Vec<usize>>(), 30.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Kick off a drag and move the pointer near the BOTTOM edge so
        // the on_drag_tick scroll delta is positive.
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(50.0, 15.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(60.0, 15.0),
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(60.0, 290.0), // inside bottom 32 px edge zone
        });

        // Drive layout a few times to accumulate on_drag_tick fires.
        for _ in 0..8 {
            tree.layout(SizeProposal::exact(400.0, 300.0));
        }
        let scroll_y = with_list_view::<usize, _>(&tree, _lv_id, |lv| lv.scroll_y_signal().get());
        assert!(
            scroll_y > 5.0,
            "Edge auto-scroll should have advanced scroll_y; got {scroll_y}"
        );

        // Clean up the drag.
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(60.0, 290.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
    }

    // -- Boundary scroll chaining -------------------------------------------

    /// A ListView (40 × 30px items in a 100px viewport → 1100px of scroll)
    /// stacked above a filler inside an outer ScrollArea, so chaining from the
    /// inner list to the outer area is observable.
    fn nested_list_fixture(inner: OverscrollBehavior) -> (WidgetTree, Signal<f32>, Signal<f32>) {
        use crate::ScrollArea;
        use crate::primitives::{FixedSize, VStack};
        let mut tree = WidgetTree::new();
        let model = ListModel::from_vec((0..40_usize).collect());
        let lv = ListView::new(model, move |_i, _item, _sel| {
            Box::new(FixedLeaf(180.0, 30.0))
        })
        .item_height(30.0)
        .overscroll_behavior(inner);
        let inner_y = lv.scroll_y_signal().clone();
        let lv_id = tree.add(lv);
        let viewport = tree.add(FixedSize::new().width(200.0).height(100.0).child_id(lv_id));
        let filler = tree.add(FixedLeaf(200.0, 200.0));
        let outer_content = tree.add(VStack::new().add_child(viewport).add_child(filler));
        let outer = ScrollArea::from_id(outer_content).smooth_scrolling(false);
        let outer_y = outer.scroll_y_signal().clone();
        let _outer = tree.add(outer);
        tree.layout(SizeProposal::exact(200.0, 150.0));
        (tree, inner_y, outer_y)
    }

    #[test]
    fn nested_list_chains_to_outer_at_boundary() {
        use teksilo_core::event::{Modifiers, ScrollDelta, WidgetEvent};
        let (mut tree, inner_y, outer_y) = nested_list_fixture(OverscrollBehavior::Chain);
        tree.pointer_move(Point::new(50.0, 40.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 9999.0 },
            modifiers: Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(200.0, 150.0));
        let inner_bottom = inner_y.get();
        assert!(
            inner_bottom > 0.0,
            "inner list should scroll down; got {inner_bottom}"
        );
        assert!(
            outer_y.get() < 0.01,
            "outer must not move while the inner absorbs"
        );

        tree.pointer_move(Point::new(50.0, 40.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 100.0 },
            modifiers: Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(200.0, 150.0));
        assert!(
            (inner_y.get() - inner_bottom).abs() < 0.01,
            "inner stays clamped at bottom"
        );
        assert!(
            outer_y.get() > 0.01,
            "outer scrolled because the inner chained the boundary"
        );
    }

    #[test]
    fn nested_list_contain_blocks_chaining() {
        use teksilo_core::event::{Modifiers, ScrollDelta, WidgetEvent};
        let (mut tree, _inner_y, outer_y) = nested_list_fixture(OverscrollBehavior::Contain);
        tree.pointer_move(Point::new(50.0, 40.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 9999.0 },
            modifiers: Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(200.0, 150.0));
        tree.pointer_move(Point::new(50.0, 40.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 100.0 },
            modifiers: Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(200.0, 150.0));
        assert!(
            outer_y.get() < 0.01,
            "Contain must prevent chaining: outer stays put"
        );
    }

    #[test]
    fn keyboard_selection_chases_outer_scroll_area() {
        // A 200px ListView (20 × 20px rows → scrolls internally) whose lower
        // half sits below a 100px outer ScrollArea's fold. Arrow-key selection
        // is not a focus change (the list keeps focus, `active_descendant`
        // style), so the framework's focus-driven follow never reveals the
        // selected row — `ctx.ensure_visible` must.
        use crate::ScrollArea;
        use crate::primitives::{FixedSize, VStack};
        use teksilo_core::event::{Key, Modifiers};
        use teksilo_data::{SelectionMode, SelectionModel};

        let mut tree = WidgetTree::new();
        let model = ListModel::from_vec((0..20_usize).collect());
        let selection = SelectionModel::new(SelectionMode::Single);
        let lv = ListView::new(model, |_i, _item, _sel| Box::new(FixedLeaf(180.0, 20.0)))
            .item_height(20.0)
            .selection(selection);
        let lv_id = tree.add(lv);
        let lv_box = tree.add(FixedSize::new().width(200.0).height(200.0).child_id(lv_id));
        let filler = tree.add(FixedLeaf(200.0, 200.0));
        let outer_content = tree.add(VStack::new().add_child(lv_box).add_child(filler));
        let outer = ScrollArea::from_id(outer_content).smooth_scrolling(false);
        let outer_y = outer.scroll_y_signal().clone();
        let _outer = tree.add(outer);
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Focus scrolls the outer to reveal the tall list; reset so any further
        // scroll is attributable to the row-selection chase.
        tree.focus(lv_id);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        outer_y.set(0.0);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert!(outer_y.get().abs() < 0.01, "reset outer to top");

        // Select down toward the bottom rows (below the outer fold).
        for _ in 0..20 {
            tree.press_key(Key::ArrowDown, Modifiers::NONE);
        }
        tree.layout(SizeProposal::exact(200.0, 100.0));

        assert!(
            outer_y.get() > 0.01,
            "selecting a row below the outer fold must scroll the enclosing \
             ScrollArea (got {})",
            outer_y.get()
        );
    }

    // --- Variable row heights ---

    /// Collect the (y, height) bounds of the realized item children (the
    /// scrollbar is always the last child), sorted by y.
    fn item_spans(tree: &WidgetTree, lv_id: WidgetId) -> Vec<(f32, f32)> {
        let children = row_ids(tree, lv_id);
        let mut spans: Vec<(f32, f32)> = children[..]
            .iter()
            .map(|c| {
                let b = tree.bounds(*c);
                (b.y, b.height)
            })
            .collect();
        spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        spans
    }

    #[test]
    fn exact_item_height_fn_positions_rows_at_callback_heights() {
        let heights = [100.0_f32, 20.0, 50.0];
        let model = ListModel::from_vec(vec![0_usize, 1, 2]);
        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model, |_i, _item, _sel| Box::new(FixedLeaf(100.0, 30.0)))
                .item_height_fn(move |i| heights[i]),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let spans = item_spans(&tree, lv_id);
        assert_eq!(spans.len(), 3);
        assert!((spans[0].0 - 0.0).abs() < 0.01 && (spans[0].1 - 100.0).abs() < 0.01);
        assert!((spans[1].0 - 100.0).abs() < 0.01 && (spans[1].1 - 20.0).abs() < 0.01);
        assert!((spans[2].0 - 120.0).abs() < 0.01 && (spans[2].1 - 50.0).abs() < 0.01);
    }

    #[test]
    fn exact_heights_with_spacing() {
        let heights = [100.0_f32, 20.0, 50.0];
        let model = ListModel::from_vec(vec![0_usize, 1, 2]);
        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model, |_i, _item, _sel| Box::new(FixedLeaf(100.0, 30.0)))
                .item_height_fn(move |i| heights[i])
                .spacing(8.0),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let spans = item_spans(&tree, lv_id);
        assert!((spans[1].0 - 108.0).abs() < 0.01);
        assert!((spans[2].0 - 136.0).abs() < 0.01);
    }

    #[test]
    fn variable_heights_virtualize() {
        let model = ListModel::from_vec((0..10_000).collect::<Vec<usize>>());
        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model, |_i, _item, _sel| Box::new(FixedLeaf(100.0, 30.0)))
                .item_height_fn(|i| 20.0 + (i % 5) as f32 * 10.0),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let item_count = row_ids(&tree, lv_id).len();
        assert!(
            item_count < 40,
            "Expected fewer than 40 realized rows, got {item_count}"
        );
        assert!(
            item_count >= 8,
            "Expected at least 8 rows, got {item_count}"
        );
    }

    #[test]
    fn auto_measure_corrects_rows_from_estimate() {
        // Delegate rows are 30 px tall; the estimate says 50. After the
        // measure pass, row 1 must sit at y = 30, not 50.
        let model = ListModel::from_vec(vec![0_usize, 1, 2, 3]);
        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model, |_i, _item, _sel| Box::new(FixedLeaf(100.0, 30.0)))
                .auto_item_height(50.0),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let spans = item_spans(&tree, lv_id);
        assert!(
            (spans[1].0 - 30.0).abs() < 0.01,
            "row 1 should sit at measured 30, got {}",
            spans[1].0
        );
        assert!((spans[1].1 - 30.0).abs() < 0.01);
    }

    #[test]
    fn auto_measure_under_realization_converges() {
        // Estimate 100, actual 20: the first build realizes far too few
        // rows for the viewport. The post-measure realization re-check
        // must request rebuilds until realized rows tile the viewport.
        let model = ListModel::from_vec((0..200).collect::<Vec<usize>>());
        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model, |_i, _item, _sel| Box::new(FixedLeaf(100.0, 20.0)))
                .auto_item_height(100.0),
        );
        // Let the re-check / rebuild cycle settle.
        for _ in 0..6 {
            tree.layout(SizeProposal::exact(400.0, 300.0));
        }

        let spans = item_spans(&tree, lv_id);
        // Contiguous tiling from the top…
        let mut expected_y = spans[0].0;
        for (y, h) in &spans {
            assert!(
                (y - expected_y).abs() < 0.01,
                "rows must tile contiguously: expected y {expected_y}, got {y}"
            );
            expected_y = y + h;
        }
        // …and full viewport coverage (no gap at the bottom).
        let last_bottom = spans.last().map(|(y, h)| y + h).unwrap();
        assert!(
            last_bottom >= 300.0,
            "realized rows must cover the viewport bottom, got {last_bottom}"
        );
    }

    #[test]
    fn auto_measure_append_preserves_measured_prefix() {
        let model = ListModel::from_vec((0..4).collect::<Vec<usize>>());
        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model.clone(), |_i, _item, _sel| {
                Box::new(FixedLeaf(100.0, 30.0))
            })
            .auto_item_height(50.0),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Rows measured to 30. Appending must keep that prefix (the
        // divergence is the old length) — row 1 stays at 30, it doesn't
        // snap back to the 50 px estimate.
        model.push(99);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let spans = item_spans(&tree, lv_id);
        assert_eq!(spans.len(), 5);
        assert!(
            (spans[1].0 - 30.0).abs() < 0.01,
            "measured prefix must survive an append, got y {}",
            spans[1].0
        );
    }

    #[test]
    fn scrollbar_reservation_self_corrects_after_auto_measure_flips_the_decision() {
        // The scrollbar decision (and the content width it drives) is made
        // from the PRE-measure estimate, since rows can't be measured at a
        // width that itself depends on the decision. When the actual
        // measured total flips "fits without a scrollbar" into "needs
        // one", the pass that measures it places rows at the stale
        // (unreserved) width and leaves the scrollbar collapsed; the NEXT
        // pass recomputes `provisional_total` from the now-measured total
        // and corrects both. Pins that the mismatch resolves by the very
        // next layout pass — see the comment on `provisional_total` in
        // `ListView::place_children` — so a refactor can't make the
        // one-frame lag persist.
        //
        // 10 rows at the 20px estimate fit a 300px viewport (no
        // scrollbar); the same 10 rows measured at their real 40px
        // height (400px total) do not. With every row already realized
        // and no scroll-anchor shift, nothing else in this scenario
        // dirties the list for another pass — `tree.layout()` short-
        // circuits a clean tree (see `WidgetTree::layout_with_ops`'s
        // `!proposal_changed && !any_needs_layout()` guard) — so the
        // second pass is driven by a `scroll_y` touch, the same
        // `Relayout`-bound signal a real scroll/resize event would flip
        // in a live app.
        let model = ListModel::from_vec((0..10).collect::<Vec<usize>>());
        let mut tree = WidgetTree::new();
        let lv = ListView::new(model, |_i, _item, _sel| Box::new(FixedLeaf(100.0, 40.0)))
            .auto_item_height(20.0);
        let scroll_y = lv.scroll_y_signal().clone();
        let lv_id = tree.add(lv);

        tree.layout(SizeProposal::exact(400.0, 300.0));
        let children = row_ids(&tree, lv_id);
        let item0_frame1 = tree.bounds(children[0]).width;
        let sb_frame1 = tree.bounds(scrollbar_of(&tree, lv_id)).width;
        assert!(
            (item0_frame1 - 400.0).abs() < 0.01,
            "frame 1 uses the pre-measure (no-scrollbar) decision, got width {item0_frame1}"
        );
        assert!(
            sb_frame1 < 0.01,
            "frame 1's scrollbar is still collapsed from the same stale decision, got {sb_frame1}"
        );

        // `Signal::set` always notifies (no equality skip), so setting the
        // same value still marks this list dirty for `Relayout` and forces
        // the next `layout()` to re-run `place_children`.
        scroll_y.set(0.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let children = row_ids(&tree, lv_id);
        let item0_frame2 = tree.bounds(children[0]).width;
        let sb_frame2 = tree.bounds(scrollbar_of(&tree, lv_id)).width;
        assert!(
            (item0_frame2 - (400.0 - SCROLLBAR_THICKNESS)).abs() < 0.01,
            "frame 2 must self-correct to the measured (needs-scrollbar) width, got {item0_frame2}"
        );
        assert!(
            (sb_frame2 - SCROLLBAR_THICKNESS).abs() < 0.01,
            "frame 2's scrollbar must appear once the measured total is known, got {sb_frame2}"
        );
    }

    #[test]
    fn ensure_index_visible_with_variable_heights() {
        let model = ListModel::from_vec((0..100).collect::<Vec<usize>>());
        let heights = |i: usize| 20.0 + (i % 3) as f32 * 20.0; // 20/40/60
        let mut tree = WidgetTree::new();
        let lv = ListView::new(model, |_i, _item, _sel| Box::new(FixedLeaf(100.0, 30.0)))
            .item_height_fn(heights);
        let scroll = lv.scroll_y_signal().clone();
        let lv_id = tree.add(lv);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // row_top(20) = sum of heights 0..20 = 6 full cycles (20+40+60) ×
        // 6 + 20 + 40 = 720 + 60 = … compute the prefix directly:
        let top_20: f32 = (0..20).map(heights).sum();
        let bottom_20 = top_20 + heights(20);

        tree.widget_as_any(lv_id)
            .and_then(|any| any.downcast_ref::<ListView<usize>>())
            .expect("ListView exposes itself via as_any")
            .ensure_index_visible(20);
        // Row 20 was below the viewport → scrolled so its bottom is at
        // the viewport bottom.
        assert!(
            (scroll.get() - (bottom_20 - 300.0)).abs() < 0.5,
            "scroll {} != bottom {} - viewport",
            scroll.get(),
            bottom_20
        );
    }

    #[test]
    fn drag_insertion_with_variable_heights() {
        // Heights [40, 10, 40, 40, 40]: dropping at y = 35 (lower half of
        // the tall row 0) must insert at index 1 — the naive midpoint
        // formula would skip past the short row 1.
        let model = ListModel::from_vec(vec![10_usize, 20, 30, 40, 50]);
        let heights = [40.0_f32, 10.0, 40.0, 40.0, 40.0];
        let mut tree = WidgetTree::new();
        let lv_id = tree.add(
            ListView::new(model.clone(), |_i, _item, _sel| {
                Box::new(FixedLeaf(100.0, 30.0))
            })
            .item_height_fn(move |i| heights.get(i).copied().unwrap_or(40.0))
            .reorderable(true),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Drag item 4 (value 50) up to y = 35.
        let children = row_ids(&tree, lv_id);
        let from = tree.bounds(children[4]).center();
        drag_item(&mut tree, from, Point::new(from.x, 35.0));

        // Insertion before row 1: [10, 50, 20, 30, 40].
        assert_eq!(model.with_item(1, |v| *v), Some(50));
        assert_eq!(model.with_item(2, |v| *v), Some(20));
    }

    // --- Cross-widget export drop (RowDragData) integration tests ---

    #[allow(clippy::type_complexity)]
    type Captured = Rc<RefCell<Option<(Vec<usize>, Option<Vec<usize>>)>>>;

    /// Scene: `VStack { FixedSize(120)[ ListView(exportable) ], sink }` where
    /// the sink records any `RowDragData<usize>` it receives. Row 0 sits at
    /// window y≈15; the sink spans y=120..200 (drop at y≈160).
    fn export_scene(
        values: Vec<usize>,
        mode: DragTransferMode,
    ) -> (WidgetTree, ListModel<usize>, SelectionModel, Captured) {
        use crate::primitives::{FixedSize, VStack};
        use teksilo_core::widget_builder::WidgetBuilder as _;
        let model = ListModel::from_vec(values);
        let sel = SelectionModel::new(teksilo_data::SelectionMode::Multi);
        let cap: Captured = Rc::new(RefCell::new(None));
        let cap2 = cap.clone();
        let lv = ListView::new(model.clone(), |_i, _item, _s| {
            Box::new(FixedLeaf(180.0, 30.0))
        })
        .item_height(30.0)
        .selection(sel.clone())
        .exportable(mode);
        let sink = FixedLeaf(180.0, 80.0).on_drop(move |mut payload, _pos, _ctx| {
            if let Some(rd) = payload.take_typed::<RowDragData<usize>>() {
                *cap2.borrow_mut() = Some((rd.rows, rd.items));
                true
            } else {
                false
            }
        });
        let mut tree = WidgetTree::new();
        tree.add(
            VStack::new()
                .spacing(0.0)
                .child(FixedSize::new().height(120.0).child(lv))
                .child(sink),
        );
        tree.layout(SizeProposal::exact(200.0, 300.0));
        (tree, model, sel, cap)
    }

    #[test]
    fn exportable_row_drops_on_foreign_sink_with_items() {
        let (mut tree, _model, _sel, cap) = export_scene(vec![10, 20, 30], DragTransferMode::Copy);
        drag_item(&mut tree, Point::new(50.0, 15.0), Point::new(50.0, 160.0));
        let (rows, items) = cap.borrow().clone().expect("sink received a RowDragData");
        assert_eq!(rows, vec![0]);
        assert_eq!(items, Some(vec![10]));
    }

    #[test]
    fn exportable_move_removes_source_row_after_foreign_accept() {
        let (mut tree, model, _sel, cap) = export_scene(vec![10, 20, 30], DragTransferMode::Move);
        drag_item(&mut tree, Point::new(50.0, 15.0), Point::new(50.0, 160.0));
        assert!(cap.borrow().is_some(), "sink accepted the drop");
        // Move: source row 0 (value 10) is removed once accepted elsewhere.
        assert_eq!(model.len(), 2);
        assert_eq!(model.with_item(0, |v| *v), Some(20));
    }

    #[test]
    fn exportable_copy_leaves_source_intact() {
        let (mut tree, model, _sel, cap) = export_scene(vec![10, 20, 30], DragTransferMode::Copy);
        drag_item(&mut tree, Point::new(50.0, 15.0), Point::new(50.0, 160.0));
        assert!(cap.borrow().is_some());
        assert_eq!(model.len(), 3);
        assert_eq!(model.with_item(0, |v| *v), Some(10));
    }

    #[test]
    fn exportable_multi_selection_drags_the_whole_set() {
        let (mut tree, _model, sel, cap) =
            export_scene(vec![10, 20, 30, 40], DragTransferMode::Copy);
        // Select rows 0 and 2, then grab row 0.
        sel.select_indices([0_usize, 2_usize], false);
        drag_item(&mut tree, Point::new(50.0, 15.0), Point::new(50.0, 160.0));
        let (rows, items) = cap.borrow().clone().expect("received");
        assert_eq!(rows, vec![0, 2]);
        assert_eq!(items, Some(vec![10, 30]));
    }

    #[test]
    fn reorder_only_view_is_not_exportable() {
        // A plain reorderable (non-exportable) view carries `items: None`, so a
        // foreign sink gating on `is_export()` gets nothing usable.
        use crate::primitives::{FixedSize, VStack};
        use teksilo_core::widget_builder::WidgetBuilder as _;
        let model: ListModel<usize> = ListModel::from_vec(vec![1, 2, 3]);
        let is_export: Rc<Cell<Option<bool>>> = Rc::new(Cell::new(None));
        let probe = is_export.clone();
        let lv = ListView::new(model.clone(), |_i, _it, _s| {
            Box::new(FixedLeaf(180.0, 30.0))
        })
        .item_height(30.0)
        .selection(SelectionModel::new(teksilo_data::SelectionMode::Single))
        .reorderable(true);
        let sink = FixedLeaf(180.0, 80.0).on_drop(move |payload, _pos, _ctx| {
            probe.set(
                payload
                    .get_typed::<RowDragData<usize>>()
                    .map(|rd| rd.is_export()),
            );
            true
        });
        let mut tree = WidgetTree::new();
        tree.add(
            VStack::new()
                .spacing(0.0)
                .child(FixedSize::new().height(120.0).child(lv))
                .child(sink),
        );
        tree.layout(SizeProposal::exact(200.0, 300.0));
        drag_item(&mut tree, Point::new(50.0, 15.0), Point::new(50.0, 160.0));
        // The reorder-only drag reaches the foreign sink, but carries no items,
        // so a receiver gating on `is_export()` correctly rejects it.
        assert_eq!(
            is_export.get(),
            Some(false),
            "reorder-only payload is not an export"
        );
    }

    /// The same per-row tooltip API as `TreeView`, on the sibling view.
    ///
    /// Both views build their rows from a delegate the app cannot reach, so
    /// both resolve and attach the tip themselves through the shared
    /// `RowTooltips`. Porting the API is only half of it — the behaviour has
    /// to match, which is what this pins.
    #[test]
    fn row_composite_tooltip_opens_for_the_hovered_row() {
        use crate::primitives::TextWidget;
        use std::time::Duration;
        use teksilo_i18n::lit;

        let model = ListModel::from_vec(vec![
            "Alpha".to_string(),
            "Beta".to_string(),
            "Gamma".to_string(),
        ]);
        let mut tree = WidgetTree::new().with_text_backend(std::rc::Rc::new(
            std::cell::RefCell::new(teksilo_canvas::MockTextBackend::new()),
        ));
        let lv =
            tree.add(
                ListView::new(model, |_i, _it, _s| Box::new(FixedLeaf(180.0, 20.0)))
                    .item_height(20.0)
                    .row_composite_tooltip(|_i, item: &String| {
                        Some(Box::new(TextWidget::new(lit!(format!("about {item}"))))
                            as Box<dyn Widget>)
                    }),
            );
        tree.layout(SizeProposal::exact(400.0, 200.0));
        assert!(tree.active_overlays().is_empty());

        // Hover row 1 (20 dp rows → centre at y = 30).
        let bounds = tree.bounds(lv);
        tree.pointer_move(teksilo_canvas::Point::new(bounds.x + 40.0, bounds.y + 30.0));
        tree.advance_time(Duration::from_millis(750));

        assert_eq!(tree.active_overlays().len(), 1);
        assert!(
            tree.find_by_label("about Beta").is_some(),
            "the tip must carry the hovered row's own content"
        );
    }

    #[test]
    fn accept_foreign_rows_receives_from_another_view() {
        use crate::primitives::{FixedSize, VStack};
        // Source A (exportable Move) above; receiver B (accept_foreign_rows) below.
        let a = ListModel::from_vec(vec![10, 20, 30]);
        let b = ListModel::from_vec(vec![100, 200]);
        let b_recv = b.clone();
        let lv_a = ListView::new(a.clone(), |_i, _it, _s| Box::new(FixedLeaf(180.0, 30.0)))
            .item_height(30.0)
            .exportable(DragTransferMode::Move);
        let lv_b = ListView::new(b.clone(), |_i, _it, _s| Box::new(FixedLeaf(180.0, 30.0)))
            .item_height(30.0)
            .accept_foreign_rows(true)
            .on_rows_received(move |items, at, _ctx| {
                for (k, v) in items.into_iter().enumerate() {
                    b_recv.insert(at + k, v);
                }
            });
        let mut tree = WidgetTree::new();
        tree.add(
            VStack::new()
                .spacing(0.0)
                .child(FixedSize::new().height(90.0).child(lv_a))
                .child(FixedSize::new().height(150.0).child(lv_b))
                .child(FixedLeaf(180.0, 10.0)),
        );
        tree.layout(SizeProposal::exact(200.0, 300.0));
        // Drag A's row 0 (y≈15) onto B's first row (B spans y=90..240; drop y≈105).
        drag_item(&mut tree, Point::new(50.0, 15.0), Point::new(50.0, 105.0));
        // B received value 10; A lost it (Move).
        assert_eq!(b.len(), 3, "receiver B gained the dragged row");
        assert!(
            (0..b.len()).any(|i| b.with_item(i, |v| *v) == Some(10)),
            "B contains the moved value 10"
        );
        assert_eq!(a.len(), 2, "source A removed the moved row");
        assert!(
            (0..a.len()).all(|i| a.with_item(i, |v| *v) != Some(10)),
            "A no longer contains 10"
        );
    }

    #[test]
    fn two_views_over_same_model_do_not_spuriously_reorder() {
        use crate::primitives::{FixedSize, VStack};
        // Two reorderable ListViews sharing ONE model have distinct ViewIds, so
        // a drag from A onto B is Foreign (rejected by ListModel), not a
        // same-view reorder — proving ids don't collide across instances.
        let model = ListModel::from_vec(vec![10, 20, 30]);
        let lv_a = ListView::new(model.clone(), |_i, _it, _s| {
            Box::new(FixedLeaf(180.0, 30.0))
        })
        .item_height(30.0)
        .reorderable(true);
        let lv_b = ListView::new(model.clone(), |_i, _it, _s| {
            Box::new(FixedLeaf(180.0, 30.0))
        })
        .item_height(30.0)
        .reorderable(true);
        let mut tree = WidgetTree::new();
        tree.add(
            VStack::new()
                .spacing(0.0)
                .child(FixedSize::new().height(90.0).child(lv_a))
                .child(FixedSize::new().height(150.0).child(lv_b))
                .child(FixedLeaf(180.0, 10.0)),
        );
        tree.layout(SizeProposal::exact(200.0, 300.0));
        drag_item(&mut tree, Point::new(50.0, 15.0), Point::new(50.0, 105.0));
        // The shared model is unchanged: B rejected A's foreign row.
        assert_eq!(model.with_item(0, |v| *v), Some(10));
        assert_eq!(model.with_item(1, |v| *v), Some(20));
        assert_eq!(model.with_item(2, |v| *v), Some(30));
    }

    #[test]
    fn exportable_not_reorderable_does_not_reorder_on_same_view_drop() {
        use crate::primitives::{FixedSize, VStack};
        // A view that is exportable + accepts foreign rows (so it IS a drop
        // target) but is NOT reorderable must not reorder itself when its own
        // row is dropped back inside it.
        let model: ListModel<usize> = ListModel::from_vec(vec![10, 20, 30, 40]);
        let lv = ListView::new(model.clone(), |_i, _it, _s| {
            Box::new(FixedLeaf(180.0, 30.0))
        })
        .item_height(30.0)
        .exportable(DragTransferMode::Move)
        .accept_foreign_rows(true)
        .on_rows_received(|_items, _at, _ctx| {});
        let mut tree = WidgetTree::new();
        tree.add(
            VStack::new()
                .spacing(0.0)
                .child(FixedSize::new().height(200.0).child(lv))
                .child(FixedLeaf(180.0, 10.0)),
        );
        tree.layout(SizeProposal::exact(200.0, 300.0));
        // Drag row 0 (y=15) and drop within the view at row 2 (y=75).
        drag_item(&mut tree, Point::new(50.0, 15.0), Point::new(50.0, 75.0));
        // No reorder happened (reorderable was never enabled).
        assert_eq!(model.with_item(0, |v| *v), Some(10));
        assert_eq!(model.with_item(3, |v| *v), Some(40));
    }

    /// A focusable container that never advertises `Action::Focus` cannot be
    /// focused by assistive technology: the tree services the action itself,
    /// but the AT only ever asks for what a node advertises.
    #[test]
    fn advertises_focus_so_assistive_tech_can_focus_the_list() {
        let (mut tree, lv_id, _model) = make_list_view(20, 20.0);
        tree.layout(SizeProposal::exact(400.0, 200.0));

        let info = tree.accessibility_node(lv_id);
        assert_eq!(info.role(), teksilo_core::accesskit::Role::ListBox);
        assert!(
            info.actions()
                .contains(&teksilo_core::accesskit::Action::Focus),
            "ListView must advertise Action::Focus; without it no screen reader \
             can move focus into the list"
        );

        // And the advertised action really lands.
        let mut ops = teksilo_core::window::NoopWindowOps;
        let handled = tree.dispatch_access_action(
            teksilo_core::accessibility::widget_id_to_node_id(lv_id),
            teksilo_core::accesskit::Action::Focus,
            None,
            &mut ops,
        );
        assert!(handled, "the Focus action must be serviced");
        assert_eq!(tree.focused(), Some(lv_id));
    }
}

#[cfg(test)]
mod checkbox_keyboard_tests {
    use super::tests::FixedLeaf;
    use super::*;
    use teksilo_core::WidgetTree;
    use teksilo_core::event::{Key, Modifiers};
    use teksilo_data::{CheckedModel, SelectionMode, SelectionModel};
    use teksilo_i18n::lit;

    /// A 200-row list showing ~10, every row carrying a checkbox.
    fn checked_list() -> (WidgetTree, SelectionModel, CheckedModel, SizeProposal) {
        let model = ListModel::from_vec((0..200usize).collect());
        let checks = CheckedModel::new();
        let selection = SelectionModel::new(SelectionMode::Multi);
        let (sel, ck) = (selection.clone(), checks.clone());
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let lv = tree.add(
            ListView::new(model, move |i, item, selected| {
                Box::new(
                    crate::StandardListItem::new(lit!(format!("Row {item}")))
                        .selected(selected)
                        .checkbox(ck.signal_for(i)),
                )
            })
            .item_height(24.0)
            .selection(sel),
        );
        let p = SizeProposal::exact(400.0, 240.0);
        tree.layout(p);
        tree.focus(lv);
        (tree, selection, checks, p)
    }

    #[test]
    fn a_list_is_one_tab_stop_however_many_rows_are_realized() {
        // The row checkboxes used to be Tab stops of their own, which made the
        // Tab order a function of the virtualization window: 31 stops in this
        // fixture, and a *different* 31 after scrolling. A listbox is one Tab
        // stop with a cursor moving inside it.
        let (mut tree, _sel, _ck, p) = checked_list();
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..40 {
            tree.press_key(Key::Tab, Modifiers::NONE);
            tree.layout(p);
            seen.insert(tree.focused());
        }
        assert_eq!(
            seen.len(),
            1,
            "Tab must not walk into the rows; got {} distinct stops",
            seen.len()
        );
    }

    #[test]
    fn space_checks_the_focused_row_and_ctrl_space_still_selects() {
        let (mut tree, sel, checks, p) = checked_list();
        sel.select(3);
        tree.layout(p);

        // Space reaches the checkbox — the row's only keyboard route to it,
        // now that it is out of the Tab order.
        tree.press_key(Key::Space, Modifiers::NONE);
        tree.layout(p);
        assert!(checks.signal_for(3).get(), "Space checks the focused row");
        assert_eq!(sel.selected_indices(), vec![3], "and leaves the selection");

        tree.press_key(Key::Space, Modifiers::NONE);
        tree.layout(p);
        assert!(!checks.signal_for(3).get(), "and unchecks it again");

        // Ctrl+Space keeps meaning "toggle the selection".
        tree.press_key(Key::Space, Modifiers::CTRL);
        tree.layout(p);
        assert_eq!(sel.selected_indices(), Vec::<usize>::new());
        assert!(!checks.signal_for(3).get(), "the check is untouched");
    }

    #[test]
    fn a_row_without_a_checkbox_keeps_space_on_the_selection() {
        let model = ListModel::from_vec((0..20usize).collect());
        let selection = SelectionModel::new(SelectionMode::Multi);
        let sel = selection.clone();
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let lv = tree.add(
            ListView::new(model, move |_i, _item, _s| Box::new(FixedLeaf(100.0, 24.0)))
                .item_height(24.0)
                .selection(sel),
        );
        let p = SizeProposal::exact(400.0, 240.0);
        tree.layout(p);
        tree.focus(lv);
        selection.select(2);

        tree.press_key(Key::Space, Modifiers::NONE);
        tree.layout(p);
        assert_eq!(
            selection.selected_indices(),
            Vec::<usize>::new(),
            "no checkbox to publish a toggle, so Space is still the selection"
        );
    }
}
