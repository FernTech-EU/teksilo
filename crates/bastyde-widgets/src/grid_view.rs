// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Virtualized 2D tile grid bound to a `ListModel<T>` / `ListDataSource`.
//!
//! `GridView` is the photo-gallery / icon-view / file-manager-grid /
//! collection-view widget — the 2D sibling of [`ListView`](crate::list_view::ListView)
//! and [`TableView`](crate::table_view::TableView). It realizes only the
//! tiles currently visible (plus a buffer), reflows on resize, supports
//! single / multi selection with 2D keyboard navigation, and is fully
//! accessible (`Role::Grid` → `Role::GridCell`).
//!
//! The layout is pluggable via `GridLayoutStrategy`;
//! the stock [`UniformGrid`] gives fixed tile size /
//! fixed column count / adaptive min-width grids. (Variable-row-height and
//! waterfall strategies, plus marquee selection, drag-reorder, sections and
//! sticky headers, are layered on in later phases.)
//!
//! ```ignore
//! GridView::new(model, |tc| {
//!     Box::new(Card::new().child(TextWidget::new(lit!(&tc.item.name))))
//! })
//! .sizing(GridSizing::Adaptive { min_width: 120.0, max_width: None, height: 140.0 })
//! .spacing(8.0)
//! .selection(selection_model)
//! ```

pub(crate) mod a11y;
pub(crate) mod body_pane;
pub(crate) mod drag;
pub(crate) mod keyboard;
pub mod layout;
pub mod sections;
pub(crate) mod selection;
#[cfg(test)]
mod tests;

use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;

use bastyde_canvas::{EdgeInsets, Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::{AccessNodeBuilder, widget_id_to_node_id};
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, ScrollDelta, WidgetEvent};
use bastyde_core::signal::Signal;
use bastyde_core::styles::GridViewStyle;
use bastyde_core::widget::{LayoutContext, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_data::{DataChange, ListModel, SelectionMode, SelectionModel};
use bastyde_tokens::SurfaceRole;

use crate::common::scroll::OverscrollBehavior;
use crate::list_source::ListSource;
use crate::primitives::TextWidget;
use crate::scroll_bar::{ScrollBar, ScrollBarOrientation};

use body_pane::{GridBodyPane, TileDelegate};
use keyboard::{GridKeyConfig, build_grid_key_handler};
use layout::masonry::VirtualizedMasonry;
use layout::sectioned::SectionedGrid;
use layout::strategy::GridLayoutStrategy;
use layout::uniform::UniformGrid;
use layout::variable_row::VariableRowGrid;
use sections::{SectionData, SectionProvider};
use selection::{MarqueeConfig, MarqueeState, build_marquee_handler};

pub use sections::{GroupingSections, SectionProvider as GridSectionProvider, grouping_sections};

/// Which layout strategy `GridView` builds.
#[derive(Debug, Clone, Copy)]
enum StrategyKind {
    /// Fixed row height (the default).
    Uniform,
    /// Each row sized to its tallest tile; `estimated` seeds unmeasured rows.
    VariableRow { estimated: f32 },
    /// Pinterest-style column-balanced waterfall; per-item variable height.
    Waterfall { estimated: f32 },
}

pub use keyboard::GridTabTraversal;
pub use layout::{GridSizing, ScrollAnchor};

/// Monotonic id distinguishing `GridView` instances (intra-grid reorder).
fn next_grid_id() -> usize {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Scrollbar thickness, matching `ListView` / `TableView`.
const SCROLLBAR_THICKNESS: f32 = 12.0;

/// Context passed to the tile delegate for each realized tile.
///
/// Richer than `ListView`'s `(index, &item, selected)` — carries the 2D
/// grid coordinates and focus state (mirrors `TableView`'s `CellContext`).
/// There is intentionally **no** `is_hovered`: hover changes on every
/// mouse-move and is handled per-tile inside the delegate's own widget
/// (its interaction signal), never by rebuilding the grid.
pub struct TileContext<'a, T: 'static> {
    /// Flat model index.
    pub index: usize,
    /// Row in the logical grid (0-based).
    pub row: usize,
    /// Column in the logical grid (0-based).
    pub col: usize,
    /// Borrow of the item.
    pub item: &'a T,
    /// Whether this tile is in the selection set.
    pub is_selected: bool,
    /// Whether this tile is the keyboard-focus current item. A build-time
    /// snapshot — the canonical focus indicator is the grid's painted focus
    /// ring (it does not rebuild tiles), so a delegate reading this for
    /// custom styling accepts a one-rebuild lag.
    pub is_focused: bool,
}

/// A virtualized 2D tile grid backed by a `ListModel<T>`.
pub struct GridView<T: 'static> {
    source: ListSource<T>,
    delegate: TileDelegate<T>,

    // Layout configuration (consumed when the strategy is first built).
    sizing: GridSizing,
    col_gap: f32,
    row_gap: f32,
    inset: EdgeInsets,
    strategy_kind: StrategyKind,
    /// Exact per-item natural height (the variable-height fast-path).
    #[allow(clippy::type_complexity)]
    exact_item_height: Option<Rc<dyn Fn(usize) -> f32>>,
    /// Lazily built on first `build()` and cached so variable-height
    /// strategies keep their measurement caches across rebuilds.
    strategy: Option<Rc<dyn GridLayoutStrategy>>,

    // Selection / focus
    selection: Option<SelectionModel>,
    #[allow(clippy::type_complexity)]
    on_selection_changed: Option<Rc<dyn Fn(&BTreeSet<usize>)>>,
    focused_index: Signal<Option<usize>>,
    /// Enable rubber-band marquee (default true; only active in Multi mode).
    marquee_selection: bool,
    marquee: Signal<Option<MarqueeState>>,

    // Keyboard
    wrap_navigation: bool,
    tab_traversal: GridTabTraversal,

    // Scroll
    show_scrollbar: bool,
    overscroll_behavior: OverscrollBehavior,
    scroll_y: Signal<f32>,
    max_scroll_y: Signal<f32>,
    viewport_ratio_y: Signal<f32>,
    /// Live column count for the current viewport width. Written in
    /// `place_children`; drives the body pane's reflow rebuild on resize and
    /// is read by the keyboard handler.
    column_count: Signal<usize>,

    // Drag-to-reorder + drop
    reorderable: bool,
    #[allow(clippy::type_complexity)]
    on_item_drop: Option<
        Rc<
            dyn Fn(
                bastyde_core::drag_payload::DragPayload,
                usize,
                &mut bastyde_core::widget::EventContext,
            ) -> bool,
        >,
    >,
    /// Insertion index during a reorder drag (painted by `GridOverlay`).
    insertion: Signal<Option<usize>>,
    model_id: usize,

    // Activation / context menu / type-ahead
    #[allow(clippy::type_complexity)]
    on_tile_activate: Option<Rc<dyn Fn(usize, &mut bastyde_core::widget::EventContext)>>,
    #[allow(clippy::type_complexity)]
    tile_context_menu: Option<
        Rc<
            dyn Fn(
                usize,
                Point,
                &mut bastyde_core::widget::EventContext,
            ) -> Option<Box<dyn Widget>>,
        >,
    >,
    type_ahead_timeout: std::time::Duration,
    #[allow(clippy::type_complexity)]
    type_ahead_label: Option<Rc<dyn Fn(usize) -> String>>,

    // Incremental loading. The callback is `Fn()` (no EventContext): it
    // fires from a reactive scroll observer, which can't carry one, and the
    // typical action is just to kick off a fetch into the model.
    #[allow(clippy::type_complexity)]
    on_near_end: Option<(usize, Rc<dyn Fn()>)>,

    // Empty / loading state
    #[allow(clippy::type_complexity)]
    empty_view: Option<Rc<dyn Fn() -> Box<dyn Widget>>>,
    #[allow(clippy::type_complexity)]
    loading_view: Option<Rc<dyn Fn() -> Box<dyn Widget>>>,
    is_loading: Option<Signal<bool>>,
    loading_id: Option<WidgetId>,

    // Sections
    section_data: Option<SectionData>,
    #[allow(clippy::type_complexity)]
    header_delegate: Option<Rc<dyn Fn(usize, &str) -> Box<dyn Widget>>>,
    header_height: f32,
    pinned_section_headers: bool,
    current_section: Signal<usize>,
    pinned_header_id: Option<WidgetId>,

    // Accessibility
    a11y_label: Option<String>,
    /// Shared map (flat index → tile wrapper id), written by the body pane,
    /// read by `accessibility` for `active_descendant` roving focus.
    tile_map: Rc<std::cell::RefCell<Vec<(usize, WidgetId)>>>,

    /// Per-call Tier-3 decoration style override (focus ring / marquee /
    /// insertion bar / pinned header). `None` → theme slot → stock default.
    style: Option<Rc<dyn GridViewStyle>>,

    // Geometry (synchronous cells, read within the layout pass)
    viewport_width: Rc<Cell<f32>>,
    viewport_height: Rc<Cell<f32>>,
    /// Remembered scrollbar decision so each layout queries the strategy at a
    /// single, stable body width — querying at two widths per frame would
    /// thrash a variable strategy's per-row measurement cache.
    last_needs_scrollbar: Cell<bool>,

    // Build state
    body_pane_id: Option<WidgetId>,
    empty_id: Option<WidgetId>,
    scrollbar_id: Option<WidgetId>,
    overlay_id: Option<WidgetId>,
}

impl<T: 'static> GridView<T> {
    /// Create a grid backed by a `ListModel<T>`. The `delegate` builds the
    /// widget for each tile from a [`TileContext`].
    pub fn new(
        model: ListModel<T>,
        delegate: impl Fn(&TileContext<'_, T>) -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self::create(ListSource::from_model(model), delegate)
    }

    /// Create a grid backed by any `ListDataSource` (large / external data).
    pub fn from_source<S: bastyde_data::ListDataSource<Item = T>>(
        source: S,
        delegate: impl Fn(&TileContext<'_, T>) -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self::create(ListSource::from_data_source(source), delegate)
    }

    fn create(
        source: ListSource<T>,
        delegate: impl Fn(&TileContext<'_, T>) -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self {
            source,
            delegate: Rc::new(delegate),
            sizing: GridSizing::Adaptive {
                min_width: 120.0,
                max_width: None,
                height: 120.0,
            },
            col_gap: 8.0,
            row_gap: 8.0,
            inset: EdgeInsets::ZERO,
            strategy_kind: StrategyKind::Uniform,
            exact_item_height: None,
            strategy: None,
            selection: None,
            on_selection_changed: None,
            focused_index: Signal::new(None),
            marquee_selection: true,
            marquee: Signal::new(None),
            wrap_navigation: false,
            tab_traversal: GridTabTraversal::OutOfGrid,
            show_scrollbar: true,
            overscroll_behavior: OverscrollBehavior::default(),
            scroll_y: Signal::new_animated(0.0),
            max_scroll_y: Signal::new(0.0),
            viewport_ratio_y: Signal::new(1.0),
            column_count: Signal::new(1),
            reorderable: false,
            on_item_drop: None,
            insertion: Signal::new(None),
            model_id: next_grid_id(),
            on_tile_activate: None,
            tile_context_menu: None,
            type_ahead_timeout: std::time::Duration::from_millis(500),
            type_ahead_label: None,
            on_near_end: None,
            empty_view: None,
            loading_view: None,
            is_loading: None,
            loading_id: None,
            section_data: None,
            header_delegate: None,
            header_height: 28.0,
            pinned_section_headers: false,
            current_section: Signal::new(0),
            pinned_header_id: None,
            a11y_label: None,
            tile_map: Rc::new(std::cell::RefCell::new(Vec::new())),
            style: None,
            viewport_width: Rc::new(Cell::new(400.0)),
            viewport_height: Rc::new(Cell::new(400.0)),
            last_needs_scrollbar: Cell::new(false),
            body_pane_id: None,
            empty_id: None,
            scrollbar_id: None,
            overlay_id: None,
        }
    }

    // ── Tile sizing & layout ────────────────────────────────────────────

    /// Set the tile sizing / column-count policy.
    pub fn sizing(mut self, sizing: GridSizing) -> Self {
        self.sizing = sizing;
        self
    }

    /// Sugar for [`GridSizing::Fixed`] — every tile is exactly `width` × `height`.
    pub fn tile_size(mut self, width: f32, height: f32) -> Self {
        self.sizing = GridSizing::Fixed { width, height };
        self
    }

    /// Sugar for [`GridSizing::FixedColumnCount`] — exactly `count` columns.
    pub fn column_count(mut self, count: usize, tile_height: f32) -> Self {
        self.sizing = GridSizing::FixedColumnCount {
            count,
            height: tile_height,
        };
        self
    }

    /// Switch to variable row heights: each row is sized to its tallest
    /// tile (SwiftUI `LazyVGrid` semantics). `estimated` seeds rows that
    /// haven't been measured yet; the scroll position is anchored when an
    /// estimate is later corrected. Combine with
    /// [`item_height`](Self::item_height) for exact heights.
    pub fn variable_row_heights(mut self, estimated: f32) -> Self {
        self.strategy_kind = StrategyKind::VariableRow {
            estimated: estimated.max(1.0),
        };
        self
    }

    /// Supply an exact per-**item** natural height. Width-independent, so it
    /// doesn't depend on the runtime column count: `VariableRowGrid` sizes
    /// each row to `max(item_height(i))` over its items. Implies variable row
    /// heights, gives an exact scrollbar, and removes anchoring jitter.
    pub fn item_height(mut self, f: impl Fn(usize) -> f32 + 'static) -> Self {
        self.exact_item_height = Some(Rc::new(f));
        if matches!(self.strategy_kind, StrategyKind::Uniform) {
            self.strategy_kind = StrategyKind::VariableRow {
                estimated: self.sizing.tile_height().max(1.0),
            };
        }
        self
    }

    /// Switch to a Pinterest-style waterfall: per-item variable heights flow
    /// into the currently-shortest column. Column count comes from the
    /// configured [`sizing`](Self::sizing); heights are auto-measured (or
    /// exact via [`item_height`](Self::item_height)). `estimated` seeds
    /// unmeasured items.
    pub fn waterfall(mut self, estimated: f32) -> Self {
        self.strategy_kind = StrategyKind::Waterfall {
            estimated: estimated.max(1.0),
        };
        self
    }

    // ── Spacing & insets ────────────────────────────────────────────────

    /// Horizontal gap between tiles (default 8).
    pub fn column_spacing(mut self, spacing: f32) -> Self {
        self.col_gap = spacing.max(0.0);
        self
    }

    /// Vertical gap between tile rows (default 8).
    pub fn row_spacing(mut self, spacing: f32) -> Self {
        self.row_gap = spacing.max(0.0);
        self
    }

    /// Set both column and row spacing.
    pub fn spacing(mut self, spacing: f32) -> Self {
        self.col_gap = spacing.max(0.0);
        self.row_gap = spacing.max(0.0);
        self
    }

    /// Inset from the scroll-content edge to the tiles.
    pub fn content_inset(mut self, inset: EdgeInsets) -> Self {
        self.inset = inset;
        self
    }

    // ── Selection ───────────────────────────────────────────────────────

    /// Set the selection model (modes `None` / `Single` / `Multi`).
    pub fn selection(mut self, sel: SelectionModel) -> Self {
        self.selection = Some(sel);
        self
    }

    /// Called whenever the selection set changes — including programmatic
    /// changes — with the new set of selected indices.
    pub fn on_selection_changed(mut self, f: impl Fn(&BTreeSet<usize>) + 'static) -> Self {
        self.on_selection_changed = Some(Rc::new(f));
        self
    }

    /// Enable / disable rubber-band marquee selection (default enabled; only
    /// active when the selection model is in `Multi` mode).
    pub fn marquee_selection(mut self, enabled: bool) -> Self {
        self.marquee_selection = enabled;
        self
    }

    // ── Keyboard ────────────────────────────────────────────────────────

    /// Whether arrow navigation wraps across row/grid edges (default false).
    pub fn wrap_navigation(mut self, enabled: bool) -> Self {
        self.wrap_navigation = enabled;
        self
    }

    /// How Tab moves out of (or within) the grid (default `OutOfGrid`).
    pub fn tab_traversal(mut self, traversal: GridTabTraversal) -> Self {
        self.tab_traversal = traversal;
        self
    }

    // ── Scrolling ───────────────────────────────────────────────────────

    /// Suppress the internal scrollbar (mount your own via the signal
    /// accessors so it survives rebuilds).
    pub fn show_scrollbar(mut self, show: bool) -> Self {
        self.show_scrollbar = show;
        self
    }

    /// Scroll-chaining behavior at the boundary (default `Chain`).
    pub fn overscroll_behavior(mut self, behavior: OverscrollBehavior) -> Self {
        self.overscroll_behavior = behavior;
        self
    }

    /// The vertical scroll offset signal.
    pub fn scroll_y_signal(&self) -> &Signal<f32> {
        &self.scroll_y
    }

    /// The maximum scroll offset signal (`content_height - viewport_height`).
    pub fn max_scroll_y_signal(&self) -> &Signal<f32> {
        &self.max_scroll_y
    }

    /// The vertical viewport-to-content ratio signal (drives the thumb size).
    pub fn viewport_ratio_y_signal(&self) -> &Signal<f32> {
        &self.viewport_ratio_y
    }

    /// Scroll the minimum distance to bring `index` into view per `anchor`.
    pub fn ensure_index_visible(&self, index: usize, anchor: ScrollAnchor) {
        let Some(ref strategy) = self.strategy else {
            return;
        };
        let delta = strategy.scroll_delta_to_reveal(
            index,
            self.scroll_y.get(),
            self.viewport_height.get(),
            self.viewport_width.get(),
            anchor,
        );
        if delta.abs() > 0.01 {
            let max = self.max_scroll_y.get();
            self.scroll_y
                .set((self.scroll_y.get() + delta).clamp(0.0, max));
        }
    }

    /// Scroll to `index`, forcing the viewport position per `anchor`
    /// (`Auto` behaves like [`ensure_index_visible`](Self::ensure_index_visible)).
    pub fn scroll_to_index(&self, index: usize, anchor: ScrollAnchor) {
        self.ensure_index_visible(index, anchor);
    }

    // ── Accessibility / empty state ─────────────────────────────────────

    // ── Sections ────────────────────────────────────────────────────────

    /// Group the flat model into sections, rendering a header above each
    /// section's tile band. Sections compose with the uniform tile layout.
    pub fn sections<P: SectionProvider>(mut self, provider: P) -> Self {
        let provider = Rc::new(provider);
        let counts_provider = provider.clone();
        let title_provider = provider.clone();
        self.section_data = Some(SectionData {
            counts_fn: Rc::new(move || counts_provider.section_counts()),
            title_fn: Rc::new(move |s| title_provider.section_title(s)),
        });
        self
    }

    /// Custom section-header widget builder `(section_index, title)`. Without
    /// it a default bold-text header is used.
    pub fn section_header_delegate(
        mut self,
        f: impl Fn(usize, &str) -> Box<dyn Widget> + 'static,
    ) -> Self {
        self.header_delegate = Some(Rc::new(f));
        self
    }

    /// Height of each section header row (default 28).
    pub fn section_header_height(mut self, height: f32) -> Self {
        self.header_height = height.max(0.0);
        self
    }

    /// Keep the current section's header pinned to the top while scrolling
    /// through it (SwiftUI `pinnedViews:[.sectionHeaders]`).
    pub fn pinned_section_headers(mut self, enabled: bool) -> Self {
        self.pinned_section_headers = enabled;
        self
    }

    /// Accessible label for the grid container.
    pub fn a11y_label(mut self, label: impl Into<String>) -> Self {
        self.a11y_label = Some(label.into());
        self
    }

    /// Per-call Tier-3 decoration style override (focus ring, marquee,
    /// insertion bar, pinned-header surface). Precedence: this override →
    /// `theme.style_slots.grid_view` → the stock `RecipeGridViewStyle`.
    pub fn style(mut self, style: impl GridViewStyle) -> Self {
        self.style = Some(Rc::new(style));
        self
    }

    /// Build the header-widget factory (section → widget) shared by the body
    /// pane and the pinned slot, falling back to a default bold-text header.
    #[allow(clippy::type_complexity)]
    fn header_factory(&self) -> Option<Rc<dyn Fn(usize) -> Box<dyn Widget>>> {
        let data = self.section_data.as_ref()?;
        let title_fn = data.title_fn.clone();
        let delegate = self.header_delegate.clone();
        Some(Rc::new(move |section| {
            let title = title_fn(section);
            match &delegate {
                Some(d) => d(section, &title),
                None => Box::new(TextWidget::new(bastyde_i18n::lit!(title))) as Box<dyn Widget>,
            }
        }))
    }

    /// Widget shown when the model is empty.
    pub fn empty_view(mut self, f: impl Fn() -> Box<dyn Widget> + 'static) -> Self {
        self.empty_view = Some(Rc::new(f));
        self
    }

    /// Widget overlaid while `is_loading` reads `true`.
    pub fn loading_view(mut self, f: impl Fn() -> Box<dyn Widget> + 'static) -> Self {
        self.loading_view = Some(Rc::new(f));
        self
    }

    /// Reactive loading flag; when `true` the [`loading_view`](Self::loading_view)
    /// is shown above the grid.
    pub fn is_loading(mut self, flag: Signal<bool>) -> Self {
        self.is_loading = Some(flag);
        self
    }

    // ── Drag-to-reorder ─────────────────────────────────────────────────

    /// Enable intra-grid drag reordering (and keyboard Alt+Arrow). Calls the
    /// model's `move_item` on drop.
    pub fn reorderable(mut self, enabled: bool) -> Self {
        self.reorderable = enabled;
        self
    }

    /// Accept external drops at a flat insertion index. Returns `true` when
    /// the drop is accepted.
    pub fn on_item_drop(
        mut self,
        f: impl Fn(
            bastyde_core::drag_payload::DragPayload,
            usize,
            &mut bastyde_core::widget::EventContext,
        ) -> bool
        + 'static,
    ) -> Self {
        self.on_item_drop = Some(Rc::new(f));
        self
    }

    // ── Activation / context menu / type-ahead / loading ────────────────

    /// Called when a tile is activated (double-click or Enter on the focused
    /// tile) — the "open / default action", distinct from selection.
    pub fn on_tile_activate(
        mut self,
        f: impl Fn(usize, &mut bastyde_core::widget::EventContext) + 'static,
    ) -> Self {
        self.on_tile_activate = Some(Rc::new(f));
        self
    }

    /// Per-tile context-menu factory: `(index, pointer_position, ctx)` →
    /// optional menu widget.
    pub fn tile_context_menu(
        mut self,
        f: impl Fn(usize, Point, &mut bastyde_core::widget::EventContext) -> Option<Box<dyn Widget>>
        + 'static,
    ) -> Self {
        self.tile_context_menu = Some(Rc::new(f));
        self
    }

    /// Supply a per-item label for type-ahead navigation (typing letters
    /// jumps to the first matching item). Required to enable type-ahead.
    pub fn type_ahead_label(mut self, f: impl Fn(usize) -> String + 'static) -> Self {
        self.type_ahead_label = Some(Rc::new(f));
        self
    }

    /// Type-ahead reset timeout (default 500 ms; `ZERO` disables).
    pub fn type_ahead_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.type_ahead_timeout = timeout;
        self
    }

    /// Called when the scroll position reaches within `threshold` items of
    /// the end — the incremental-loading hook. Fires from a reactive scroll
    /// observer (no `EventContext`); load more into the model.
    pub fn on_near_end(mut self, threshold: usize, f: impl Fn() + 'static) -> Self {
        self.on_near_end = Some((threshold, Rc::new(f)));
        self
    }

    // ── Internals ───────────────────────────────────────────────────────

    /// Build (once) and return the layout strategy. Cached so variable
    /// strategies keep their measurement caches across rebuilds.
    fn ensure_strategy(&mut self) -> Rc<dyn GridLayoutStrategy> {
        if self.strategy.is_none() {
            // Sections override the strategy kind (uniform tiles + headers).
            if let Some(ref data) = self.section_data {
                let s: Rc<dyn GridLayoutStrategy> = Rc::new(SectionedGrid::new(
                    self.sizing,
                    self.col_gap,
                    self.row_gap,
                    self.inset,
                    self.header_height,
                    data.counts_fn.clone(),
                ));
                self.strategy = Some(s);
                return self.strategy.as_ref().unwrap().clone();
            }
            let s: Rc<dyn GridLayoutStrategy> = match self.strategy_kind {
                StrategyKind::Uniform => Rc::new(UniformGrid::new(
                    self.sizing,
                    self.col_gap,
                    self.row_gap,
                    self.inset,
                )),
                StrategyKind::VariableRow { estimated } => Rc::new(VariableRowGrid::new(
                    self.sizing,
                    self.col_gap,
                    self.row_gap,
                    self.inset,
                    estimated,
                    self.exact_item_height.clone(),
                )),
                StrategyKind::Waterfall { estimated } => Rc::new(VirtualizedMasonry::new(
                    self.sizing,
                    self.col_gap,
                    self.row_gap,
                    self.inset,
                    estimated,
                    self.exact_item_height.clone(),
                )),
            };
            self.strategy = Some(s);
        }
        self.strategy.as_ref().unwrap().clone()
    }
}

impl<T: 'static> std::fmt::Debug for GridView<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GridView")
            .field("items", &self.source.len())
            .field("scroll_y", &self.scroll_y.get())
            .finish()
    }
}

impl<T: 'static> Widget for GridView<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let strategy = self.ensure_strategy();

        // Rebuild trigger (data changes, empty/non-empty transition).
        let version = ctx.signal(0_u64);
        version.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        // scroll_y at Relayout so place_children re-writes max_scroll/ratio.
        self.scroll_y.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );
        ctx.register_animated_signal(&self.scroll_y);

        // Re-walk container a11y when selection / focus changes.
        if let Some(ref sel) = self.selection {
            sel.selection_signal().bind_to(
                ctx.self_id(),
                ctx.binding_registry(),
                BindingLevel::AccessibilityOnly,
            );
        }
        self.focused_index.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::AccessibilityOnly,
        );

        // Observe model changes.
        {
            let v = version.clone();
            let counter = Rc::new(Cell::new(0_u64));
            let strategy_obs = strategy.clone();
            let selection_obs = self.selection.clone();
            let len_fn = self.source.len_fn.clone();
            let scroll_reset = self.scroll_y.clone();
            let handle = (self.source.observe_fn)(Box::new(move |change| {
                match change {
                    DataChange::ItemsInserted { range } => {
                        strategy_obs.invalidate_rows(range.start..usize::MAX);
                        strategy_obs.resize((len_fn)());
                        if let Some(ref s) = selection_obs {
                            s.adjust_for_insert(range.start, range.end - range.start);
                        }
                    }
                    DataChange::ItemsRemoved { range } => {
                        strategy_obs.invalidate_rows(range.start..usize::MAX);
                        strategy_obs.resize((len_fn)());
                        if let Some(ref s) = selection_obs {
                            s.adjust_for_remove(range.start, range.end - range.start);
                        }
                    }
                    DataChange::ItemsMoved { .. } => {
                        strategy_obs.invalidate_rows(0..usize::MAX);
                    }
                    DataChange::ItemUpdated { index } => {
                        strategy_obs.invalidate_rows(*index..index + 1);
                    }
                    DataChange::WindowLoaded { range } => {
                        strategy_obs.invalidate_rows(range.start..range.end);
                    }
                    DataChange::Reset => {
                        strategy_obs.invalidate_rows(0..usize::MAX);
                        strategy_obs.resize(0);
                        if let Some(ref s) = selection_obs {
                            s.clear();
                        }
                        scroll_reset.set(0.0);
                    }
                }
                let next = counter.get() + 1;
                counter.set(next);
                v.set(next);
            }));
            ctx.own_handle(handle);
        }

        // Fire on_selection_changed on every selection change (interactive
        // or programmatic). The framework's reactive observers don't carry
        // an EventContext, so the callback receives only the selection set.
        if let (Some(sel), Some(cb)) = (&self.selection, &self.on_selection_changed) {
            let cb = cb.clone();
            ctx.effect(&sel.selection_signal(), move |set| cb(set));
        }

        // Rebuild when the loading flag toggles (shows/hides the overlay).
        if let Some(flag) = &self.is_loading {
            let v = version.clone();
            let c = Rc::new(Cell::new(0_u64));
            ctx.effect(flag, move |_| {
                c.set(c.get() + 1);
                v.set(c.get());
            });
        }

        // Self handlers: scroll wheel + keyboard.
        let mut handlers = HandlerSet::new().clips_children(true).focusable(true);
        {
            let scroll_y = self.scroll_y.clone();
            let max_scroll = self.max_scroll_y.clone();
            let line_height = strategy.estimated_row_height().max(1.0);
            let overscroll = self.overscroll_behavior;
            handlers = handlers.on_scroll(move |event, _ctx| match event {
                WidgetEvent::Scroll { delta, .. } => {
                    let dy = match delta {
                        ScrollDelta::Lines { y, .. } => y * line_height,
                        ScrollDelta::Pixels { y, .. } => *y,
                    };
                    let (new_y, moved) = crate::common::scroll::scroll_clamp_axis(
                        scroll_y.get(),
                        dy,
                        max_scroll.get(),
                    );
                    scroll_y.set(new_y);
                    crate::common::scroll::scroll_response(
                        moved,
                        overscroll == OverscrollBehavior::Contain,
                    )
                }
                _ => EventResponse::Ignored,
            });
        }
        handlers = handlers.on_key(build_grid_key_handler(GridKeyConfig {
            len_fn: self.source.len_fn.clone(),
            col_count: self.column_count.clone(),
            focused_index: self.focused_index.clone(),
            selection: self.selection.clone(),
            scroll_y: self.scroll_y.clone(),
            max_scroll_y: self.max_scroll_y.clone(),
            viewport_height: self.viewport_height.clone(),
            viewport_width: self.viewport_width.clone(),
            strategy: strategy.clone(),
            wrap_navigation: self.wrap_navigation,
            tab_traversal: self.tab_traversal,
            on_tile_activate: self.on_tile_activate.clone(),
            reorderable: self.reorderable,
            move_item_fn: self.source.move_item_fn.clone(),
            type_ahead_timeout: self.type_ahead_timeout,
            type_ahead_label: self.type_ahead_label.clone(),
        }));

        // Rubber-band marquee (Multi mode only). A container pointer handler
        // records the modifier state at press time for additive selection;
        // the drag handler sweeps the rectangle.
        let marquee_on = self.marquee_selection
            && self
                .selection
                .as_ref()
                .map(|s| s.mode() == SelectionMode::Multi)
                .unwrap_or(false);
        if marquee_on {
            let additive_mods = Rc::new(Cell::new(false));
            {
                let mods = additive_mods.clone();
                handlers = handlers.on_pointer_event(move |event, _ctx| {
                    if let WidgetEvent::PointerDown { modifiers, .. } = event {
                        mods.set(modifiers.ctrl() || modifiers.shift());
                    }
                    EventResponse::Ignored
                });
            }
            handlers = handlers.on_drag(build_marquee_handler(MarqueeConfig {
                marquee: self.marquee.clone(),
                selection: self.selection.clone().unwrap(),
                strategy: strategy.clone(),
                scroll_y: self.scroll_y.clone(),
                viewport_width: self.viewport_width.clone(),
                len_fn: self.source.len_fn.clone(),
                additive_mods,
            }));
        }

        // Drop target: intra-grid reorder + external drops, with an insertion
        // indicator painted by the overlay.
        if self.reorderable || self.on_item_drop.is_some() {
            let strategy_h = strategy.clone();
            let scroll_h = self.scroll_y.clone();
            let vp_w_h = self.viewport_width.clone();
            let len_h = self.source.len_fn.clone();
            let insertion_h = self.insertion.clone();
            let my_id = self.model_id;
            handlers = handlers.on_drag_hover(move |_payload, position, _ctx| {
                let idx = drag::insertion_index(
                    strategy_h.as_ref(),
                    position,
                    scroll_h.get(),
                    vp_w_h.get(),
                    (len_h)(),
                );
                insertion_h.set(Some(idx));
                bastyde_core::DropFeedback::NoFeedback
            });

            let insertion_leave = self.insertion.clone();
            handlers = handlers.on_drag_leave(move |_ctx| {
                insertion_leave.set(None);
            });

            let strategy_d = strategy.clone();
            let scroll_d = self.scroll_y.clone();
            let vp_w_d = self.viewport_width.clone();
            let len_d = self.source.len_fn.clone();
            let move_d = self.source.move_item_fn.clone();
            let drop_cb = self.on_item_drop.clone();
            let insertion_d = self.insertion.clone();
            handlers = handlers.on_drop(move |mut payload, position, ctx| {
                insertion_d.set(None);
                let to = drag::insertion_index(
                    strategy_d.as_ref(),
                    position,
                    scroll_d.get(),
                    vp_w_d.get(),
                    (len_d)(),
                );
                if let Some(data) = payload.take_typed::<drag::GridViewDragData>() {
                    if data.source_model_id == my_id {
                        let from = data.source_index;
                        let adjusted = if from < to { to.saturating_sub(1) } else { to };
                        if from != adjusted {
                            if let Some(ref mf) = move_d {
                                mf(from, adjusted);
                            }
                        }
                        return true;
                    }
                }
                if let Some(ref cb) = drop_cb {
                    return cb(payload, to, ctx);
                }
                false
            });
        }
        ctx.apply_self_handlers(handlers);

        // Incremental-loading hook: fire when the scroll nears the end.
        if let Some((threshold, cb)) = &self.on_near_end {
            let cb = cb.clone();
            let threshold = *threshold;
            let strategy_n = strategy.clone();
            let len_n = self.source.len_fn.clone();
            let vp_w_n = self.viewport_width.clone();
            let vp_h_n = self.viewport_height.clone();
            let fired_for = Rc::new(Cell::new(usize::MAX));
            let handle = self.scroll_y.observe(move |y| {
                let len = (len_n)();
                if len == 0 {
                    return;
                }
                let vr = strategy_n.visible_range(*y, vp_h_n.get(), vp_w_n.get(), len);
                if vr.end + threshold >= len && fired_for.get() != len {
                    fired_for.set(len);
                    cb();
                }
            });
            ctx.own_handle(handle);
        }

        // Children: body pane (or empty view), scrollbar, overlay.
        self.body_pane_id = None;
        self.empty_id = None;
        self.scrollbar_id = None;
        self.overlay_id = None;
        self.pinned_header_id = None;

        let len = self.source.len();
        if len == 0 {
            self.tile_map.borrow_mut().clear();
            if let Some(ref ef) = self.empty_view {
                self.empty_id = Some(ctx.add_boxed(ef()));
            }
        } else {
            // Pane → root total refresh (measuring strategies): re-place
            // this root when the body pane's measurements changed the
            // content total, so `max_scroll_y` / the thumb ratio pick up
            // the corrected value next frame.
            let pane_total_refresh = ctx.signal(0_u64);
            pane_total_refresh.bind_to(
                ctx.self_id(),
                ctx.binding_registry(),
                bastyde_core::binding::BindingLevel::Relayout,
            );
            let pane = GridBodyPane {
                len_fn: self.source.len_fn.clone(),
                with_item_fn: self.source.with_item_fn.clone(),
                delegate: self.delegate.clone(),
                strategy: strategy.clone(),
                viewport_width: self.viewport_width.clone(),
                viewport_height: self.viewport_height.clone(),
                column_count: self.column_count.clone(),
                scroll_y: self.scroll_y.clone(),
                selection: self.selection.clone(),
                focused_index: self.focused_index.clone(),
                on_tile_activate: self.on_tile_activate.clone(),
                tile_context_menu: self.tile_context_menu.clone(),
                reorderable: self.reorderable,
                model_id: self.model_id,
                tile_map: self.tile_map.clone(),
                header_factory: self.header_factory(),
                header_title: self.section_data.as_ref().map(|d| d.title_fn.clone()),
                // Fresh per GridView rebuild; persists across the
                // pane's own (buffer-exit / re-check) rebuilds.
                version: Signal::new(0_u64),
                prev_built_start: Rc::new(Cell::new(0)),
                prev_built_end: Rc::new(Cell::new(0)),
                total_refresh: pane_total_refresh,
                tile_entries: Vec::new(),
                header_entries: Vec::new(),
            };
            self.body_pane_id = Some(ctx.add(pane));

            let overlay = GridOverlay {
                focused_index: self.focused_index.clone(),
                scroll_y: self.scroll_y.clone(),
                strategy: strategy.clone(),
                viewport_width: self.viewport_width.clone(),
                marquee: self.marquee.clone(),
                insertion: self.insertion.clone(),
                style: self.style.clone(),
            };
            self.overlay_id = Some(ctx.add(overlay));

            // Sticky pinned header slot (reused widget showing the current
            // section's header at the viewport top).
            self.pinned_header_id = None;
            if self.pinned_section_headers {
                if let Some(factory) = self.header_factory() {
                    let ph = PinnedHeader {
                        current_section: self.current_section.clone(),
                        factory,
                        child: None,
                        style: self.style.clone(),
                    };
                    self.pinned_header_id = Some(ctx.add(ph));
                }
            }
        }

        if self.show_scrollbar {
            let sb = ScrollBar::new(
                ScrollBarOrientation::Vertical,
                self.scroll_y.clone(),
                self.max_scroll_y.clone(),
                self.viewport_ratio_y.clone(),
            );
            self.scrollbar_id = Some(ctx.add(sb));
        }

        // Loading overlay (on top of everything).
        self.loading_id = None;
        if let Some(flag) = &self.is_loading {
            if flag.get() {
                if let Some(ref lv) = self.loading_view {
                    self.loading_id = Some(ctx.add_boxed(lv()));
                }
            }
        }

        // Order = paint order. Overlay then loading paint last (on top).
        let mut children = Vec::new();
        if let Some(id) = self.body_pane_id {
            children.push(id);
        }
        if let Some(id) = self.empty_id {
            children.push(id);
        }
        if let Some(id) = self.scrollbar_id {
            children.push(id);
        }
        if let Some(id) = self.overlay_id {
            children.push(id);
        }
        if let Some(id) = self.pinned_header_id {
            children.push(id);
        }
        if let Some(id) = self.loading_id {
            children.push(id);
        }
        children
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let width = proposal.width.unwrap_or(400.0);
        let height = proposal.height.unwrap_or(400.0);
        self.viewport_width.set(width);
        self.viewport_height.set(height);
        Size::new(width, height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let Some(ref strategy) = self.strategy else {
            return;
        };
        let len = self.source.len();
        let vp_h = bounds.height;

        // Query the strategy at a SINGLE, stable body width per frame (using
        // the previous frame's scrollbar decision). Querying at two widths
        // would flip a variable strategy's column count back and forth and
        // reset its measurement cache every frame. The scrollbar appearing /
        // disappearing settles in one frame.
        let body_w = if self.last_needs_scrollbar.get() {
            (bounds.width - SCROLLBAR_THICKNESS).max(0.0)
        } else {
            bounds.width
        };
        self.viewport_width.set(body_w);

        let cols = strategy.column_count(body_w).max(1);
        if self.column_count.get() != cols {
            self.column_count.set(cols);
        }

        let total = strategy.total_content_height(len, body_w);
        let needs_sb = self.show_scrollbar && total > vp_h + 0.5;
        if self.last_needs_scrollbar.get() != needs_sb {
            self.last_needs_scrollbar.set(needs_sb);
        }
        let max_y = (total - vp_h).max(0.0);
        self.max_scroll_y.set(max_y);
        let ratio = if total > 0.0 {
            (vp_h / total).clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.viewport_ratio_y.set(ratio);
        // Clamp scroll (matches ListView).
        let cur = self.scroll_y.get();
        let clamped = cur.clamp(0.0, max_y);
        if (clamped - cur).abs() > 0.001 {
            self.scroll_y.set(clamped);
        }

        // Sticky pinned header: track the current section and decide whether
        // the in-flow header has scrolled above the top.
        let pinned_rect = if self.pinned_header_id.is_some() {
            let cur = strategy.current_section(self.scroll_y.get(), body_w);
            if let Some(cur) = cur {
                if self.current_section.get() != cur {
                    self.current_section.set(cur);
                }
                // Show the pinned slot only once the real header is above top.
                strategy.header_rect(cur, body_w).map(|r| {
                    let screen_y = bounds.y + r.y - self.scroll_y.get();
                    let visible = screen_y < bounds.y - 0.5;
                    (visible, r.height)
                })
            } else {
                None
            }
        } else {
            None
        };

        let body_rect_origin = bounds.origin();
        let body_size = Size::new(body_w, vp_h);
        for child in children.iter_mut() {
            if Some(child.id) == self.scrollbar_id {
                if needs_sb {
                    child.origin = Point::new(bounds.x + body_w, bounds.y);
                    child.size = Size::new(SCROLLBAR_THICKNESS, vp_h);
                } else {
                    child.origin = bounds.origin();
                    child.size = Size::ZERO;
                }
            } else if Some(child.id) == self.pinned_header_id {
                match pinned_rect {
                    Some((true, h)) => {
                        child.origin = bounds.origin();
                        child.size = Size::new(body_w, h);
                    }
                    _ => {
                        child.origin = bounds.origin();
                        child.size = Size::ZERO;
                    }
                }
            } else {
                // body pane / empty view / overlay all fill the body rect.
                child.origin = body_rect_origin;
                child.size = body_size;
            }
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Grid);
        if let Some(ref label) = self.a11y_label {
            builder.set_name(label.clone());
        }

        let total = self.source.len();
        let cols = self.column_count.get().max(1);
        let rows = total.div_ceil(cols);
        builder.set_row_count(rows);
        builder.set_column_count(cols);

        if let Some(ref sel) = self.selection {
            if sel.mode() == SelectionMode::Multi {
                builder.set_multiselectable(true);
            }
            let count = sel.count();
            if count > 0 {
                builder.set_value(format!(
                    "{} item{} selected",
                    count,
                    if count == 1 { "" } else { "s" }
                ));
            }
            builder.set_live(bastyde_core::accesskit::Live::Polite);
        }

        // Roving focus: point active_descendant at the focused tile node.
        if let Some(idx) = self.focused_index.get() {
            let map = self.tile_map.borrow();
            if let Some((_, tile_id)) = map.iter().find(|(i, _)| *i == idx) {
                builder.set_active_descendant(widget_id_to_node_id(*tile_id));
            }
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn children(&self) -> Vec<WidgetId> {
        let mut ids = Vec::new();
        if let Some(id) = self.body_pane_id {
            ids.push(id);
        }
        if let Some(id) = self.empty_id {
            ids.push(id);
        }
        if let Some(id) = self.scrollbar_id {
            ids.push(id);
        }
        if let Some(id) = self.overlay_id {
            ids.push(id);
        }
        if let Some(id) = self.pinned_header_id {
            ids.push(id);
        }
        if let Some(id) = self.loading_id {
            ids.push(id);
        }
        ids
    }

    fn clips_children(&self) -> bool {
        true
    }
}

/// A top-most, event-transparent leaf that paints the focus ring (and, in
/// later phases, the marquee rectangle and drag-insertion feedback). Drawing
/// here rather than in the container sidesteps any parent-vs-child paint-order
/// ambiguity — a last sibling always paints over the tiles.
struct GridOverlay {
    focused_index: Signal<Option<usize>>,
    scroll_y: Signal<f32>,
    strategy: Rc<dyn GridLayoutStrategy>,
    viewport_width: Rc<Cell<f32>>,
    marquee: Signal<Option<MarqueeState>>,
    insertion: Signal<Option<usize>>,
    style: Option<Rc<dyn GridViewStyle>>,
}

impl std::fmt::Debug for GridOverlay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GridOverlay").finish()
    }
}

impl GridOverlay {
    fn focus_recipe(&self, ctx: &PaintContext) -> bastyde_core::styles::GridFocusRingRecipe {
        resolve_grid_style(&self.style, ctx, |s| s.focus_ring())
    }
    fn marquee_recipe(&self, ctx: &PaintContext) -> bastyde_core::styles::GridMarqueeRecipe {
        resolve_grid_style(&self.style, ctx, |s| s.marquee())
    }
    fn insertion_recipe(&self, ctx: &PaintContext) -> bastyde_core::styles::GridInsertionRecipe {
        resolve_grid_style(&self.style, ctx, |s| s.insertion())
    }
}

/// Resolve a decoration recipe from the per-call override → theme slot →
/// stock default.
fn resolve_grid_style<R: Default>(
    override_style: &Option<Rc<dyn GridViewStyle>>,
    ctx: &PaintContext,
    f: impl Fn(&dyn GridViewStyle) -> R,
) -> R {
    if let Some(s) = override_style {
        f(s.as_ref())
    } else if let Some(s) = ctx.theme.style_slots.grid_view.as_ref() {
        f(s.as_ref())
    } else {
        R::default()
    }
}

impl Widget for GridOverlay {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Repaint on focus / scroll / marquee / insertion change.
        self.scroll_y.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        self.focused_index.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        self.marquee.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        self.insertion.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        // Transparent to pointer events so the body beneath stays interactive.
        ctx.apply_self_handlers(HandlerSet::new().event_pass_through(true));
        Vec::new()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut bastyde_canvas::Canvas, ctx: &PaintContext) {
        // Marquee rectangle (in widget-local coords → offset by bounds origin).
        if let Some(m) = self.marquee.get() {
            let lr = m.rect();
            let rect = Rect::new(bounds.x + lr.x, bounds.y + lr.y, lr.width, lr.height);
            let recipe = self.marquee_recipe(ctx);
            let c = recipe.role.resolve(&ctx.theme.colors);
            let fill = bastyde_tokens::Color::new(c.r(), c.g(), c.b(), recipe.fill_alpha);
            canvas.fill_rect(rect, fill);
            canvas.stroke_rect(rect, c, recipe.stroke_width);
        }

        // Drag-reorder insertion bar: a vertical accent bar at the leading
        // edge of the target tile (or trailing edge of the last tile when
        // appending).
        if let Some(ins) = self.insertion.get() {
            let vp_w = bounds.width;
            let scroll_y = self.scroll_y.get();
            let (bar_x, r) = if ins == 0 {
                (0.0, self.strategy.tile_rect(0, vp_w))
            } else {
                let prev = self.strategy.tile_rect(ins - 1, vp_w);
                // After the previous tile (handles both mid-row and append).
                (prev.x + prev.width + 1.0, prev)
            };
            let bar_x = if ins == 0 { r.x } else { bar_x };
            let y = bounds.y + r.y - scroll_y;
            let h = r.height;
            if y + h >= bounds.y && y <= bounds.bottom() {
                let recipe = self.insertion_recipe(ctx);
                let color = recipe.role.resolve(&ctx.theme.colors);
                let t = recipe.thickness;
                canvas.fill_rect(Rect::new(bounds.x + bar_x - t * 0.5, y, t, h), color);
            }
        }

        // Focus ring.
        let Some(idx) = self.focused_index.get() else {
            return;
        };
        let vp_w = bounds.width;
        let r = self.strategy.tile_rect(idx, vp_w);
        let scroll_y = self.scroll_y.get();
        let recipe = self.focus_recipe(ctx);
        let inset = recipe.inset;
        let stroke = recipe.thickness;
        let rx = bounds.x + r.x + inset;
        let ry = bounds.y + r.y - scroll_y + inset;
        let rw = (r.width - inset * 2.0).max(0.0);
        let rh = (r.height - inset * 2.0).max(0.0);
        // Cull if fully outside the viewport.
        if ry + rh < bounds.y || ry > bounds.bottom() {
            return;
        }
        let color = recipe.role.resolve(&ctx.theme.colors);
        canvas.fill_rect(Rect::new(rx, ry, rw, stroke), color); // top
        canvas.fill_rect(Rect::new(rx, ry + rh - stroke, rw, stroke), color); // bottom
        canvas.fill_rect(Rect::new(rx, ry, stroke, rh), color); // left
        canvas.fill_rect(Rect::new(rx + rw - stroke, ry, stroke, rh), color); // right
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }
}

/// The reused sticky-header slot: rebuilds its child from the section header
/// factory whenever the current section changes, and paints an opaque
/// background so tiles scrolling underneath don't show through.
struct PinnedHeader {
    current_section: Signal<usize>,
    #[allow(clippy::type_complexity)]
    factory: Rc<dyn Fn(usize) -> Box<dyn Widget>>,
    child: Option<WidgetId>,
    style: Option<Rc<dyn GridViewStyle>>,
}

impl std::fmt::Debug for PinnedHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PinnedHeader")
            .field("section", &self.current_section.get())
            .finish()
    }
}

impl Widget for PinnedHeader {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.current_section
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);
        let section = self.current_section.get();
        let id = ctx.add_boxed((self.factory)(section));
        self.child = Some(id);
        vec![id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
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

    fn paint(&self, bounds: Rect, canvas: &mut bastyde_canvas::Canvas, ctx: &PaintContext) {
        if bounds.height > 0.5 {
            let surface = self
                .style
                .as_ref()
                .or(ctx.theme.style_slots.grid_view.as_ref())
                .map(|s| s.pinned_header_surface())
                .unwrap_or(SurfaceRole::Raised);
            canvas.fill_rect(bounds, surface.resolve(&ctx.theme.colors));
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }

    fn clips_children(&self) -> bool {
        true
    }
}
