// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Overlays tab — active overlays in the OverlayManager.

use std::cell::RefCell;

use bastyde_canvas::{Canvas, Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::arena::WidgetArena;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::TextRole;

use crate::state::InspectorState;
use crate::tabs::{ROW_HEIGHT, ROW_PADDING_X, last_segment};

#[derive(Clone, Debug)]
struct OverlayRow {
    overlay_id: String,
    content_label: String,
    anchor_label: String,
}

pub(crate) struct OverlaysTab {
    state: InspectorState,
    rows: RefCell<Vec<OverlayRow>>,
}

impl OverlaysTab {
    pub fn new(state: InspectorState) -> Self {
        Self {
            state,
            rows: RefCell::new(Vec::new()),
        }
    }
}

impl std::fmt::Debug for OverlaysTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OverlaysTab").finish()
    }
}

impl Widget for OverlaysTab {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Reactive: relayout on every overlay-manager version bump
        // (overlay shown / dismissed). Bridged from
        // `tree.overlay_manager().version()`.
        let self_id = ctx.self_id();
        self.state
            .overlay_version
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let mut rows: Vec<OverlayRow> = Vec::new();
        if let (Some(arena), Some(om)) = (ctx.arena(), ctx.overlay_manager()) {
            for overlay_id in om.active_ids() {
                let content_id = om.active_content_ids().into_iter().find(|cid| {
                    // Map overlay id → content id by scanning active list.
                    // The OverlayManager exposes `active_ids` and
                    // `active_content_ids` separately; the lists are
                    // index-aligned in practice, but we don't depend
                    // on that here.
                    om.find_by_content(*cid) == Some(overlay_id)
                });
                let content_label = content_id
                    .and_then(|id| widget_label(arena, id))
                    .unwrap_or_else(|| "(unknown)".to_string());
                let anchor_label = om
                    .anchor_for(overlay_id)
                    .and_then(|id| widget_label(arena, id))
                    .unwrap_or_else(|| "—".to_string());
                rows.push(OverlayRow {
                    overlay_id: format!("{:?}", overlay_id),
                    content_label,
                    anchor_label,
                });
            }
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
            canvas.draw_text("(no active overlays)", text_rect, style, secondary);
            return;
        }

        for (i, row) in rows.iter().enumerate() {
            let y = bounds.y + (i as f32) * ROW_HEIGHT + 2.0;
            let line = format!(
                "{}  content={}  anchor={}",
                row.overlay_id, row.content_label, row.anchor_label
            );
            let text_rect = Rect::new(bounds.x + ROW_PADDING_X, y, bounds.width, ROW_HEIGHT);
            canvas.draw_text(&line, text_rect, style, primary);
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

fn widget_label(arena: &WidgetArena, id: WidgetId) -> Option<String> {
    let node = arena.get(id)?;
    Some(format!(
        "{}({:?})",
        last_segment(node.widget.type_name()),
        id
    ))
}
