//! Theme tab — preset switcher (Light / Dark) + JSON Export/Import +
//! read-only color list.
//!
//! Per-color editing waits on a real `ColorPicker` widget (Phase B in
//! `widgets-plan.md`).

use fern_canvas::{Canvas, Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_platform::ClipboardHandle;
use fern_tokens::{Color, ColorTokens, CornerRadius, TextRole, Theme};
use fern_widgets::Button;
use fern_widgets::primitives::{HStack, Padding, VStack};

use crate::state::InspectorState;
use crate::tabs::{ROW_HEIGHT, ROW_PADDING_X};

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
    state: InspectorState,
    root_child_id: Option<WidgetId>,
}

impl ThemeTab {
    pub fn new(state: InspectorState) -> Self {
        Self {
            state,
            root_child_id: None,
        }
    }
}

impl std::fmt::Debug for ThemeTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThemeTab").finish()
    }
}

impl Widget for ThemeTab {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let _ = &self.state;
        let theme_sig = ctx.theme_signal();

        // Preset buttons.
        let light_btn = Button::new_literal("Light")
            .on_activate_fn(|c| c.set_theme(Theme::light_default()));
        let dark_btn =
            Button::new_literal("Dark").on_activate_fn(|c| c.set_theme(Theme::dark_default()));

        // Export → JSON dump → clipboard.
        let theme_for_export = theme_sig.clone();
        let export_btn = Button::new_literal("Export")
            .on_activate_fn(move |c| {
                if let Some(cb) = c.app_state::<ClipboardHandle>() {
                    let theme = theme_for_export.get();
                    if let Ok(json) = serde_json::to_string_pretty(&theme) {
                        let _ = cb.set_text(&json);
                    }
                }
            });

        // Import ← clipboard JSON → set_theme. Silently ignores parse
        // errors (a debug tool — the developer can check the clipboard
        // and try again).
        let import_btn = Button::new_literal("Import").on_activate_fn(|c| {
            let Some(cb) = c.app_state::<ClipboardHandle>() else {
                return;
            };
            let Ok(text) = cb.get_text() else {
                return;
            };
            if let Ok(theme) = serde_json::from_str::<Theme>(&text) {
                c.set_theme(theme);
            }
        });

        let toolbar = Padding::symmetric(2.0, 4.0).child(
            HStack::new()
                .spacing(6.0)
                .child(light_btn)
                .child(dark_btn)
                .child(export_btn)
                .child(import_btn),
        );

        let color_list = ColorList::new();
        let root = ctx.add(VStack::new().spacing(2.0).child(toolbar).child(color_list));
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

#[derive(Default)]
struct ColorList;

impl ColorList {
    fn new() -> Self {
        Self
    }
}

impl std::fmt::Debug for ColorList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ColorList").finish()
    }
}

impl Widget for ColorList {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        let height = (SHOWN_COLORS.len() as f32) * ROW_HEIGHT;
        proposal.resolve(0.0, height).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let theme = ctx.theme;
        let style = &theme.typography.body;
        let primary = TextRole::Primary.resolve(&theme.colors);
        let secondary = TextRole::Secondary.resolve(&theme.colors);
        let border = theme.colors.border;

        for (i, (name, getter)) in SHOWN_COLORS.iter().enumerate() {
            let y = bounds.y + (i as f32) * ROW_HEIGHT;
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
