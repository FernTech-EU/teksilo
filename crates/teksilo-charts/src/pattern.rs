// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Drawing the non-colour series channel — WCAG 1.4.1.
//!
//! [`SeriesPattern`] (in `teksilo-data`) says *what* a series' second channel
//! is; this module draws it. Three renderings, one per shape a chart uses:
//!
//! - [`stroke_style`] — the dash pattern for a line chart's series line.
//! - [`draw_marker`] — the glyph at a data point, and in a legend swatch.
//! - [`fill_hatch`] — the hatch laid over a bar, an area fill, or a legend
//!   swatch.
//!
//! # When a chart draws it
//!
//! [`PatternPolicy::Auto`] — the default — draws the channel exactly when
//! colour is doing identification work: from the **second visible series**
//! onwards. A chart with one series has nothing to tell apart, so dashing its
//! line and hatching its bars would add visual noise carrying no information;
//! a chart with two or more cannot be read without it by anyone who does not
//! see the palette. `Always` and `Never` override the judgement — `Never` is a
//! deliberate accessibility regression and is named plainly so it reads as one
//! at the call site.
//!
//! # Hatch geometry
//!
//! Hatches are parallel strokes clipped to the region being filled. The canvas
//! clips to rectangles only, so hatching works on bars and swatches (both
//! rectangles) and on anything a caller can bound by one. A pie slice is not a
//! rectangle, which is why `PieChart` carries its channel as a centroid marker
//! instead of a hatch.
//!
//! The hatch colour is derived from the fill rather than taken from the theme:
//! a token would have to be either light or dark and would vanish against half
//! the palette. [`hatch_color`] picks black or white by the fill's luminance,
//! at an alpha low enough to read as texture rather than as a second series.

use teksilo_canvas::{Canvas, Path, Point, Rect, StrokeStyle};
use teksilo_data::{SeriesHatch, SeriesMarker, SeriesPattern};
use teksilo_tokens::Color;

/// Spacing between adjacent hatch strokes, in logical pixels. Wide enough that
/// a 4 dp bar still shows the pattern rather than turning into a solid block.
const HATCH_SPACING: f32 = 6.0;
/// Hatch stroke width.
const HATCH_WIDTH: f32 = 1.5;
/// Hatch alpha over the fill — texture, not a second colour.
const HATCH_ALPHA: f32 = 0.55;

/// Whether a chart draws its series' non-colour channel.
///
/// See the [module docs](self) — `Auto` is the default and the accessible one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PatternPolicy {
    /// Draw the channel once more than one series is visible. Default.
    #[default]
    Auto,
    /// Always draw it, even for a single series.
    Always,
    /// Never draw it.
    ///
    /// This leaves colour as the only means of telling series apart, which
    /// fails WCAG 1.4.1 for any chart with more than one series. Reach for it
    /// only where something else in the design already carries the distinction
    /// — direct series labels on the plot, one series per chart in a small
    /// multiple.
    Never,
}

impl PatternPolicy {
    /// Whether to draw the channel with `visible_series` series on the plot.
    pub fn applies(self, visible_series: usize) -> bool {
        match self {
            Self::Auto => visible_series > 1,
            Self::Always => true,
            Self::Never => false,
        }
    }
}

/// The pattern for the series at `index`: its own, or the one its position
/// implies.
pub fn resolve(explicit: Option<SeriesPattern>, index: usize) -> SeriesPattern {
    explicit.unwrap_or_else(|| SeriesPattern::for_index(index))
}

/// The stroke style for a series line: `pattern`'s dash at `width`, or a plain
/// solid stroke when the chart is not drawing the channel.
pub fn stroke_style(pattern: SeriesPattern, width: f32, enabled: bool) -> StrokeStyle {
    match enabled.then(|| pattern.dash(width)).flatten() {
        Some((dash, gap)) => StrokeStyle::dashed(width, dash, gap),
        None => StrokeStyle::solid(width),
    }
}

/// A hatch stroke colour that reads against `fill`.
///
/// Black over a light fill, white over a dark one, both translucent. Derived
/// rather than themed: a fixed token would disappear against half of any
/// palette, and the hatch exists precisely to stay visible.
pub fn hatch_color(fill: Color) -> Color {
    // Rec. 601 luma — the same weighting the tokens crate uses for contrast
    // decisions, and close enough for a "is this fill light or dark" test.
    let luma = 0.299 * fill.r() + 0.587 * fill.g() + 0.114 * fill.b();
    let base = if luma > 0.55 {
        Color::BLACK
    } else {
        Color::WHITE
    };
    base.with_alpha(HATCH_ALPHA)
}

/// Lay `pattern`'s hatch over the rectangle `rect`, already filled with `fill`.
///
/// A no-op for [`SeriesHatch::None`] (the `Solid` pattern) and for a rectangle
/// too small to show one. Clips to `rect`, then restores the previous clip
/// state by clearing it — callers that rely on an outer clip must re-establish
/// it afterwards.
pub fn fill_hatch(canvas: &mut Canvas, rect: Rect, pattern: SeriesPattern, fill: Color) {
    let hatch = pattern.hatch();
    if hatch == SeriesHatch::None || rect.width < 1.0 || rect.height < 1.0 {
        return;
    }
    let color = hatch_color(fill);
    canvas.set_clip(rect);
    for path in hatch_paths(rect, hatch) {
        canvas.stroke_path(&path, color, StrokeStyle::solid(HATCH_WIDTH));
    }
    canvas.clear_clip();
}

/// The unclipped stroke paths making up `hatch` across `rect`.
///
/// Split out from [`fill_hatch`] so the geometry is testable without a canvas.
fn hatch_paths(rect: Rect, hatch: SeriesHatch) -> Vec<Path> {
    let mut paths = Vec::new();
    let (x0, y0) = (rect.x, rect.y);
    let (x1, y1) = (rect.x + rect.width, rect.y + rect.height);

    match hatch {
        SeriesHatch::None => {}
        SeriesHatch::Horizontal => {
            let mut y = y0 + HATCH_SPACING * 0.5;
            while y < y1 {
                paths.push(Path::line(Point::new(x0, y), Point::new(x1, y)));
                y += HATCH_SPACING;
            }
        }
        SeriesHatch::Vertical => {
            let mut x = x0 + HATCH_SPACING * 0.5;
            while x < x1 {
                paths.push(Path::line(Point::new(x, y0), Point::new(x, y1)));
                x += HATCH_SPACING;
            }
        }
        SeriesHatch::Forward | SeriesHatch::Backward | SeriesHatch::Cross => {
            // A 45° family is swept by walking the x-intercept from
            // `-height` to `width`, so every line that crosses the rectangle
            // is emitted regardless of aspect ratio. Each segment is drawn
            // corner-to-corner past the edges and clipped by the caller.
            let span = rect.width + rect.height;
            let step = HATCH_SPACING * std::f32::consts::SQRT_2;
            let mut offset = -rect.height;
            while offset < rect.width {
                if matches!(hatch, SeriesHatch::Forward | SeriesHatch::Cross) {
                    paths.push(Path::line(
                        Point::new(x0 + offset, y1),
                        Point::new(x0 + offset + span, y1 - span),
                    ));
                }
                if matches!(hatch, SeriesHatch::Backward | SeriesHatch::Cross) {
                    paths.push(Path::line(
                        Point::new(x0 + offset, y0),
                        Point::new(x0 + offset + span, y0 + span),
                    ));
                }
                offset += step;
            }
        }
    }
    paths
}

/// Draw `pattern`'s marker glyph centred at `center` with radius `radius`.
///
/// Filled, in the series' own colour — the *shape* is the channel, so the
/// colour stays the series' own and the glyph reads as part of the line.
pub fn draw_marker(
    canvas: &mut Canvas,
    center: Point,
    radius: f32,
    marker: SeriesMarker,
    color: Color,
) {
    let r = radius.max(1.0);
    match marker {
        SeriesMarker::Circle => canvas.fill_circle(center, r, color),
        SeriesMarker::Square => canvas.fill_rect(
            Rect::new(center.x - r, center.y - r, r * 2.0, r * 2.0),
            color,
        ),
        SeriesMarker::Triangle => {
            // Slightly enlarged: an equilateral triangle inscribed in a circle
            // of radius r covers about half the area of the circle, so at the
            // same r it reads as a smaller mark than the others.
            let t = r * 1.25;
            canvas.fill_path(
                &Path::polygon(&[
                    Point::new(center.x, center.y - t),
                    Point::new(center.x + t * 0.87, center.y + t * 0.5),
                    Point::new(center.x - t * 0.87, center.y + t * 0.5),
                ]),
                color,
            );
        }
        SeriesMarker::Diamond => {
            let t = r * 1.25;
            canvas.fill_path(
                &Path::polygon(&[
                    Point::new(center.x, center.y - t),
                    Point::new(center.x + t, center.y),
                    Point::new(center.x, center.y + t),
                    Point::new(center.x - t, center.y),
                ]),
                color,
            );
        }
        SeriesMarker::Cross => {
            let t = r * 1.2;
            let w = StrokeStyle::solid((r * 0.8).max(1.0));
            canvas.stroke_path(
                &Path::line(
                    Point::new(center.x - t, center.y - t),
                    Point::new(center.x + t, center.y + t),
                ),
                color,
                w.clone(),
            );
            canvas.stroke_path(
                &Path::line(
                    Point::new(center.x - t, center.y + t),
                    Point::new(center.x + t, center.y - t),
                ),
                color,
                w,
            );
        }
        SeriesMarker::Plus => {
            let t = r * 1.3;
            let w = StrokeStyle::solid((r * 0.8).max(1.0));
            canvas.stroke_path(
                &Path::line(
                    Point::new(center.x - t, center.y),
                    Point::new(center.x + t, center.y),
                ),
                color,
                w.clone(),
            );
            canvas.stroke_path(
                &Path::line(
                    Point::new(center.x, center.y - t),
                    Point::new(center.x, center.y + t),
                ),
                color,
                w,
            );
        }
    }
}

/// How a legend swatch samples its series.
///
/// The swatch has to look like what the plot draws, or the legend hands the
/// reader a second mapping to learn — which is the whole failure the pattern
/// channel exists to fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegendSwatch {
    /// A filled chip carrying the series' hatch. Matches a bar or a pie slice.
    Block,
    /// A short line sample carrying the series' dash, with the marker glyph at
    /// its centre when the plot draws markers. Matches a line series.
    Line {
        /// Whether the plot draws point markers (`LineChart::show_points`).
        marker: bool,
    },
    /// A filled chip stamped with the marker glyph in a contrasting tone.
    /// Matches a pie slice, which carries the same glyph at its centroid — a
    /// wedge is not a rectangle, so it cannot be hatched by the rect clip.
    Marked,
}

/// Draw one legend swatch for `pattern` in `color` inside `rect`.
///
/// `patterned` is the chart's live [`PatternPolicy`] verdict: `false` falls
/// back to the plain colour chip this drew before the channel existed, so the
/// legend never advertises a distinction the plot is not making.
pub fn draw_legend_swatch(
    canvas: &mut Canvas,
    rect: Rect,
    swatch: LegendSwatch,
    pattern: SeriesPattern,
    color: Color,
    patterned: bool,
) {
    use teksilo_tokens::CornerRadius;

    match swatch {
        LegendSwatch::Block => {
            canvas.fill_rounded_rect(rect, CornerRadius::uniform(2.0), color);
            if patterned {
                fill_hatch(canvas, rect, pattern, color);
            }
        }
        LegendSwatch::Marked => {
            canvas.fill_rounded_rect(rect, CornerRadius::uniform(2.0), color);
            if patterned {
                draw_marker(
                    canvas,
                    Point::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5),
                    rect.height * 0.28,
                    pattern.marker(),
                    hatch_color(color),
                );
            }
        }
        LegendSwatch::Line { marker } => {
            let center = Point::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5);
            let width = (rect.height * 0.22).max(1.5);
            // The sample is short, so the dash is scaled to the sample rather
            // than to the plot's line width — at 10 dp a plot-scale dash shows
            // barely one segment and every pattern looks alike.
            let stroke = stroke_style(pattern, width, patterned);
            canvas.stroke_path(
                &Path::line(
                    Point::new(rect.x, center.y),
                    Point::new(rect.x + rect.width, center.y),
                ),
                color,
                stroke,
            );
            if marker {
                draw_marker(
                    canvas,
                    center,
                    rect.height * 0.3,
                    if patterned {
                        pattern.marker()
                    } else {
                        SeriesMarker::Circle
                    },
                    color,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_holds_off_until_colour_is_doing_work() {
        assert!(
            !PatternPolicy::Auto.applies(1),
            "a single-series chart has nothing to disambiguate"
        );
        assert!(
            PatternPolicy::Auto.applies(2),
            "two series cannot be told apart by colour alone (WCAG 1.4.1)"
        );
        assert!(PatternPolicy::Always.applies(1));
        assert!(!PatternPolicy::Never.applies(9));
    }

    #[test]
    fn an_unset_pattern_falls_back_to_the_series_position() {
        assert_eq!(resolve(None, 1), SeriesPattern::for_index(1));
        assert_eq!(
            resolve(Some(SeriesPattern::Dotted), 1),
            SeriesPattern::Dotted,
            "an explicit pattern must survive the series' position"
        );
    }

    #[test]
    fn a_disabled_channel_strokes_solid_whatever_the_pattern_says() {
        assert_eq!(
            stroke_style(SeriesPattern::Dashed, 2.0, false).dash_pattern,
            None
        );
        assert!(
            stroke_style(SeriesPattern::Dashed, 2.0, true)
                .dash_pattern
                .is_some()
        );
        assert_eq!(
            stroke_style(SeriesPattern::Solid, 2.0, true).dash_pattern,
            None,
            "the Solid pattern is solid even when the channel is on"
        );
    }

    #[test]
    fn the_hatch_contrasts_with_the_fill_it_covers() {
        assert_eq!(hatch_color(Color::WHITE).r(), Color::BLACK.r());
        assert_eq!(hatch_color(Color::BLACK).r(), Color::WHITE.r());
        assert!(
            hatch_color(Color::WHITE).a() < 1.0,
            "a fully opaque hatch would read as a second series, not a texture"
        );
    }

    /// The union of every stroke's bounds.
    fn swept(paths: &[Path]) -> Rect {
        paths.iter().fold(paths[0].bounds(), |acc, p| {
            let b = p.bounds();
            let x0 = acc.x.min(b.x);
            let y0 = acc.y.min(b.y);
            let x1 = (acc.x + acc.width).max(b.x + b.width);
            let y1 = (acc.y + acc.height).max(b.y + b.height);
            Rect::new(x0, y0, x1 - x0, y1 - y0)
        })
    }

    #[test]
    fn the_diagonal_sweep_reaches_every_corner_of_a_tall_rectangle() {
        // The failure this pins: a 45° sweep whose x-intercept starts at the
        // rectangle's own left edge misses the whole bottom-left region of a
        // rectangle taller than it is wide — the exact shape of a bar. The
        // sweep must therefore start a full `height` to the left.
        let rect = Rect::new(10.0, 20.0, 30.0, 120.0);
        for hatch in [
            SeriesHatch::Forward,
            SeriesHatch::Backward,
            SeriesHatch::Cross,
        ] {
            let paths = hatch_paths(rect, hatch);
            assert!(!paths.is_empty(), "{hatch:?} produced no strokes");
            let union = swept(&paths);
            assert!(
                union.x <= rect.x
                    && union.y <= rect.y
                    && union.x + union.width >= rect.x + rect.width
                    && union.y + union.height >= rect.y + rect.height,
                "{hatch:?} sweeps {union:?}, which does not cover {rect:?}"
            );
        }
    }

    #[test]
    fn axis_aligned_hatches_are_evenly_spread_across_the_rectangle() {
        // These are inset by half a period at each end by design — evenly
        // distributed lines, not lines flush with the edges — so the property
        // is that no run of bare pixels exceeds one hatch period.
        let rect = Rect::new(10.0, 20.0, 30.0, 120.0);
        for hatch in [SeriesHatch::Horizontal, SeriesHatch::Vertical] {
            let paths = hatch_paths(rect, hatch);
            assert!(paths.len() > 1, "{hatch:?} produced too few strokes");
            let union = swept(&paths);
            assert!(
                union.x >= rect.x - 0.01 && union.y >= rect.y - 0.01,
                "{hatch:?} strokes outside the rectangle: {union:?}"
            );
            assert!(
                union.x - rect.x <= HATCH_SPACING
                    && union.y - rect.y <= HATCH_SPACING
                    && (rect.x + rect.width) - (union.x + union.width) <= HATCH_SPACING
                    && (rect.y + rect.height) - (union.y + union.height) <= HATCH_SPACING,
                "{hatch:?} leaves a bare band wider than one period: {union:?} vs {rect:?}"
            );
        }
    }

    #[test]
    fn a_degenerate_rectangle_draws_nothing() {
        assert!(hatch_paths(Rect::new(0.0, 0.0, 0.0, 0.0), SeriesHatch::Cross).is_empty());
        assert!(hatch_paths(Rect::new(0.0, 0.0, 40.0, 10.0), SeriesHatch::None).is_empty());
    }
}
