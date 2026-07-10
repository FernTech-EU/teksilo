// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Constructors and builder-pattern configuration for [`TreeView`].
//!

use super::*;

impl<T: 'static> TreeView<T> {
    /// Create a new TreeView backed by a `TreeModel<T>`.
    ///
    /// The delegate receives `(&item, &FlatEntry, selected)` and returns a
    /// boxed widget. The `FlatEntry` provides `depth`, `has_children`, and
    /// `is_expanded` for rendering indentation and expand/collapse toggles.
    pub fn new(
        model: TreeModel<T>,
        delegate: impl Fn(&T, &FlatEntry, bool) -> Box<dyn Widget> + 'static,
    ) -> Self {
        // Adapt the 3-arg delegate to the internal 4-arg shape by
        // discarding the context.
        let adapted =
            move |item: &T, entry: &FlatEntry, sel: bool, _ctx: &TreeRowContext<'_, T>| {
                delegate(item, entry, sel)
            };
        Self::new_internal(model, Rc::new(adapted))
    }

    /// Like [`new`](Self::new), but the delegate also receives a
    /// [`TreeRowContext`] from which `.toggle_callback()` can be
    /// pulled in a single line — eliminating the need to manually
    /// clone the slice handle outside the closure.
    ///
    /// ```rust
    /// # use bastyde_widgets::{TreeView, StandardTreeItem};
    /// # use bastyde_data::TreeModel;
    /// # use bastyde_i18n::lit;
    /// # struct Item { title: String }
    /// # let model: TreeModel<Item> = TreeModel::new();
    /// let _w = TreeView::new_with_context(model, |item, entry, selected, ctx| {
    ///     Box::new(
    ///         StandardTreeItem::new(lit!(&item.title))
    ///             .from_entry(entry)
    ///             .selected(selected)
    ///             .on_toggle_rc(ctx.toggle_callback())
    ///     )
    /// });
    /// ```
    pub fn new_with_context(
        model: TreeModel<T>,
        delegate: impl Fn(&T, &FlatEntry, bool, &TreeRowContext<'_, T>) -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self::new_internal(model, Rc::new(delegate))
    }

    fn new_internal(model: TreeModel<T>, delegate: Rc<TreeDelegate<T>>) -> Self {
        let slice = Rc::new(TreeSlice::new(model));
        let source = Rc::new(TreeSource::from_data_source(slice.clone()));
        // Built-in wrapper: rebuild the `NodeId` `FlatEntry` + `TreeRowContext`
        // from the visible index so the existing 3-/4-arg delegate keeps its
        // exact API. `with_row` only invokes this for a present row, so
        // `visible_node_id(i)` is `Some`; the `None` arm is an unreachable guard.
        let slice_for_rows = slice.clone();
        let row_delegate: Rc<RowDelegate<T>> = Rc::new(move |i, item, meta, selected| {
            let handle = slice_for_rows.handle();
            match handle.visible_node_id(i) {
                Some(node_id) => {
                    let entry = FlatEntry {
                        node_id,
                        depth: meta.depth,
                        has_children: meta.has_children,
                        is_expanded: meta.is_expanded,
                    };
                    let row_ctx = TreeRowContext {
                        slice: &handle,
                        node_id,
                    };
                    delegate(item, &entry, selected, &row_ctx)
                }
                None => crate::data_views::default_placeholder(),
            }
        });
        Self::assemble(source, Some(slice), row_delegate)
    }

    /// Create a TreeView backed by any [`TreeDataSource`] — an external source of
    /// truth (e.g. an entity store) carrying its own `Key`, so it needs no
    /// `TreeModel` mirror. The delegate receives `(&item, &TreeRow, selected)`;
    /// [`TreeRow`] exposes `depth` / `has_children` / `is_expanded` and a one-call
    /// chevron `toggle_callback()`. Drop validation + lazy windowing route
    /// through the source's `can_accept` / `accept_drop` / `row_state`.
    pub fn from_source<S: TreeDataSource<Item = T>>(
        source: S,
        delegate: impl Fn(&T, &TreeRow, bool) -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self::from_source_rc(Rc::new(source), Rc::new(delegate))
    }

    fn from_source_rc<S: TreeDataSource<Item = T>>(
        s: Rc<S>,
        delegate: Rc<SourceTreeDelegate<T>>,
    ) -> Self {
        let source = Rc::new(TreeSource::from_data_source(s));
        let source_for_rows = source.clone();
        let row_delegate: Rc<RowDelegate<T>> = Rc::new(move |i, item, _meta, selected| {
            let row = TreeSource::row_context(&source_for_rows, i);
            delegate(item, &row, selected)
        });
        Self::assemble(source, None, row_delegate)
    }

    /// Like [`from_source`](Self::from_source) but with **keyed** selection: the
    /// `KeyedSelectionModel<S::Key>` tracks selection by source identity, so it
    /// survives expand / collapse / filter / reorder and stays consistent across
    /// two views of the same source. The view stays `TreeView<T>` — the `Key` is
    /// captured here. Pruning consults the source's
    /// [`contains_key`](bastyde_data::TreeDataSource::contains_key), so a
    /// collapsed-but-present node keeps its selection.
    pub fn from_source_keyed<S: TreeDataSource<Item = T>>(
        source: S,
        keyed: KeyedSelectionModel<S::Key>,
        delegate: impl Fn(&T, &TreeRow, bool) -> Box<dyn Widget> + 'static,
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
            Rc::new(move || s.visible_count()) as Rc<dyn Fn() -> usize>
        };
        let contains = {
            let s = s.clone();
            Rc::new(move |k: &S::Key| s.contains_key(k)) as Rc<dyn Fn(&S::Key) -> bool>
        };
        let row_selection = RowSelection::from_keyed(keyed, key_at, len, contains);
        let mut view = Self::from_source_rc(s, Rc::new(delegate));
        view.row_selection = Some(row_selection);
        view
    }

    fn assemble(
        source: Rc<TreeSource<T>>,
        slice: Option<Rc<TreeSlice<T>>>,
        row_delegate: Rc<RowDelegate<T>>,
    ) -> Self {
        let view_id = ViewId::next(ViewKind::Tree);
        Self {
            source,
            slice,
            row_delegate,
            item_height: DEFAULT_ITEM_HEIGHT,
            height_source: HeightSource::Uniform,
            metrics: Rc::new(RefCell::new(RowMetrics::uniform(DEFAULT_ITEM_HEIGHT, 0.0))),
            row_selection: None,
            focused_index: Rc::new(Cell::new(None)),
            type_ahead_label: None,
            type_ahead_timeout: crate::common::type_ahead::DEFAULT_TYPE_AHEAD_TIMEOUT,
            type_ahead: crate::common::type_ahead::TypeAheadState::new(),
            reorderable: false,
            export: crate::data_views::RowExport::default(),
            row_click_expands: true,
            drop_feedback: Signal::new(None),
            // Replaced at build with the live tree signals.
            view_focused: Signal::new(false),
            focus_visible: Signal::new(false),
            on_activate: None,
            activate_on: crate::data_views::ActivateOn::default(),
            overscroll_behavior: OverscrollBehavior::default(),
            smooth_scrolling: true,
            smooth_scroll_duration: Duration::from_millis(150),
            scroll_bar_style: ScrollBarMode::Permanent,
            scroll_y: Signal::new_animated(0.0),
            max_scroll_y: Signal::new(0.0),
            viewport_ratio_y: Signal::new(1.0),
            version: Signal::new(0_u64),
            prev_built_start: Rc::new(Cell::new(0)),
            prev_built_end: Rc::new(Cell::new(0)),
            item_entries: Vec::new(),
            scrollbar_id: None,
            viewport_height: Rc::new(Cell::new(600.0)),
            viewport_bounds: Rc::new(Cell::new(Rect::ZERO)),
            tree_id: view_id,
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

    /// Re-materialize `self.metrics` after a height-mode / item-height
    /// builder call.
    fn remake_metrics(&self) {
        *self.metrics.borrow_mut() = self.height_source.make_metrics(self.item_height, 0.0);
    }

    /// Set the fixed height per row (default 28.0) — the uniform fast
    /// path. Mutually exclusive with [`item_height_fn`](Self::item_height_fn)
    /// and [`auto_item_height`](Self::auto_item_height); the last mode
    /// setter wins.
    pub fn item_height(mut self, height: f32) -> Self {
        self.item_height = height;
        self.height_source = HeightSource::Uniform;
        self.remake_metrics();
        self
    }

    /// Enable or disable animated wheel scrolling (enabled by default).
    /// When disabled, wheel events snap immediately to the new offset.
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
    /// layout column for it, mirroring `ScrollArea::scroll_bar_style`.
    pub fn scroll_bar_style(mut self, style: ScrollBarMode) -> Self {
        self.scroll_bar_style = style;
        self
    }

    /// Per-row heights from a callback over the *flat (visible) index*.
    /// The callback must be pure (same index + same data → same height);
    /// it is re-swept from the first changed flat index on every model
    /// change or expand/collapse. No measurement pass runs.
    pub fn item_height_fn(mut self, f: impl Fn(usize) -> f32 + 'static) -> Self {
        self.height_source = HeightSource::Exact(Rc::new(f));
        self.remake_metrics();
        self
    }

    /// Auto-measured row heights: each realized row is measured at the
    /// tree's content width (height-for-width), unrealized rows assume
    /// `estimated`. Scroll anchoring keeps content above the viewport
    /// stationary as estimates are corrected; measured heights above a
    /// toggled row survive expand/collapse (divergence-driven
    /// invalidation).
    pub fn auto_item_height(mut self, estimated: f32) -> Self {
        self.height_source = HeightSource::Auto { estimated };
        self.remake_metrics();
        self
    }

    /// Whether a row-body PointerUp on a branch row auto-toggles its
    /// expansion (default `true`). Set to `false` when the delegate
    /// provides its own chevron tap target (e.g. `StandardTreeItem`)
    /// — without this, the auto-toggle fires in addition to the
    /// chevron's own click and they cancel out, leaving the row
    /// expanded only on body clicks.
    pub fn row_click_expands(mut self, b: bool) -> Self {
        self.row_click_expands = b;
        self
    }

    /// Set the index-based selection model (visible positions). For
    /// identity-based selection that survives expand / collapse / filter and
    /// node moves, use [`keyed_selection`](Self::keyed_selection) instead.
    pub fn selection(mut self, sel: SelectionModel) -> Self {
        self.row_selection = Some(RowSelection::from_index(sel));
        self
    }

    /// Set a keyed selection model (by `NodeId`). Selection is tracked by node
    /// identity, so it survives expand / collapse, filtering, and node moves —
    /// and stays consistent if two views share the model. Pruned of deleted
    /// nodes on each slice change. Mutually exclusive with
    /// [`selection`](Self::selection) (last one set wins).
    pub fn keyed_selection(mut self, keyed: KeyedSelectionModel<NodeId>) -> Self {
        // Built-in `TreeModel` path only; on `from_source` use
        // [`from_source_keyed`](Self::from_source_keyed) (the `Key` differs).
        let Some(slice) = self.slice.clone() else {
            return self;
        };
        let key_at = {
            let tsh = slice.handle();
            Rc::new(move |i| tsh.visible_node_id(i)) as Rc<dyn Fn(usize) -> Option<NodeId>>
        };
        let len = {
            let tsh = slice.handle();
            Rc::new(move || tsh.visible_count()) as Rc<dyn Fn() -> usize>
        };
        // A collapsed-but-present node must NOT be pruned, so existence is
        // checked against the tree, not the visible projection.
        let contains = {
            let tsh = slice.handle();
            Rc::new(move |n: &NodeId| tsh.tree().with_item(*n, |_| ()).is_some())
                as Rc<dyn Fn(&NodeId) -> bool>
        };
        self.row_selection = Some(RowSelection::from_keyed(keyed, key_at, len, contains));
        self
    }

    /// Enable intra-widget drag reordering.
    ///
    /// When enabled, tree rows can be dragged to reparent or reorder them.
    /// Before/Into/After is chosen by where in the row the pointer drops; the
    /// move is cycle-guarded — a drop onto the node itself or into its own
    /// subtree is refused and shows no insertion line. Keyboard equivalent:
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
    /// `on_drag_out` (works out of the box for a `TreeSlice`/`TreeModel`).
    pub fn on_rows_transferred_out(
        mut self,
        f: impl Fn(&[usize], &mut bastyde_core::widget::EventContext) + 'static,
    ) -> Self {
        self.export.set_on_rows_transferred_out(f);
        self
    }

    /// Accept exported rows dropped from a **different** view or source without
    /// writing a custom `TreeDataSource`. Pair with
    /// [`on_rows_received`](Self::on_rows_received), which is handed the dropped
    /// items and the insertion index. (Same-view reorder is
    /// [`reorderable`](Self::reorderable); a custom `TreeDataSource` can still
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
        f: impl Fn(Vec<T>, usize, &mut bastyde_core::widget::EventContext) + 'static,
    ) -> Self {
        self.export.set_on_rows_received(f);
        self
    }

    /// Set the row-**activation** handler — invoked with the flat row index on a
    /// primary click on the row body, or **Enter** on the focused row.
    /// Activation is distinct from *selection*: arrow-key navigation and
    /// **Space** move / toggle the selection but do **not** activate, so a view
    /// can open/commit a row on a deliberate click/Enter without firing on
    /// every navigation step.
    pub fn on_activate(mut self, f: impl Fn(usize) + 'static) -> Self {
        self.on_activate = Some(Rc::new(f));
        self
    }

    /// Choose single- vs double-click activation (default
    /// [`ActivateOn::DoubleClick`](crate::ActivateOn) — the cross-platform
    /// convention; pass [`SingleClick`](crate::ActivateOn::SingleClick) for the
    /// KDE/web/Scrivener feel). Enter activates in either mode.
    pub fn activate_on(mut self, mode: crate::data_views::ActivateOn) -> Self {
        self.activate_on = mode;
        self
    }

    /// Enable **type-ahead** ("type to jump"): typing a printable character
    /// while the tree has keyboard focus jumps the selection to the next
    /// *visible* row whose label starts with the accumulated search term,
    /// wrapping around (Qt `keyboardSearch` / macOS & Windows type-select).
    /// `label(&item)` yields the searchable text; matching is
    /// ASCII-case-insensitive. A pause longer than the
    /// [`type_ahead_timeout`](Self::type_ahead_timeout) starts a fresh term.
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

    /// Expand a node programmatically. No-op on the `from_source` path (which
    /// owns its own expand state — use the source's `set_expanded`).
    pub fn expand(&self, node: bastyde_data::NodeId) {
        if let Some(slice) = &self.slice {
            slice.expand(node);
        }
    }

    /// Collapse a node programmatically. No-op on the `from_source` path.
    pub fn collapse(&self, node: bastyde_data::NodeId) {
        if let Some(slice) = &self.slice {
            slice.collapse(node);
        }
    }

    /// Toggle a node's expand/collapse state. No-op on the `from_source` path.
    pub fn toggle(&self, node: bastyde_data::NodeId) {
        if let Some(slice) = &self.slice {
            slice.toggle(node);
        }
    }

    /// Expand all nodes. No-op on the `from_source` path.
    pub fn expand_all(&self) {
        if let Some(slice) = &self.slice {
            slice.expand_all();
        }
    }

    /// Collapse all nodes. No-op on the `from_source` path.
    pub fn collapse_all(&self) {
        if let Some(slice) = &self.slice {
            slice.collapse_all();
        }
    }

    /// Access the internal `TreeSlice` (for persistence of expand state).
    /// `None` on the [`from_source`](Self::from_source) path, which has no
    /// `TreeSlice` (the external source owns expand state).
    pub fn tree_slice(&self) -> Option<&TreeSlice<T>> {
        self.slice.as_deref()
    }

    pub(super) fn total_content_height(&self) -> f32 {
        self.metrics
            .borrow_mut()
            .total_height(self.source.visible_count())
    }

    pub(super) fn visible_range(&self) -> (usize, usize) {
        self.metrics.borrow_mut().visible_range(
            self.scroll_y.get(),
            self.viewport_height.get(),
            self.source.visible_count(),
            BUFFER_ITEMS,
        )
    }

    pub(super) fn clamp_scroll(&self) {
        let max = self.max_scroll_y.get();
        let current = self.scroll_y.get();
        let clamped = current.clamp(0.0, max);
        if (clamped - current).abs() > 0.001 {
            self.scroll_y.set(clamped);
        }
    }
}
