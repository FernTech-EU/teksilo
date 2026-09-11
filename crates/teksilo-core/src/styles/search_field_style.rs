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

use teksilo_tokens::InputTokens;

use crate::build_context::BuildContext;
use crate::styles::density::spacing;
use crate::widget_id::WidgetId;

pub struct SearchFieldStyleConfig {
    /// Pre-assembled body — the `TextInput` already carrying the
    /// magnifier glyph in its leading slot and the clear button
    /// enabled via `show_clear_button(true)`.
    pub body: WidgetId,
}

pub trait SearchFieldStyle: 'static {
    fn make_body(&self, cfg: &SearchFieldStyleConfig, ctx: &mut BuildContext) -> WidgetId;

    /// Horizontal padding inside one suggestion row, in logical pixels.
    ///
    /// The suggestion panel is built by `SearchField` itself — the style's
    /// only composition hook is [`make_body`](Self::make_body), which wraps
    /// the *field* — so the row gutter cannot come from the recipe unless the
    /// trait hands it over, and the widget holds an
    /// `Rc<dyn SearchFieldStyle>` that cannot reach a recipe field.
    ///
    /// **Defaulted**, so a `SearchFieldStyle` implemented outside this
    /// workspace keeps compiling and keeps the ladder it had. The default is
    /// `teksilo_widgets::styles::recipe_search_field_style::ROW_PADDING_HORIZONTAL`
    /// put through [`spacing`] — restated as a literal because `teksilo-core`
    /// cannot name a `teksilo-widgets` constant, and pinned equal to it by
    /// `the_search_field_trait_defaults_restate_the_module_constants`.
    fn row_padding_horizontal(&self, tokens: &InputTokens) -> f32 {
        spacing(10.0, tokens)
    }

    /// Vertical padding inside one suggestion row, in logical pixels.
    /// Defaulted on the same terms as
    /// [`row_padding_horizontal`](Self::row_padding_horizontal).
    fn row_padding_vertical(&self, tokens: &InputTokens) -> f32 {
        spacing(4.0, tokens)
    }
}

pub type SharedSearchFieldStyle = Rc<dyn SearchFieldStyle>;
