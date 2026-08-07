// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `ColorPickerStyle` impl + the picker's design tokens.
//!
//! Design tokens for `ColorPicker` live as `pub const`s on this module.
//! The four functional renderers (`HsvCanvas`, `HueStrip`, `AlphaStrip`,
//! `ColorSwatch`) read these directly when they need dimensions for
//! their colour-space rendering.
//!
//! `RecipeColorPickerStyle::make_body` ports the IntUI layout exactly:
//! one of three column / row arrangements per `ColorPickerLayout`,
//! wrapped in a `Panel` surface (1 dp border + corner radius +
//! `PADDING` interior).

use teksilo_core::build_context::BuildContext;
use teksilo_core::styles::{ColorPickerLayout, ColorPickerStyle, ColorPickerStyleConfig};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::Color;

use crate::panel::Panel;
use crate::primitives::{HStack, VStack};

// ─── IntUI design tokens for ColorPicker ───────────────────────────

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

/// Configurable dimension recipe for `RecipeColorPickerStyle`.
///
/// All fields default to the module-level `pub const` tokens so existing
/// callers that use `RecipeColorPickerStyle::default()` are unaffected.
/// Override individual fields to create a custom-sized picker without
/// writing a full `ColorPickerStyle` implementation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorPickerRecipe {
    // HSV canvas
    pub canvas_width: f32,
    pub canvas_height: f32,
    pub canvas_corner_radius: f32,
    // 1D strips
    pub strip_thickness: f32,
    pub strip_length: f32,
    pub strip_corner_radius: f32,
    // HSV-canvas indicator
    pub indicator_radius: f32,
    pub indicator_outer_stroke_width: f32,
    pub indicator_inner_stroke_width: f32,
    pub indicator_outer_color: Color,
    pub indicator_inner_color: Color,
    // Strip thumb
    pub strip_thumb_width: f32,
    pub strip_thumb_height: f32,
    pub strip_thumb_corner_radius: f32,
    // Layout spacing
    pub padding: f32,
    pub gap: f32,
    // Preset swatches
    pub swatch_size: f32,
    pub swatch_spacing: f32,
    pub swatch_corner_radius: f32,
    pub swatch_selected_stroke_width: f32,
    // Checkerboard (alpha visualisation)
    pub checker_cell: f32,
    pub checker_color_a: Color,
    pub checker_color_b: Color,
    // Current-color preview swatch
    pub preview_width: f32,
    pub preview_height: f32,
    pub preview_corner_radius: f32,
    // Input fields
    pub spinner_field_width: f32,
    pub hex_field_width: f32,
}

impl Default for ColorPickerRecipe {
    fn default() -> Self {
        Self {
            canvas_width: CANVAS_WIDTH,
            canvas_height: CANVAS_HEIGHT,
            canvas_corner_radius: CANVAS_CORNER_RADIUS,
            strip_thickness: STRIP_THICKNESS,
            strip_length: STRIP_LENGTH,
            strip_corner_radius: STRIP_CORNER_RADIUS,
            indicator_radius: INDICATOR_RADIUS,
            indicator_outer_stroke_width: INDICATOR_OUTER_STROKE_WIDTH,
            indicator_inner_stroke_width: INDICATOR_INNER_STROKE_WIDTH,
            indicator_outer_color: INDICATOR_OUTER_COLOR,
            indicator_inner_color: INDICATOR_INNER_COLOR,
            strip_thumb_width: STRIP_THUMB_WIDTH,
            strip_thumb_height: STRIP_THUMB_HEIGHT,
            strip_thumb_corner_radius: STRIP_THUMB_CORNER_RADIUS,
            padding: PADDING,
            gap: GAP,
            swatch_size: SWATCH_SIZE,
            swatch_spacing: SWATCH_SPACING,
            swatch_corner_radius: SWATCH_CORNER_RADIUS,
            swatch_selected_stroke_width: SWATCH_SELECTED_STROKE_WIDTH,
            checker_cell: CHECKER_CELL,
            checker_color_a: CHECKER_COLOR_A,
            checker_color_b: CHECKER_COLOR_B,
            preview_width: PREVIEW_WIDTH,
            preview_height: PREVIEW_HEIGHT,
            preview_corner_radius: PREVIEW_CORNER_RADIUS,
            spinner_field_width: SPINNER_FIELD_WIDTH,
            hex_field_width: HEX_FIELD_WIDTH,
        }
    }
}

/// Default `ColorPickerStyle` shipped with Teksilo.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeColorPickerStyle {
    pub recipe: ColorPickerRecipe,
}

impl RecipeColorPickerStyle {
    pub fn new(recipe: ColorPickerRecipe) -> Self {
        Self { recipe }
    }
}

impl ColorPickerStyle for RecipeColorPickerStyle {
    fn make_body(&self, cfg: &ColorPickerStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let body = match cfg.layout {
            ColorPickerLayout::Compact => {
                let mut col = VStack::new()
                    .spacing(self.recipe.gap)
                    .add_child(cfg.top_row);
                if let Some(hex) = cfg.compact_hex {
                    col = col.add_child(hex);
                }
                if let Some(footer) = cfg.footer {
                    col = col.add_child(footer);
                }
                ctx.add(col)
            }
            ColorPickerLayout::Standard => {
                let mut col = VStack::new()
                    .spacing(self.recipe.gap)
                    .add_child(cfg.top_row);
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
                let mut side_col = VStack::new().spacing(self.recipe.gap);
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
                        .spacing(self.recipe.gap)
                        .add_child(cfg.top_row)
                        .add_child(side_col_id),
                );
                let mut col = VStack::new()
                    .spacing(self.recipe.gap)
                    .add_child(main_row_id);
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
                .padding(self.recipe.padding)
                .border_width(1.0)
                .child_id(body),
        )
    }
}
