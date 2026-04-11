//! Virtualized scrollable list widget.
//!
//! `ListView` creates widget subtrees only for the items currently visible in
//! the viewport (plus a small buffer). When the user scrolls or the data model
//! changes, the widget rebuilds to show the new visible range.
//!
//! For small collections where all items should exist simultaneously, use
//! `Repeater` instead.

use std::cell::Cell;
use std::rc::Rc;

use fern_canvas::{Point, Rect, Size, SizeProposal};

use fern_core::accessibility::AccessNodeBuilder;
use fern_core::signal::Signal;
use fern_core::state::BindingLevel;
use fern_core::widget::{LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;

use fern_data::ListModel;
use fern_data::selection_model::SelectionModel;

use crate::scroll_bar::{ScrollBar, ScrollBarOrientation};

/// Default number of extra items to create above and below the viewport.
const BUFFER_ITEMS: usize = 5;
/// Default item height.
const DEFAULT_ITEM_HEIGHT: f32 = 32.0;
/// Scrollbar thickness.
const SCROLLBAR_THICKNESS: f32 = 12.0;

/// A virtualized scrollable list backed by a `ListModel<T>`.
///
/// ```ignore
/// ListView::new(model, |index, item, selected| {
///     Box::new(HStack::new()
///         .child(TextWidget::new(&item.title))
///         .child(Spacer::new()))
/// })
/// .item_height(28.0)
/// .selection(selection_model)
/// ```
pub struct ListView<T: 'static> {
    model: ListModel<T>,
    delegate: Rc<dyn Fn(usize, &T, bool) -> Box<dyn Widget>>,
    item_height: f32,
    spacing: f32,
    selection: Option<SelectionModel>,

    // Persistent state (survives rebuild)
    scroll_y: Signal<f32>,
    max_scroll_y: Signal<f32>,
    viewport_ratio_y: Signal<f32>,

    // Set during build
    item_entries: Vec<(usize, WidgetId)>, // (model_index, widget_id)
    scrollbar_id: Option<WidgetId>,
    viewport_height: Cell<f32>,
    data_version: Option<Signal<u64>>,
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
        Self {
            model,
            delegate: Rc::new(delegate),
            item_height: DEFAULT_ITEM_HEIGHT,
            spacing: 0.0,
            selection: None,
            scroll_y: Signal::new_animated(0.0),
            max_scroll_y: Signal::new(0.0),
            viewport_ratio_y: Signal::new(1.0),
            item_entries: Vec::new(),
            scrollbar_id: None,
            viewport_height: Cell::new(600.0), // reasonable default for first build
            data_version: None,
        }
    }

    /// Set the fixed height per item (default 32.0).
    pub fn item_height(mut self, height: f32) -> Self {
        self.item_height = height;
        self
    }

    /// Set spacing between items (default 0.0).
    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Set the selection model.
    pub fn selection(mut self, sel: SelectionModel) -> Self {
        self.selection = Some(sel);
        self
    }

    /// Total content height (all items + spacing).
    fn total_content_height(&self) -> f32 {
        let count = self.model.len();
        if count == 0 {
            return 0.0;
        }
        let row_step = self.item_height + self.spacing;
        count as f32 * row_step - self.spacing
    }

    /// Compute the visible range of model indices for the current scroll and viewport.
    fn visible_range(&self) -> (usize, usize) {
        let count = self.model.len();
        if count == 0 {
            return (0, 0);
        }
        let row_step = self.item_height + self.spacing;
        let scroll = self.scroll_y.get().max(0.0);
        let viewport = self.viewport_height.get();

        let first_visible = (scroll / row_step).floor() as usize;
        let last_visible = ((scroll + viewport) / row_step).ceil() as usize;

        let start = first_visible.saturating_sub(BUFFER_ITEMS);
        let end = (last_visible + BUFFER_ITEMS).min(count);

        (start, end)
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
}

impl<T: 'static> std::fmt::Debug for ListView<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListView")
            .field("item_count", &self.model.len())
            .field("item_height", &self.item_height)
            .field("scroll_y", &self.scroll_y.get())
            .finish()
    }
}

impl<T: 'static> Widget for ListView<T> {
    fn build(&mut self, ctx: &mut fern_core::build_context::BuildContext) -> Vec<WidgetId> {
        // --- Version signal for rebuild triggering ---
        let version = ctx.signal(0_u64);
        version.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);
        self.data_version = Some(version.clone());

        // Register animated signal for smooth scrolling
        ctx.register_animated_signal(&self.scroll_y);

        // --- Observe model changes ---
        let version_for_data = version.clone();
        let data_ver = Rc::new(Cell::new(0_u64));
        let data_handle = self.model.observe_changes({
            let dv = data_ver.clone();
            move |_change| {
                let next = dv.get() + 1;
                dv.set(next);
                version_for_data.set(next);
            }
        });
        ctx.own_handle(data_handle);

        // --- Observe scroll position changes (rebuild only when visible range changes) ---
        let item_height = self.item_height;
        let spacing = self.spacing;
        let row_step = item_height + spacing;
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
                let new_start = if row_step > 0.0 {
                    (scroll / row_step).floor() as usize
                } else {
                    0
                };
                let new_end = if row_step > 0.0 {
                    ((scroll + vp) / row_step).ceil() as usize
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

        // --- Set up scroll event handler ---
        let scroll_y = self.scroll_y.clone();
        let max_scroll = self.max_scroll_y.clone();
        let line_height = self.item_height;
        let handlers = HandlerSet::new()
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
            .clips_children(true);
        ctx.apply_self_handlers(handlers);

        // --- Create visible item widgets ---
        let (start, end) = self.visible_range();
        self.item_entries.clear();
        let selection = &self.selection;
        for i in start..end {
            let selected = selection
                .as_ref()
                .map(|s| s.is_selected(i))
                .unwrap_or(false);
            if let Some(widget) = self
                .model
                .with_item(i, |item| (self.delegate)(i, item, selected))
            {
                let child_id = ctx.add_boxed(widget);
                self.item_entries.push((i, child_id));
            }
        }

        // --- Create scrollbar ---
        let scrollbar = ScrollBar::new(
            ScrollBarOrientation::Vertical,
            self.scroll_y.clone(),
            self.max_scroll_y.clone(),
            self.viewport_ratio_y.clone(),
        );
        let sb_id = ctx.add(scrollbar);
        self.scrollbar_id = Some(sb_id);

        // Return all children (items + scrollbar)
        let mut children: Vec<WidgetId> = self.item_entries.iter().map(|(_, id)| *id).collect();
        children.push(sb_id);
        children
    }

    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        // The viewport takes whatever the parent offers.
        let width = proposal.width.unwrap_or(300.0);
        let height = proposal.height.unwrap_or(200.0);

        // Cache viewport height for visible range computation.
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

        // Update reactive scroll state
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
        let row_step = self.item_height + self.spacing;
        let content_width = (bounds.width - SCROLLBAR_THICKNESS).max(0.0);

        // Place item widgets
        let item_count = self.item_entries.len();
        for (idx, child) in children.iter_mut().enumerate() {
            if idx < item_count {
                let (model_index, _) = self.item_entries[idx];
                let y = bounds.y + model_index as f32 * row_step - scroll_y;
                child.origin = Point::new(bounds.x, y);
                child.size = Size::new(content_width, self.item_height);
            }
        }

        // Place scrollbar (last child)
        if let Some(sb_child) = children.last_mut() {
            let needs_scrollbar = total_height > viewport_height + 0.5;
            if needs_scrollbar {
                sb_child.origin =
                    Point::new(bounds.x + bounds.width - SCROLLBAR_THICKNESS, bounds.y);
                sb_child.size = Size::new(SCROLLBAR_THICKNESS, bounds.height);
            } else {
                // Collapse scrollbar when not needed
                sb_child.origin = bounds.origin();
                sb_child.size = Size::ZERO;
            }
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::List);
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

    #[test]
    fn virtualization_creates_only_visible_items() {
        let (mut tree, lv_id, _model) = make_list_view(10_000, 30.0);
        // Viewport: 300px tall, items 30px each = ~10 visible + 2*5 buffer = ~20
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let children = tree.children(lv_id);
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

        let children = tree.children(lv_id);
        // Only the scrollbar child (items = 0)
        assert_eq!(children.len(), 1);
    }

    #[test]
    fn data_change_triggers_rebuild() {
        let (mut tree, lv_id, model) = make_list_view(5, 30.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let initial_items = tree.children(lv_id).len() - 1; // minus scrollbar
        assert_eq!(initial_items, 5);

        model.push(99);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let new_items = tree.children(lv_id).len() - 1;
        assert_eq!(new_items, 6);
    }

    #[test]
    fn remove_triggers_rebuild() {
        let (mut tree, lv_id, model) = make_list_view(5, 30.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        assert_eq!(tree.children(lv_id).len() - 1, 5);

        model.remove(0);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        assert_eq!(tree.children(lv_id).len() - 1, 4);
    }

    #[test]
    fn items_positioned_correctly() {
        let (mut tree, lv_id, _model) = make_list_view(3, 40.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let children = tree.children(lv_id);
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

        let children = tree.children(lv_id);
        for i in 0..3 {
            let h = tree.bounds(children[i]).height;
            assert!((h - 40.0).abs() < 0.01, "Item {} height {} != 40.0", i, h);
        }
    }

    #[test]
    fn scrollbar_positioned_on_right_edge() {
        let (mut tree, lv_id, _model) = make_list_view(100, 30.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let children = tree.children(lv_id);
        let sb = children.last().unwrap();
        let sb_bounds = tree.bounds(*sb);
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

        let children = tree.children(lv_id);
        let sb = children.last().unwrap();
        let sb_bounds = tree.bounds(*sb);
        assert!(
            sb_bounds.width < 0.01 && sb_bounds.height < 0.01,
            "Scrollbar should be collapsed for small lists"
        );
    }

    #[test]
    fn item_width_leaves_room_for_scrollbar() {
        let (mut tree, lv_id, _model) = make_list_view(100, 30.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let children = tree.children(lv_id);
        let item_width = tree.bounds(children[0]).width;
        assert!(
            (item_width - (400.0 - SCROLLBAR_THICKNESS)).abs() < 0.01,
            "Item width {} should be {}",
            item_width,
            400.0 - SCROLLBAR_THICKNESS
        );
    }
}
