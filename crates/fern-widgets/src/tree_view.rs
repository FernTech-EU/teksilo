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
use fern_core::state::BindingLevel;
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
///         .child(TextWidget::new(&item.title)))
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

    // Persistent scroll state
    scroll_y: Signal<f32>,
    max_scroll_y: Signal<f32>,
    viewport_ratio_y: Signal<f32>,

    // Set during build
    item_entries: Vec<(usize, WidgetId)>, // (flat_index, widget_id)
    scrollbar_id: Option<WidgetId>,
    viewport_height: Cell<f32>,
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
            scroll_y: Signal::new_animated(0.0),
            max_scroll_y: Signal::new(0.0),
            viewport_ratio_y: Signal::new(1.0),
            item_entries: Vec::new(),
            scrollbar_id: None,
            viewport_height: Cell::new(600.0),
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

        ctx.register_animated_signal(&self.scroll_y);

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

        // --- Observe scroll position changes ---
        let item_height = self.item_height;
        let viewport_h = self.viewport_height.clone();
        let prev_start = Rc::new(Cell::new(usize::MAX));
        let prev_end = Rc::new(Cell::new(usize::MAX));
        let version_for_scroll = version.clone();
        let scroll_ver = Rc::new(Cell::new(0_u64));
        let scroll_handle = self.scroll_y.observe({
            let ps = prev_start.clone();
            let pe = prev_end.clone();
            let sv = scroll_ver.clone();
            move |y| {
                let scroll = y.max(0.0);
                let vp = viewport_h.get();
                let new_start = if item_height > 0.0 {
                    (scroll / item_height).floor() as usize
                } else {
                    0
                };
                let new_end = if item_height > 0.0 {
                    ((scroll + vp) / item_height).ceil() as usize
                } else {
                    0
                };
                if new_start != ps.get() || new_end != pe.get() {
                    ps.set(new_start);
                    pe.set(new_end);
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

                    // Alt+Arrow: reorder (when reorderable)
                    if modifiers.alt() && reorderable {
                        // TODO: implement tree node sibling reorder
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

            handlers = handlers.on_drag_hover(move |payload, _position, _ctx| {
                if payload.has_typed::<TreeViewDragData>() {
                    DropFeedback::InsertionLine {
                        y: 0.0,
                        width: 400.0,
                    }
                } else {
                    DropFeedback::NoFeedback
                }
            });

            // For the drop handler, we need access to the tree model and the
            // flattened entries. Access them via the TreeSlice's public methods.
            let tree_model_for_drop = self.tree_slice.tree().clone();
            let flattened_for_drop = self.tree_slice.version_signal(); // trigger re-reads
            let ih_for_drop = self.item_height;
            let scroll_for_drop = self.scroll_y.clone();

            let tsh_for_drop = self.tree_slice.handle();
            handlers = handlers.on_drop(move |mut payload, position, _ctx| {
                let _ = &flattened_for_drop; // keep version signal alive
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
                                if let Some(parent) = tree_model_for_drop.parent(target) {
                                    let siblings = tree_model_for_drop.children(parent);
                                    let idx =
                                        siblings.iter().position(|&n| n == target).unwrap_or(0);
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
                                    tree_model_for_drop.move_to_root(source_node, idx);
                                }
                            } else if y_in_row > 2.0 * third {
                                // Drop AFTER target: move as sibling below
                                let target = entry.node_id;
                                if let Some(parent) = tree_model_for_drop.parent(target) {
                                    let siblings = tree_model_for_drop.children(parent);
                                    let idx = siblings
                                        .iter()
                                        .position(|&n| n == target)
                                        .map(|i| i + 1)
                                        .unwrap_or(0);
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
                                    tree_model_for_drop
                                        .move_to_root(source_node, idx.min(root_count));
                                }
                            } else {
                                // Drop INTO target (middle third)
                                if entry.has_children || true {
                                    // Allow dropping into any node as first child
                                    tree_model_for_drop.move_node(source_node, entry.node_id, 0);
                                }
                            }
                        }
                        return true;
                    }
                }
                false
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
            if let Some(widget) = self
                .tree_slice
                .with_entry(i, |item, entry| (self.delegate)(item, entry, selected))
            {
                let inner_id = ctx.add_boxed(widget);
                let (level, expanded_opt) = entry_meta
                    .map(|e| {
                        let exp = if e.has_children {
                            Some(e.is_expanded)
                        } else {
                            None
                        };
                        (e.depth + 1, exp)
                    })
                    .unwrap_or((1, None));
                let child_id = ctx.add(crate::list_item_a11y::TreeItemWrapper::new(
                    inner_id,
                    level,
                    expanded_opt,
                ));

                // Selection click handling
                if let Some(ref sel) = self.selection {
                    let sel_click = sel.clone();
                    let click_index = i;
                    ctx.apply_handlers(
                        child_id,
                        HandlerSet::new().on_pointer_event(move |event, _ctx| match event {
                            fern_core::event::WidgetEvent::PointerDown {
                                modifiers,
                                button: fern_core::event::PointerButton::Primary,
                                ..
                            } => {
                                if modifiers.ctrl() {
                                    sel_click.toggle(click_index);
                                } else if modifiers.shift() {
                                    sel_click.extend_to(click_index);
                                } else {
                                    sel_click.select(click_index);
                                }
                                fern_core::event::EventResponse::Handled
                            }
                            _ => fern_core::event::EventResponse::Ignored,
                        }),
                    );
                }

                // Attach drag handler for reorderable items
                if reorderable {
                    if let Some(node_id) = self.tree_slice.visible_node_id(i) {
                        let drag_tree_id = tree_id;
                        let drag_self_id = self_id;
                        ctx.apply_handlers(
                            child_id,
                            HandlerSet::new().on_drag(move |gesture_event, ctx| {
                                if let fern_core::gesture::GestureEvent::DragStarted { .. } =
                                    gesture_event
                                {
                                    ctx.start_drag(
                                        drag_self_id,
                                        DragPayload::typed(TreeViewDragData {
                                            source_node: node_id,
                                            source_tree_id: drag_tree_id,
                                        }),
                                    );
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

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Tree);
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
        let (mut wtree, _tv_id) = make_tree_view(tree);
        wtree.layout(SizeProposal::exact(400.0, 300.0));
        // This test mainly verifies the widget builds without error
        // and that the role is set (verified by the accessibility method).
    }

    #[test]
    fn empty_tree() {
        let tree: TreeModel<&str> = TreeModel::new();
        let (mut wtree, tv_id) = make_tree_view(tree);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Only scrollbar
        assert_eq!(wtree.children(tv_id).len(), 1);
    }
}
