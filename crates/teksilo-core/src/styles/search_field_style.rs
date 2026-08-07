// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `SearchField`. See `docs/styling-system.md`.
//!
//! `SearchField` composes a themed `TextInput` (which already owns its
//! bordered surface chrome via `TextInputStyle`) with a leading
//! magnifier glyph dropped into the field's leading slot and a
//! trailing clear-button toggled by `show_clear_button(true)`. The
//! suggestions popup surface is routed through `PopoverStyle::Menu`.
//!
//! What remains is a thin chrome hook for apps that want to wrap or
//! replace the field — same pattern as `DateEditStyle`. Default is a
//! passthrough. Dim constants (glyph size, row metrics, panel padding,
//! input/panel gap) live as `pub const`s on
//! `teksilo_widgets::styles::recipe_search_field_style`.

use std::rc::Rc;

use crate::build_context::BuildContext;
use crate::widget_id::WidgetId;

pub struct SearchFieldStyleConfig {
    /// Pre-assembled body — the `TextInput` already carrying the
    /// magnifier glyph in its leading slot and the clear button
    /// enabled via `show_clear_button(true)`.
    pub body: WidgetId,
}

pub trait SearchFieldStyle: 'static {
    fn make_body(&self, cfg: &SearchFieldStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedSearchFieldStyle = Rc<dyn SearchFieldStyle>;
