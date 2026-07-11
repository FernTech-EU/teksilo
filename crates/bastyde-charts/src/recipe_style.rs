// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `RecipeChartStyle` — the default [`ChartStyle`] implementation.
//!
//! Produces the same flat-color chrome bastyde-charts always painted
//! before Tier-3 styling landed: a solid fill resolved from the series
//! color (bars / donut), the same solid color at a caller-given opacity
//! (line-chart area fill), and a theme-`BorderRole::Default`-at-40%
//! gridline. Apps that want gradient bars/areas/donuts implement
//! [`ChartStyle`] themselves and install it per-chart (`.style(...)`) or
//! theme-wide (`theme.style_slots.chart = Some(Rc::new(...))`).

use bastyde_core::styles::{
    BorderPosition, BorderRecipe, BorderStyle, ChartFillContext, ChartStyle, FillRecipe,
    RecipeColor, Theme,
};
use bastyde_tokens::BorderRole;

use crate::style::GRIDLINE_WIDTH;

/// The shipped default [`ChartStyle`]. Stateless — safe to share via a
/// single `Rc<RecipeChartStyle>` (or construct fresh per chart, it's
/// zero-sized).
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeChartStyle;

impl ChartStyle for RecipeChartStyle {
    fn bar_fill(&self, cfg: &ChartFillContext) -> FillRecipe {
        FillRecipe::Solid(RecipeColor::Static(cfg.resolved_color))
    }

    fn area_fill(&self, cfg: &ChartFillContext, opacity: f32) -> FillRecipe {
        FillRecipe::Solid(RecipeColor::Static(cfg.resolved_color.with_alpha(opacity)))
    }

    fn donut_fill(&self, cfg: &ChartFillContext) -> FillRecipe {
        FillRecipe::Solid(RecipeColor::Static(cfg.resolved_color))
    }

    fn gridline(&self, theme: &Theme) -> BorderRecipe {
        BorderRecipe {
            width: GRIDLINE_WIDTH,
            color: RecipeColor::Static(BorderRole::Default.resolve(&theme.colors).with_alpha(0.4)),
            style: BorderStyle::Solid,
            position: BorderPosition::Center,
            sides: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_tokens::Color;

    fn ctx(theme: &Theme) -> ChartFillContext<'_> {
        ChartFillContext {
            series_index: 0,
            resolved_color: Color::RED,
            theme,
        }
    }

    #[test]
    fn bar_and_donut_fills_are_solid_with_resolved_color() {
        let theme = bastyde_core::presets::intui::light();
        let style = RecipeChartStyle;
        let cfg = ctx(&theme);
        match style.bar_fill(&cfg) {
            FillRecipe::Solid(RecipeColor::Static(c)) => assert_eq!(c, Color::RED),
            other => panic!("expected solid, got {other:?}"),
        }
        match style.donut_fill(&cfg) {
            FillRecipe::Solid(RecipeColor::Static(c)) => assert_eq!(c, Color::RED),
            other => panic!("expected solid, got {other:?}"),
        }
    }

    #[test]
    fn area_fill_applies_opacity() {
        let theme = bastyde_core::presets::intui::light();
        let style = RecipeChartStyle;
        let cfg = ctx(&theme);
        match style.area_fill(&cfg, 0.15) {
            FillRecipe::Solid(RecipeColor::Static(c)) => assert!((c.a() - 0.15).abs() < 1e-4),
            other => panic!("expected solid, got {other:?}"),
        }
    }

    #[test]
    fn gridline_is_default_border_role_at_low_alpha() {
        let theme = bastyde_core::presets::intui::light();
        let style = RecipeChartStyle;
        let recipe = style.gridline(&theme);
        assert_eq!(recipe.width, GRIDLINE_WIDTH);
        assert_eq!(recipe.position, BorderPosition::Center);
        match recipe.color {
            RecipeColor::Static(c) => assert!((c.a() - 0.4).abs() < 1e-4),
            other => panic!("expected static color, got {other:?}"),
        }
    }
}
