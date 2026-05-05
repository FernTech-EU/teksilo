//! Tree tab — live widget hierarchy with substring filter, click to
//! select.
//!
//! Composed of a top filter `TextInput` (bound to
//! `state.tree_filter`) above a `TreeRows` leaf that paints rows
//! filtered by case-insensitive substring match against the
//! `last_segment` of each widget's type name.

use std::cell::{Cell, RefCell};

use fern_canvas::{Canvas, Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::arena::WidgetArena;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, TextRole};
use fern_widgets::primitives::{Expand, Padding, VStack};
use fern_widgets::{ScrollArea, TextInput};

use crate::state::InspectorState;
use crate::tabs::{ROW_HEIGHT, ROW_INDENT_PX, ROW_PADDING_X, last_segment};

#[derive(Clone, Debug)]
struct TreeRow {
    id: WidgetId,
    depth: u32,
    label: String,
}

/// Composing widget for the Tree tab. Builds the filter input plus
/// the rows leaf as siblings in a `VStack`.
pub(crate) struct TreeTab {
    state: InspectorState,
    root_child_id: Option<WidgetId>,
}

impl TreeTab {
    pub fn new(state: InspectorState) -> Self {
        Self {
            state,
            root_child_id: None,
        }
    }
}

impl std::fmt::Debug for TreeTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreeTab").finish()
    }
}

impl Widget for TreeTab {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let filter_input = Padding::symmetric(4.0, 4.0).child(
            TextInput::new(self.state.tree_filter.clone()).placeholder("filter type names…"),
        );
        // Build the ScrollArea ourselves so we can capture its
        // `scroll_y_signal` and let `TreeRows` drive auto-scroll-into-view
        // when the picker resolves to a widget that's currently off-screen.
        let scroll_area = ScrollArea::new();
        let scroll_y = scroll_area.scroll_y_signal().clone();
        let rows = scroll_area.child(TreeRows::new(self.state.clone(), scroll_y));
        let root = ctx.add(
            VStack::new()
                .spacing(2.0)
                .child(filter_input)
                .child(Expand::new().flex(1.0).child(rows)),
        );
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .map(LayoutResponse::from)
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into())
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for c in children.iter_mut() {
            c.origin = fern_canvas::Point::new(bounds.x, bounds.y);
            c.size = fern_canvas::Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

/// Snapshot-driven rows leaf. Walks the arena from roots in
/// `layout_response`, paints each row in `paint`, dispatches clicks
/// via `on_pointer_event`. Excludes every InspectorShell subtree from
/// the displayed list. Auto-scrolls the parent `ScrollArea` to bring
/// an externally-changed selection into view (e.g. picker tool).
struct TreeRows {
    state: InspectorState,
    rows: RefCell<Vec<TreeRow>>,
    /// Vertical scroll offset of the enclosing `ScrollArea`. Mutated
    /// in `layout_response` to keep the picker-selected row visible.
    scroll_y: Signal<f32>,
    /// Last selection observed by this leaf. Compared against the
    /// current `state.selected_id` to detect changes that warrant
    /// auto-scroll.
    last_seen_selection: Cell<Option<WidgetId>>,
}

impl TreeRows {
    fn new(state: InspectorState, scroll_y: Signal<f32>) -> Self {
        Self {
            state,
            rows: RefCell::new(Vec::new()),
            scroll_y,
            last_seen_selection: Cell::new(None),
        }
    }
}

impl std::fmt::Debug for TreeRows {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreeRows").finish()
    }
}

impl Widget for TreeRows {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // Repaint on selection change; relayout on filter / open flips
        // (initial mount + filter typing).
        self.state
            .selected_id
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        self.state
            .open
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
        self.state
            .tree_filter
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);

        let state_for_handler = self.state.clone();
        let handlers = HandlerSet::new()
            .focusable(true)
            .on_tap(move |event, _ctx| {
                state_for_handler
                    .pending_tree_click_y
                    .set(Some(event.position.y));
            });
        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        // Snapshot tree once per layout pass (cheap — a flat traversal).
        let mut rows: Vec<TreeRow> = Vec::new();
        if let Some(arena) = ctx.arena() {
            // Walk the user-root subtrees only — never `arena.roots()`
            // (which after wrapping is just the InspectorShell). This
            // keeps the inspector's own panel / overlays out of the
            // Tree listing without needing per-node exclusion logic.
            let user_roots = self.state.user_root_ids.get();
            for &root in user_roots.iter() {
                push_subtree(arena, root, 0, &mut rows);
            }
        }
        // Apply filter — case-insensitive substring match against the
        // type's last segment. Empty filter passes everything.
        let filter = self.state.tree_filter.get().to_lowercase();
        if !filter.trim().is_empty() {
            rows.retain(|r| r.label.to_lowercase().contains(filter.trim()));
        }
        let height = rows.len() as f32 * ROW_HEIGHT;
        *self.rows.borrow_mut() = rows;

        // Resolve any deferred click via local y. Track whether this
        // layout's selection change came from our own click — if it
        // did, skip the auto-scroll-into-view (the user already sees
        // the row they clicked and the jump would be jarring).
        let click_consumed = self.state.pending_tree_click_y.get().is_some();
        if let Some(y) = self.state.pending_tree_click_y.get() {
            let idx = (y / ROW_HEIGHT).floor() as usize;
            if let Some(row) = self.rows.borrow().get(idx) {
                self.state.selected_id.set(Some(row.id));
            }
            self.state.pending_tree_click_y.set(None);
        }

        // Auto-scroll: when the selection changes from somewhere
        // outside this leaf (typically the picker), scroll the
        // visible row near the top of the viewport with a small
        // margin so the user sees what just got selected.
        let cur_sel = self.state.selected_id.get();
        if cur_sel != self.last_seen_selection.get() {
            self.last_seen_selection.set(cur_sel);
            if !click_consumed
                && let Some(id) = cur_sel
                && let Some(idx) = self.rows.borrow().iter().position(|r| r.id == id)
            {
                let target = ((idx as f32) * ROW_HEIGHT - 20.0).max(0.0);
                if (self.scroll_y.get() - target).abs() > 0.5 {
                    self.scroll_y.set(target);
                }
            }
        }

        proposal.resolve(0.0, height).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let theme = ctx.theme;
        let style = &theme.typography.body;
        let color = TextRole::Primary.resolve(&theme.colors);
        let secondary = TextRole::Secondary.resolve(&theme.colors);
        let selected_id = self.state.selected_id.get();

        for (i, row) in self.rows.borrow().iter().enumerate() {
            let y = bounds.y + (i as f32) * ROW_HEIGHT;
            let row_rect = Rect::new(bounds.x, y, bounds.width, ROW_HEIGHT);
            // Highlight the selected row.
            if Some(row.id) == selected_id {
                let bg = Color::from_rgba(0.13, 0.55, 1.0, 0.15);
                canvas.fill_rounded_rect(row_rect, fern_tokens::CornerRadius::ZERO, bg);
            }
            let x = bounds.x + ROW_PADDING_X + (row.depth as f32) * ROW_INDENT_PX;
            let text_rect = Rect::new(
                x,
                y + 2.0,
                (bounds.width - (x - bounds.x)).max(0.0),
                ROW_HEIGHT,
            );
            let text_color = if Some(row.id) == selected_id {
                color
            } else {
                secondary
            };
            canvas.draw_text(&row.label, text_rect, style, text_color);
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

fn push_subtree(arena: &WidgetArena, id: WidgetId, depth: u32, out: &mut Vec<TreeRow>) {
    if !arena.is_active(id) {
        return;
    }
    let label = match arena.get(id) {
        Some(node) => last_segment(node.widget.type_name()).to_string(),
        None => return,
    };
    out.push(TreeRow { id, depth, label });
    let children: Vec<WidgetId> = arena.children(id).to_vec();
    for child in children {
        push_subtree(arena, child, depth + 1, out);
    }
}
