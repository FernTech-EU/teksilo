// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Accessibility tab — AccessKit info for the selected widget.

use std::cell::RefCell;

use teksilo_canvas::{Canvas, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::TextRole;

use crate::state::InspectorState;
use crate::tabs::{ROW_HEIGHT, ROW_PADDING_X};

const KEY_COLUMN_WIDTH: f32 = 140.0;

#[derive(Clone, Debug)]
struct KvRow {
    key: String,
    value: String,
}

pub(crate) struct A11yTab {
    state: InspectorState,
    rows: RefCell<Vec<KvRow>>,
}

impl A11yTab {
    pub fn new(state: InspectorState) -> Self {
        Self {
            state,
            rows: RefCell::new(Vec::new()),
        }
    }
}

impl std::fmt::Debug for A11yTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("A11yTab").finish()
    }
}

impl Widget for A11yTab {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        self.state
            .selected_id
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let mut rows: Vec<KvRow> = Vec::new();
        if let (Some(arena), Some(id)) = (ctx.arena(), self.state.selected_id.get())
            && arena.is_active(id)
            && let Some(node) = arena.get(id)
        {
            let mut builder = AccessNodeBuilder::new();
            node.widget.accessibility(&mut builder);
            push_from_builder(&builder, &mut rows);
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
            let key_rect = Rect::new(bounds.x + ROW_PADDING_X, y, KEY_COLUMN_WIDTH, ROW_HEIGHT);
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

fn push_from_builder(builder: &AccessNodeBuilder, out: &mut Vec<KvRow>) {
    let push = |key: &str, value: String, out: &mut Vec<KvRow>| {
        out.push(KvRow {
            key: key.to_string(),
            value,
        });
    };
    push("role", format!("{:?}", builder.role()), out);
    if let Some(name) = builder.name() {
        push("name", name.to_string(), out);
    }
    if let Some(value) = builder.value() {
        push("value", value.to_string(), out);
    }
    let actions = builder.actions();
    if !actions.is_empty() {
        let names: Vec<String> = actions.iter().map(|a| format!("{:?}", a)).collect();
        push("actions", names.join(", "), out);
    }
    if let Some(t) = builder.toggled() {
        push("toggled", t.to_string(), out);
    }
    if let Some(e) = builder.expanded() {
        push("expanded", e.to_string(), out);
    }
    if let Some(s) = builder.selected() {
        push("selected", s.to_string(), out);
    }
    if builder.is_hidden() {
        push("hidden", "true".to_string(), out);
    }
}
