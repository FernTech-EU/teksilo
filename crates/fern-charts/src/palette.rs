//! Series color palette resolution.
//!
//! Charts pick per-series colors in this priority order:
//!
//! 1. The series's own `color: Some(ColorProp)` if set.
//! 2. The chart's `palette` (defaults to `ChartPalette::FromTheme`,
//!    which reads `theme.colors.chart_palette` — Okabe-Ito by default).
//! 3. Black, as the last-resort fallback if both are empty.

use fern_core::Theme;
use fern_tokens::Color;

/// A series color palette. `FromTheme` is the typical choice — it tracks
/// the active theme's `chart_palette`. `Custom` lets a chart override
/// just for itself without touching the theme.
#[derive(Debug, Clone, Default)]
pub enum ChartPalette {
    /// Use `theme.colors.chart_palette` (Okabe-Ito by default; theme can override).
    #[default]
    FromTheme,
    /// Use this fixed list of colors instead of the theme palette.
    Custom(Vec<Color>),
}

impl ChartPalette {
    /// Resolve the color for series at `index`, wrapping if `index` exceeds
    /// the palette length. Returns `Color::BLACK` if both the chart palette
    /// and the theme palette are empty (defensive fallback).
    pub fn color_for(&self, index: usize, theme: &Theme) -> Color {
        let palette: &[Color] = match self {
            ChartPalette::FromTheme => &theme.colors.chart_palette,
            ChartPalette::Custom(v) => v,
        };
        if palette.is_empty() {
            Color::BLACK
        } else {
            palette[index % palette.len()]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_theme_reads_okabe_ito() {
        let theme = fern_core::presets::intui::light();
        let palette = ChartPalette::FromTheme;
        // Series 0 = Okabe-Ito orange (#E69F00)
        let c0 = palette.color_for(0, &theme);
        assert_eq!(c0, Color::from_hex("#E69F00"));
    }

    #[test]
    fn wraps_when_index_exceeds_palette_len() {
        let theme = fern_core::presets::intui::light();
        let palette = ChartPalette::FromTheme;
        let len = theme.colors.chart_palette.len();
        // index = len wraps to 0
        assert_eq!(palette.color_for(len, &theme), palette.color_for(0, &theme));
    }

    #[test]
    fn custom_overrides_theme() {
        let theme = fern_core::presets::intui::light();
        let palette = ChartPalette::Custom(vec![Color::RED, Color::BLUE]);
        assert_eq!(palette.color_for(0, &theme), Color::RED);
        assert_eq!(palette.color_for(1, &theme), Color::BLUE);
        assert_eq!(palette.color_for(2, &theme), Color::RED); // wraps
    }

    #[test]
    fn empty_custom_falls_back_to_black() {
        let theme = fern_core::presets::intui::light();
        let palette = ChartPalette::Custom(vec![]);
        assert_eq!(palette.color_for(0, &theme), Color::BLACK);
    }
}
