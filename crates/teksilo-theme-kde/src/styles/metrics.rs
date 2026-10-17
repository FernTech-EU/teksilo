// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The dimensions the Breeze widget styles are built with, at a density.
//!
//! Every height here starts from a Breeze rule in [`crate::geometry`]: the
//! text-driven ones from the theme's body line box, the icon-driven ones
//! from an icon size. A height of 24 or more is a target, so it goes through
//! the density ladder's `dp(.., Target)`, which leaves it alone at Compact
//! and raises it to the density's target size above it (`[unpublished]`:
//! KDE publishes no touch ladder, so the ladder is Teksilo's generic one).
//! A gap or padding goes through `spacing`. At Compact every value is
//! Breeze's own.

use teksilo_core::styles::density::{dp, spacing};
use teksilo_tokens::{InputTokens, TargetRole, TypographyTokens};

use crate::geometry::{
    BUTTON_MARGIN_WIDTH, BreezeGeometry, FRAME_WIDTH, LARGE_ICON_SIZE, MEDIUM_ICON_SIZE,
    TOOL_BUTTON_MARGIN_WIDTH,
};

/// The body line box `H` of `typography`, in whole pixels: the height every
/// text-driven Breeze control is derived from. 18 for the Breeze typography.
pub fn body_line_height(typography: &TypographyTokens) -> f32 {
    (typography.body.size * typography.body.line_height).round()
}

/// The Breeze geometry of `typography`'s body line box.
pub fn geometry_of(typography: &TypographyTokens) -> BreezeGeometry {
    BreezeGeometry::for_line_height(body_line_height(typography))
}

/// The dimensions of a push button, a default button and a flat push button,
/// which share them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeButtonMetrics {
    /// `[S]` The button's height, at least: `H + 16`, through the density
    /// ladder. 34 at Compact for the Breeze typography.
    pub height: f32,
    /// `[S]` The button's width, at least: 84, Breeze's 80 plus the 2 px
    /// frame margin on each side.
    pub min_width: f32,
    /// `[S]` The space between the frame and the label on the leading and
    /// trailing sides: the 6 px button margin and the 2 px frame margin,
    /// scaled by the density's spacing.
    pub padding_horizontal: f32,
    /// `[S]` The same above and below the label. It is part of the height
    /// rule rather than a gap, so it does not scale: the height floor rises
    /// with the density instead.
    pub padding_vertical: f32,
}

/// The button dimensions at the density of `tokens`, for `typography`.
pub fn breeze_button_metrics_for(
    tokens: &InputTokens,
    typography: &TypographyTokens,
) -> BreezeButtonMetrics {
    let g = geometry_of(typography);
    BreezeButtonMetrics {
        height: dp(g.push_button_height, TargetRole::Target, tokens),
        min_width: g.push_button_min_width,
        padding_horizontal: spacing(BUTTON_MARGIN_WIDTH + FRAME_WIDTH, tokens),
        padding_vertical: BUTTON_MARGIN_WIDTH + FRAME_WIDTH,
    }
}

/// [`breeze_button_metrics_for`] at the Compact density and the Breeze
/// typography.
pub fn breeze_button_metrics() -> BreezeButtonMetrics {
    breeze_button_metrics_for(
        &InputTokens::default(),
        &crate::typography::breeze_typography(),
    )
}

/// The square an `IconButton` is drawn in at each of its five sizes: Breeze's
/// auto-raise tool button, an icon with the 6 px tool-button margin around
/// it, through the density ladder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeToolButtonSizes {
    /// `[unpublished]` Breeze has no tool button smaller than the one around
    /// a 16 px icon; the compact rung is the density's target size, 24 at
    /// Compact.
    pub compact: f32,
    /// `[S]` A 16 px icon: 28 at Compact.
    pub default: f32,
    /// `[S]` A 22 px icon, the toolbar size: 34 at Compact.
    pub toolbar: f32,
    /// `[derived]` Breeze's rule at Kirigami's 32 px medium icon: 44.
    pub large: f32,
    /// `[derived]` Breeze's rule at Kirigami's 48 px large icon: 60.
    pub hero: f32,
}

/// The tool-button squares at the density of `tokens`.
pub fn breeze_tool_button_sizes_for(tokens: &InputTokens) -> BreezeToolButtonSizes {
    // Icon-driven, so the font does not enter: the default geometry's
    // tool-button heights are those of any font.
    let g = BreezeGeometry::breeze_default();
    let around = |icon: f32| icon + 2.0 * TOOL_BUTTON_MARGIN_WIDTH;
    let target = |h: f32| dp(h, TargetRole::Target, tokens);
    BreezeToolButtonSizes {
        compact: target(tokens.min_target_conformance),
        default: target(g.tool_button_height),
        toolbar: target(g.tool_button_height_medium),
        large: target(around(MEDIUM_ICON_SIZE)),
        hero: target(around(LARGE_ICON_SIZE)),
    }
}

/// [`breeze_tool_button_sizes_for`] at the Compact density.
pub fn breeze_tool_button_sizes() -> BreezeToolButtonSizes {
    breeze_tool_button_sizes_for(&InputTokens::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_tokens::TargetDensity;

    fn at(density: TargetDensity) -> InputTokens {
        InputTokens::for_density(density)
    }

    #[test]
    fn the_compact_button_is_breezes() {
        let m = breeze_button_metrics();
        assert_eq!(m.height, 34.0);
        assert_eq!(m.min_width, 84.0);
        assert_eq!(m.padding_horizontal, 8.0);
        assert_eq!(m.padding_vertical, 8.0);
        // The label's 18 px line box and the two gutters make the height.
        let h = body_line_height(&crate::typography::breeze_typography());
        assert_eq!(h, 18.0);
        assert_eq!(h + 2.0 * m.padding_vertical, m.height);
        // The width rule: max(text + 12, 80) + 4 is text + 16 or 84.
        assert_eq!(2.0 * m.padding_horizontal, 12.0 + 2.0 * FRAME_WIDTH);
    }

    #[test]
    fn the_button_rises_with_the_density_and_never_falls() {
        let t = crate::typography::breeze_typography();
        let compact = breeze_button_metrics_for(&at(TargetDensity::Compact), &t);
        let comfortable = breeze_button_metrics_for(&at(TargetDensity::Comfortable), &t);
        let touch = breeze_button_metrics_for(&at(TargetDensity::Touch), &t);
        assert_eq!(comfortable.height, 34.0, "34 already clears 32");
        assert_eq!(touch.height, 44.0);
        assert!((comfortable.padding_horizontal - 9.2).abs() < 1e-4);
        assert!((touch.padding_horizontal - 10.4).abs() < 1e-4);
        for m in [compact, comfortable, touch] {
            assert_eq!(m.min_width, 84.0);
            assert_eq!(m.padding_vertical, 8.0);
        }
    }

    #[test]
    fn a_larger_body_font_makes_a_taller_button() {
        let mut t = crate::typography::breeze_typography();
        // Plasma's 12 pt: a 16 px face on a 22 px line, which Qt makes 38.
        t.body.size = 16.0;
        t.body.line_height = 22.0 / 16.0;
        let m = breeze_button_metrics_for(&InputTokens::default(), &t);
        assert_eq!(m.height, 38.0);
    }

    #[test]
    fn the_tool_buttons_are_breezes_and_grow_with_the_density() {
        let s = breeze_tool_button_sizes();
        assert_eq!(
            (s.compact, s.default, s.toolbar, s.large, s.hero),
            (24.0, 28.0, 34.0, 44.0, 60.0)
        );
        let comfortable = breeze_tool_button_sizes_for(&at(TargetDensity::Comfortable));
        assert_eq!(
            (
                comfortable.compact,
                comfortable.default,
                comfortable.toolbar
            ),
            (32.0, 32.0, 34.0)
        );
        let touch = breeze_tool_button_sizes_for(&at(TargetDensity::Touch));
        assert_eq!(
            (
                touch.compact,
                touch.default,
                touch.toolbar,
                touch.large,
                touch.hero
            ),
            (44.0, 44.0, 44.0, 44.0, 60.0)
        );
    }

    #[test]
    fn every_tool_button_clears_the_conformance_floor_without_a_box() {
        for density in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            let tokens = at(density);
            let s = breeze_tool_button_sizes_for(&tokens);
            for size in [s.compact, s.default, s.toolbar, s.large, s.hero] {
                assert!(size >= tokens.min_target_conformance, "{density:?}: {size}");
            }
            assert!(s.compact <= s.default && s.default <= s.toolbar);
            assert!(s.toolbar <= s.large && s.large <= s.hero);
        }
    }
}
