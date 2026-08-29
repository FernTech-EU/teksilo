// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`SeriesPattern`] — the non-colour channel that identifies a chart series.
//!
//! A chart that tells its series apart by colour and nothing else fails WCAG
//! 1.4.1 (Use of Color), *whatever* palette it uses. A CVD-safe palette like
//! Okabe–Ito answers a different question — whether the colours are
//! distinguishable from one another — and does not answer this one: a reader
//! with monochrome vision, a monochrome printout, a display in bright sun, or a
//! forced-colours setting has no colour channel at all. It also does not answer
//! the wrap-around problem, where a ninth series repeats the first's colour
//! exactly.
//!
//! So every series carries a second, orthogonal identity: a **pattern**. One
//! value drives all three renderings a chart needs, so a series looks like
//! *itself* whether it is drawn as a line, a bar, a slice, or a legend swatch:
//!
//! | | line | marker | filled area |
//! | --- | --- | --- | --- |
//! | [`Solid`](SeriesPattern::Solid) | solid | circle | plain |
//! | [`Dashed`](SeriesPattern::Dashed) | long dash | square | 45° hatch |
//! | [`Dotted`](SeriesPattern::Dotted) | dotted | triangle | back-hatch |
//! | [`DashDot`](SeriesPattern::DashDot) | dash-dot | diamond | cross-hatch |
//! | [`ShortDash`](SeriesPattern::ShortDash) | short dash | cross | horizontal |
//! | [`WideDash`](SeriesPattern::WideDash) | wide dash | plus | vertical |
//!
//! Six patterns against the theme palette's eight colours means the pair
//! `(colour, pattern)` does not repeat until the 24th series — where colour
//! alone repeated at the 9th.
//!
//! A series with no explicit pattern is assigned one from its position by
//! [`SeriesPattern::for_index`], so the channel exists without any application
//! code. Whether a chart *draws* it is the chart's decision (the stock charts
//! draw it once more than one series is visible, since a single-series chart
//! has nothing to disambiguate).

/// The non-colour visual channel identifying one chart series.
///
/// See the [module docs](self) for the rendering table and the reasoning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SeriesPattern {
    /// Unbroken line, round marker, plain fill.
    #[default]
    Solid,
    /// Long dash, square marker, forward (45°) hatch.
    Dashed,
    /// Dotted line, triangular marker, back (135°) hatch.
    Dotted,
    /// Dash-dot line, diamond marker, cross-hatch.
    DashDot,
    /// Short dash, ×-shaped marker, horizontal hatch.
    ShortDash,
    /// Wide-spaced dash, +-shaped marker, vertical hatch.
    WideDash,
}

/// The marker glyph drawn at a line chart's data points, and next to a series
/// in a legend. Shape, not colour — that is the whole point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SeriesMarker {
    Circle,
    Square,
    Triangle,
    Diamond,
    Cross,
    Plus,
}

/// How a filled region (a bar, an area, a pie slice) carries its series'
/// pattern. `None` is a plain fill; the rest are line hatches at the named
/// angle, drawn in a contrasting tone over the fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SeriesHatch {
    /// No hatch — a plain fill.
    None,
    /// Parallel lines rising to the right (45°).
    Forward,
    /// Parallel lines falling to the right (135°).
    Backward,
    /// Both diagonals.
    Cross,
    /// Parallel horizontal lines.
    Horizontal,
    /// Parallel vertical lines.
    Vertical,
}

impl SeriesPattern {
    /// Every pattern, in assignment order. The order is the cycle
    /// [`for_index`](Self::for_index) walks.
    pub const ALL: [SeriesPattern; 6] = [
        SeriesPattern::Solid,
        SeriesPattern::Dashed,
        SeriesPattern::Dotted,
        SeriesPattern::DashDot,
        SeriesPattern::ShortDash,
        SeriesPattern::WideDash,
    ];

    /// The pattern a series at `index` gets when it declares none.
    ///
    /// Wraps, like [`ChartPalette::color_for`](../teksilo_charts/palette) does
    /// with colours — but at a different period (6 against the theme palette's
    /// 8), so the wrap points do not coincide and `(colour, pattern)` stays
    /// unique far longer than either channel alone.
    pub fn for_index(index: usize) -> Self {
        Self::ALL[index % Self::ALL.len()]
    }

    /// The dash pattern for a stroked line, as `(dash, gap)` in logical
    /// pixels, or `None` for an unbroken line.
    ///
    /// Scaled by `line_width` so a 1 dp line and a 4 dp line read as the same
    /// pattern rather than the thick one looking almost solid.
    pub fn dash(self, line_width: f32) -> Option<(f32, f32)> {
        let w = line_width.max(0.5);
        match self {
            Self::Solid => None,
            Self::Dashed => Some((w * 4.0, w * 2.5)),
            Self::Dotted => Some((w * 0.9, w * 1.8)),
            // Approximated as a dash: the canvas dash model is a single
            // (on, off) pair, so a true dash-dot would need a 4-element
            // pattern. Kept visually distinct from the others by length.
            Self::DashDot => Some((w * 6.0, w * 2.0)),
            Self::ShortDash => Some((w * 2.0, w * 1.5)),
            Self::WideDash => Some((w * 3.0, w * 5.0)),
        }
    }

    /// The marker glyph for this pattern.
    pub fn marker(self) -> SeriesMarker {
        match self {
            Self::Solid => SeriesMarker::Circle,
            Self::Dashed => SeriesMarker::Square,
            Self::Dotted => SeriesMarker::Triangle,
            Self::DashDot => SeriesMarker::Diamond,
            Self::ShortDash => SeriesMarker::Cross,
            Self::WideDash => SeriesMarker::Plus,
        }
    }

    /// The hatch for a filled region carrying this pattern.
    pub fn hatch(self) -> SeriesHatch {
        match self {
            Self::Solid => SeriesHatch::None,
            Self::Dashed => SeriesHatch::Forward,
            Self::Dotted => SeriesHatch::Backward,
            Self::DashDot => SeriesHatch::Cross,
            Self::ShortDash => SeriesHatch::Horizontal,
            Self::WideDash => SeriesHatch::Vertical,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_six_series_get_six_distinct_patterns() {
        let assigned: Vec<SeriesPattern> = (0..6).map(SeriesPattern::for_index).collect();
        let unique: std::collections::HashSet<_> = assigned.iter().collect();
        assert_eq!(
            unique.len(),
            6,
            "each of the first six series must be distinguishable without colour"
        );
    }

    #[test]
    fn the_pattern_cycle_does_not_share_a_period_with_the_palette() {
        // The theme palette holds 8 colours and wraps there; patterns wrap at
        // 6. Coinciding periods would put series 0 and series 8 on the same
        // colour AND the same pattern, which is the wrap bug the second
        // channel exists to fix.
        assert_ne!(
            SeriesPattern::for_index(0),
            SeriesPattern::for_index(8),
            "a ninth series must not repeat the first in both channels"
        );
    }

    #[test]
    fn every_pattern_carries_all_three_renderings() {
        // A pattern that produced a distinct dash but a shared marker would
        // leave point-only and bar charts colour-only.
        let dashes: std::collections::HashSet<_> = SeriesPattern::ALL
            .iter()
            .map(|p| p.dash(2.0).map(|(d, g)| (d.to_bits(), g.to_bits())))
            .collect();
        let markers: std::collections::HashSet<_> =
            SeriesPattern::ALL.iter().map(|p| p.marker()).collect();
        let hatches: std::collections::HashSet<_> =
            SeriesPattern::ALL.iter().map(|p| p.hatch()).collect();
        assert_eq!(dashes.len(), 6, "line dashes must all differ");
        assert_eq!(markers.len(), 6, "markers must all differ");
        assert_eq!(hatches.len(), 6, "hatches must all differ");
    }

    #[test]
    fn dashes_scale_with_the_line_width() {
        // A 4 dp line dashed on a 1 dp scale reads as very nearly solid.
        let thin = SeriesPattern::Dashed.dash(1.0).unwrap();
        let thick = SeriesPattern::Dashed.dash(4.0).unwrap();
        assert!(thick.0 > thin.0 && thick.1 > thin.1);
    }
}
