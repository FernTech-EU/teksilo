// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `TextSelectionStyle` impl — the shipped look of the touch selection
//! handles and the magnifier.
//!
//! An accent disc on an accent stem, ringed in the surface colour so it stays
//! visible over text of its own hue; a lens on the editor's own background with
//! a hairline border. Apps that want another treatment write their own
//! `impl TextSelectionStyle` and install it theme-wide
//! (`theme.style_slots.text_selection = Some(Rc::new(MyHandles))`).
//!
//! The dimensions are **not** this module's to invent: they are
//! `teksilo_core::text_touch`'s constants, because the controller hit-tests a
//! handle from the same numbers the style paints it with, and two copies would
//! drift. Everything this file adds is which token each part is coloured from.

use teksilo_core::styles::density::dp;
use teksilo_core::styles::{
    RecipeColor, TextMagnifierRecipe, TextSelectionHandleRecipe, TextSelectionStyle, Theme,
};
use teksilo_core::text_touch::{HANDLE_DIAMETER, HANDLE_HIT_SIZE, HANDLE_STEM_WIDTH, magnifier};
use teksilo_tokens::{BorderRole, InputTokens, SurfaceRole, TargetRole};

/// Width of the ring drawn around a handle's disc, in dp.
pub const HANDLE_OUTLINE_WIDTH: f32 = 1.0;

/// Width of the magnifier's frame, in dp.
pub const MAGNIFIER_BORDER_WIDTH: f32 = 1.0;

/// Corner radius of the magnifier's frame, in dp — **zero**, so the shipped
/// lens is a rectangle.
///
/// Not a stylistic preference. The clip behind the frame is rectangular and
/// cannot be anything else (see [`teksilo_core::text_touch::magnifier`]), so a
/// frame with radius `r` leaves `r × (√2 − 1)` dp of magnified content standing
/// outside each of its corners — 4 dp at a 10 dp radius, which a 1 dp frame
/// does not begin to cover. At zero the frame is exactly the clip and nothing
/// escapes. A style that wants a rounded lens may raise this and accept the
/// corners; it is the one number in this recipe with a correctness consequence.
pub const MAGNIFIER_CORNER_RADIUS: f32 = 0.0;

/// Dimension recipe for [`RecipeTextSelectionStyle`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextSelectionRecipe {
    pub handle_diameter: f32,
    pub handle_hit_size: f32,
    pub handle_stem_width: f32,
    pub handle_outline_width: f32,
    pub magnifier_radius: f32,
    pub magnifier_half_height: f32,
    pub magnifier_scale: f32,
    pub magnifier_rise: f32,
    pub magnifier_corner_radius: f32,
    pub magnifier_border_width: f32,
}

impl TextSelectionRecipe {
    /// The shipped dimensions resolved against a density's [`InputTokens`].
    ///
    /// [`Default`] is `for_tokens(&InputTokens::default())` — the Compact
    /// ladder — so the constants above are the Compact column by construction.
    ///
    /// Only the handle's **hit** extent is a
    /// [`Target`](teksilo_tokens::TargetRole::Target). The disc, the stem and
    /// the whole lens are decoration: they are already sized for a fingertip,
    /// and nothing meets them but the eye.
    pub fn for_tokens(tokens: &InputTokens) -> Self {
        Self {
            handle_diameter: dp(HANDLE_DIAMETER, TargetRole::Decoration, tokens),
            handle_hit_size: dp(HANDLE_HIT_SIZE, TargetRole::Target, tokens),
            handle_stem_width: HANDLE_STEM_WIDTH,
            handle_outline_width: HANDLE_OUTLINE_WIDTH,
            magnifier_radius: magnifier::MAGNIFIER_RADIUS,
            magnifier_half_height: magnifier::MAGNIFIER_HALF_HEIGHT,
            magnifier_scale: magnifier::MAGNIFIER_SCALE,
            magnifier_rise: magnifier::MAGNIFIER_RISE,
            magnifier_corner_radius: MAGNIFIER_CORNER_RADIUS,
            magnifier_border_width: MAGNIFIER_BORDER_WIDTH,
        }
    }
}

impl Default for TextSelectionRecipe {
    fn default() -> Self {
        Self::for_tokens(&InputTokens::default())
    }
}

/// Default `TextSelectionStyle` shipped with Teksilo. Colours come from the
/// active theme's roles, so a theme swap repaints for free.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeTextSelectionStyle {
    pub recipe: TextSelectionRecipe,
}

impl RecipeTextSelectionStyle {
    pub fn new(recipe: TextSelectionRecipe) -> Self {
        Self { recipe }
    }

    /// This style with every dimension resolved against a density's
    /// [`InputTokens`] — `RecipeTextSelectionStyle::for_tokens(&ctx.theme().input)`
    /// at the host's own build site.
    pub fn for_tokens(tokens: &InputTokens) -> Self {
        Self {
            recipe: TextSelectionRecipe::for_tokens(tokens),
        }
    }
}

impl TextSelectionStyle for RecipeTextSelectionStyle {
    fn handle(&self, _theme: &Theme) -> TextSelectionHandleRecipe {
        TextSelectionHandleRecipe {
            diameter: self.recipe.handle_diameter,
            hit_size: self.recipe.handle_hit_size,
            stem_width: self.recipe.handle_stem_width,
            fill: RecipeColor::Surface(SurfaceRole::Accent),
            // The ring is the surface behind the text, not a border colour: its
            // job is to separate the disc from the glyphs it sits on, which the
            // page's own background does better than any line would.
            outline: RecipeColor::Surface(SurfaceRole::EditorBg),
            outline_width: self.recipe.handle_outline_width,
        }
    }

    fn magnifier(&self, _theme: &Theme) -> TextMagnifierRecipe {
        TextMagnifierRecipe {
            radius: self.recipe.magnifier_radius,
            half_height: self.recipe.magnifier_half_height,
            scale: self.recipe.magnifier_scale,
            rise: self.recipe.magnifier_rise,
            corner_radius: self.recipe.magnifier_corner_radius,
            background: RecipeColor::Surface(SurfaceRole::EditorBg),
            border: RecipeColor::Border(BorderRole::Default),
            border_width: self.recipe.magnifier_border_width,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_tokens::TargetDensity;

    /// The shipped constants are the Compact column, by construction rather
    /// than by a second table someone has to keep in step.
    #[test]
    fn the_default_recipe_is_the_compact_projection() {
        assert_eq!(
            TextSelectionRecipe::default(),
            TextSelectionRecipe::for_tokens(&InputTokens::for_density(TargetDensity::Compact))
        );
        assert_eq!(
            TextSelectionRecipe::default().handle_diameter,
            HANDLE_DIAMETER
        );
        assert_eq!(
            TextSelectionRecipe::default().handle_hit_size,
            HANDLE_HIT_SIZE
        );
    }

    /// The controller hit-tests a handle from the recipe the style paints it
    /// with, so the two must be the same numbers at every density.
    #[test]
    fn the_controllers_metrics_come_from_this_recipe() {
        for density in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            let tokens = InputTokens::for_density(density);
            let style = RecipeTextSelectionStyle::for_tokens(&tokens);
            let recipe = style.handle(&teksilo_core::presets::intui::light());
            let metrics = teksilo_core::text_touch::HandleMetrics::from(&recipe);
            assert_eq!(
                metrics,
                teksilo_core::text_touch::HandleMetrics::for_tokens(&tokens),
                "{density:?}"
            );
        }
    }

    /// The shipped lens is rectangular because its clip is. A non-zero radius
    /// would leave `r × (√2 − 1)` dp of magnified content outside each corner
    /// of the frame, with nothing in the pipeline able to remove it.
    #[test]
    fn the_shipped_lens_frame_matches_the_rectangular_clip() {
        assert_eq!(TextSelectionRecipe::default().magnifier_corner_radius, 0.0);
    }

    /// A handle's target may never fall below the WCAG 2.2 minimum, at any
    /// density.
    #[test]
    fn a_handles_target_clears_the_conformance_floor_at_every_density() {
        for density in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            let tokens = InputTokens::for_density(density);
            let recipe = TextSelectionRecipe::for_tokens(&tokens);
            assert!(
                recipe.handle_hit_size >= tokens.min_target_conformance,
                "{density:?}: {} dp target",
                recipe.handle_hit_size
            );
        }
    }
}
