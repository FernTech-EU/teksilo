//! Properties tab — key/value rows for the selected widget.

use std::cell::RefCell;

use fern_canvas::{Canvas, Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::arena::WidgetArena;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use fern_core::widget_id::WidgetId;
use fern_tokens::TextRole;

use crate::state::InspectorState;
use crate::tabs::{ROW_HEIGHT, ROW_PADDING_X, last_segment};

const KEY_COLUMN_WIDTH: f32 = 140.0;

#[derive(Clone, Debug)]
struct KvRow {
    key: String,
    value: String,
}

pub(crate) struct PropertiesTab {
    state: InspectorState,
    rows: RefCell<Vec<KvRow>>,
}

impl PropertiesTab {
    pub fn new(state: InspectorState) -> Self {
        Self {
            state,
            rows: RefCell::new(Vec::new()),
        }
    }
}

impl std::fmt::Debug for PropertiesTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PropertiesTab").finish()
    }
}

impl Widget for PropertiesTab {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // Re-layout (and therefore re-snapshot) on selection change.
        self.state
            .selected_id
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let mut rows: Vec<KvRow> = Vec::new();
        if let (Some(arena), Some(id)) = (ctx.arena(), self.state.selected_id.get()) {
            collect_properties(arena, id, &mut rows);
        }
        let height = rows.len() as f32 * ROW_HEIGHT;
        *self.rows.borrow_mut() = rows;
        proposal.resolve(0.0, height).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let theme = ctx.theme;
        let style = &theme.typography.body;
        let key_color = TextRole::Secondary.resolve(&theme.colors);
        let value_color = TextRole::Primary.resolve(&theme.colors);

        for (i, row) in self.rows.borrow().iter().enumerate() {
            let y = bounds.y + (i as f32) * ROW_HEIGHT + 2.0;
            let key_rect =
                Rect::new(bounds.x + ROW_PADDING_X, y, KEY_COLUMN_WIDTH, ROW_HEIGHT);
            let value_x = bounds.x + ROW_PADDING_X + KEY_COLUMN_WIDTH + ROW_PADDING_X;
            let value_rect = Rect::new(
                value_x,
                y,
                (bounds.x + bounds.width - value_x).max(0.0),
                ROW_HEIGHT,
            );
            canvas.draw_text(&row.key, key_rect, style, key_color);
            canvas.draw_text(&row.value, value_rect, style, value_color);
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

fn collect_properties(arena: &WidgetArena, id: WidgetId, out: &mut Vec<KvRow>) {
    let Some(node) = arena.get(id) else {
        return;
    };
    let push = |key: &str, value: String, out: &mut Vec<KvRow>| {
        out.push(KvRow {
            key: key.to_string(),
            value,
        });
    };
    push("type", node.widget.type_name().to_string(), out);
    push("short_type", last_segment(node.widget.type_name()).to_string(), out);
    push("widget_id", format!("{:?}", id), out);
    let bounds = arena.bounds(id);
    push(
        "bounds",
        format!(
            "x={:.1} y={:.1} w={:.1} h={:.1}",
            bounds.x, bounds.y, bounds.width, bounds.height
        ),
        out,
    );
    push("clips_children", node.clips_children.to_string(), out);
    push("event_pass_through", node.event_pass_through.to_string(), out);
    if let Some(parent) = node.parent {
        push("parent", format!("{:?}", parent), out);
    }
    push("children", format!("{}", node.children.len()), out);
    push(
        "needs_layout",
        node.dirty.needs_layout.to_string(),
        out,
    );
    push("needs_paint", node.dirty.needs_paint.to_string(), out);
    push(
        "needs_rebuild",
        node.dirty.needs_rebuild.to_string(),
        out,
    );
    push("activation", format!("{:?}", node.activation), out);
}
