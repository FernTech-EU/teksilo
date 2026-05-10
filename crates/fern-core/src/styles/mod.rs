//! FernUI's theming system: [`Theme`] aggregator, the
//! [`ThemeAppearance`] flag, and the typed [`ThemeExtensions`]
//! registry.
//!
//! The Tier 2 paint-recipe types (`ShapeRecipe`, `FillRecipe`, …) and
//! the Tier 3 per-widget style protocols (`ButtonStyle`, `ToggleStyle`,
//! …) land in this module in subsequent migration steps.
//!
//! See `docs/styling-system.md` for the full four-tier ladder.

pub mod recipe;
pub mod theme;
pub mod theme_appearance;
pub mod theme_extension;

pub use recipe::{
    BorderPosition, BorderRecipe, BorderStyle, FillRecipe, GradientStop, PerStateRecipe,
    RecipeColor, ShadowRecipe, ShapeRecipe, WidgetState,
};
pub use theme::Theme;
pub use theme_appearance::ThemeAppearance;
pub use theme_extension::ThemeExtensions;
