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
//! ## Pan to scroll
//!
//! The view installs [`common::scrollable::ScrollableBehavior`](crate::common::scrollable::ScrollableBehavior),
//! which gives it the shared wheel arithmetic, a finger's pan and the
//! `PanClaim` that puts it on a pan's claimant chain. A pan scrolls it, the
//! release coasts, and a pan it cannot absorb hands the **whole** event to the
//! container outside — never a residual. Vertical only: this view owns no
//! horizontal offset, so a horizontal pan is declined and chains outward. A pan
//! that starts on a row scrolls rather than activating it or collapsing a
//! multi-selection onto it.
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
use teksilo_tokens::{BorderRole, InputTokens, OverscrollStyle, TargetRole};

use teksilo_core::DropFeedback;
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::drag_payload::DragPayload;
use teksilo_core::kinetic::KineticScroller;
use teksilo_core::pointer::touch_action::PanAxes;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget::{LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;

use teksilo_data::selection_model::SelectionModel;
use teksilo_data::{ItemKey, KeyedSelectionModel};

use crate::data_views::RowSelection;
use teksilo_data::{DataChange, DropResponse, ListModel};

// Qualified rather than glob-imported: `data_views::ViewKind` is already in
// scope here and means something else (which data view a drag came from).
use crate::common::list_nav;
use crate::common::row_metrics::{HeightSource, RowMetrics, SharedRowMetrics};
use crate::common::scroll::OverscrollBehavior;
use crate::data_views::{DragTransferMode, RowDragData, ViewId, ViewKind, flat_insertion_target};
use crate::list_source::ListSource;
use crate::scroll_area::ScrollBarMode;
use crate::scroll_bar::{ScrollBar, ScrollBarOrientation, ScrollBarVisual};
use teksilo_core::styles::density::dp;

mod body_pane;
mod widget_impl;

/// Default number of extra items to create above and below the viewport.
const BUFFER_ITEMS: usize = 5;
/// Default item height.
const DEFAULT_ITEM_HEIGHT: f32 = 32.0;

/// [`DEFAULT_ITEM_HEIGHT`] raised to the density's `target_size`
/// (24 / 32 / 44 dp). The identity at Compact.
fn default_item_height(tokens: &InputTokens) -> f32 {
    dp(DEFAULT_ITEM_HEIGHT, TargetRole::Target, tokens)
}
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

    /// This surface's pan physics: the range a finger's pan is clamped to and
    /// the offset it is currently holding. Owned by the view rather than by
    /// the [`ScrollableBehavior`](crate::common::scrollable::ScrollableBehavior)
    /// so it survives a rebuild, and so `place_children` — the only pass that
    /// knows the viewport extent — can publish into it.
    scroller: Rc<RefCell<KineticScroller>>,

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
            scroller: Rc::new(RefCell::new(KineticScroller::new(OverscrollStyle::Clamp))),
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

#[cfg(test)]
mod tests;

#[cfg(test)]
mod checkbox_keyboard_tests;
