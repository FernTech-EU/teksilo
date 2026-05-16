//! Default `ColorPickerStyle` impl + the picker's design tokens.
//!
//! All `theme.components.color_picker` constants relocate here as
//! `pub const`s — Stage D2 of the group-5 styling migration. The four
//! functional renderers (`HsvCanvas`, `HueStrip`, `AlphaStrip`,
//! `ColorSwatch`) read these directly when they need dimensions for
//! their colour-space rendering; they stay `pub(crate)` per principle 6.
//!
//! `RecipeColorPickerStyle::make_body` ports the IntUI layout exactly:
//! one of three column / row arrangements per `ColorPickerLayout`,
//! wrapped in a `Panel` surface (1 dp border + corner radius +
//! `PADDING` interior).

use fern_core::build_context::BuildContext;
use fern_core::styles::{ColorPickerLayout, ColorPickerStyle, ColorPickerStyleConfig};
use fern_core::widget_id::WidgetId;
use fern_tokens::Color;

use crate::panel::Panel;
use crate::primitives::{HStack, VStack};

// ─── IntUI design tokens for ColorPicker ───────────────────────────
// Relocated from `theme.components.color_picker` in Stage D2.

// HSV (saturation × value) canvas dimensions.
pub const CANVAS_WIDTH: f32 = 224.0;
pub const CANVAS_HEIGHT: f32 = 192.0;
pub const CANVAS_CORNER_RADIUS: f32 = 4.0;

// 1D hue and alpha strips (vertical orientation in default layouts).
pub const STRIP_THICKNESS: f32 = 14.0;
pub const STRIP_LENGTH: f32 = 192.0;
pub const STRIP_CORNER_RADIUS: f32 = 4.0;

// HSV-canvas indicator (double-ring: white outer, dark inner).
pub const INDICATOR_RADIUS: f32 = 7.0;
pub const INDICATOR_OUTER_STROKE_WIDTH: f32 = 1.5;
pub const INDICATOR_INNER_STROKE_WIDTH: f32 = 1.0;
pub const INDICATOR_OUTER_COLOR: Color = Color::WHITE;
pub const INDICATOR_INNER_COLOR: Color = Color::new(0.0, 0.0, 0.0, 0.6);

// Strip thumb (the bar across hue / alpha sliders).
pub const STRIP_THUMB_WIDTH: f32 = 18.0;
pub const STRIP_THUMB_HEIGHT: f32 = 8.0;
pub const STRIP_THUMB_CORNER_RADIUS: f32 = 2.0;

// Outer padding + inter-section gap.
pub const PADDING: f32 = 12.0;
pub const GAP: f32 = 10.0;

// Preset swatch cells.
pub const SWATCH_SIZE: f32 = 22.0;
pub const SWATCH_SPACING: f32 = 6.0;
pub const SWATCH_CORNER_RADIUS: f32 = 4.0;
pub const SWATCH_SELECTED_STROKE_WIDTH: f32 = 2.0;

// Checkerboard pattern (alpha visualization on swatches and the
// alpha strip background).
pub const CHECKER_CELL: f32 = 6.0;
pub const CHECKER_COLOR_A: Color = Color::new(0.78, 0.78, 0.78, 1.0);
pub const CHECKER_COLOR_B: Color = Color::WHITE;

// Current-color preview swatch (inside the picker, distinct from the
// individual ColorSwatch sizes).
pub const PREVIEW_WIDTH: f32 = 64.0;
pub const PREVIEW_HEIGHT: f32 = 28.0;
pub const PREVIEW_CORNER_RADIUS: f32 = 4.0;

// RGB / HSV spinner cell width and hex-input field width.
pub const SPINNER_FIELD_WIDTH: f32 = 56.0;
pub const HEX_FIELD_WIDTH: f32 = 96.0;

/// Default `ColorPickerStyle` shipped with FernUI.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeColorPickerStyle;

impl ColorPickerStyle for RecipeColorPickerStyle {
    fn make_body(&self, cfg: &ColorPickerStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let body = match cfg.layout {
            ColorPickerLayout::Compact => {
                let mut col = VStack::new().spacing(GAP).add_child(cfg.top_row);
                if let Some(hex) = cfg.compact_hex {
                    col = col.add_child(hex);
                }
                if let Some(footer) = cfg.footer {
                    col = col.add_child(footer);
                }
                ctx.add(col)
            }
            ColorPickerLayout::Standard => {
                let mut col = VStack::new().spacing(GAP).add_child(cfg.top_row);
                if let Some(preview) = cfg.preview_row {
                    col = col.add_child(preview);
                }
                if let Some(rgb) = cfg.rgb_row {
                    col = col.add_child(rgb);
                }
                if let Some(hsv) = cfg.hsv_row {
                    col = col.add_child(hsv);
                }
                if let Some(grid) = cfg.swatches {
                    col = col.add_child(grid);
                }
                if let Some(footer) = cfg.footer {
                    col = col.add_child(footer);
                }
                ctx.add(col)
            }
            ColorPickerLayout::Wide => {
                let mut side_col = VStack::new().spacing(GAP);
                if let Some(preview) = cfg.preview_row {
                    side_col = side_col.add_child(preview);
                }
                if let Some(rgb) = cfg.rgb_row {
                    side_col = side_col.add_child(rgb);
                }
                if let Some(hsv) = cfg.hsv_row {
                    side_col = side_col.add_child(hsv);
                }
                let side_col_id = ctx.add(side_col);
                let main_row_id = ctx.add(
                    HStack::new()
                        .spacing(GAP)
                        .add_child(cfg.top_row)
                        .add_child(side_col_id),
                );
                let mut col = VStack::new().spacing(GAP).add_child(main_row_id);
                if let Some(grid) = cfg.swatches {
                    col = col.add_child(grid);
                }
                if let Some(footer) = cfg.footer {
                    col = col.add_child(footer);
                }
                ctx.add(col)
            }
        };

        // Wrap in a Panel so the picker reads as a self-contained
        // surface (background + border + corner radius). Embedding in
        // an overlay without this produces a transparent popup.
        ctx.add(
            Panel::new()
                .padding(PADDING)
                .border_width(1.0)
                .child_id(body),
        )
    }
}
