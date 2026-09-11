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

use teksilo_core::build_context::BuildContext;
use teksilo_core::styles::density::{dp, spacing};
use teksilo_core::styles::{SearchFieldStyle, SearchFieldStyleConfig, SharedSearchFieldStyle};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{InputTokens, TargetRole};

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

/// Dimension tokens for `SearchField` bundled into a copyable struct.
///
/// `RecipeSearchFieldStyle::default()` fills every field from the
/// corresponding `pub const` above.  Apps that want a custom size can
/// construct `SearchFieldRecipe { row_height: 32.0, ..Default::default() }`
/// and pass it to `RecipeSearchFieldStyle::new(recipe)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchFieldRecipe {
    pub glyph_size: f32,
    pub glyph_slot_width: f32,
    pub input_panel_gap: f32,
    pub panel_padding: f32,
    pub panel_corner_radius: f32,
    pub row_corner_radius: f32,
    pub row_padding_horizontal: f32,
    pub row_padding_vertical: f32,
    pub row_height: f32,
}

impl SearchFieldRecipe {
    /// This recipe's dimensions resolved against a density's [`InputTokens`].
    ///
    /// [`Default`] is `for_tokens(&InputTokens::default())` — the Compact
    /// ladder — so the shipped values below are the Compact column by
    /// construction and cannot drift from it.
    pub fn for_tokens(tokens: &InputTokens) -> Self {
        Self {
            glyph_size: GLYPH_SIZE,
            glyph_slot_width: GLYPH_SLOT_WIDTH,
            input_panel_gap: spacing(INPUT_PANEL_GAP, tokens),
            panel_padding: spacing(PANEL_PADDING, tokens),
            panel_corner_radius: PANEL_CORNER_RADIUS,
            row_corner_radius: ROW_CORNER_RADIUS,
            row_padding_horizontal: spacing(ROW_PADDING_HORIZONTAL, tokens),
            row_padding_vertical: spacing(ROW_PADDING_VERTICAL, tokens),
            row_height: dp(ROW_HEIGHT, TargetRole::Target, tokens),
        }
    }
}

impl Default for SearchFieldRecipe {
    fn default() -> Self {
        Self::for_tokens(&InputTokens::default())
    }
}

/// Default `SearchFieldStyle` shipped with Teksilo. Passthrough —
/// IntUI's search-field chrome lives inside `TextInput`'s leading
/// slot + clear button, which are already themed.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeSearchFieldStyle {
    pub recipe: SearchFieldRecipe,
}

impl RecipeSearchFieldStyle {
    pub fn new(recipe: SearchFieldRecipe) -> Self {
        Self { recipe }
    }

    /// This style with every dimension resolved against a density's
    /// [`InputTokens`], as `RecipeSearchFieldStyle::for_tokens(&ctx.theme().input)` at
    /// the widget's own build site.
    ///
    /// [`Default`] is the `TargetDensity::Compact` projection, so a Compact
    /// tree gets exactly the values this module documents.
    pub fn for_tokens(tokens: &InputTokens) -> Self {
        Self {
            recipe: SearchFieldRecipe::for_tokens(tokens),
        }
    }
}

impl SearchFieldStyle for RecipeSearchFieldStyle {
    fn make_body(&self, cfg: &SearchFieldStyleConfig, _ctx: &mut BuildContext) -> WidgetId {
        cfg.body
    }

    /// This style's own row gutter, so a recipe built by a preset — macOS's
    /// 8 dp `NSSearchField` results row, say — is the one a suggestion row
    /// pads by. `tokens` is unused: the recipe was already resolved against a
    /// density by [`SearchFieldRecipe::for_tokens`] at the widget's build site.
    fn row_padding_horizontal(&self, _tokens: &InputTokens) -> f32 {
        self.recipe.row_padding_horizontal
    }

    /// This style's own vertical row gutter. See
    /// [`row_padding_horizontal`](Self::row_padding_horizontal).
    fn row_padding_vertical(&self, _tokens: &InputTokens) -> f32 {
        self.recipe.row_padding_vertical
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
        .unwrap_or_else(|| {
            std::rc::Rc::new(RecipeSearchFieldStyle::for_tokens(&ctx.theme().input))
                as SharedSearchFieldStyle
        })
}
