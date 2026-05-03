//! Data Models tab — registered models from `fern_data::debug_registry`.
//!
//! For Slice 4, lists every registered model's name + kind + len.
//! Per-model `debug_dump` output is shown for the most recent
//! registration; future slices add row selection.

use std::cell::RefCell;

use fern_canvas::{Canvas, Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use fern_core::widget_id::WidgetId;
use fern_tokens::TextRole;

use crate::state::InspectorState;
use crate::tabs::{ROW_HEIGHT, ROW_PADDING_X};

const HEADER_HEIGHT: f32 = ROW_HEIGHT;
const NAME_COL_WIDTH: f32 = 160.0;
const KIND_COL_WIDTH: f32 = 100.0;
const LEN_COL_WIDTH: f32 = 60.0;
const DUMP_PREVIEW_LINES: usize = 12;

#[derive(Clone, Debug)]
struct ModelRow {
    name: String,
    kind: &'static str,
    len: usize,
}

pub(crate) struct DataModelsTab {
    #[allow(dead_code)]
    state: InspectorState,
    rows: RefCell<Vec<ModelRow>>,
    /// Snapshot of the most-recent registration's `debug_dump` output.
    dump: RefCell<String>,
}

impl DataModelsTab {
    pub fn new(state: InspectorState) -> Self {
        Self {
            state,
            rows: RefCell::new(Vec::new()),
            dump: RefCell::new(String::new()),
        }
    }
}

impl std::fmt::Debug for DataModelsTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataModelsTab").finish()
    }
}

impl Widget for DataModelsTab {
    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> {
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        let snapshot = fern_data::debug_registry::snapshot();
        let mut rows: Vec<ModelRow> = Vec::with_capacity(snapshot.len());
        let mut dump = String::new();
        for (name, model) in &snapshot {
            rows.push(ModelRow {
                name: name.clone(),
                kind: model.kind(),
                len: model.len(),
            });
        }
        // Dump the last registered model into the preview area —
        // typically the most recently created one. Future slice 5
        // adds click-to-select per row.
        if let Some((_, last)) = snapshot.last() {
            last.debug_dump(&mut dump);
        }

        let row_count = rows.len().max(1);
        let dump_lines = dump.lines().take(DUMP_PREVIEW_LINES).count().max(1);
        let height = HEADER_HEIGHT
            + (row_count as f32) * ROW_HEIGHT
            + ROW_HEIGHT
            + (dump_lines as f32) * ROW_HEIGHT;

        *self.rows.borrow_mut() = rows;
        *self.dump.borrow_mut() = dump;
        proposal.resolve(0.0, height).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let theme = ctx.theme;
        let style = &theme.typography.body;
        let mono = &theme.typography.mono;
        let primary = TextRole::Primary.resolve(&theme.colors);
        let secondary = TextRole::Secondary.resolve(&theme.colors);

        // Header row: "name" "kind" "len".
        let mut y = bounds.y + 2.0;
        canvas.draw_text(
            "name",
            Rect::new(bounds.x + ROW_PADDING_X, y, NAME_COL_WIDTH, ROW_HEIGHT),
            style,
            secondary,
        );
        canvas.draw_text(
            "kind",
            Rect::new(
                bounds.x + ROW_PADDING_X + NAME_COL_WIDTH,
                y,
                KIND_COL_WIDTH,
                ROW_HEIGHT,
            ),
            style,
            secondary,
        );
        canvas.draw_text(
            "len",
            Rect::new(
                bounds.x + ROW_PADDING_X + NAME_COL_WIDTH + KIND_COL_WIDTH,
                y,
                LEN_COL_WIDTH,
                ROW_HEIGHT,
            ),
            style,
            secondary,
        );
        y += HEADER_HEIGHT;

        let rows = self.rows.borrow();
        if rows.is_empty() {
            canvas.draw_text(
                "(no models registered — call `.debug_named(\"name\")` on a ListModel)",
                Rect::new(bounds.x + ROW_PADDING_X, y + 2.0, bounds.width, ROW_HEIGHT),
                style,
                secondary,
            );
            return;
        }

        for row in rows.iter() {
            canvas.draw_text(
                &row.name,
                Rect::new(bounds.x + ROW_PADDING_X, y + 2.0, NAME_COL_WIDTH, ROW_HEIGHT),
                style,
                primary,
            );
            canvas.draw_text(
                row.kind,
                Rect::new(
                    bounds.x + ROW_PADDING_X + NAME_COL_WIDTH,
                    y + 2.0,
                    KIND_COL_WIDTH,
                    ROW_HEIGHT,
                ),
                style,
                secondary,
            );
            canvas.draw_text(
                &row.len.to_string(),
                Rect::new(
                    bounds.x + ROW_PADDING_X + NAME_COL_WIDTH + KIND_COL_WIDTH,
                    y + 2.0,
                    LEN_COL_WIDTH,
                    ROW_HEIGHT,
                ),
                style,
                primary,
            );
            y += ROW_HEIGHT;
        }

        // Separator + "dump" label
        y += ROW_HEIGHT * 0.25;
        canvas.draw_text(
            "dump (most recent registration):",
            Rect::new(bounds.x + ROW_PADDING_X, y, bounds.width, ROW_HEIGHT),
            style,
            secondary,
        );
        y += ROW_HEIGHT;

        // Dump preview — monospace, line-wrapped at lines, cap.
        let dump = self.dump.borrow();
        for line in dump.lines().take(DUMP_PREVIEW_LINES) {
            canvas.draw_text(
                line,
                Rect::new(bounds.x + ROW_PADDING_X, y, bounds.width, ROW_HEIGHT),
                mono,
                primary,
            );
            y += ROW_HEIGHT;
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}
