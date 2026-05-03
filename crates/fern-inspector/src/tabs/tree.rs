//! Tree tab — live widget hierarchy, click to select.

use std::cell::RefCell;

use fern_canvas::{Canvas, Rect, SizeProposal};
use fern_tokens::Color;
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::arena::WidgetArena;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::TextRole;

use crate::state::InspectorState;
use crate::tabs::{ROW_HEIGHT, ROW_INDENT_PX, ROW_PADDING_X, last_segment};

#[derive(Clone, Debug)]
struct TreeRow {
    id: WidgetId,
    depth: u32,
    label: String,
}

/// Snapshot-driven tree view. Walks the arena from roots in
/// `layout_response`, paints each row in `paint`, dispatches clicks
/// via `on_pointer_event`. Excludes the inspector shell's own subtree
/// from the displayed list.
pub(crate) struct TreeTab {
    state: InspectorState,
    rows: RefCell<Vec<TreeRow>>,
}

impl TreeTab {
    pub fn new(state: InspectorState) -> Self {
        Self {
            state,
            rows: RefCell::new(Vec::new()),
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
        // Re-paint when selection changes (selected row gets a different
        // background). Re-layout when the inspector's own root id is
        // resolved (initial mount) or panel state flips.
        let self_id = ctx.self_id();
        self.state
            .selected_id
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        self.state
            .open
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);

        let state_for_handler = self.state.clone();
        let handlers = HandlerSet::new()
            .focusable(true)
            .on_tap(move |position, _ctx| {
                // `on_tap` gives widget-local coordinates — perfect
                // for translating y → row index. Defer the actual
                // selection update to the next layout pass via a
                // signal so we don't need handler-side row data.
                state_for_handler.pending_tree_click_y.set(Some(position.y));
            });
        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        // Snapshot tree once per layout pass (cheap — a flat traversal).
        let mut rows: Vec<TreeRow> = Vec::new();
        if let Some(arena) = ctx.arena() {
            let exclude = self.state.shell_root_id.get();
            for &root in arena.roots().iter() {
                if Some(root) == exclude {
                    continue;
                }
                push_subtree(arena, root, 0, exclude, &mut rows);
            }
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
    exclude: Option<WidgetId>,
    out: &mut Vec<TreeRow>,
) {
    if Some(id) == exclude {
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
        push_subtree(arena, child, depth + 1, exclude, out);
    }
}
