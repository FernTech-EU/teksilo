// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Focus tab — current focused widget + ancestor chain.

use std::cell::RefCell;

use teksilo_canvas::{Canvas, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::arena::WidgetArena;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::TextRole;

use crate::state::InspectorState;
use crate::tabs::{ROW_HEIGHT, ROW_INDENT_PX, ROW_PADDING_X, last_segment};

#[derive(Clone, Debug)]
struct FocusRow {
    depth: u32,
    label: String,
    is_focused: bool,
}

/// Snapshot-driven view of the focus chain. Walks from the focused
/// widget back to the root via parent links.
pub(crate) struct FocusTab {
    state: InspectorState,
    rows: RefCell<Vec<FocusRow>>,
}

impl FocusTab {
    pub fn new(state: InspectorState) -> Self {
        Self {
            state,
            rows: RefCell::new(Vec::new()),
        }
    }
}

impl std::fmt::Debug for FocusTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusTab").finish()
    }
}

impl Widget for FocusTab {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Reactive: relayout whenever the framework's focused widget
        // changes. The bridge in `state::install` mirrors
        // `tree.focused_signal()` into `state.focus_id`.
        let self_id = ctx.self_id();
        self.state
            .focus_id
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let mut rows: Vec<FocusRow> = Vec::new();
        if let (Some(arena), Some(focused)) = (ctx.arena(), ctx.focused()) {
            collect_focus_chain(arena, focused, &mut rows);
        }
        let height = if rows.is_empty() {
            ROW_HEIGHT
        } else {
            rows.len() as f32 * ROW_HEIGHT
        };
        *self.rows.borrow_mut() = rows;
        proposal.resolve(0.0, height).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let theme = ctx.theme;
        let style = &theme.typography.body;
        let primary = TextRole::Primary.resolve(&theme.colors);
        let secondary = TextRole::Secondary.resolve(&theme.colors);

        let rows = self.rows.borrow();
        if rows.is_empty() {
            let text_rect = Rect::new(
                bounds.x + ROW_PADDING_X,
                bounds.y + 2.0,
                bounds.width,
                ROW_HEIGHT,
            );
            canvas.draw_text("(no focused widget)", text_rect, style, secondary);
            return;
        }

        for (i, row) in rows.iter().enumerate() {
            let y = bounds.y + (i as f32) * ROW_HEIGHT + 2.0;
            let x = bounds.x + ROW_PADDING_X + (row.depth as f32) * ROW_INDENT_PX;
            let text_rect = Rect::new(x, y, (bounds.width - (x - bounds.x)).max(0.0), ROW_HEIGHT);
            let color = if row.is_focused { primary } else { secondary };
            canvas.draw_text(&row.label, text_rect, style, color);
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

fn collect_focus_chain(arena: &WidgetArena, focused: WidgetId, out: &mut Vec<FocusRow>) {
    // Walk up parent → root, collecting; then reverse to root → focused.
    let mut chain: Vec<WidgetId> = Vec::new();
    let mut cur = Some(focused);
    while let Some(id) = cur {
        chain.push(id);
        cur = arena.parent(id);
    }
    chain.reverse();

    for (depth, id) in chain.iter().enumerate() {
        if let Some(node) = arena.get(*id) {
            out.push(FocusRow {
                depth: depth as u32,
                label: format!("{} ({:?})", last_segment(node.widget.type_name()), id),
                is_focused: *id == focused,
            });
        }
    }
}
