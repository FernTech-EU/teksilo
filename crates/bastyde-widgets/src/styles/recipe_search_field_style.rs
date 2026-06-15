// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `SearchFieldStyle` impl + design tokens.
//!
//! Design tokens for `SearchField` live as `pub const`s on this module.
//! `SearchField` and `SuggestionPanel` read them directly when
//! building the magnifier glyph, the suggestion-list row chrome, and
//! the popup padding. The suggestion popup *surface* is routed through
//! `PopoverStyle::Menu`; only the row chrome + panel padding remain
//! SearchField-specific.

use bastyde_core::build_context::BuildContext;
use bastyde_core::styles::{SearchFieldStyle, SearchFieldStyleConfig, SharedSearchFieldStyle};
use bastyde_core::widget_id::WidgetId;

// ─── IntUI design tokens for SearchField ───────────────────────────

/// Visual size of the magnifier glyph drawn inside the leading slot.
pub const GLYPH_SIZE: f32 = 14.0;
/// Reserved width of the leading slot — wider than the glyph so it
/// doesn't sit flush against the field's leading edge.
pub const GLYPH_SLOT_WIDTH: f32 = 22.0;
/// Vertical gap between the input field and the suggestions popup
/// rendered below it.
pub const INPUT_PANEL_GAP: f32 = 2.0;
/// Padding between the popup surface border and the row column.
pub const PANEL_PADDING: f32 = 4.0;
/// Suggestion popup outer corner radius.
pub const PANEL_CORNER_RADIUS: f32 = 6.0;
/// Per-row hover-highlight corner radius.
pub const ROW_CORNER_RADIUS: f32 = 2.0;
pub const ROW_PADDING_HORIZONTAL: f32 = 10.0;
pub const ROW_PADDING_VERTICAL: f32 = 4.0;
pub const ROW_HEIGHT: f32 = 26.0;

/// Default `SearchFieldStyle` shipped with Bastyde. Passthrough —
/// IntUI's search-field chrome lives inside `TextInput`'s leading
/// slot + clear button, which are already themed.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeSearchFieldStyle;

impl SearchFieldStyle for RecipeSearchFieldStyle {
    fn make_body(&self, cfg: &SearchFieldStyleConfig, _ctx: &mut BuildContext) -> WidgetId {
        cfg.body
    }
}

pub fn resolve_search_field_style(
    override_: &Option<SharedSearchFieldStyle>,
    ctx: &BuildContext,
) -> SharedSearchFieldStyle {
    if let Some(s) = override_.clone() {
        return s;
    }
    ctx.theme_signal()
        .get()
        .style_slots
        .search_field
        .clone()
        .unwrap_or_else(|| std::rc::Rc::new(RecipeSearchFieldStyle) as SharedSearchFieldStyle)
}
