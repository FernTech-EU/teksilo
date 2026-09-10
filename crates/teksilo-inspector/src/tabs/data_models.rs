// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Data Models tab — registered models from `teksilo_data::debug_registry`.
//!
//! Lists every registered model's name + kind + len. Click a row to
//! select that model; the dump area below shows the selected model's
//! `debug_dump` output. With nothing selected, falls back to the most
//! recently registered model.

use std::cell::RefCell;

use teksilo_canvas::{Canvas, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{Color, CornerRadius, TextRole};

use crate::state::InspectorState;
use crate::tabs::{ROW_PADDING_X, row_height};

/// The header is exactly one row tall — so it, the rows and the click
/// coordinate that indexes them all move together with the density.
fn header_height(tokens: &teksilo_tokens::InputTokens) -> f32 {
    row_height(tokens)
}
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
    state: InspectorState,
    rows: RefCell<Vec<ModelRow>>,
    /// Snapshot of the selected (or fallback last) model's dump output.
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
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Repaint on selection change; relayout when the panel opens
        // (initial mount).
        let self_id = ctx.self_id();
        self.state.selected_model_index.bind_to(
            self_id,
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        self.state
            .open
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);

        let state_for_handler = self.state.clone();
        let handlers = HandlerSet::new()
            .focusable(true)
            .on_tap(move |event, _ctx| {
                state_for_handler
                    .pending_models_click_y
                    .set(Some(event.position.y));
            });
        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        // Rows are pressed here, so their height follows the density; the
        // header offset and the dump lines ride the same number, because the
        // paint is one cumulative y walk and a mixed walk is exactly how a
        // press lands on the wrong row.
        let rh = row_height(&ctx.theme.input);
        let header = header_height(&ctx.theme.input);
        let snapshot = teksilo_data::debug_registry::snapshot();
        let mut rows: Vec<ModelRow> = Vec::with_capacity(snapshot.len());
        for (name, model) in &snapshot {
            rows.push(ModelRow {
                name: name.clone(),
                kind: model.kind(),
                len: model.len(),
            });
        }

        // Resolve any deferred row click. Click coordinate space:
        // y=0..header is the header (no selection); below is
        // rows[0..rows.len()].
        if let Some(y) = self.state.pending_models_click_y.get() {
            self.state.pending_models_click_y.set(None);
            if y >= header {
                let idx = ((y - header) / rh).floor() as usize;
                if idx < snapshot.len() {
                    let current = self.state.selected_model_index.get();
                    if current == Some(idx) {
                        // Toggle off — fall back to "last registered".
                        self.state.selected_model_index.set(None);
                    } else {
                        self.state.selected_model_index.set(Some(idx));
                    }
                }
            }
        }

        // Decide which model's contents to dump. Explicit selection
        // wins; otherwise the most recent registration (matches the
        // behavior shipped in slice 4).
        let mut dump = String::new();
        let chosen = self
            .state
            .selected_model_index
            .get()
            .filter(|i| *i < snapshot.len())
            .or_else(|| snapshot.len().checked_sub(1));
        if let Some(idx) = chosen
            && let Some((_, model)) = snapshot.get(idx)
        {
            model.debug_dump(&mut dump);
        }

        let row_count = rows.len().max(1);
        let dump_lines = dump.lines().take(DUMP_PREVIEW_LINES).count().max(1);
        let height = header + (row_count as f32) * rh + rh + (dump_lines as f32) * rh;

        *self.rows.borrow_mut() = rows;
        *self.dump.borrow_mut() = dump;
        proposal.resolve(0.0, height).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let rh = row_height(&ctx.theme.input);
        let header = header_height(&ctx.theme.input);
        let theme = ctx.theme;
        let style = &theme.typography.body;
        let mono = &theme.typography.mono;
        let primary = TextRole::Primary.resolve(&theme.colors);
        let secondary = TextRole::Secondary.resolve(&theme.colors);

        // Header row: "name" "kind" "len".
        let mut y = bounds.y + 2.0;
        canvas.draw_text(
            "name",
            Rect::new(bounds.x + ROW_PADDING_X, y, NAME_COL_WIDTH, rh),
            style,
            secondary,
        );
        canvas.draw_text(
            "kind",
            Rect::new(
                bounds.x + ROW_PADDING_X + NAME_COL_WIDTH,
                y,
                KIND_COL_WIDTH,
                rh,
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
                rh,
            ),
            style,
            secondary,
        );
        y += header;

        let rows = self.rows.borrow();
        if rows.is_empty() {
            canvas.draw_text(
                "(no models registered — call `.debug_named(\"name\")` on a ListModel)",
                Rect::new(bounds.x + ROW_PADDING_X, y + 2.0, bounds.width, rh),
                style,
                secondary,
            );
            return;
        }

        let selected_idx = self.state.selected_model_index.get();
        // Effective "shown in dump" highlight — covers the implicit
        // last-registered fallback when no explicit selection exists.
        let effective_idx = selected_idx.or_else(|| rows.len().checked_sub(1));
        let selection_bg = Color::from_rgba(0.13, 0.55, 1.0, 0.18);
        let fallback_bg = Color::from_rgba(0.13, 0.55, 1.0, 0.08);

        for (i, row) in rows.iter().enumerate() {
            let row_rect = Rect::new(bounds.x, y, bounds.width, rh);
            if Some(i) == selected_idx {
                canvas.fill_rounded_rect(row_rect, CornerRadius::ZERO, selection_bg);
            } else if Some(i) == effective_idx {
                canvas.fill_rounded_rect(row_rect, CornerRadius::ZERO, fallback_bg);
            }
            canvas.draw_text(
                &row.name,
                Rect::new(bounds.x + ROW_PADDING_X, y + 2.0, NAME_COL_WIDTH, rh),
                style,
                primary,
            );
            canvas.draw_text(
                row.kind,
                Rect::new(
                    bounds.x + ROW_PADDING_X + NAME_COL_WIDTH,
                    y + 2.0,
                    KIND_COL_WIDTH,
                    rh,
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
                    rh,
                ),
                style,
                primary,
            );
            y += rh;
        }

        // Separator + "dump" label. Distinguish explicit vs fallback
        // selection in the label so the dim row tint isn't a mystery.
        y += rh * 0.25;
        let dump_label = match selected_idx {
            Some(i) => match rows.get(i) {
                Some(row) => format!("dump ({}):", row.name),
                None => "dump:".to_string(),
            },
            None => "dump (most recent registration):".to_string(),
        };
        canvas.draw_text(
            &dump_label,
            Rect::new(bounds.x + ROW_PADDING_X, y, bounds.width, rh),
            style,
            secondary,
        );
        y += rh;

        // Dump preview — monospace, line-wrapped at lines, cap.
        let dump = self.dump.borrow();
        for line in dump.lines().take(DUMP_PREVIEW_LINES) {
            canvas.draw_text(
                line,
                Rect::new(bounds.x + ROW_PADDING_X, y, bounds.width, rh),
                mono,
                primary,
            );
            y += rh;
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // The registered models, one `name kind len` line each. The dump below
        // them is not repeated here: it is the selected model's own Debug output,
        // which a screen reader reaches through the Copy button in Properties.
        // The house convention for painted text (`TextWidget` does exactly
        // this): one `Role::Label` whose name is what is on the screen.
        // Without it the tab is a blank rectangle to a screen reader.
        builder.set_role(teksilo_core::accesskit::Role::Label);
        builder.set_name(
            self.rows
                .borrow()
                .iter()
                .map(|row| format!("{}  {}  {}", row.name, row.kind, row.len))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
}
