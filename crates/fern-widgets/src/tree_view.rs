//! Virtualized hierarchical tree widget.
//!
//! `TreeView` displays a `TreeModel<T>` as an indented, expandable/collapsible
//! list. Internally it creates a `TreeSlice` for per-view expand state and
//! virtualizes rendering like `ListView` (fixed row height, only visible rows
//! have widgets).

use std::cell::Cell;
use std::rc::Rc;

use fern_canvas::{Point, Rect, Size, SizeProposal};

use fern_core::DropFeedback;
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::drag_payload::DragPayload;
use fern_core::signal::Signal;
use fern_core::binding::BindingLevel;
use fern_core::widget::{LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;

use fern_data::selection_model::SelectionModel;
use fern_data::tree_slice::{FlatEntry, TreeSlice};
use fern_data::{NodeId, TreeModel};

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

/// A virtualized hierarchical tree widget backed by a `TreeModel<T>`.
///
/// ```ignore
/// TreeView::new(tree_model, |item, entry, selected| {
///     Box::new(HStack::new()
///         .child(Padding::left(entry.depth as f32 * 20.0))
///         .child(TextWidget::new_literal(&item.title)))
/// })
/// .item_height(28.0)
/// ```
pub struct TreeView<T: 'static> {
    tree_slice: TreeSlice<T>,
    delegate: Rc<dyn Fn(&T, &FlatEntry, bool) -> Box<dyn Widget>>,
    item_height: f32,
    selection: Option<SelectionModel>,

    /// Keyboard-focused flat index.
    focused_index: Rc<Cell<Option<usize>>>,

    /// Enable intra-widget drag reordering.
    reorderable: bool,

    /// Active drop feedback (set by on_drag_hover, cleared by on_drag_leave,
    /// read by paint). Reactive Signal — bound at `RepaintOnly` so any
    /// `set(...)` call dirties the TreeView for repaint automatically.
    drop_feedback: Signal<Option<(f32, f32)>>, // (y, width) for insertion line

    // Persistent scroll state
    scroll_y: Signal<f32>,
    max_scroll_y: Signal<f32>,
    viewport_ratio_y: Signal<f32>,

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
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
        let tree_slice = TreeSlice::new(model);
        Self {
            tree_slice,
            delegate: Rc::new(delegate),
            item_height: DEFAULT_ITEM_HEIGHT,
            selection: None,
            focused_index: Rc::new(Cell::new(None)),
            reorderable: false,
            drop_feedback: Signal::new(None),
            scroll_y: Signal::new_animated(0.0),
            max_scroll_y: Signal::new(0.0),
            viewport_ratio_y: Signal::new(1.0),
            item_entries: Vec::new(),
            scrollbar_id: None,
            viewport_height: Rc::new(Cell::new(600.0)),
            tree_id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// Set the fixed height per row (default 28.0).
    pub fn item_height(mut self, height: f32) -> Self {
        self.item_height = height;
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
    pub fn expand(&self, node: fern_data::NodeId) {
        self.tree_slice.expand(node);
    }

    /// Collapse a node programmatically.
    pub fn collapse(&self, node: fern_data::NodeId) {
        self.tree_slice.collapse(node);
    }

    /// Toggle a node's expand/collapse state.
    pub fn toggle(&self, node: fern_data::NodeId) {
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
        let count = self.tree_slice.visible_count();
        if count == 0 {
            return 0.0;
        }
        count as f32 * self.item_height
    }

    fn visible_range(&self) -> (usize, usize) {
        let count = self.tree_slice.visible_count();
        if count == 0 {
            return (0, 0);
        }
        let scroll = self.scroll_y.get().max(0.0);
        let viewport = self.viewport_height.get();

        let first_visible = (scroll / self.item_height).floor() as usize;
        let last_visible = ((scroll + viewport) / self.item_height).ceil() as usize;

        let start = first_visible.saturating_sub(BUFFER_ITEMS);
        let end = (last_visible + BUFFER_ITEMS).min(count);

        (start, end)
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
    fn build(&mut self, ctx: &mut fern_core::build_context::BuildContext) -> Vec<WidgetId> {
        // --- Version signal for rebuild triggering ---
        let version = ctx.signal(0_u64);
        version.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        // Bind scroll_y at Relayout so place_children runs on every scroll
        // position change (repositions items) without a full rebuild.
        self.scroll_y
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Relayout);

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
            move |_| {
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
        let item_height = self.item_height;
        let viewport_h = self.viewport_height.clone();
        // Track the buffered range from this build. Only trigger a rebuild
        // when the visible range exceeds the buffer — most scrolls just need
        // a relayout (handled by scroll_y's Relayout binding above).
        let (built_start, built_end) = self.visible_range();
        let prev_built_start = Rc::new(Cell::new(built_start));
        let prev_built_end = Rc::new(Cell::new(built_end));
        let version_for_scroll = version.clone();
        let scroll_ver = Rc::new(Cell::new(0_u64));
        let scroll_handle = self.scroll_y.observe({
            let pbs = prev_built_start.clone();
            let pbe = prev_built_end.clone();
            let sv = scroll_ver.clone();
            move |y| {
                let scroll = y.max(0.0);
                let vp = viewport_h.get();
                let visible_start = if item_height > 0.0 {
                    (scroll / item_height).floor() as usize
                } else {
                    0
                };
                let visible_end = if item_height > 0.0 {
                    ((scroll + vp) / item_height).ceil() as usize
                } else {
                    0
                };
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
        let mut handlers = HandlerSet::new()
            .on_scroll(move |event, _ctx| match event {
                fern_core::event::WidgetEvent::Scroll { delta, .. } => {
                    let dy = match delta {
                        fern_core::event::ScrollDelta::Lines { y, .. } => y * line_height,
                        fern_core::event::ScrollDelta::Pixels { y, .. } => *y,
                    };
                    let current = scroll_y.get();
                    let max = max_scroll.get();
                    let new_y = (current + dy).clamp(0.0, max);
                    scroll_y.set(new_y);
                    fern_core::event::EventResponse::Handled
                }
                _ => fern_core::event::EventResponse::Ignored,
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
            let ih_for_nav = self.item_height;
            let vh_for_nav = self.viewport_height.clone();

            handlers = handlers.on_key(move |event, _ctx| {
                if let fern_core::event::WidgetEvent::KeyDown { key, modifiers, .. } = event {
                    let visible_count = tsh.visible_count();
                    if visible_count == 0 {
                        return fern_core::event::EventResponse::Ignored;
                    }

                    let current = fi.get().unwrap_or(0).min(visible_count - 1);

                    // Alt+Arrow: sibling reorder (when reorderable)
                    if modifiers.alt() && reorderable {
                        let selected_idx = sel_for_key
                            .as_ref()
                            .and_then(|s| s.selected_indices().first().copied())
                            .or(fi.get());
                        
                        if let Some(flat_idx) = selected_idx {
                            if let Some(entry) = tsh.entry_at(flat_idx) {
                                let node_id = entry.node_id;
                                let tree = tsh.tree();
                                let parent = tree.parent(node_id);
                                
                                // Determine siblings: either children of parent or root list
                                let (siblings, is_root_level) = if let Some(parent_id) = parent {
                                    (tree.children(parent_id), false)
                                } else {
                                    // Node is a root - get all roots
                                    let root_count = tree.root_count();
                                    let siblings: Vec<NodeId> = (0..root_count)
                                        .map(|i| tree.root(i))
                                        .collect();
                                    (siblings, true)
                                };
                                
                                let sibling_idx = siblings
                                    .iter()
                                    .position(|&n| n == node_id)
                                    .unwrap_or(0);
                                
                                match key {
                                    fern_core::event::Key::ArrowUp if sibling_idx > 0 => {
                                        if is_root_level {
                                            tree.move_to_root(node_id, sibling_idx - 1);
                                        } else {
                                            tree.move_node(node_id, parent.unwrap(), sibling_idx - 1);
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
                                        return fern_core::event::EventResponse::Handled;
                                    }
                                    fern_core::event::Key::ArrowDown
                                        if sibling_idx + 1 < siblings.len() =>
                                    {
                                        if is_root_level {
                                            tree.move_to_root(node_id, sibling_idx + 1);
                                        } else {
                                            tree.move_node(node_id, parent.unwrap(), sibling_idx + 1);
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
                                        return fern_core::event::EventResponse::Handled;
                                    }
                                    _ => {}
                                }
                            }
                        }
                        return fern_core::event::EventResponse::Ignored;
                    }

                    // ArrowRight: expand / ArrowLeft: collapse or move to parent
                    match key {
                        fern_core::event::Key::ArrowRight => {
                            if let Some(entry) = tsh.entry_at(current) {
                                if entry.has_children && !entry.is_expanded {
                                    tsh.expand(entry.node_id);
                                    return fern_core::event::EventResponse::Handled;
                                }
                            }
                        }
                        fern_core::event::Key::ArrowLeft => {
                            if let Some(entry) = tsh.entry_at(current) {
                                if entry.is_expanded {
                                    tsh.collapse(entry.node_id);
                                    return fern_core::event::EventResponse::Handled;
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
                                            return fern_core::event::EventResponse::Handled;
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }

                    // Navigation keys
                    let new_idx = match key {
                        fern_core::event::Key::ArrowDown => {
                            Some(current.saturating_add(1).min(visible_count - 1))
                        }
                        fern_core::event::Key::ArrowUp => Some(current.saturating_sub(1)),
                        fern_core::event::Key::Home => Some(0),
                        fern_core::event::Key::End => Some(visible_count - 1),
                        fern_core::event::Key::Enter | fern_core::event::Key::Space => {
                            if let Some(ref sel) = sel_for_key {
                                sel.select(current);
                            }
                            return fern_core::event::EventResponse::Handled;
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
                        let item_top = idx as f32 * ih_for_nav;
                        let item_bottom = item_top + ih_for_nav;
                        let vp = vh_for_nav.get();
                        let scroll = scroll_for_nav.get();
                        if item_top < scroll {
                            scroll_for_nav.set(item_top);
                        } else if item_bottom > scroll + vp {
                            scroll_for_nav.set(item_bottom - vp);
                        }
                        return fern_core::event::EventResponse::Handled;
                    }
                }
                fern_core::event::EventResponse::Ignored
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

            let ih_for_hover = self.item_height;
            let scroll_for_hover = self.scroll_y.clone();
            let tsh_for_hover = self.tree_slice.handle();
            let feedback_for_hover = self.drop_feedback.clone();
            let hn_for_hover = hovered_node.clone();

            handlers = handlers.on_drag_hover(move |payload, position, _ctx| {
                if payload.has_typed::<TreeViewDragData>() {
                    let scroll = scroll_for_hover.get().max(0.0);
                    let content_y = position.y + scroll;
                    let count = tsh_for_hover.visible_count();
                    let flat_idx = if ih_for_hover > 0.0 {
                        ((content_y + ih_for_hover * 0.5) / ih_for_hover)
                            .floor()
                            .max(0.0)
                            .min(count as f32) as usize
                    } else {
                        0
                    };
                    let insertion_y = flat_idx as f32 * ih_for_hover - scroll;
                    feedback_for_hover.set(Some((insertion_y, 400.0)));

                    // Track the currently-hovered node for spring-load.
                    // Use the flat index *at* the pointer Y (not rounded
                    // to half-step) so the spring timer tracks the row
                    // the pointer actually sits on.
                    let row_idx = if ih_for_hover > 0.0 {
                        (content_y / ih_for_hover).floor().max(0.0) as usize
                    } else {
                        0
                    };
                    let node = tsh_for_hover.entry_at(row_idx).map(|e| e.node_id);
                    let prev = hn_for_hover.get();
                    match (prev, node) {
                        (Some((p, t)), Some(n)) if p == n => {
                            hn_for_hover.set(Some((n, t)))
                        }
                        (_, Some(n)) => hn_for_hover
                            .set(Some((n, std::time::Instant::now()))),
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
            let ih_for_drop = self.item_height;
            let scroll_for_drop = self.scroll_y.clone();

            let tsh_for_drop = self.tree_slice.handle();
            handlers = handlers.on_drop(move |mut payload, position, _ctx| {
                if let Some(drag_data) = payload.take_typed::<TreeViewDragData>() {
                    if drag_data.source_tree_id == my_tree_id {
                        let source_node = drag_data.source_node;

                        // Compute target flat index from Y
                        let scroll = scroll_for_drop.get().max(0.0);
                        let content_y = position.y + scroll;
                        let flat_idx = if ih_for_drop > 0.0 {
                            (content_y / ih_for_drop).floor().max(0.0) as usize
                        } else {
                            0
                        };

                        // Get the target entry for drop zone computation
                        if let Some(entry) = tsh_for_drop.entry_at(flat_idx) {
                            if entry.node_id == source_node {
                                return true; // dropped on self, no-op
                            }

                            // Compute drop zone from Y within the row:
                            // top third = before, middle = into (if has children), bottom = after
                            let row_top = flat_idx as f32 * ih_for_drop;
                            let y_in_row = content_y - row_top;
                            let third = ih_for_drop / 3.0;

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
                                        if let Some(si) = src_idx {
                                            if si < idx {
                                                idx -= 1;
                                            }
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
                                        if let Some(si) = src_idx {
                                            if si < idx {
                                                idx -= 1;
                                            }
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
                                    tree_model_for_drop
                                        .move_to_root(source_node, idx.min(root_count));
                                }
                            } else {
                                // Drop INTO target (middle third): reparent as first child
                                tree_model_for_drop.move_node(source_node, entry.node_id, 0);
                            }
                        }
                        return true;
                    }
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
            if let Some(widget) = self
                .tree_slice
                .with_entry(i, |item, entry| (self.delegate)(item, entry, selected))
            {
                let inner_id = ctx.add_boxed(widget);
                let (level, position_1based, total_siblings, expanded_opt) =
                    if let Some(ref e) = entry_meta {
                        let exp = if e.has_children { Some(e.is_expanded) } else { None };
                        let tree_model = self.tree_slice.tree();
                        let (pos, total) =
                            if let Some(parent_id) = tree_model.parent(e.node_id) {
                                let siblings = tree_model.children(parent_id);
                                let idx = siblings
                                    .iter()
                                    .position(|&s| s == e.node_id)
                                    .unwrap_or(0);
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
                    let has_children = item_has_children;
                    let node_for_toggle = self.tree_slice.visible_node_id(i);

                    ctx.apply_handlers(
                        child_id,
                        HandlerSet::new().on_pointer_event(move |event, _ctx| match event {
                            fern_core::event::WidgetEvent::PointerDown {
                                modifiers,
                                button: fern_core::event::PointerButton::Primary,
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
                                fern_core::event::EventResponse::Ignored
                            }
                            fern_core::event::WidgetEvent::PointerUp {
                                button: fern_core::event::PointerButton::Primary,
                                ..
                            } => {
                                // Expand/collapse fires on release so a drag
                                // gesture pre-empts it (once active_drag is
                                // set, PointerUp is routed to handle_drag_drop
                                // and never reaches this widget).
                                if has_children {
                                    if let Some(node_id) = node_for_toggle {
                                        tsh_click.toggle_expand(node_id);
                                    }
                                }
                                fern_core::event::EventResponse::Ignored
                            }
                            _ => fern_core::event::EventResponse::Ignored,
                        }),
                    );
                }

                // Attach drag handler for reorderable items. Produces a
                // visible preview by re-invoking the delegate for this row
                // and wrapping it in a DragPreview so it reads as
                // "picked up" at the pointer.
                if reorderable {
                    if let Some(node_id) = self.tree_slice.visible_node_id(i) {
                        let drag_tree_id = tree_id;
                        let drag_self_id = self_id;
                        let delegate_for_preview = self.delegate.clone();
                        let tsh_for_preview = self.tree_slice.handle();
                        let tree_model_for_preview = self.tree_slice.tree().clone();
                        let flat_idx = i;
                        let item_height_for_preview = self.item_height;
                        ctx.apply_handlers(
                            child_id,
                            HandlerSet::new().on_drag(move |phase, ctx| {
                                if let fern_core::gesture::DragPhase::Started { .. } = phase {
                                    let payload = DragPayload::typed(TreeViewDragData {
                                        source_node: node_id,
                                        source_tree_id: drag_tree_id,
                                    });
                                    let delegate = delegate_for_preview.clone();
                                    const PREVIEW_WIDTH: f32 = 240.0;
                                    let h = item_height_for_preview;
                                    // Build the preview from the source
                                    // node's item + entry metadata. The
                                    // entry captures depth / expansion
                                    // state so the floating preview matches
                                    // the row it was plucked from.
                                    let entry_meta = tsh_for_preview.entry_at(flat_idx);
                                    let preview_opt = entry_meta.and_then(|entry| {
                                        tree_model_for_preview.with_item(node_id, |item| {
                                            Box::new(crate::drag_preview::DragPreview::new(
                                                PREVIEW_WIDTH,
                                                h,
                                                delegate(item, &entry, false),
                                            )) as Box<dyn Widget>
                                        })
                                    });
                                    if let Some(preview) = preview_opt {
                                        ctx.start_drag_with_preview(
                                            drag_self_id,
                                            payload,
                                            preview,
                                        );
                                    } else {
                                        ctx.start_drag(drag_self_id, payload);
                                    }
                                }
                            }),
                        );
                    }
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

    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        let width = proposal.width.unwrap_or(300.0);
        let height = proposal.height.unwrap_or(200.0);
        self.viewport_height.set(height);
        Size::new(width, height)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        if children.is_empty() {
            return;
        }

        let total_height = self.total_content_height();
        let viewport_height = bounds.height;

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
        let content_width = (bounds.width - SCROLLBAR_THICKNESS).max(0.0);

        let item_count = self.item_entries.len();
        for (idx, child) in children.iter_mut().enumerate() {
            if idx < item_count {
                let (flat_index, _) = self.item_entries[idx];
                let y = bounds.y + flat_index as f32 * self.item_height - scroll_y;
                child.origin = Point::new(bounds.x, y);
                child.size = Size::new(content_width, self.item_height);
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
        canvas: &mut fern_canvas::Canvas,
        _ctx: &fern_core::widget::PaintContext,
    ) {
        // Draw insertion line during drag hover
        if let Some((y, width)) = self.drop_feedback.get() {
            let line_y = bounds.y + y;
            let line_x = bounds.x;
            canvas.fill_rect(
                Rect::new(line_x, line_y - 1.0, width, 2.0),
                fern_tokens::Color::from_rgba(0.2, 0.4, 0.9, 0.8),
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Tree);
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
    use fern_core::widget_tree::WidgetTree;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn size_that_fits(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
            Size::new(self.0, self.1)
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
        assert_eq!(info.role(), fern_core::accesskit::Role::Tree);
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
        assert_eq!(info_a.role(), fern_core::accesskit::Role::TreeItem);
        // A has children and is collapsed → is_expanded returns false
        assert!(
            !info_a.is_expanded(),
            "Root A should report not expanded (collapsed)"
        );

        // Third child (C) is a leaf → also not expanded
        let info_c = wtree.accessibility_node(children[2]);
        assert_eq!(info_c.role(), fern_core::accesskit::Role::TreeItem);
        assert!(!info_c.is_expanded(), "Leaf C should not be expanded");
    }

    #[test]
    fn keyboard_arrow_down_navigates() {
        use fern_core::event::{Key, Modifiers};
        use fern_data::{SelectionMode, SelectionModel};

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
        wtree.dispatch_event(fern_core::event::WidgetEvent::KeyDown {
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
        wtree.dispatch_event(fern_core::event::WidgetEvent::KeyDown {
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
        use fern_core::event::{Modifiers, PointerButton, WidgetEvent};
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
        use fern_data::TreeChange;
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
        use crate::primitives::{HStack, Padding, Spacer, TextWidget, ZStack};
        use crate::RectWidget;

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
                    fern_tokens::Color::from_rgba(0.25, 0.47, 0.85, 0.25)
                } else {
                    fern_tokens::Color::TRANSPARENT
                };
                Box::new(
                    ZStack::new().child(RectWidget::new().background(bg)).child(
                        Padding::symmetric(4.0, 12.0).child(
                            HStack::new()
                                .spacing(8.0)
                                .child(TextWidget::new_literal(arrow))
                                .child(TextWidget::new_literal(name.clone()))
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
        use crate::primitives::{HStack, Padding, Spacer, TextWidget, ZStack};
        use crate::RectWidget;

        let tree = sample_tree();
        let a = tree.root(0);
        let c = tree.root(2);
        let mut wtree = WidgetTree::new();
        let _tv_id = wtree.add(
            TreeView::new(tree.clone(), |name, _entry, _sel| {
                Box::new(
                    ZStack::new()
                        .child(RectWidget::new().background(fern_tokens::Color::TRANSPARENT))
                        .child(
                            Padding::symmetric(4.0, 12.0).child(
                                HStack::new()
                                    .spacing(8.0)
                                    .child(TextWidget::new_literal(name.clone()))
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
    fn spring_loaded_folder_expands_after_dwell() {
        // Drag a leaf over a collapsed folder and hold. After the dwell
        // delay (SPRING_DELAY_MS = 700 real ms), the folder should
        // auto-expand. Test drives real wall-clock time via `sleep` —
        // it's slow but accurate. Runs in ~750 ms; still headless.
        use fern_core::event::{Modifiers, PointerButton, WidgetEvent};
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
        assert!(!tree.with_item(b, |_| ()).is_none());
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
        use fern_core::event::{Key, Modifiers};
        use fern_data::{SelectionMode, SelectionModel};

        let model = sample_tree(); // A, B, C (3 roots)
        let a = model.root(0);
        let b = model.root(1);
        let c = model.root(2);
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
        wtree.dispatch_event(fern_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowUp,
            modifiers: Modifiers::ALT,
            text: None,
        });

        // After move: the roots should be reordered as B, A, C
        let new_roots: Vec<NodeId> = (0..model.root_count())
            .map(|i| model.root(i))
            .collect();
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
        wtree.dispatch_event(fern_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowDown,
            modifiers: Modifiers::ALT,
            text: None,
        });

        // After move: order should be A, B, C again
        let new_roots: Vec<NodeId> = (0..model.root_count())
            .map(|i| model.root(i))
            .collect();
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
        use fern_core::event::{Key, Modifiers};
        use fern_data::{SelectionMode, SelectionModel};

        // Tree: A with children A1, A2 (in that order)
        let tree = TreeModel::new();
        let a = tree.insert_root(0, "A");
        let a1 = tree.insert_child(a, 0, "A1");
        let a2 = tree.insert_child(a, 1, "A2");
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
        wtree.dispatch_event(fern_core::event::WidgetEvent::KeyDown {
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
        wtree.dispatch_event(fern_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowUp,
            modifiers: Modifiers::ALT,
            text: None,
        });
        // After move, relayout to refresh the tree view
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Check model: A2 should now be at index 0 under A, A1 at index 1
        let children_of_a = tree.children(a);
        assert_eq!(
            children_of_a.len(),
            2,
            "A should still have 2 children"
        );
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
        use fern_core::event::{Key, Modifiers};
        use fern_data::{SelectionMode, SelectionModel};

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
        wtree.dispatch_event(fern_core::event::WidgetEvent::KeyDown {
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
        wtree.dispatch_event(fern_core::event::WidgetEvent::KeyDown {
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
}
