//! Shortcuts tab — registered shortcuts and their effective keystrokes.

use std::cell::RefCell;

use fern_canvas::{Canvas, Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use fern_core::widget_id::WidgetId;
use fern_tokens::TextRole;

use crate::state::InspectorState;
use crate::tabs::{ROW_HEIGHT, ROW_PADDING_X};

const KEY_COLUMN_WIDTH: f32 = 220.0;

#[derive(Clone, Debug)]
struct ShortcutRow {
    id: String,
    keystroke: String,
    framework_reserved: bool,
}

pub(crate) struct ShortcutsTab {
    #[allow(dead_code)]
    state: InspectorState,
    rows: RefCell<Vec<ShortcutRow>>,
}

impl ShortcutsTab {
    pub fn new(state: InspectorState) -> Self {
        Self {
            state,
            rows: RefCell::new(Vec::new()),
        }
    }
}

impl std::fmt::Debug for ShortcutsTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShortcutsTab").finish()
    }
}

impl Widget for ShortcutsTab {
    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> {
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let mut rows: Vec<ShortcutRow> = Vec::new();
        if let Some(reg) = ctx.shortcut_registry() {
            for eff in reg.iter_effective() {
                let id = eff.shortcut.id.to_string();
                let primary = eff
                    .primary
                    .map(|k| k.to_string())
                    .unwrap_or_else(|| "—".to_string());
                rows.push(ShortcutRow {
                    framework_reserved: id.starts_with("__"),
                    id,
                    keystroke: primary,
                });
            }
        }
        rows.sort_by(|a, b| a.id.cmp(&b.id));
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
        let disabled = TextRole::Disabled.resolve(&theme.colors);

        let rows = self.rows.borrow();
        if rows.is_empty() {
            let text_rect = Rect::new(
                bounds.x + ROW_PADDING_X,
                bounds.y + 2.0,
                bounds.width,
                ROW_HEIGHT,
            );
            canvas.draw_text("(no shortcuts registered)", text_rect, style, secondary);
            return;
        }

        for (i, row) in rows.iter().enumerate() {
            let y = bounds.y + (i as f32) * ROW_HEIGHT + 2.0;
            let id_rect = Rect::new(
                bounds.x + ROW_PADDING_X,
                y,
                KEY_COLUMN_WIDTH,
                ROW_HEIGHT,
            );
            let key_x = bounds.x + ROW_PADDING_X + KEY_COLUMN_WIDTH + ROW_PADDING_X;
            let key_rect = Rect::new(
                key_x,
                y,
                (bounds.x + bounds.width - key_x).max(0.0),
                ROW_HEIGHT,
            );
            let id_color = if row.framework_reserved {
                disabled
            } else {
                primary
            };
            canvas.draw_text(&row.id, id_rect, style, id_color);
            canvas.draw_text(&row.keystroke, key_rect, style, secondary);
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}
