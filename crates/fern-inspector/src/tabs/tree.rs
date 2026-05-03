//! Tree tab — live widget hierarchy with substring filter, click to
//! select.
//!
//! Composed of a top filter `TextInput` (bound to
//! `state.tree_filter`) above a `TreeRows` leaf that paints rows
//! filtered by case-insensitive substring match against the
//! `last_segment` of each widget's type name.

use std::cell::RefCell;

use fern_canvas::{Canvas, Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::arena::WidgetArena;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, TextRole};
use fern_widgets::TextInput;
use fern_widgets::primitives::{Padding, VStack};

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
        let filter_input = Padding::symmetric(4.0, 4.0)
            .child(TextInput::new(self.state.tree_filter.clone()).placeholder("filter type names…"));
        let rows = TreeRows::new(self.state.clone());
        let root = ctx.add(VStack::new().spacing(2.0).child(filter_input).child(rows));
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
/// the displayed list.
struct TreeRows {
    state: InspectorState,
    rows: RefCell<Vec<TreeRow>>,
}

impl TreeRows {
    fn new(state: InspectorState) -> Self {
        Self {
            state,
            rows: RefCell::new(Vec::new()),
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
        self.state.selected_id.bind_to(
            self_id,
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        self.state
            .open
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
        self.state
            .tree_filter
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);

        let state_for_handler = self.state.clone();
        let handlers = HandlerSet::new()
            .focusable(true)
            .on_tap(move |position, _ctx| {
                state_for_handler.pending_tree_click_y.set(Some(position.y));
            });
        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        // Snapshot tree once per layout pass (cheap — a flat traversal).
        let mut rows: Vec<TreeRow> = Vec::new();
        if let Some(arena) = ctx.arena() {
            let excludes = self.state.shell_root_ids.get();
            for &root in arena.roots().iter() {
                if excludes.contains(&root) {
                    continue;
                }
                push_subtree(arena, root, 0, &excludes, &mut rows);
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

        // Resolve any deferred click via local y.
        if let Some(y) = self.state.pending_tree_click_y.get() {
            let idx = (y / ROW_HEIGHT).floor() as usize;
            if let Some(row) = self.rows.borrow().get(idx) {
                self.state.selected_id.set(Some(row.id));
            }
            self.state.pending_tree_click_y.set(None);
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

fn push_subtree(
    arena: &WidgetArena,
    id: WidgetId,
    depth: u32,
    excludes: &[WidgetId],
    out: &mut Vec<TreeRow>,
) {
    if excludes.contains(&id) {
        return;
    }
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
        push_subtree(arena, child, depth + 1, excludes, out);
    }
}
