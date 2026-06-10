//! Virtualized hierarchical tree widget.
//!
//! `TreeView` displays a `TreeModel<T>` as an indented, expandable/collapsible
//! list. Internally it creates a `TreeSlice` for per-view expand state and
//! virtualizes rendering like `ListView` (only visible rows have widgets).
//! Row heights come in three modes: uniform (`item_height`, the default),
//! exact per-flat-index callback (`item_height_fn`), and auto-measured
//! (`auto_item_height`) — expand/collapse keeps measured heights above the
//! toggle via the slice's `first_changed_index` divergence.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use bastyde_canvas::{Point, Rect, Size, SizeProposal};

use bastyde_core::DropFeedback;
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::drag_payload::DragPayload;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;

use bastyde_data::selection_model::SelectionModel;
use bastyde_data::tree_slice::{FlatEntry, TreeSlice, TreeSliceHandle};
use bastyde_data::{NodeId, TreeModel};

use crate::common::row_metrics::{HeightSource, RowMetrics, SharedRowMetrics};
use crate::common::scroll::OverscrollBehavior;
use crate::scroll_bar::{ScrollBar, ScrollBarOrientation};

const BUFFER_ITEMS: usize = 5;
const DEFAULT_ITEM_HEIGHT: f32 = 28.0;
const SCROLLBAR_THICKNESS: f32 = 12.0;

/// Internal drag payload for intra-TreeView reordering.
#[derive(Debug, Clone)]
struct TreeViewDragData {
    /// The NodeId of the node being dragged.
    source_node: NodeId,
    /// Stable ID to identify this TreeView instance.
    source_tree_id: usize,
}

/// Per-row context passed to a 4-arg TreeView delegate. Carries a
/// reference to the slice handle and the row's `NodeId` so the
/// delegate can wire chevron toggles and other tree-aware behavior
/// without manually cloning state outside the closure.
///
/// Created internally by [`TreeView::new_with_context`]. Not
/// constructed directly by user code.
pub struct TreeRowContext<'a, T: 'static> {
    slice: &'a TreeSliceHandle<T>,
    node_id: bastyde_data::NodeId,
}

impl<'a, T: 'static> TreeRowContext<'a, T> {
    /// Toggle callback for this row's chevron. Wires in one line:
    /// `.on_toggle_rc(ctx.toggle_callback())`.
    pub fn toggle_callback(&self) -> std::rc::Rc<dyn Fn(&mut bastyde_core::widget::EventContext)> {
        let slice = self.slice.clone();
        let node = self.node_id;
        std::rc::Rc::new(move |_ctx| slice.toggle_expand(node))
    }

    /// Cloned handle to the slice — call `.toggle_expand(node)`,
    /// `.expand(node)`, `.collapse(node)` directly.
    pub fn slice_handle(&self) -> TreeSliceHandle<T> {
        self.slice.clone()
    }

    pub fn node_id(&self) -> bastyde_data::NodeId {
        self.node_id
    }
}

/// Internal delegate type: takes the inputs the 3-arg form gets plus
/// the optional `TreeRowContext`. Both the 3-arg `new` and the 4-arg
/// `new_with_context` produce a closure of this shape.
type TreeDelegate<T> = dyn Fn(&T, &FlatEntry, bool, &TreeRowContext<'_, T>) -> Box<dyn Widget>;

/// A virtualized hierarchical tree widget backed by a `TreeModel<T>`.
///
/// ```ignore
/// TreeView::new(tree_model, |item, entry, selected| {
///     let indent = entry.depth as f32 * 20.0;
///     Box::new(HStack::new()
///         .child(Padding::new(0.0, 0.0, 0.0, indent))
///         .child(TextWidget::new(lit!(&item.title))))
/// })
/// .item_height(28.0)
/// ```
pub struct TreeView<T: 'static> {
    tree_slice: TreeSlice<T>,
    delegate: Rc<TreeDelegate<T>>,
    item_height: f32,
    /// Height-mode selection (uniform / exact callback / auto-measure).
    height_source: HeightSource,
    /// Row geometry — all virtualization consumers go through this.
    metrics: SharedRowMetrics,
    selection: Option<SelectionModel>,

    /// Keyboard-focused flat index.
    focused_index: Rc<Cell<Option<usize>>>,

    /// Enable intra-widget drag reordering.
    reorderable: bool,

    /// Whether a row-body PointerUp on a branch row auto-toggles its
    /// expansion. Defaults to `true` (legacy behavior — convenient
    /// for hand-built delegates without an explicit chevron). Set to
    /// `false` when the delegate provides its own chevron tap target
    /// (e.g. `StandardTreeItem`) to avoid the auto-toggle firing in
    /// addition to the chevron's own click and cancelling out.
    row_click_expands: bool,

    /// Active drop feedback (set by on_drag_hover, cleared by on_drag_leave,
    /// read by paint). Reactive Signal — bound at `RepaintOnly` so any
    /// `set(...)` call dirties the TreeView for repaint automatically.
    drop_feedback: Signal<Option<(f32, f32)>>, // (y, width) for insertion line

    // Persistent scroll state
    scroll_y: Signal<f32>,
    max_scroll_y: Signal<f32>,
    /// Scroll-chaining behavior at the boundary (default `Chain`).
    overscroll_behavior: OverscrollBehavior,
    viewport_ratio_y: Signal<f32>,

    /// Rebuild trigger. A persistent field (re-bound each build) so
    /// `place_children`'s post-measure realization re-check can request
    /// a rebuild when corrected offsets reveal unrealized viewport rows.
    version: Signal<u64>,
    /// Buffered row range materialized by the latest build.
    prev_built_start: Rc<Cell<usize>>,
    prev_built_end: Rc<Cell<usize>>,

    // Set during build
    item_entries: Vec<(usize, WidgetId)>, // (flat_index, widget_id)
    scrollbar_id: Option<WidgetId>,
    viewport_height: Rc<Cell<f32>>,
    tree_id: usize,
}

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
    /// ```ignore
    /// TreeView::new_with_context(model, |item, entry, selected, ctx| {
    ///     Box::new(
    ///         StandardTreeItem::new(lit!(&item.title))
    ///             .from_entry(entry)
    ///             .selected(selected)
    ///             .on_toggle_rc(ctx.toggle_callback())
    ///     )
    /// })
    /// ```
    pub fn new_with_context(
        model: TreeModel<T>,
        delegate: impl Fn(&T, &FlatEntry, bool, &TreeRowContext<'_, T>) -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self::new_internal(model, Rc::new(delegate))
    }

    fn new_internal(model: TreeModel<T>, delegate: Rc<TreeDelegate<T>>) -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
        let tree_slice = TreeSlice::new(model);
        Self {
            tree_slice,
            delegate,
            item_height: DEFAULT_ITEM_HEIGHT,
            height_source: HeightSource::Uniform,
            metrics: Rc::new(RefCell::new(RowMetrics::uniform(DEFAULT_ITEM_HEIGHT, 0.0))),
            selection: None,
            focused_index: Rc::new(Cell::new(None)),
            reorderable: false,
            row_click_expands: true,
            drop_feedback: Signal::new(None),
            overscroll_behavior: OverscrollBehavior::default(),
            scroll_y: Signal::new_animated(0.0),
            max_scroll_y: Signal::new(0.0),
            viewport_ratio_y: Signal::new(1.0),
            version: Signal::new(0_u64),
            prev_built_start: Rc::new(Cell::new(0)),
            prev_built_end: Rc::new(Cell::new(0)),
            item_entries: Vec::new(),
            scrollbar_id: None,
            viewport_height: Rc::new(Cell::new(600.0)),
            tree_id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        }
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

    /// Set the selection model.
    pub fn selection(mut self, sel: SelectionModel) -> Self {
        self.selection = Some(sel);
        self
    }

    /// Enable intra-widget drag reordering.
    ///
    /// When enabled, tree items can be dragged and dropped to reparent or
    /// reorder them. The underlying `TreeModel::move_node()` is called
    /// automatically. Keyboard equivalent: Alt+ArrowUp/Down.
    pub fn reorderable(mut self, enabled: bool) -> Self {
        self.reorderable = enabled;
        self
    }

    /// Expand a node programmatically.
    pub fn expand(&self, node: bastyde_data::NodeId) {
        self.tree_slice.expand(node);
    }

    /// Collapse a node programmatically.
    pub fn collapse(&self, node: bastyde_data::NodeId) {
        self.tree_slice.collapse(node);
    }

    /// Toggle a node's expand/collapse state.
    pub fn toggle(&self, node: bastyde_data::NodeId) {
        self.tree_slice.toggle(node);
    }

    /// Expand all nodes.
    pub fn expand_all(&self) {
        self.tree_slice.expand_all();
    }

    /// Collapse all nodes.
    pub fn collapse_all(&self) {
        self.tree_slice.collapse_all();
    }

    /// Access the internal `TreeSlice` (for persistence of expand state).
    pub fn tree_slice(&self) -> &TreeSlice<T> {
        &self.tree_slice
    }

    fn total_content_height(&self) -> f32 {
        self.metrics
            .borrow_mut()
            .total_height(self.tree_slice.visible_count())
    }

    fn visible_range(&self) -> (usize, usize) {
        self.metrics.borrow_mut().visible_range(
            self.scroll_y.get(),
            self.viewport_height.get(),
            self.tree_slice.visible_count(),
            BUFFER_ITEMS,
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

impl<T: 'static> std::fmt::Debug for TreeView<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreeView")
            .field("visible_count", &self.tree_slice.visible_count())
            .field("item_height", &self.item_height)
            .field("scroll_y", &self.scroll_y.get())
            .finish()
    }
}

impl<T: 'static> Widget for TreeView<T> {
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        // --- Version signal for rebuild triggering ---
        // A persistent field (not `ctx.signal`) so the realization
        // re-check in `place_children` can bump it after measurement.
        let version = self.version.clone();
        version.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        // Bind scroll_y at Relayout so place_children runs on every scroll
        // position change (repositions items) without a full rebuild.
        self.scroll_y.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );

        ctx.register_animated_signal(&self.scroll_y);

        // Bind drop_feedback at RepaintOnly so `set(...)` calls from
        // on_drag_hover / on_drag_leave dirty the TreeView's paint cache
        // without triggering a rebuild.
        self.drop_feedback.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );

        // --- Observe tree slice version (covers both data mutations and expand/collapse) ---
        let slice_version = self.tree_slice.version_signal();
        let version_for_data = version.clone();
        let data_ver = Rc::new(Cell::new(0_u64));
        ctx.effect(&slice_version, {
            let dv = data_ver.clone();
            let ver = version_for_data.clone();
            let metrics = self.metrics.clone();
            let slice = self.tree_slice.handle();
            move |_| {
                // Slice observers fire synchronously per reflatten, so
                // `first_changed_index()` describes exactly this change:
                // heights of flat rows before it (e.g. above an
                // expand/collapse point) stay valid.
                metrics
                    .borrow_mut()
                    .apply_divergence(slice.first_changed_index(), slice.visible_count());
                let next = dv.get() + 1;
                dv.set(next);
                ver.set(next);
            }
        });

        // --- Observe selection changes (rebuild to update delegate's `selected` param) ---
        if let Some(ref sel) = self.selection {
            let version_for_sel = version.clone();
            let sel_ver = Rc::new(Cell::new(0_u64));
            ctx.effect(&sel.selection_signal(), {
                let sv = sel_ver.clone();
                move |_| {
                    let next = sv.get() + 1;
                    sv.set(next);
                    version_for_sel.set(next);
                }
            });
        }

        // --- Observe scroll position changes (rebuild only when items leave/enter buffer) ---
        let viewport_h = self.viewport_height.clone();
        // Track the buffered range from this build. Only trigger a rebuild
        // when the visible range exceeds the buffer — most scrolls just need
        // a relayout (handled by scroll_y's Relayout binding above).
        let (built_start, built_end) = self.visible_range();
        self.prev_built_start.set(built_start);
        self.prev_built_end.set(built_end);
        let version_for_scroll = version.clone();
        let scroll_ver = Rc::new(Cell::new(0_u64));
        let scroll_handle = self.scroll_y.observe({
            let pbs = self.prev_built_start.clone();
            let pbe = self.prev_built_end.clone();
            let sv = scroll_ver.clone();
            let metrics = self.metrics.clone();
            let slice = self.tree_slice.handle();
            move |y| {
                let (visible_start, visible_end) = metrics.borrow_mut().visible_range(
                    *y,
                    viewport_h.get(),
                    slice.visible_count(),
                    0,
                );
                // Only rebuild when visible items fall outside the currently-built range
                if visible_start < pbs.get() || visible_end > pbe.get() {
                    let new_start = visible_start.saturating_sub(BUFFER_ITEMS);
                    let new_end = visible_end + BUFFER_ITEMS;
                    pbs.set(new_start);
                    pbe.set(new_end);
                    let next = sv.get() + 1;
                    sv.set(next);
                    version_for_scroll.set(next);
                }
            }
        });
        ctx.own_handle(scroll_handle);

        // --- Scroll event handler + DnD ---
        let scroll_y = self.scroll_y.clone();
        let max_scroll = self.max_scroll_y.clone();
        let line_height = self.item_height;
        let overscroll_behavior = self.overscroll_behavior;
        let mut handlers = HandlerSet::new()
            .on_scroll(move |event, _ctx| match event {
                bastyde_core::event::WidgetEvent::Scroll { delta, .. } => {
                    let dy = match delta {
                        bastyde_core::event::ScrollDelta::Lines { y, .. } => y * line_height,
                        bastyde_core::event::ScrollDelta::Pixels { y, .. } => *y,
                    };
                    let current = scroll_y.get();
                    let max = max_scroll.get();
                    let (new_y, moved) = crate::common::scroll::scroll_clamp_axis(current, dy, max);
                    scroll_y.set(new_y);
                    // Chain to an ancestor scrollable when fully clamped
                    // (unless Contain), otherwise consume.
                    crate::common::scroll::scroll_response(
                        moved,
                        overscroll_behavior == OverscrollBehavior::Contain,
                    )
                }
                _ => bastyde_core::event::EventResponse::Ignored,
            })
            .clips_children(true)
            .focusable(true);

        // --- Keyboard navigation + expand/collapse + Alt+Arrow reorder ---
        {
            let tsh = self.tree_slice.handle();
            let sel_for_key = self.selection.clone();
            let fi = self.focused_index.clone();
            let reorderable = self.reorderable;
            let scroll_for_nav = self.scroll_y.clone();
            let metrics_for_nav = self.metrics.clone();
            let max_for_nav = self.max_scroll_y.clone();
            let vh_for_nav = self.viewport_height.clone();

            handlers = handlers.on_key(move |event, _ctx| {
                if let bastyde_core::event::WidgetEvent::KeyDown { key, modifiers, .. } = event {
                    let visible_count = tsh.visible_count();
                    if visible_count == 0 {
                        return bastyde_core::event::EventResponse::Ignored;
                    }

                    let current = fi.get().unwrap_or(0).min(visible_count - 1);

                    // Alt+Arrow: sibling reorder (when reorderable)
                    if modifiers.alt() && reorderable {
                        let selected_idx = sel_for_key
                            .as_ref()
                            .and_then(|s| s.selected_indices().first().copied())
                            .or(fi.get());

                        if let Some(flat_idx) = selected_idx
                            && let Some(entry) = tsh.entry_at(flat_idx)
                        {
                            let node_id = entry.node_id;
                            let tree = tsh.tree();
                            let parent = tree.parent(node_id);

                            // Determine siblings: either children of parent or root list
                            let (siblings, is_root_level) = if let Some(parent_id) = parent {
                                (tree.children(parent_id), false)
                            } else {
                                // Node is a root - get all roots
                                let root_count = tree.root_count();
                                let siblings: Vec<NodeId> =
                                    (0..root_count).map(|i| tree.root(i)).collect();
                                (siblings, true)
                            };

                            let sibling_idx =
                                siblings.iter().position(|&n| n == node_id).unwrap_or(0);

                            match key {
                                bastyde_core::event::Key::ArrowUp if sibling_idx > 0 => {
                                    if is_root_level {
                                        tree.move_to_root(node_id, sibling_idx - 1);
                                    } else {
                                        tree.move_node(
                                            node_id,
                                            parent.expect("non-root branch implies parent is Some"),
                                            sibling_idx - 1,
                                        );
                                    }
                                    // Find new flat index after node was moved
                                    for new_flat in 0..visible_count {
                                        if tsh.visible_node_id(new_flat) == Some(node_id) {
                                            fi.set(Some(new_flat));
                                            if let Some(ref sel) = sel_for_key {
                                                sel.select(new_flat);
                                            }
                                            break;
                                        }
                                    }
                                    return bastyde_core::event::EventResponse::Handled;
                                }
                                bastyde_core::event::Key::ArrowDown
                                    if sibling_idx + 1 < siblings.len() =>
                                {
                                    if is_root_level {
                                        tree.move_to_root(node_id, sibling_idx + 1);
                                    } else {
                                        tree.move_node(
                                            node_id,
                                            parent.expect("non-root branch implies parent is Some"),
                                            sibling_idx + 1,
                                        );
                                    }
                                    // Find new flat index after node was moved
                                    for new_flat in 0..visible_count {
                                        if tsh.visible_node_id(new_flat) == Some(node_id) {
                                            fi.set(Some(new_flat));
                                            if let Some(ref sel) = sel_for_key {
                                                sel.select(new_flat);
                                            }
                                            break;
                                        }
                                    }
                                    return bastyde_core::event::EventResponse::Handled;
                                }
                                _ => {}
                            }
                        }
                        return bastyde_core::event::EventResponse::Ignored;
                    }

                    // ArrowRight: expand / ArrowLeft: collapse or move to parent
                    match key {
                        bastyde_core::event::Key::ArrowRight => {
                            if let Some(entry) = tsh.entry_at(current)
                                && entry.has_children
                                && !entry.is_expanded
                            {
                                tsh.expand(entry.node_id);
                                return bastyde_core::event::EventResponse::Handled;
                            }
                        }
                        bastyde_core::event::Key::ArrowLeft => {
                            if let Some(entry) = tsh.entry_at(current) {
                                if entry.is_expanded {
                                    tsh.collapse(entry.node_id);
                                    return bastyde_core::event::EventResponse::Handled;
                                }
                                // If leaf or collapsed, move to parent
                                let parent = tsh.tree().parent(entry.node_id);
                                if let Some(parent_id) = parent {
                                    // Find parent's flat index
                                    for i in 0..visible_count {
                                        if tsh.visible_node_id(i) == Some(parent_id) {
                                            fi.set(Some(i));
                                            if let Some(ref sel) = sel_for_key {
                                                sel.select(i);
                                            }
                                            return bastyde_core::event::EventResponse::Handled;
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }

                    // Navigation keys
                    let new_idx = match key {
                        bastyde_core::event::Key::ArrowDown => {
                            Some(current.saturating_add(1).min(visible_count - 1))
                        }
                        bastyde_core::event::Key::ArrowUp => Some(current.saturating_sub(1)),
                        bastyde_core::event::Key::Home => Some(0),
                        bastyde_core::event::Key::End => Some(visible_count - 1),
                        bastyde_core::event::Key::Enter | bastyde_core::event::Key::Space => {
                            if let Some(ref sel) = sel_for_key {
                                sel.select(current);
                            }
                            return bastyde_core::event::EventResponse::Handled;
                        }
                        _ => None,
                    };

                    if let Some(idx) = new_idx {
                        fi.set(Some(idx));
                        if let Some(ref sel) = sel_for_key {
                            if modifiers.shift() {
                                sel.extend_to(idx);
                            } else {
                                sel.select(idx);
                            }
                        }
                        // Scroll into view
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
                        return bastyde_core::event::EventResponse::Handled;
                    }
                }
                bastyde_core::event::EventResponse::Ignored
            });
        }

        // --- DnD: register as drop target when reorderable ---
        if self.reorderable {
            let my_tree_id = self.tree_id;

            // Shared across on_drag_hover / on_drag_tick / on_drag_leave:
            // the node currently under the pointer (if any) and the instant
            // at which we first saw it. Used by spring-loaded folders to
            // expand a branch after the pointer dwells on it for
            // `SPRING_DELAY_MS`. Reset whenever the hovered node changes
            // or the drag leaves this widget.
            let hovered_node: Rc<Cell<Option<(NodeId, std::time::Instant)>>> =
                Rc::new(Cell::new(None));

            let metrics_for_hover = self.metrics.clone();
            let scroll_for_hover = self.scroll_y.clone();
            let tsh_for_hover = self.tree_slice.handle();
            let feedback_for_hover = self.drop_feedback.clone();
            let hn_for_hover = hovered_node.clone();

            handlers = handlers.on_drag_hover(move |payload, position, _ctx| {
                if payload.has_typed::<TreeViewDragData>() {
                    let scroll = scroll_for_hover.get().max(0.0);
                    let content_y = position.y + scroll;
                    // Insertion line at the snapped boundary; spring-load
                    // tracks the row the pointer actually sits on.
                    let (insertion_top, row_idx) = {
                        let mut m = metrics_for_hover.borrow_mut();
                        m.resize(tsh_for_hover.visible_count());
                        let flat_idx = m.insertion_index(content_y);
                        (m.row_top(flat_idx), m.row_at(content_y))
                    };
                    let insertion_y = insertion_top - scroll;
                    feedback_for_hover.set(Some((insertion_y, 400.0)));

                    let node = tsh_for_hover.entry_at(row_idx).map(|e| e.node_id);
                    let prev = hn_for_hover.get();
                    match (prev, node) {
                        (Some((p, t)), Some(n)) if p == n => hn_for_hover.set(Some((n, t))),
                        (_, Some(n)) => hn_for_hover.set(Some((n, std::time::Instant::now()))),
                        (_, None) => hn_for_hover.set(None),
                    }

                    DropFeedback::InsertionLine {
                        y: insertion_y,
                        width: 400.0,
                    }
                } else {
                    feedback_for_hover.set(None);
                    hn_for_hover.set(None);
                    DropFeedback::NoFeedback
                }
            });

            // For the drop handler, we need access to the tree model and the
            // flattened entries. Access them via the TreeSlice's public methods.
            let tree_model_for_drop = self.tree_slice.tree().clone();
            let metrics_for_drop = self.metrics.clone();
            let scroll_for_drop = self.scroll_y.clone();

            let tsh_for_drop = self.tree_slice.handle();
            handlers = handlers.on_drop(move |mut payload, position, _ctx| {
                if let Some(drag_data) = payload.take_typed::<TreeViewDragData>()
                    && drag_data.source_tree_id == my_tree_id
                {
                    let source_node = drag_data.source_node;

                    // Compute target flat index from Y
                    let scroll = scroll_for_drop.get().max(0.0);
                    let content_y = position.y + scroll;
                    let (flat_idx, row_top, row_h) = {
                        let mut m = metrics_for_drop.borrow_mut();
                        m.resize(tsh_for_drop.visible_count());
                        let idx = m.row_at(content_y);
                        (idx, m.row_top(idx), m.row_height(idx))
                    };

                    // Get the target entry for drop zone computation
                    if let Some(entry) = tsh_for_drop.entry_at(flat_idx) {
                        if entry.node_id == source_node {
                            return true; // dropped on self, no-op
                        }

                        // Compute drop zone from Y within the row:
                        // top third = before, middle = into (if has children), bottom = after
                        let y_in_row = content_y - row_top;
                        let third = row_h / 3.0;

                        if y_in_row < third {
                            // Drop BEFORE target: move as sibling above
                            let target = entry.node_id;
                            let source_parent = tree_model_for_drop.parent(source_node);
                            if let Some(parent) = tree_model_for_drop.parent(target) {
                                let siblings = tree_model_for_drop.children(parent);
                                let mut idx =
                                    siblings.iter().position(|&n| n == target).unwrap_or(0);
                                // Adjust: if source is an earlier sibling under the same
                                // parent, move_node removes it first, shifting indices down.
                                if source_parent == Some(parent) {
                                    let src_idx = siblings.iter().position(|&n| n == source_node);
                                    if let Some(si) = src_idx
                                        && si < idx
                                    {
                                        idx -= 1;
                                    }
                                }
                                tree_model_for_drop.move_node(source_node, parent, idx);
                            } else {
                                // Target is a root — move to root before it
                                let root_count = tree_model_for_drop.root_count();
                                let mut idx = 0;
                                for i in 0..root_count {
                                    if tree_model_for_drop.root(i) == target {
                                        idx = i;
                                        break;
                                    }
                                }
                                // Adjust if source is also a root before target
                                if source_parent.is_none() {
                                    for i in 0..root_count {
                                        if tree_model_for_drop.root(i) == source_node {
                                            if i < idx {
                                                idx -= 1;
                                            }
                                            break;
                                        }
                                    }
                                }
                                tree_model_for_drop.move_to_root(source_node, idx);
                            }
                        } else if y_in_row > 2.0 * third {
                            // Drop AFTER target: move as sibling below
                            let target = entry.node_id;
                            let source_parent = tree_model_for_drop.parent(source_node);
                            if let Some(parent) = tree_model_for_drop.parent(target) {
                                let siblings = tree_model_for_drop.children(parent);
                                let mut idx = siblings
                                    .iter()
                                    .position(|&n| n == target)
                                    .map(|i| i + 1)
                                    .unwrap_or(0);
                                // Adjust for same-parent removal shifting indices
                                if source_parent == Some(parent) {
                                    let src_idx = siblings.iter().position(|&n| n == source_node);
                                    if let Some(si) = src_idx
                                        && si < idx
                                    {
                                        idx -= 1;
                                    }
                                }
                                tree_model_for_drop.move_node(source_node, parent, idx);
                            } else {
                                let root_count = tree_model_for_drop.root_count();
                                let mut idx = root_count;
                                for i in 0..root_count {
                                    if tree_model_for_drop.root(i) == target {
                                        idx = i + 1;
                                        break;
                                    }
                                }
                                // Adjust if source is also a root before target
                                if source_parent.is_none() {
                                    for i in 0..root_count {
                                        if tree_model_for_drop.root(i) == source_node {
                                            if i < idx {
                                                idx -= 1;
                                            }
                                            break;
                                        }
                                    }
                                }
                                tree_model_for_drop.move_to_root(source_node, idx.min(root_count));
                            }
                        } else {
                            // Drop INTO target (middle third): reparent as first child
                            tree_model_for_drop.move_node(source_node, entry.node_id, 0);
                        }
                    }
                    return true;
                }
                false
            });

            // Clear insertion line + spring-load timer whenever the drag
            // leaves this widget.
            let feedback_for_leave = self.drop_feedback.clone();
            let hn_for_leave = hovered_node.clone();
            handlers = handlers.on_drag_leave(move |_ctx| {
                feedback_for_leave.set(None);
                hn_for_leave.set(None);
            });

            // Per-frame tick: viewport-edge auto-scroll plus spring-loaded
            // folders. The tick fires regardless of pointer movement, so
            // edge-scroll and spring-open still progress when the hand
            // is stationary.
            let scroll_for_tick = self.scroll_y.clone();
            let max_scroll_for_tick = self.max_scroll_y.clone();
            let viewport_for_tick = self.viewport_height.clone();
            let hn_for_tick = hovered_node.clone();
            let tsh_for_tick = self.tree_slice.handle();
            let tree_model_for_tick = self.tree_slice.tree().clone();
            const SPRING_DELAY_MS: u64 = 700;
            handlers = handlers.on_drag_tick(move |pos, _ctx| {
                // --- 1. Edge auto-scroll ---
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

                // --- 2. Spring-loaded folders ---
                if let Some((node, first_seen)) = hn_for_tick.get() {
                    let elapsed_ms = first_seen.elapsed().as_millis() as u64;
                    if elapsed_ms >= SPRING_DELAY_MS
                        && tree_model_for_tick.has_children(node)
                        && !tsh_for_tick.is_expanded(node)
                    {
                        tsh_for_tick.expand(node);
                        // Reset so we don't keep re-firing on the same
                        // node; next time the pointer moves to a
                        // different row the hover handler re-arms.
                        hn_for_tick.set(None);
                    }
                }
            });
        }

        ctx.apply_self_handlers(handlers);

        // --- Create visible item widgets ---
        let (start, end) = self.visible_range();
        self.item_entries.clear();
        let reorderable = self.reorderable;
        let tree_id = self.tree_id;
        let self_id = ctx.self_id();
        for i in start..end {
            let selected = self
                .selection
                .as_ref()
                .map(|s| s.is_selected(i))
                .unwrap_or(false);
            // Get entry metadata for accessibility
            let entry_meta = self.tree_slice.entry_at(i);
            let item_has_children = entry_meta.as_ref().is_some_and(|e| e.has_children);
            // Borrow a fresh handle once per row so we can build a
            // TreeRowContext for the delegate. The handle is cheap
            // (Rc-only) and goes out of scope after the closure runs.
            let slice_handle = self.tree_slice.handle();
            if let Some(widget) = self.tree_slice.with_entry(i, |item, entry| {
                let row_ctx = TreeRowContext {
                    slice: &slice_handle,
                    node_id: entry.node_id,
                };
                (self.delegate)(item, entry, selected, &row_ctx)
            }) {
                let inner_id = ctx.add_boxed(widget);
                let (level, position_1based, total_siblings, expanded_opt) =
                    if let Some(ref e) = entry_meta {
                        let exp = if e.has_children {
                            Some(e.is_expanded)
                        } else {
                            None
                        };
                        let tree_model = self.tree_slice.tree();
                        let (pos, total) = if let Some(parent_id) = tree_model.parent(e.node_id) {
                            let siblings = tree_model.children(parent_id);
                            let idx = siblings.iter().position(|&s| s == e.node_id).unwrap_or(0);
                            (idx + 1, siblings.len())
                        } else {
                            let root_count = tree_model.root_count();
                            let idx = (0..root_count)
                                .find(|&k| tree_model.root(k) == e.node_id)
                                .unwrap_or(0);
                            (idx + 1, root_count)
                        };
                        (e.depth + 1, pos, total, exp)
                    } else {
                        (1, 1, 1, None)
                    };
                let child_id = ctx.add(crate::list_item_a11y::TreeItemWrapper::new(
                    inner_id,
                    level,
                    position_1based,
                    total_siblings,
                    expanded_opt,
                    selected,
                ));

                // Click handling: selection + expand/collapse for items with children
                {
                    let sel_click = self.selection.clone();
                    let click_index = i;
                    let tsh_click = self.tree_slice.handle();
                    let has_children = item_has_children && self.row_click_expands;
                    let node_for_toggle = self.tree_slice.visible_node_id(i);

                    ctx.apply_handlers(
                        child_id,
                        HandlerSet::new().on_pointer_event(move |event, _ctx| match event {
                            bastyde_core::event::WidgetEvent::PointerDown {
                                modifiers,
                                button: bastyde_core::event::PointerButton::Primary,
                                ..
                            } => {
                                // Selection lands on press — snappy, and the
                                // modifier information is only in the event
                                // stream (TapRecognizer strips it).
                                if let Some(ref sel) = sel_click {
                                    if modifiers.ctrl() {
                                        sel.toggle(click_index);
                                    } else if modifiers.shift() {
                                        sel.extend_to(click_index);
                                    } else {
                                        sel.select(click_index);
                                    }
                                }
                                // Ignored lets the gesture arena also see the
                                // PointerDown so DragRecognizer can capture the
                                // press position and enable drag-to-reorder.
                                bastyde_core::event::EventResponse::Ignored
                            }
                            bastyde_core::event::WidgetEvent::PointerUp {
                                button: bastyde_core::event::PointerButton::Primary,
                                ..
                            } => {
                                // Expand/collapse fires on release so a drag
                                // gesture pre-empts it (once active_drag is
                                // set, PointerUp is routed to handle_drag_drop
                                // and never reaches this widget).
                                if has_children && let Some(node_id) = node_for_toggle {
                                    tsh_click.toggle_expand(node_id);
                                }
                                bastyde_core::event::EventResponse::Ignored
                            }
                            _ => bastyde_core::event::EventResponse::Ignored,
                        }),
                    );
                }

                // Attach drag handler for reorderable items. Produces a
                // visible preview by re-invoking the delegate for this row
                // and wrapping it in a DragPreview so it reads as
                // "picked up" at the pointer.
                if reorderable && let Some(node_id) = self.tree_slice.visible_node_id(i) {
                    let drag_tree_id = tree_id;
                    let drag_self_id = self_id;
                    let delegate_for_preview = self.delegate.clone();
                    let tsh_for_preview = self.tree_slice.handle();
                    let tree_model_for_preview = self.tree_slice.tree().clone();
                    let flat_idx = i;
                    let metrics_for_preview = self.metrics.clone();
                    ctx.apply_handlers(
                        child_id,
                        HandlerSet::new().on_drag(move |phase, ctx| {
                            if let bastyde_core::gesture::DragPhase::Started { .. } = phase {
                                let payload = DragPayload::typed(TreeViewDragData {
                                    source_node: node_id,
                                    source_tree_id: drag_tree_id,
                                });
                                let delegate = delegate_for_preview.clone();
                                const PREVIEW_WIDTH: f32 = 240.0;
                                let h = metrics_for_preview.borrow_mut().row_height(flat_idx);
                                // Build the preview from the source
                                // node's item + entry metadata. The
                                // entry captures depth / expansion
                                // state so the floating preview matches
                                // the row it was plucked from.
                                let entry_meta = tsh_for_preview.entry_at(flat_idx);
                                let preview_opt = entry_meta.and_then(|entry| {
                                    tree_model_for_preview.with_item(node_id, |item| {
                                        let preview_ctx = TreeRowContext {
                                            slice: &tsh_for_preview,
                                            node_id,
                                        };
                                        Box::new(crate::drag_preview::DragPreview::new(
                                            PREVIEW_WIDTH,
                                            h,
                                            delegate(item, &entry, false, &preview_ctx),
                                        ))
                                            as Box<dyn Widget>
                                    })
                                });
                                if let Some(preview) = preview_opt {
                                    ctx.start_drag_with_preview(drag_self_id, payload, preview);
                                } else {
                                    ctx.start_drag(drag_self_id, payload);
                                }
                            }
                        }),
                    );
                }

                self.item_entries.push((i, child_id));
            }
        }

        // --- Scrollbar ---
        let scrollbar = ScrollBar::new(
            ScrollBarOrientation::Vertical,
            self.scroll_y.clone(),
            self.max_scroll_y.clone(),
            self.viewport_ratio_y.clone(),
        );
        let sb_id = ctx.add(scrollbar);
        self.scrollbar_id = Some(sb_id);

        let mut children: Vec<WidgetId> = self.item_entries.iter().map(|(_, id)| *id).collect();
        children.push(sb_id);
        children
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let width = proposal.width.unwrap_or(300.0);
        let height = proposal.height.unwrap_or(200.0);
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

        let viewport_height = bounds.height;
        let count = self.tree_slice.visible_count();
        let item_count = self.item_entries.len();
        let content_width = (bounds.width - SCROLLBAR_THICKNESS).max(0.0);

        // Auto-measure pass: measure every realized row at the content
        // width (height-for-width), feed the heights back, and apply the
        // scroll-anchor delta so content above the viewport stays put.
        // Measurements are collected with NO metrics borrow held.
        if self.metrics.borrow().needs_measure() {
            let mut measured = Vec::with_capacity(item_count);
            for (idx, child) in children.iter().enumerate() {
                if idx < item_count
                    && let Some(size) =
                        ctx.child_size(child.id, SizeProposal::with_width(content_width))
                {
                    let (flat_index, _) = self.item_entries[idx];
                    measured.push((flat_index, size.height));
                }
            }
            let anchor = self
                .metrics
                .borrow_mut()
                .observe_measured(&measured, self.scroll_y.get());
            if anchor.abs() > 0.01 {
                // Safe from place_children: the dirty flag is set but the
                // binding flush already ran this pass — lands next frame.
                self.scroll_y
                    .set((self.scroll_y.get() + anchor).max(0.0));
            }

            // Realization re-check: corrected offsets may reveal viewport
            // rows the estimated offsets never realized. Request a
            // rebuild for next frame; the 0.01 measurement epsilon
            // guarantees convergence.
            let (vs, ve) = self.metrics.borrow_mut().visible_range(
                self.scroll_y.get(),
                viewport_height,
                count,
                0,
            );
            if vs < self.prev_built_start.get() || ve > self.prev_built_end.get() {
                self.prev_built_start.set(vs.saturating_sub(BUFFER_ITEMS));
                self.prev_built_end.set(ve + BUFFER_ITEMS);
                self.version.set(self.version.get() + 1);
            }
        }

        // Post-measure totals so even frame 1's scrollbar reflects the
        // measured window.
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

        let scroll_y = self.scroll_y.get();

        for (idx, child) in children.iter_mut().enumerate() {
            if idx < item_count {
                let (flat_index, _) = self.item_entries[idx];
                let (top, height) = {
                    let mut m = self.metrics.borrow_mut();
                    (m.row_top(flat_index), m.row_height(flat_index))
                };
                let y = bounds.y + top - scroll_y;
                child.origin = Point::new(bounds.x, y);
                child.size = Size::new(content_width, height);
            }
        }

        // Scrollbar
        if let Some(sb_child) = children.last_mut() {
            let needs_scrollbar = total_height > viewport_height + 0.5;
            if needs_scrollbar {
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
        canvas: &mut bastyde_canvas::Canvas,
        ctx: &bastyde_core::widget::PaintContext,
    ) {
        // Draw insertion line during drag hover — recipe-driven role +
        // thickness via `ListContainerStyle::insertion()`.
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
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Tree);
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn children(&self) -> Vec<WidgetId> {
        let mut ids: Vec<WidgetId> = self.item_entries.iter().map(|(_, id)| *id).collect();
        if let Some(sb) = self.scrollbar_id {
            ids.push(sb);
        }
        ids
    }

    fn clips_children(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_i18n::lit;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> bastyde_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    /// Build a sample tree:
    /// A (has children: A1, A2)
    /// B (has children: B1)
    /// C (leaf)
    fn sample_tree() -> TreeModel<&'static str> {
        let tree = TreeModel::new();
        let a = tree.insert_root(0, "A");
        tree.insert_child(a, 0, "A1");
        tree.insert_child(a, 1, "A2");
        let b = tree.insert_root(1, "B");
        tree.insert_child(b, 0, "B1");
        tree.insert_root(2, "C");
        tree
    }

    fn make_tree_view(tree: TreeModel<&'static str>) -> (WidgetTree, WidgetId) {
        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(tree, |_item, entry, _selected| {
                // Width encodes depth, height is fixed
                Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
            })
            .item_height(28.0),
        );
        (wtree, tv_id)
    }

    #[test]
    fn initial_shows_only_roots() {
        let tree = sample_tree();
        let (mut wtree, tv_id) = make_tree_view(tree);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        let children = wtree.children(tv_id);
        // 3 root items + 1 scrollbar
        assert_eq!(children.len() - 1, 3);
    }

    #[test]
    fn insert_child_into_root_updates_view() {
        let tree = sample_tree();
        let a = tree.root(0);
        let (mut wtree, tv_id) = make_tree_view(tree.clone());
        wtree.layout(SizeProposal::exact(400.0, 300.0));
        assert_eq!(wtree.children(tv_id).len() - 1, 3);

        // Insert a new child under A — since A is collapsed, visible count stays 3
        tree.insert_child(a, 2, "A3");
        wtree.layout(SizeProposal::exact(400.0, 300.0));
        // Still 3 visible (A collapsed), but the tree knows about A3
        assert_eq!(wtree.children(tv_id).len() - 1, 3);
    }

    #[test]
    fn model_mutation_triggers_rebuild() {
        let tree = sample_tree();
        let (mut wtree, tv_id) = make_tree_view(tree.clone());
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        assert_eq!(wtree.children(tv_id).len() - 1, 3);

        tree.insert_root(3, "D");
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        assert_eq!(wtree.children(tv_id).len() - 1, 4);
    }

    #[test]
    fn remove_triggers_rebuild() {
        let tree = sample_tree();
        let c = tree.root(2);
        let (mut wtree, tv_id) = make_tree_view(tree.clone());
        wtree.layout(SizeProposal::exact(400.0, 300.0));
        assert_eq!(wtree.children(tv_id).len() - 1, 3);

        tree.remove(c);
        wtree.layout(SizeProposal::exact(400.0, 300.0));
        assert_eq!(wtree.children(tv_id).len() - 1, 2);
    }

    #[test]
    fn items_positioned_vertically() {
        let tree = sample_tree();
        let (mut wtree, tv_id) = make_tree_view(tree);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        let children = wtree.children(tv_id);
        let y0 = wtree.bounds(children[0]).y;
        let y1 = wtree.bounds(children[1]).y;
        let y2 = wtree.bounds(children[2]).y;
        assert!((y0 - 0.0).abs() < 0.01);
        assert!((y1 - 28.0).abs() < 0.01);
        assert!((y2 - 56.0).abs() < 0.01);
    }

    #[test]
    fn virtualization_with_large_tree() {
        // Create a tree with 500 root nodes
        let tree = TreeModel::new();
        for i in 0..500 {
            tree.insert_root(i, format!("Node {}", i).leak() as &'static str);
        }
        let (mut wtree, tv_id) = make_tree_view(tree);
        // Viewport 300px, item height 28px → ~11 visible + 2*5 buffer = ~21
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        let item_count = wtree.children(tv_id).len() - 1;
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
    fn scrollbar_collapses_when_not_needed() {
        let tree = sample_tree(); // 3 roots, 3*28=84 < 300
        let (mut wtree, tv_id) = make_tree_view(tree);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        let children = wtree.children(tv_id);
        let sb = children.last().unwrap();
        let sb_bounds = wtree.bounds(*sb);
        assert!(
            sb_bounds.width < 0.01 && sb_bounds.height < 0.01,
            "Scrollbar should be collapsed"
        );
    }

    #[test]
    fn accessibility_role_is_tree() {
        let tree = sample_tree();
        let (mut wtree, tv_id) = make_tree_view(tree);
        wtree.layout(SizeProposal::exact(400.0, 300.0));
        let info = wtree.accessibility_node(tv_id);
        assert_eq!(info.role(), bastyde_core::accesskit::Role::Tree);
    }

    #[test]
    fn empty_tree() {
        let tree: TreeModel<&str> = TreeModel::new();
        let (mut wtree, tv_id) = make_tree_view(tree);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Only scrollbar
        assert_eq!(wtree.children(tv_id).len(), 1);
    }

    #[test]
    fn tree_item_has_a11y_role_and_expanded() {
        let tree = sample_tree(); // A (has children), B (has children), C (leaf)
        let (mut wtree, tv_id) = make_tree_view(tree);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        let children = wtree.children(tv_id);
        // First child (A) should be a TreeItemWrapper with TreeItem role
        let info_a = wtree.accessibility_node(children[0]);
        assert_eq!(info_a.role(), bastyde_core::accesskit::Role::TreeItem);
        // A has children and is collapsed → is_expanded returns false
        assert!(
            !info_a.is_expanded(),
            "Root A should report not expanded (collapsed)"
        );

        // Third child (C) is a leaf → also not expanded
        let info_c = wtree.accessibility_node(children[2]);
        assert_eq!(info_c.role(), bastyde_core::accesskit::Role::TreeItem);
        assert!(!info_c.is_expanded(), "Leaf C should not be expanded");
    }

    #[test]
    fn keyboard_arrow_down_navigates() {
        use bastyde_core::event::{Key, Modifiers};
        use bastyde_data::{SelectionMode, SelectionModel};

        let tree = sample_tree(); // A, B, C (3 roots)
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel_clone = selection.clone();

        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(tree, |_item, entry, _selected| {
                Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
            })
            .item_height(28.0)
            .selection(sel_clone),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Focus the TreeView
        wtree.focus(tv_id);

        // ArrowDown should select item 0 first (from no focus), then 1
        wtree.dispatch_event(bastyde_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowDown,
            modifiers: Modifiers::NONE,
            text: None,
        });

        // focused_index starts at None → unwrap_or(0) → ArrowDown moves to 1
        assert_eq!(
            selection.selected_indices(),
            vec![1],
            "ArrowDown from initial state should select index 1 (second root)"
        );

        // Another ArrowDown should move to index 2
        wtree.dispatch_event(bastyde_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowDown,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(
            selection.selected_indices(),
            vec![2],
            "Second ArrowDown should select index 2 (third root)"
        );
    }

    // --- Drag-and-drop integration tests ---

    /// Run a full drag gesture: PointerDown on source, Move to cross threshold,
    /// Move to target, Up. Mirrors `list_view::tests::drag_item`.
    fn drag_item(tree: &mut WidgetTree, from: Point, to: Point) {
        use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: from,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
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

    /// Build a reorderable TreeView at the tree root with three top-level
    /// nodes A (collapsed, with A1/A2 children), B (collapsed, with B1), C
    /// (leaf). Item height is 28px, so rows are at y=0..28, 28..56, 56..84.
    fn make_reorderable_tree_view() -> (
        WidgetTree,
        WidgetId,
        TreeModel<&'static str>,
        NodeId,
        NodeId,
        NodeId,
    ) {
        let model = sample_tree();
        let a = model.root(0);
        let b = model.root(1);
        let c = model.root(2);
        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(model.clone(), |_item, entry, _sel| {
                Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
            })
            .item_height(28.0)
            .reorderable(true),
        );
        (wtree, tv_id, model, a, b, c)
    }

    #[test]
    fn drag_reorders_root_before() {
        // Drag C (row 2, y=56..84) to the top third of row 0 (before A).
        let (mut wtree, _tv_id, model, a, _b, c) = make_reorderable_tree_view();
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        drag_item(&mut wtree, Point::new(50.0, 70.0), Point::new(50.0, 2.0));

        // After move: C becomes root 0, A shifts to root 1.
        assert_eq!(model.root(0), c, "C should be first root");
        assert_eq!(model.root(1), a, "A should be second root");
    }

    #[test]
    fn drag_reorders_root_after() {
        // Drag B (row 1, y=28..56) to the bottom third of row 2 (after C).
        let (mut wtree, _tv_id, model, _a, b, c) = make_reorderable_tree_view();
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        drag_item(&mut wtree, Point::new(50.0, 42.0), Point::new(50.0, 80.0));

        // After move: order is [A, C, B]
        assert_eq!(model.root_count(), 3);
        assert_eq!(model.root(1), c, "C should shift up to root 1");
        assert_eq!(model.root(2), b, "B should land at root 2");
    }

    #[test]
    fn drag_reparents_into_target() {
        // Drag C (row 2) into the middle third of row 0 (into A as first child).
        let (mut wtree, _tv_id, model, a, _b, c) = make_reorderable_tree_view();
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Middle third of a 28px row is [9.33, 18.67]. Use y=14.
        drag_item(&mut wtree, Point::new(50.0, 70.0), Point::new(50.0, 14.0));

        // C should now be A's first child (A's existing children were A1, A2).
        let a_children = model.children(a);
        assert_eq!(a_children.len(), 3, "A should have three children");
        assert_eq!(a_children[0], c, "C should be A's first child");
        // C is no longer a root.
        assert_eq!(model.root_count(), 2);
    }

    #[test]
    fn drag_emits_node_moved_change() {
        use bastyde_data::TreeChange;
        use std::cell::Cell;
        use std::rc::Rc;

        let (mut wtree, _tv_id, model, _a, b, _c) = make_reorderable_tree_view();
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        let emitted = Rc::new(Cell::new(false));
        let e = emitted.clone();
        let moved_node = Rc::new(Cell::new(None::<NodeId>));
        let mn = moved_node.clone();
        let handle = model.observe_changes(move |change| {
            if let TreeChange::NodeMoved { node, .. } = change {
                e.set(true);
                mn.set(Some(*node));
            }
        });

        // Drag B up — before A.
        drag_item(&mut wtree, Point::new(50.0, 42.0), Point::new(50.0, 2.0));

        assert!(emitted.get(), "TreeChange::NodeMoved should be emitted");
        assert_eq!(moved_node.get(), Some(b));
        drop(handle);
    }

    #[test]
    fn click_on_branch_with_nested_delegate_expands() {
        // Like click_on_branch_expands_and_collapses, but the delegate
        // builds a nested subtree (ZStack + Padding + HStack + Texts +
        // Spacer) so the pointer hit-target is a deep leaf, NOT the
        // TreeItemWrapper. Regression for the case where the wrapper's
        // on_pointer_event has to route through the preview/bubble path
        // to fire toggle_expand.
        use crate::RectWidget;
        use crate::primitives::{HStack, Padding, Spacer, TextWidget, ZStack};

        let tree = sample_tree();
        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(tree, |name, entry, selected| {
                let arrow: &'static str = if entry.has_children {
                    if entry.is_expanded { "v" } else { ">" }
                } else {
                    " "
                };
                let bg = if selected {
                    bastyde_tokens::Color::from_rgba(0.25, 0.47, 0.85, 0.25)
                } else {
                    bastyde_tokens::Color::TRANSPARENT
                };
                Box::new(
                    ZStack::new().child(RectWidget::new().background(bg)).child(
                        Padding::symmetric(4.0, 12.0).child(
                            HStack::new()
                                .spacing(8.0)
                                .child(TextWidget::new(lit!(arrow)))
                                .child(TextWidget::new(lit!(name.to_string())))
                                .child(Spacer::new()),
                        ),
                    ),
                )
            })
            .item_height(28.0),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Sanity check: 3 roots visible.
        assert_eq!(wtree.children(tv_id).len() - 1, 3);

        // Click A (row 0). Use the wrapper's bounds center — hit_test will
        // walk down to whatever deep leaf is at that point.
        let children = wtree.children(tv_id);
        wtree.click(children[0]);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        assert_eq!(
            wtree.children(tv_id).len() - 1,
            5,
            "Click on A (branch) should expand it even with a nested delegate"
        );
    }

    #[test]
    fn drag_with_nested_delegate_still_works() {
        // Same nested delegate as above, but exercising drag. Regression
        // for the real-app scenario where the pointer hit-target is a
        // deep leaf (TextWidget) and the wrapper holding the gesture
        // arena + on_drag is an ancestor.
        use crate::RectWidget;
        use crate::primitives::{HStack, Padding, Spacer, TextWidget, ZStack};

        let tree = sample_tree();
        let a = tree.root(0);
        let c = tree.root(2);
        let mut wtree = WidgetTree::new();
        let _tv_id = wtree.add(
            TreeView::new(tree.clone(), |name, _entry, _sel| {
                Box::new(
                    ZStack::new()
                        .child(RectWidget::new().background(bastyde_tokens::Color::TRANSPARENT))
                        .child(
                            Padding::symmetric(4.0, 12.0).child(
                                HStack::new()
                                    .spacing(8.0)
                                    .child(TextWidget::new(lit!(name.to_string())))
                                    .child(Spacer::new()),
                            ),
                        ),
                )
            })
            .item_height(28.0)
            .reorderable(true),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Drag C (row 2, y=70) to the top third of row 0 (y=2) → drop-before A.
        drag_item(&mut wtree, Point::new(50.0, 70.0), Point::new(50.0, 2.0));

        assert_eq!(tree.root(0), c, "C should be first root after drag");
        assert_eq!(tree.root(1), a, "A should shift to second root");
    }

    #[test]
    fn click_on_branch_expands_and_collapses() {
        // Click a folder-with-children and verify its subtree appears; click
        // again and verify it collapses. Regression test for the previous
        // on_pointer_event double-dispatch bug that toggled expand twice per
        // click (net no-op).
        let tree = sample_tree();
        let (mut wtree, tv_id) = make_tree_view(tree);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Initially collapsed — 3 roots visible.
        assert_eq!(wtree.children(tv_id).len() - 1, 3);

        // Click A (row 0, center y=14).
        let children = wtree.children(tv_id);
        wtree.click(children[0]);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // A should now be expanded, showing its two children A1, A2.
        assert_eq!(
            wtree.children(tv_id).len() - 1,
            5,
            "After clicking A, its two children should become visible"
        );

        // Click A again — collapses.
        let children = wtree.children(tv_id);
        wtree.click(children[0]);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        assert_eq!(
            wtree.children(tv_id).len() - 1,
            3,
            "Second click should collapse A back to 3 visible roots"
        );
    }

    #[test]
    fn row_click_expands_false_disables_auto_toggle() {
        // With `.row_click_expands(false)` set, clicking a branch
        // row's body must NOT toggle its expansion. This is the
        // contract used by `StandardTreeItem`, which provides its
        // own chevron tap target — without this opt-out, body clicks
        // would still toggle (and chevron clicks would toggle twice,
        // cancelling out).
        let tree = sample_tree();
        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(tree, |_item, entry, _selected| {
                Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
            })
            .item_height(28.0)
            .row_click_expands(false),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        assert_eq!(wtree.children(tv_id).len() - 1, 3);

        // Click A (a branch with children). Body click should NOT
        // expand it.
        let children = wtree.children(tv_id);
        wtree.click(children[0]);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        assert_eq!(
            wtree.children(tv_id).len() - 1,
            3,
            "row body click on a branch must not auto-expand when row_click_expands=false"
        );
    }

    #[test]
    fn spring_loaded_folder_expands_after_dwell() {
        // Drag a leaf over a collapsed folder and hold. After the dwell
        // delay (SPRING_DELAY_MS = 700 real ms), the folder should
        // auto-expand. Test drives real wall-clock time via `sleep` —
        // it's slow but accurate. Runs in ~750 ms; still headless.
        use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};
        use std::thread::sleep;
        use std::time::Duration;

        let tree = sample_tree(); // A (A1 A2), B (B1), C (leaf)
        let a = tree.root(0);
        let b = tree.root(1);
        let mut wtree = WidgetTree::new();
        let _tv_id = wtree.add(
            TreeView::new(tree.clone(), |_item, entry, _sel| {
                Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
            })
            .item_height(28.0)
            .reorderable(true),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Start a drag on C (y=70, row 2), then hover over B (row 1, y=42).
        wtree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(50.0, 70.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        wtree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(60.0, 70.0),
        });
        wtree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(60.0, 42.0),
        });

        // Confirm B is currently collapsed.
        assert!(tree.with_item(b, |_| ()).is_some());
        assert_eq!(
            wtree.children(_tv_id).len() - 1,
            3,
            "Precondition: 3 visible roots, nothing expanded"
        );

        // Wait past the 700 ms spring delay, then drive a layout tick
        // so on_drag_tick fires and the elapsed check passes.
        sleep(Duration::from_millis(750));
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // B should now be expanded, revealing B1 (4 visible rows).
        assert_eq!(
            wtree.children(_tv_id).len() - 1,
            4,
            "B should have spring-opened after the dwell"
        );

        // A was never hovered — still collapsed.
        assert!(!wtree.children(_tv_id).is_empty());
        let _ = a;

        // Clean up drag.
        wtree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(60.0, 42.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
    }

    // --- Alt+Arrow keyboard reorder test ---

    #[test]
    fn alt_arrow_reorders_flat_root_sibling() {
        use bastyde_core::event::{Key, Modifiers};
        use bastyde_data::{SelectionMode, SelectionModel};

        let model = sample_tree(); // A, B, C (3 roots)
        let _a = model.root(0);
        let _b = model.root(1);
        let _c = model.root(2);
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel_clone = selection.clone();
        let model_clone = model.clone();

        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(model_clone, move |_item, entry, _sel| {
                Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
            })
            .item_height(28.0)
            .selection(sel_clone)
            .reorderable(true),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Focus the TreeView and select the middle item (B)
        wtree.focus(tv_id);
        wtree.click(wtree.children(tv_id)[1]); // B at index 1
        assert_eq!(selection.selected_indices(), vec![1]);

        // Press Alt+ArrowUp: B should move above A
        wtree.dispatch_event(bastyde_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowUp,
            modifiers: Modifiers::ALT,
            text: None,
        });

        // After move: the roots should be reordered as B, A, C
        let new_roots: Vec<NodeId> = (0..model.root_count()).map(|i| model.root(i)).collect();
        assert_eq!(
            model.with_item(new_roots[0], |&v| v),
            Some("B"),
            "B should now be first root"
        );
        assert_eq!(
            model.with_item(new_roots[1], |&v| v),
            Some("A"),
            "A should now be second root"
        );
        // Selection should follow the moved node
        assert_eq!(
            selection.selected_indices(),
            vec![0],
            "Selection should now be at index 0 (B moved to top)"
        );

        // Press Alt+ArrowDown on B: B should move below A
        wtree.dispatch_event(bastyde_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowDown,
            modifiers: Modifiers::ALT,
            text: None,
        });

        // After move: order should be A, B, C again
        let new_roots: Vec<NodeId> = (0..model.root_count()).map(|i| model.root(i)).collect();
        assert_eq!(
            model.with_item(new_roots[0], |&v| v),
            Some("A"),
            "A should be back at first root"
        );
        assert_eq!(
            model.with_item(new_roots[1], |&v| v),
            Some("B"),
            "B should be back at second root"
        );
        assert_eq!(
            selection.selected_indices(),
            vec![1],
            "Selection should be back at index 1"
        );
    }

    #[test]
    fn alt_arrow_reorders_nested_sibling() {
        use bastyde_core::event::{Key, Modifiers};
        use bastyde_data::{SelectionMode, SelectionModel};

        // Tree: A with children A1, A2 (in that order)
        let tree = TreeModel::new();
        let a = tree.insert_root(0, "A");
        let _a1 = tree.insert_child(a, 0, "A1");
        let _a2 = tree.insert_child(a, 1, "A2");
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel_clone = selection.clone();
        let model = tree.clone();

        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(model, |_item, entry, _sel| {
                Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
            })
            .item_height(28.0)
            .selection(sel_clone)
            .reorderable(true),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Focus the TreeView so ArrowRight expands the focused node (A)
        wtree.focus(tv_id);

        // Expand A so children are visible
        wtree.dispatch_event(bastyde_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowRight,
            modifiers: Modifiers::NONE,
            text: None,
        });
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Select A2 (flat index 2: A at 0, A1 at 1, A2 at 2)
        let children = wtree.children(tv_id);
        wtree.click(children[2]);
        assert_eq!(selection.selected_indices(), vec![2]);

        // Press Alt+ArrowUp: A2 should move above A1
        wtree.dispatch_event(bastyde_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowUp,
            modifiers: Modifiers::ALT,
            text: None,
        });
        // After move, relayout to refresh the tree view
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Check model: A2 should now be at index 0 under A, A1 at index 1
        let children_of_a = tree.children(a);
        assert_eq!(children_of_a.len(), 2, "A should still have 2 children");
        assert_eq!(
            tree.with_item(children_of_a[0], |&v| v),
            Some("A2"),
            "A2 should now be first child of A"
        );
        assert_eq!(
            tree.with_item(children_of_a[1], |&v| v),
            Some("A1"),
            "A1 should now be second child of A"
        );

        // Selection should now be at flat index 1 (A2 moved up, now at position 1)
        assert_eq!(
            selection.selected_indices(),
            vec![1],
            "Selection should follow A2 to flat index 1"
        );
    }

    #[test]
    fn alt_arrow_cannot_move_past_boundaries() {
        use bastyde_core::event::{Key, Modifiers};
        use bastyde_data::{SelectionMode, SelectionModel};

        let model = sample_tree(); // A, B, C (3 roots)
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel_clone = selection.clone();
        let model_clone = model.clone();

        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(model_clone, move |_item, entry, _sel| {
                Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
            })
            .item_height(28.0)
            .selection(sel_clone)
            .reorderable(true),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Focus and select first item (A)
        wtree.focus(tv_id);
        wtree.click(wtree.children(tv_id)[0]);

        let a = model.root(0);
        let c = model.root(2);

        // Alt+ArrowUp on first item should do nothing
        wtree.dispatch_event(bastyde_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowUp,
            modifiers: Modifiers::ALT,
            text: None,
        });
        assert_eq!(
            model.with_item(a, |&v| v),
            Some("A"),
            "A should still be first after Alt+Up on first item"
        );

        // Select last item (C)
        wtree.click(wtree.children(tv_id)[2]);

        // Alt+ArrowDown on last item should do nothing
        wtree.dispatch_event(bastyde_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowDown,
            modifiers: Modifiers::ALT,
            text: None,
        });
        assert_eq!(
            model.with_item(c, |&v| v),
            Some("C"),
            "C should still be last after Alt+Down on last item"
        );
    }

    // -- Boundary scroll chaining -------------------------------------------

    /// A TreeView of 40 flat roots (20px each → 800px) in a 100px viewport,
    /// above a filler inside an outer ScrollArea. TreeView doesn't expose its
    /// scroll signal, so chaining is observed via the outer: the inner
    /// absorbing the first (huge) scroll leaves the outer at 0 (the
    /// anti-trivial guard), and the clamped second scroll then moves the
    /// outer under `Chain` but not under `Contain`.
    fn nested_tree_fixture(inner: OverscrollBehavior) -> (WidgetTree, Signal<f32>) {
        use crate::ScrollArea;
        use crate::primitives::{FixedSize, VStack};
        let model = TreeModel::new();
        for i in 0..40 {
            model.insert_root(i, i as i32);
        }
        let mut tree = WidgetTree::new();
        let tv = TreeView::new(model, |_item: &i32, _entry, _sel| {
            Box::new(FixedLeaf(180.0, 20.0))
        })
        .item_height(20.0)
        .overscroll_behavior(inner);
        let tv_id = tree.add(tv);
        let viewport = tree.add(
            FixedSize::new()
                .bind_width(200.0)
                .bind_height(100.0)
                .child_id(tv_id),
        );
        let filler = tree.add(FixedLeaf(200.0, 200.0));
        let outer_content = tree.add(VStack::new().add_child(viewport).add_child(filler));
        let outer = ScrollArea::from_id(outer_content).smooth_scrolling(false);
        let outer_y = outer.scroll_y_signal().clone();
        let _outer = tree.add(outer);
        tree.layout(SizeProposal::exact(200.0, 150.0));
        (tree, outer_y)
    }

    #[test]
    fn nested_tree_chains_to_outer_at_boundary() {
        use bastyde_core::event::{Modifiers, ScrollDelta, WidgetEvent};
        let (mut tree, outer_y) = nested_tree_fixture(OverscrollBehavior::Chain);
        tree.pointer_move(Point::new(50.0, 40.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 9999.0 },
            modifiers: Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(200.0, 150.0));
        // The inner tree absorbed the big scroll (didn't chain) → outer at 0.
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
            outer_y.get() > 0.01,
            "outer scrolled because the clamped tree chained"
        );
    }

    #[test]
    fn nested_tree_contain_blocks_chaining() {
        use bastyde_core::event::{Modifiers, ScrollDelta, WidgetEvent};
        let (mut tree, outer_y) = nested_tree_fixture(OverscrollBehavior::Contain);
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

    // --- Variable row heights ---

    /// Collect the (y, height) bounds of the realized rows (the
    /// scrollbar is always the last child), sorted by y.
    fn row_spans(tree: &WidgetTree, tv_id: WidgetId) -> Vec<(f32, f32)> {
        let children = tree.children(tv_id);
        let mut spans: Vec<(f32, f32)> = children[..children.len() - 1]
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
    fn exact_item_height_fn_positions_tree_rows() {
        let tree = sample_tree();
        let heights = [60.0_f32, 20.0, 40.0];
        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(tree, |_item, _entry, _sel| Box::new(FixedLeaf(100.0, 28.0)))
                .item_height_fn(move |i| heights.get(i).copied().unwrap_or(28.0)),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        let spans = row_spans(&wtree, tv_id);
        assert_eq!(spans.len(), 3);
        assert!((spans[0].0 - 0.0).abs() < 0.01 && (spans[0].1 - 60.0).abs() < 0.01);
        assert!((spans[1].0 - 60.0).abs() < 0.01 && (spans[1].1 - 20.0).abs() < 0.01);
        assert!((spans[2].0 - 80.0).abs() < 0.01 && (spans[2].1 - 40.0).abs() < 0.01);
    }

    #[test]
    fn auto_measure_tree_rows_at_measured_heights() {
        // Delegate rows are 30 px tall; estimate says 50 → row 1 must
        // settle at y = 30.
        let tree = sample_tree();
        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(tree, |_item, _entry, _sel| Box::new(FixedLeaf(100.0, 30.0)))
                .auto_item_height(50.0),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        let spans = row_spans(&wtree, tv_id);
        assert!(
            (spans[1].0 - 30.0).abs() < 0.01,
            "row 1 should sit at measured 30, got {}",
            spans[1].0
        );
    }

    #[test]
    fn expand_preserves_measured_heights_above_toggle() {
        // Rows measure 30 (estimate 50). Expanding B (flat index 1) must
        // keep A's measured height — row B stays at y = 30, it doesn't
        // snap back to the estimate.
        let tree = sample_tree();
        let b = tree.root(1);
        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(tree, |_item, _entry, _sel| Box::new(FixedLeaf(100.0, 30.0)))
                .auto_item_height(50.0)
                .row_click_expands(false),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        wtree
            .widget_as_any(tv_id)
            .and_then(|any| any.downcast_ref::<TreeView<&'static str>>())
            .expect("TreeView exposes itself via as_any")
            .expand(b);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        let spans = row_spans(&wtree, tv_id);
        assert_eq!(spans.len(), 4); // A, B, B1, C
        assert!(
            (spans[1].0 - 30.0).abs() < 0.01,
            "A's measured height must survive the expand below it, got {}",
            spans[1].0
        );
    }

    #[test]
    fn drop_zone_thirds_with_variable_heights() {
        // Roots A (60 px), B (20 px), C (40 px), reorderable. Dropping C
        // in the top third of the SHORT row B (y ∈ 60..~66) must insert
        // it before B — uniform math would misattribute that y band.
        let tree = TreeModel::new();
        tree.insert_root(0, "A");
        tree.insert_root(1, "B");
        tree.insert_root(2, "C");
        let heights = [60.0_f32, 20.0, 40.0];
        let mut wtree = WidgetTree::new();
        let _tv_id = wtree.add(
            TreeView::new(tree.clone(), |_item, _entry, _sel| {
                Box::new(FixedLeaf(100.0, 28.0))
            })
            .item_height_fn(move |i| heights.get(i).copied().unwrap_or(28.0))
            .reorderable(true),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // C spans 80..120; grab its center. Drop at y = 62: row B's top
        // third (60..60+20/3).
        drag_item(
            &mut wtree,
            Point::new(50.0, 100.0),
            Point::new(50.0, 62.0),
        );

        let order: Vec<&str> = (0..tree.root_count())
            .map(|i| tree.with_item(tree.root(i), |v| *v).unwrap())
            .collect();
        assert_eq!(order, vec!["A", "C", "B"]);
    }
}
