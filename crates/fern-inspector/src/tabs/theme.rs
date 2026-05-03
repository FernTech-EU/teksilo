//! Theme tab — preset switcher (Light / Dark) + read-only color list.
//!
//! Slice 4 ships preset switching. Per-color editing is queued for a
//! later slice once a real `ColorPicker` widget lands (currently
//! Phase B in `widgets-plan.md`).

use fern_canvas::{Canvas, Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, ColorTokens, CornerRadius, TextRole, Theme};

use crate::state::InspectorState;
use crate::tabs::{ROW_HEIGHT, ROW_PADDING_X};

const PRESET_BUTTON_WIDTH: f32 = 70.0;
const PRESET_ROW_HEIGHT: f32 = 28.0;
const SWATCH_WIDTH: f32 = 28.0;
const NAME_COLUMN_WIDTH: f32 = 180.0;

/// Curated subset of color tokens we surface in the inspector. The
/// full `ColorTokens` struct has 50+ fields; showing them all would
/// drown the panel. This list covers the colors a developer typically
/// reaches for first.
const SHOWN_COLORS: &[(&str, fn(&ColorTokens) -> Color)] = &[
    ("accent", |t| t.accent),
    ("accent_hover", |t| t.accent_hover),
    ("surface_main", |t| t.surface_main),
    ("surface_content", |t| t.surface_content),
    ("surface_hover", |t| t.surface_hover),
    ("surface_selected", |t| t.surface_selected),
    ("text_primary", |t| t.text_primary),
    ("text_secondary", |t| t.text_secondary),
    ("text_disabled", |t| t.text_disabled),
    ("text_link", |t| t.text_link),
    ("border", |t| t.border),
    ("border_focused", |t| t.border_focused),
    ("focus_ring", |t| t.focus_ring),
    ("status_error_fg", |t| t.status_error_fg),
    ("status_warning_fg", |t| t.status_warning_fg),
    ("status_success_fg", |t| t.status_success_fg),
];

pub(crate) struct ThemeTab {
    #[allow(dead_code)]
    state: InspectorState,
}

impl ThemeTab {
    pub fn new(state: InspectorState) -> Self {
        Self { state }
    }
}

impl std::fmt::Debug for ThemeTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThemeTab").finish()
    }
}

impl Widget for ThemeTab {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Click handlers: top-row layout reserves a Light/Dark button
        // pair; we translate widget-local clicks to a preset choice
        // and call `set_theme` from the handler closure.
        let handlers = HandlerSet::new()
            .focusable(true)
            .on_tap(move |position, event_ctx| {
                if position.y > PRESET_ROW_HEIGHT {
                    return;
                }
                // Two preset buttons starting at x=ROW_PADDING_X with
                // PRESET_BUTTON_WIDTH each.
                let bx = position.x - ROW_PADDING_X;
                if (0.0..PRESET_BUTTON_WIDTH).contains(&bx) {
                    event_ctx.set_theme(Theme::light_default());
                } else if (PRESET_BUTTON_WIDTH..PRESET_BUTTON_WIDTH * 2.0).contains(&bx) {
                    event_ctx.set_theme(Theme::dark_default());
                }
            });
        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        let height = PRESET_ROW_HEIGHT + (SHOWN_COLORS.len() as f32) * ROW_HEIGHT;
        proposal.resolve(0.0, height).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let theme = ctx.theme;
        let style = &theme.typography.body;
        let primary = TextRole::Primary.resolve(&theme.colors);
        let secondary = TextRole::Secondary.resolve(&theme.colors);
        let border = theme.colors.border;

        // Preset row: [Light] [Dark]
        let preset_y = bounds.y + 2.0;
        let light_rect = Rect::new(
            bounds.x + ROW_PADDING_X,
            preset_y,
            PRESET_BUTTON_WIDTH,
            PRESET_ROW_HEIGHT - 4.0,
        );
        let dark_rect = Rect::new(
            bounds.x + ROW_PADDING_X + PRESET_BUTTON_WIDTH,
            preset_y,
            PRESET_BUTTON_WIDTH,
            PRESET_ROW_HEIGHT - 4.0,
        );
        canvas.stroke_rounded_rect(light_rect, CornerRadius::uniform(4.0), border, 1.0);
        canvas.stroke_rounded_rect(dark_rect, CornerRadius::uniform(4.0), border, 1.0);
        canvas.draw_text("Light", inset(light_rect, 6.0, 4.0), style, primary);
        canvas.draw_text("Dark", inset(dark_rect, 6.0, 4.0), style, primary);

        // Color rows.
        let list_top = bounds.y + PRESET_ROW_HEIGHT;
        for (i, (name, getter)) in SHOWN_COLORS.iter().enumerate() {
            let y = list_top + (i as f32) * ROW_HEIGHT;
            let swatch = Rect::new(
                bounds.x + ROW_PADDING_X,
                y + 2.0,
                SWATCH_WIDTH,
                ROW_HEIGHT - 4.0,
            );
            let color = getter(&theme.colors);
            canvas.fill_rounded_rect(swatch, CornerRadius::uniform(2.0), color);
            canvas.stroke_rounded_rect(swatch, CornerRadius::uniform(2.0), border, 1.0);

            let name_x = bounds.x + ROW_PADDING_X + SWATCH_WIDTH + ROW_PADDING_X;
            let name_rect = Rect::new(name_x, y + 2.0, NAME_COLUMN_WIDTH, ROW_HEIGHT);
            canvas.draw_text(name, name_rect, style, primary);

            let hex = format_hex(color);
            let hex_x = name_x + NAME_COLUMN_WIDTH;
            let hex_rect = Rect::new(
                hex_x,
                y + 2.0,
                (bounds.x + bounds.width - hex_x).max(0.0),
                ROW_HEIGHT,
            );
            canvas.draw_text(&hex, hex_rect, style, secondary);
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

fn inset(r: Rect, dx: f32, dy: f32) -> Rect {
    Rect::new(
        r.x + dx,
        r.y + dy,
        (r.width - dx * 2.0).max(0.0),
        (r.height - dy * 2.0).max(0.0),
    )
}

fn format_hex(c: Color) -> String {
    let r = (c.r() * 255.0).round().clamp(0.0, 255.0) as u8;
    let g = (c.g() * 255.0).round().clamp(0.0, 255.0) as u8;
    let b = (c.b() * 255.0).round().clamp(0.0, 255.0) as u8;
    let a = (c.a() * 255.0).round().clamp(0.0, 255.0) as u8;
    if a == 255 {
        format!("#{:02X}{:02X}{:02X}", r, g, b)
    } else {
        format!("#{:02X}{:02X}{:02X}{:02X}", r, g, b, a)
    }
}
