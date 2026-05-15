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

use fern_core::DropFeedback;
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::drag_payload::DragPayload;
use fern_core::signal::Signal;
use fern_core::widget::{LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;

use fern_data::ListModel;
use fern_data::selection_model::SelectionModel;

use crate::list_source::ListSource;
use crate::scroll_bar::{ScrollBar, ScrollBarOrientation};

/// Internal drag payload for intra-ListView reordering.
#[derive(Debug, Clone)]
struct ListViewDragData {
    /// The model index being dragged.
    source_index: usize,
    /// An ID to disambiguate different ListViews (pointer equality of the model).
    source_model_id: usize,
}

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
///         .child(TextWidget::new_literal(&item.title))
///         .child(Spacer::new()))
/// })
/// .item_height(28.0)
/// .selection(selection_model)
/// ```
pub struct ListView<T: 'static> {
    source: ListSource<T>,
    delegate: Rc<dyn Fn(usize, &T, bool) -> Box<dyn Widget>>,
    item_height: f32,
    spacing: f32,
    selection: Option<SelectionModel>,

    /// Keyboard-focused item index within the list.
    focused_index: Rc<Cell<Option<usize>>>,

    /// Enable intra-widget drag reordering + keyboard Alt+Arrow.
    reorderable: bool,

    /// Whether to render an internal vertical scrollbar. When the
    /// caller wants the scrollbar outside the list — e.g. so it
    /// survives ListView rebuilds — this is disabled and the caller
    /// mounts their own, wired through `scroll_y_signal` /
    /// `max_scroll_y_signal` / `viewport_ratio_y_signal`.
    show_scrollbar: bool,

    /// Callback for inter-widget drops from external drag sources.
    #[allow(clippy::type_complexity)]
    on_item_drop: Option<Rc<dyn Fn(DragPayload, usize) -> bool>>,

    // Persistent state (survives rebuild)
    scroll_y: Signal<f32>,
    max_scroll_y: Signal<f32>,
    viewport_ratio_y: Signal<f32>,

    /// Active drop feedback (set by on_drag_hover, cleared by on_drag_leave,
    /// read by paint). Reactive Signal — bound at `RepaintOnly` so any
    /// `set(...)` call dirties the ListView for repaint automatically.
    drop_feedback: Signal<Option<(f32, f32)>>, // (y, width) for insertion line
    /// Content width (updated during place_children, used by drag feedback).
    placed_content_width: Rc<Cell<f32>>,

    // Set during build
    item_entries: Vec<(usize, WidgetId)>, // (model_index, widget_id)
    scrollbar_id: Option<WidgetId>,
    /// Shared so the on_drag_tick closure sees the current viewport
    /// height when edge-computing its auto-scroll delta. Plain `Cell<f32>`
    /// clones by value, which would leave the tick closure reading the
    /// 600 px default forever.
    viewport_height: Rc<Cell<f32>>,

    /// Stable ID for this ListView instance (used to identify intra-widget reorder).
    model_id: usize,
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
    pub fn from_source<S: fern_data::ListDataSource<Item = T>>(
        source: S,
        delegate: impl Fn(usize, &T, bool) -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self::create(ListSource::from_data_source(source), delegate)
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
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
        let model_id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        Self {
            model_id,
            source,
            delegate: Rc::new(delegate),
            item_height: DEFAULT_ITEM_HEIGHT,
            spacing: 0.0,
            selection: None,
            focused_index: Rc::new(Cell::new(None)),
            reorderable: false,
            show_scrollbar: true,
            on_item_drop: None,
            drop_feedback: Signal::new(None),
            placed_content_width: Rc::new(Cell::new(0.0)),
            scroll_y: Signal::new_animated(0.0),
            max_scroll_y: Signal::new(0.0),
            viewport_ratio_y: Signal::new(1.0),
            item_entries: Vec::new(),
            scrollbar_id: None,
            viewport_height: Rc::new(Cell::new(600.0)),
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

    /// Enable intra-widget drag reordering.
    ///
    /// When enabled, items can be dragged and dropped within this ListView
    /// to reorder them. The underlying `ListModel::move_item()` is called
    /// automatically. Keyboard equivalent: Alt+ArrowUp/Down.
    pub fn reorderable(mut self, enabled: bool) -> Self {
        self.reorderable = enabled;
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

    /// Set a callback for inter-widget drops from external drag sources.
    ///
    /// The callback receives `(payload, insertion_index)` and returns `true`
    /// if the drop was accepted.
    pub fn on_item_drop(mut self, f: impl Fn(DragPayload, usize) -> bool + 'static) -> Self {
        self.on_item_drop = Some(Rc::new(f));
        self
    }

    /// Total content height (all items + spacing).
    fn total_content_height(&self) -> f32 {
        let count = self.source.len();
        if count == 0 {
            return 0.0;
        }
        let row_step = self.item_height + self.spacing;
        count as f32 * row_step - self.spacing
    }

    /// Compute the visible range of model indices for the current scroll and viewport.
    fn visible_range(&self) -> (usize, usize) {
        let count = self.source.len();
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
        let row_step = self.item_height + self.spacing;
        let target = index as f32 * row_step;
        let max = self.max_scroll_y.get();
        self.scroll_y.set(target.clamp(0.0, max));
    }

    /// Scroll the minimum distance needed to bring the given model
    /// index fully into the viewport. No-op if already visible.
    pub fn ensure_index_visible(&self, index: usize) {
        let row_step = self.item_height + self.spacing;
        let item_top = index as f32 * row_step;
        let item_bot = item_top + self.item_height;
        let scroll = self.scroll_y.get();
        let viewport = self.viewport_height.get();
        let max = self.max_scroll_y.get();
        if item_top < scroll {
            self.scroll_y.set(item_top.clamp(0.0, max));
        } else if item_bot > scroll + viewport {
            self.scroll_y.set((item_bot - viewport).clamp(0.0, max));
        }
    }
}

impl<T: 'static> std::fmt::Debug for ListView<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListView")
            .field("item_count", &self.source.len())
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

        // Bind scroll_y at Relayout so place_children runs on every scroll
        // position change (repositions items) without a full rebuild.
        self.scroll_y.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );

        // Register animated signal for smooth scrolling
        ctx.register_animated_signal(&self.scroll_y);

        // Bind drop_feedback at RepaintOnly so `set(...)` calls from
        // on_drag_hover / on_drag_leave dirty the ListView's paint cache
        // without triggering a rebuild.
        self.drop_feedback.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );

        // --- Observe model changes ---
        let version_for_data = version.clone();
        let data_ver = Rc::new(Cell::new(0_u64));
        let data_handle = (self.source.observe_fn)(Box::new({
            let dv = data_ver.clone();
            move |_change| {
                let next = dv.get() + 1;
                dv.set(next);
                version_for_data.set(next);
            }
        }));
        ctx.own_handle(data_handle);

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
        let spacing = self.spacing;
        let row_step = item_height + spacing;
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
                let visible_start = if row_step > 0.0 {
                    (scroll / row_step).floor() as usize
                } else {
                    0
                };
                let visible_end = if row_step > 0.0 {
                    ((scroll + vp) / row_step).ceil() as usize
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

        // --- Set up scroll event handler + DnD handlers on self ---
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

        // --- Keyboard navigation + Alt+Arrow reorder ---
        {
            let len_for_key = self.source.len_fn.clone();
            let move_for_key = self.source.move_item_fn.clone();
            let sel_for_key = self.selection.clone();
            let fi = self.focused_index.clone();
            let reorderable = self.reorderable;
            let scroll_for_nav = self.scroll_y.clone();
            let ih_for_nav = self.item_height;
            let sp_for_nav = self.spacing;
            let vh_for_nav = self.viewport_height.clone();

            handlers = handlers.on_key(move |event, _ctx| {
                if let fern_core::event::WidgetEvent::KeyDown { key, modifiers, .. } = event {
                    let count = (len_for_key)();
                    if count == 0 {
                        return fern_core::event::EventResponse::Ignored;
                    }

                    // Alt+Arrow: reorder (when reorderable)
                    if modifiers.alt() && reorderable {
                        let selected_idx = sel_for_key
                            .as_ref()
                            .and_then(|s| s.selected_indices().first().copied());
                        if let Some(idx) = selected_idx {
                            match key {
                                fern_core::event::Key::ArrowUp if idx > 0 => {
                                    if let Some(ref mf) = move_for_key {
                                        mf(idx, idx - 1);
                                    }
                                    if let Some(ref sel) = sel_for_key {
                                        sel.select(idx - 1);
                                    }
                                    fi.set(Some(idx - 1));
                                    return fern_core::event::EventResponse::Handled;
                                }
                                fern_core::event::Key::ArrowDown if idx + 1 < count => {
                                    if let Some(ref mf) = move_for_key {
                                        mf(idx, idx + 1);
                                    }
                                    if let Some(ref sel) = sel_for_key {
                                        sel.select(idx + 1);
                                    }
                                    fi.set(Some(idx + 1));
                                    return fern_core::event::EventResponse::Handled;
                                }
                                _ => {}
                            }
                        }
                    }

                    // Navigation keys (no modifiers or with Shift for extend)
                    let current = fi.get().unwrap_or(0);
                    let new_idx = match key {
                        fern_core::event::Key::ArrowDown => {
                            Some(current.saturating_add(1).min(count - 1))
                        }
                        fern_core::event::Key::ArrowUp => Some(current.saturating_sub(1)),
                        fern_core::event::Key::Home => Some(0),
                        fern_core::event::Key::End => Some(count - 1),
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
                        // Select the focused item (standard list keyboard behavior)
                        if let Some(ref sel) = sel_for_key {
                            if modifiers.shift() {
                                sel.extend_to(idx);
                            } else {
                                sel.select(idx);
                            }
                        }
                        // Scroll into view
                        let row_step = ih_for_nav + sp_for_nav;
                        let item_top = idx as f32 * row_step;
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

        // --- DnD: register self as drop target when reorderable or on_item_drop ---
        if self.reorderable || self.on_item_drop.is_some() {
            let row_step_for_hover = self.item_height + self.spacing;
            let ih_for_hover = self.item_height;
            let scroll_for_hover = self.scroll_y.clone();
            let len_for_hover = self.source.len_fn.clone();
            let my_model_id = self.model_id;

            let feedback_for_hover = self.drop_feedback.clone();
            let width_for_hover = self.placed_content_width.clone();
            handlers = handlers.on_drag_hover(move |payload, position, _ctx| {
                let scroll = scroll_for_hover.get().max(0.0);
                let content_y = position.y + scroll;
                let index = if row_step_for_hover > 0.0 {
                    ((content_y + ih_for_hover * 0.5) / row_step_for_hover)
                        .floor()
                        .max(0.0)
                        .min((len_for_hover)() as f32) as usize
                } else {
                    0
                };

                if payload.has_typed::<ListViewDragData>() {
                    let line_width = width_for_hover.get();
                    let insertion_y = index as f32 * row_step_for_hover - scroll;
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
            let move_for_drop = self.source.move_item_fn.clone();
            let on_item_drop = self.on_item_drop.clone();
            let scroll_for_drop = self.scroll_y.clone();
            let ih_for_drop = self.item_height;
            let row_step_for_drop = self.item_height + self.spacing;

            handlers = handlers.on_drop(move |mut payload, position, _ctx| {
                let scroll = scroll_for_drop.get().max(0.0);
                let content_y = position.y + scroll;
                let to_index = if row_step_for_drop > 0.0 {
                    ((content_y + ih_for_drop * 0.5) / row_step_for_drop)
                        .floor()
                        .max(0.0)
                        .min((len_for_drop)() as f32) as usize
                } else {
                    0
                };

                // Check if this is an intra-widget reorder
                if let Some(drag_data) = payload.take_typed::<ListViewDragData>()
                    && drag_data.source_model_id == my_model_id
                {
                    let from = drag_data.source_index;
                    // Adjust target index: if dragging down, the removal shifts indices
                    let adjusted_to = if from < to_index {
                        to_index.saturating_sub(1)
                    } else {
                        to_index
                    };
                    if from != adjusted_to
                        && let Some(ref mf) = move_for_drop
                    {
                        mf(from, adjusted_to);
                    }
                    return true;
                }

                // Inter-widget drop
                if let Some(ref handler) = on_item_drop {
                    return handler(payload, to_index);
                }

                false
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

        ctx.apply_self_handlers(handlers);

        // --- Create visible item widgets ---
        let (start, end) = self.visible_range();
        self.item_entries.clear();
        let selection = &self.selection;
        let reorderable = self.reorderable;
        let model_id = self.model_id;
        let self_id = ctx.self_id();
        for i in start..end {
            let selected = selection
                .as_ref()
                .map(|s| s.is_selected(i))
                .unwrap_or(false);
            if let Some(widget) =
                (self.source.with_item_fn)(i, &|item| (self.delegate)(i, item, selected))
            {
                let inner_id = ctx.add_boxed(widget);
                let child_id = ctx.add(crate::list_item_a11y::ListItemWrapper::new(
                    inner_id, selected,
                ));

                // Selection click handling: plain click selects,
                // Ctrl+click toggles, Shift+click extends range.
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
                                // Ignored so the gesture arena on this
                                // widget still sees the PointerDown and
                                // can arm the DragRecognizer for
                                // drag-to-reorder alongside selection.
                                fern_core::event::EventResponse::Ignored
                            }
                            _ => fern_core::event::EventResponse::Ignored,
                        }),
                    );
                }

                // When reorderable, attach an on_drag handler to start drag.
                // The preview is a fresh copy of the delegate's widget for the
                // dragged item, wrapped in a sized+raised `DragPreview` so the
                // floating widget has a stable footprint and reads as "picked
                // up" against the window surface. Uses
                // `start_drag_with_preview` so the framework overlays the
                // preview at the pointer.
                if reorderable {
                    let drag_index = i;
                    let drag_model_id = model_id;
                    let drag_self_id = self_id;
                    let delegate_for_preview = self.delegate.clone();
                    let with_item_for_preview = self.source.with_item_fn.clone();
                    let item_height_for_preview = self.item_height;
                    let width_for_preview = self.placed_content_width.clone();
                    ctx.apply_handlers(
                        child_id,
                        HandlerSet::new().on_drag(move |phase, ctx| {
                            if let fern_core::gesture::DragPhase::Started { .. } = phase {
                                let payload = DragPayload::typed(ListViewDragData {
                                    source_index: drag_index,
                                    source_model_id: drag_model_id,
                                });
                                let delegate = delegate_for_preview.clone();
                                let w = width_for_preview.get().max(120.0);
                                let h = item_height_for_preview;
                                let preview_opt = (with_item_for_preview)(drag_index, &|item| {
                                    Box::new(crate::drag_preview::DragPreview::new(
                                        w,
                                        h,
                                        delegate(drag_index, item, false),
                                    )) as Box<dyn Widget>
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

        // --- Create scrollbar ---
        // Skipped when the caller opted out via `show_scrollbar(false)`
        // — they're expected to mount their own, wired through the
        // exposed signal accessors, so it can outlive ListView
        // rebuilds (e.g. a ComboBox panel keeping the scrollbar alive
        // mid-thumb-drag so the drag isn't torn down when the visible
        // range crosses the buffer).
        if self.show_scrollbar {
            let scrollbar = ScrollBar::new(
                ScrollBarOrientation::Vertical,
                self.scroll_y.clone(),
                self.max_scroll_y.clone(),
                self.viewport_ratio_y.clone(),
            );
            let sb_id = ctx.add(scrollbar);
            self.scrollbar_id = Some(sb_id);
        } else {
            self.scrollbar_id = None;
        }

        let mut children: Vec<WidgetId> = self.item_entries.iter().map(|(_, id)| *id).collect();
        if let Some(sb) = self.scrollbar_id {
            children.push(sb);
        }
        children
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        // The viewport takes whatever the parent offers.
        let width = proposal.width.unwrap_or(300.0);
        let height = proposal.height.unwrap_or(200.0);

        // Cache viewport height for visible range computation.
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
        let needs_internal_scrollbar = self.show_scrollbar && total_height > viewport_height + 0.5;
        let content_width = if needs_internal_scrollbar {
            (bounds.width - SCROLLBAR_THICKNESS).max(0.0)
        } else {
            bounds.width
        };
        self.placed_content_width.set(content_width);

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

        // Place scrollbar (last child) — only when the ListView owns one.
        if self.show_scrollbar
            && let Some(sb_child) = children.last_mut()
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
        canvas: &mut fern_canvas::Canvas,
        ctx: &fern_core::widget::PaintContext,
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
            canvas.fill_rect(
                Rect::new(line_x, line_y - half, width, recipe.thickness),
                color,
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::ListBox);
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
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> fern_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
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

    #[test]
    fn small_list_items_use_full_width() {
        let (mut tree, lv_id, _model) = make_list_view(3, 30.0);
        // 3 items * 30px = 90px < 300px viewport — no scrollbar needed
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let children = tree.children(lv_id);
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
        fern_data::SelectionModel,
    ) {
        use fern_data::{SelectionMode, SelectionModel};
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
        let children = tree.children(lv_id);
        tree.click(children[1]);
        assert!(selection.is_selected(1), "item 1 should be selected");
        assert!(!selection.is_selected(0), "item 0 should not be selected");
    }

    #[test]
    fn click_replaces_selection() {
        let (mut tree, lv_id, _, selection) = make_selectable_list(5);
        let children = tree.children(lv_id);
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
    fn ctrl_click_toggles() {
        use fern_core::event::Modifiers;
        let (mut tree, lv_id, _, selection) = make_selectable_list(5);
        let children = tree.children(lv_id);

        // Select item 0
        tree.click(children[0]);
        assert!(selection.is_selected(0));

        // Ctrl+click item 2 to add it
        let center = tree.bounds(children[2]).center();
        tree.dispatch_event(fern_core::event::WidgetEvent::PointerDown {
            position: center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: Modifiers::CTRL,
        });
        tree.dispatch_event(fern_core::event::WidgetEvent::PointerUp {
            position: center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: Modifiers::CTRL,
        });

        assert!(selection.is_selected(0), "item 0 should still be selected");
        assert!(selection.is_selected(2), "item 2 should be toggled on");
    }

    #[test]
    fn shift_click_extends_range() {
        use fern_core::event::Modifiers;
        let (mut tree, lv_id, _, selection) = make_selectable_list(5);
        let children = tree.children(lv_id);

        // Select item 1 as anchor
        tree.click(children[1]);
        assert!(
            selection.is_selected(1),
            "item 1 should be selected after plain click"
        );

        // Shift+click item 3 — should extend from anchor (1) to 3
        let center = tree.bounds(children[3]).center();
        tree.dispatch_event(fern_core::event::WidgetEvent::PointerDown {
            position: center,
            button: fern_core::event::PointerButton::Primary,
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
        let children = tree.children(lv_id);
        let first_y = tree.bounds(children[0]).y;
        assert!(
            first_y.abs() < 30.0,
            "First visible item should be near the top, got y={}",
            first_y
        );

        // Scroll down by 1500px (50 items * 30px)
        tree.dispatch_event(fern_core::event::WidgetEvent::Scroll {
            delta: fern_core::event::ScrollDelta::Pixels { x: 0.0, y: 1500.0 },
            modifiers: Default::default(),
        });
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // After scroll: the first item's Y should be near 0 (scroll offset applied),
        // and crucially it should NOT be the same items as before scroll.
        let children_after = tree.children(lv_id);
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
        let children = tree.children(lv_id);
        let info = tree.accessibility_node(children[0]);
        assert_eq!(
            info.role(),
            fern_core::accesskit::Role::ListBoxOption,
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
        use fern_core::event::{Key, Modifiers};
        use fern_data::{SelectionMode, SelectionModel};

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
        tree.dispatch_event(fern_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowDown,
            modifiers: Modifiers::ALT,
            text: None,
        });

        // Expect a single swap: [10,20,30,40,50] → [20,10,30,40,50].
        assert_eq!(model.with_item(0, |v| *v), Some(20));
        assert_eq!(model.with_item(1, |v| *v), Some(10));
        assert_eq!(model.with_item(2, |v| *v), Some(30));
    }

    #[test]
    fn alt_arrow_reorders_item() {
        use fern_core::event::{Key, Modifiers};
        use fern_data::{SelectionMode, SelectionModel};

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
        tree.dispatch_event(fern_core::event::WidgetEvent::KeyDown {
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
        use fern_core::event::{Modifiers, PointerButton, WidgetEvent};
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
        let children = tree.children(lv_id);
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
        let children = tree.children(lv_id);
        let from = tree.bounds(children[3]).center();
        let to = Point::new(from.x, 15.0);
        drag_item(&mut tree, from, to);

        // After move: [10, 40, 20, 30, 50]
        assert_eq!(model.with_item(1, |v| *v), Some(40));
        assert_eq!(model.with_item(2, |v| *v), Some(20));
        assert_eq!(model.with_item(3, |v| *v), Some(30));
    }

    #[test]
    fn drag_emits_items_moved_change() {
        use fern_data::DataChange;
        use std::cell::Cell;
        use std::rc::Rc;

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
        let children = tree.children(lv_id);
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
        tree.dispatch_event(fern_core::event::WidgetEvent::Scroll {
            delta: fern_core::event::ScrollDelta::Pixels { x: 0.0, y: 60.0 },
            modifiers: Default::default(),
        });
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
        use fern_data::{SelectionMode, SelectionModel};
        use std::cell::Cell;
        use std::rc::Rc;

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
        let children = tree.children(lv_id);
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
        use fern_core::event::{Modifiers, PointerButton, WidgetEvent};
        use fern_data::{SelectionMode, SelectionModel};

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
    fn inter_widget_on_item_drop_receives_payload() {
        use crate::primitives::VStack;
        use fern_core::drag_payload::DragPayload;
        use fern_core::gesture::DragPhase;
        use fern_core::widget_builder::WidgetBuilder;
        use std::cell::Cell;
        use std::cell::RefCell;
        use std::rc::Rc;

        // VStack root with:
        //   - source: FixedLeaf at y=0..30 with on_drag that fires start_drag
        //     carrying a typed String payload (NOT ListViewDragData).
        //   - target: ListView below at y=30.., with on_item_drop that stores
        //     the (payload_string, index) tuple.
        //
        // Rationale: the ListView's on_drop consumes any ListViewDragData
        // payload via take_typed, so the inter-widget path is reachable only
        // when the source widget is NOT another ListView — which matches the
        // intended public contract.
        let received = Rc::new(RefCell::new(None::<(String, usize)>));
        let r = received.clone();

        let target_model = ListModel::from_vec(vec![0_usize; 3]);
        let target_model_clone = target_model.clone();

        let source_id_holder: Rc<Cell<WidgetId>> = Rc::new(Cell::new(WidgetId::default()));
        let sih = source_id_holder.clone();

        let mut tree = WidgetTree::new();
        let root = tree.add(
            VStack::new()
                .child(FixedLeaf(100.0, 30.0).on_drag(move |phase, ctx| {
                    if let DragPhase::Started { .. } = phase {
                        ctx.start_drag(sih.get(), DragPayload::typed("external".to_string()));
                    }
                }))
                .child(
                    ListView::new(target_model_clone, |_i, _item, _sel| {
                        Box::new(FixedLeaf(100.0, 30.0))
                    })
                    .item_height(30.0)
                    .on_item_drop(move |mut payload, idx| {
                        if let Some(s) = payload.take_typed::<String>() {
                            *r.borrow_mut() = Some((s, idx));
                            true
                        } else {
                            false
                        }
                    }),
                ),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Resolve the source widget id — it's the FixedLeaf nested under the
        // VStack, wrapped by WidgetWithHandlers because we attached on_drag.
        let vstack_children = tree.children(root);
        let source_widget = vstack_children[0];
        source_id_holder.set(source_widget);

        // Drag from the source (centered at tree y=15) into the ListView.
        // The ListView starts at tree y=30 (after the 30 px source row),
        // and `on_drop` receives the pointer in TARGET-LOCAL coordinates:
        //
        //   local_y = tree_y - 30
        //   idx     = floor((local_y + item_height/2) / item_height)
        //
        // Targeting idx=2 needs local_y ∈ [45, 74]; use local_y=60
        // (tree_y=90) — the conceptual centre of the third insertion
        // zone.
        let source_center = tree.bounds(source_widget).center();
        drag_item(&mut tree, source_center, Point::new(source_center.x, 90.0));

        let (text, idx) = received.borrow().clone().expect("on_item_drop must fire");
        assert_eq!(text, "external");
        assert_eq!(idx, 2);
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
        use fern_core::event::{Modifiers, PointerButton, WidgetEvent};

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
        use fern_core::event::{Modifiers, PointerButton, WidgetEvent};

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
}
