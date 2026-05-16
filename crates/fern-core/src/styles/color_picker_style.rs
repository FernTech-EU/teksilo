//! Tier-3 style protocol for `ColorPicker`. See `docs/styling-system.md`.
//!
//! The trait owns only **container chrome and layout arrangement**.
//! The four functional renderers — `HsvCanvas` (2-D saturation × value
//! gradient), `HueStrip`, `AlphaStrip`, and `ColorSwatch` — are
//! widget-internal `pub(crate)` types that paint the colour space
//! itself; replacing them is a fork-worthy change, not a theming
//! one (principle 6 in the migration plan).
//!
//! Each row the picker can show (top row: canvas + hue + alpha;
//! preview / hex; RGB spinners; HSV spinners; swatch grid; footer;
//! compact-mode hex) is built by the widget and threaded into the
//! config as an `Option<WidgetId>`. The recipe arranges them per
//! the active `ColorPickerLayout` and wraps the result in a Panel
//! surface (padding, border, corner radius).

use std::rc::Rc;

use crate::build_context::BuildContext;
use crate::widget_id::WidgetId;

/// Layout variant — controls how the recipe arranges the rows.
/// Re-exported from `color_picker_style` so the config and any
/// custom `ColorPickerStyle` impl can branch on it without an
/// upward dep on `fern-widgets`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorPickerLayout {
    /// Single column. HSV canvas + hue strip on top, hex input below,
    /// footer at the bottom. No preview swatch, no RGB / HSV spinner
    /// rows, no swatch grid.
    Compact,
    /// Single column. Canvas + strips on top, preview + hex row, RGB
    /// spinners row, HSV spinners row, swatch grid, footer.
    #[default]
    Standard,
    /// Canvas + strips on the leading side, preview / spinners stacked
    /// on the trailing side. Swatch grid + footer below the main row.
    Wide,
}

/// Pre-built sections passed to `ColorPickerStyle::make_body`. The
/// widget assembles each section's children (canvas, strips, spinners,
/// swatches, buttons) and hands them to the recipe via these slots;
/// the recipe lays them out per `layout` and wraps the result in a
/// surface frame.
pub struct ColorPickerStyleConfig {
    pub layout: ColorPickerLayout,
    /// HSV canvas + hue strip + optional alpha strip. Always present;
    /// individual sub-widgets are gated by the picker's `show_*` flags
    /// before they reach the row, so an "empty" top row simply isn't
    /// added by the widget.
    pub top_row: WidgetId,
    /// Preview swatch + hex input (Standard / Wide layouts).
    pub preview_row: Option<WidgetId>,
    /// Red / Green / Blue (+ optional Alpha) spinner row.
    pub rgb_row: Option<WidgetId>,
    /// Hue / Saturation / Value spinner row.
    pub hsv_row: Option<WidgetId>,
    /// Preset swatch grid.
    pub swatches: Option<WidgetId>,
    /// Footer row (Cancel + Done buttons).
    pub footer: Option<WidgetId>,
    /// Compact-mode hex input (Compact layout only).
    pub compact_hex: Option<WidgetId>,
}

pub trait ColorPickerStyle: 'static {
    fn make_body(&self, cfg: &ColorPickerStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedColorPickerStyle = Rc<dyn ColorPickerStyle>;
