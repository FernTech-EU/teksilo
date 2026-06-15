// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `DateEditStyle` impl + date-edit family design tokens.
//!
//! Design tokens for the date-edit family live as `pub const`s on this
//! module. The four widgets (`DateEdit`, `TimeEdit`, `DateRangeEdit`,
//! `DateTimeEdit`) read them directly when building the calendar / clock
//! trigger icon and the segmented-field separators.
//!
//! In IntUI the trigger sits inside `TextInput`'s trailing slot, so the
//! default `make_body` is a passthrough — the widget pre-assembles the
//! complete body before handing it to the recipe. Custom recipes can
//! wrap the body or surround it with siblings.

use bastyde_core::build_context::BuildContext;
use bastyde_core::styles::{DateEditStyle, DateEditStyleConfig, SharedDateEditStyle};
use bastyde_core::widget_id::WidgetId;

// ─── IntUI design tokens for the date-edit family ──────────────────

/// Width of the trailing calendar / clock trigger button slot.
pub const CALENDAR_BUTTON_WIDTH: f32 = 24.0;
/// Edge length of the calendar / clock trigger glyph.
pub const CALENDAR_ICON_SIZE: f32 = 14.0;
/// Gap between adjacent segments (visual separator characters sit in
/// this gap). Used by the segmented date/time editors.
pub const SEGMENT_GAP: f32 = 1.0;

/// Default `DateEditStyle` shipped with Bastyde. Currently a
/// passthrough — IntUI's date-edit chrome lives inside `TextInput`,
/// which is already themed. Custom styles can wrap the body to add
/// extra siblings (clear button, status icon, etc.).
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeDateEditStyle;

impl DateEditStyle for RecipeDateEditStyle {
    fn make_body(&self, cfg: &DateEditStyleConfig, _ctx: &mut BuildContext) -> WidgetId {
        cfg.body
    }
}

/// Convenience for callers that need the active style (per-call
/// override → theme slot → `RecipeDateEditStyle`).
pub fn resolve_date_edit_style(
    override_: &Option<SharedDateEditStyle>,
    ctx: &BuildContext,
) -> SharedDateEditStyle {
    if let Some(s) = override_.clone() {
        return s;
    }
    ctx.theme_signal()
        .get()
        .style_slots
        .date_edit
        .clone()
        .unwrap_or_else(|| std::rc::Rc::new(RecipeDateEditStyle) as SharedDateEditStyle)
}
