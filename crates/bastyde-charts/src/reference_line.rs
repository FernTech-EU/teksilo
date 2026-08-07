// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Reference lines: the values a chart is *about*, drawn on it.
//!
//! A chart that sorts, tints or ranks its data against something — a median, a target, a
//! budget, a limit — is asking the reader to judge a distance. Leaving the something
//! undrawn turns that into arithmetic performed from a caption: "the median is 2,495
//! words" above thirty-six bars is a number the reader has to hold in their head and
//! eyeball every bar against, and they will not.
//!
//! Any number of them, any colour, because the comparisons are not interchangeable: a
//! median and a target are different claims about the same axis and should not look alike.
//! Colour is a [`ColorProp`], so a theme role (`TextRole::Secondary`,
//! `SurfaceRole::StatusError`) travels with the theme rather than being frozen to a hex
//! value that only works in one of them.

use bastyde_canvas::{Canvas, Path, Point, Rect, StrokeStyle};
use bastyde_core::Theme;
use bastyde_core::color_prop::ColorProp;
use bastyde_i18n::LocalizedString;
use bastyde_tokens::{TextRole, TextStyle};

use crate::text::measure_text_width;

/// A labelled line across a chart's plot at one value on the value axis.
///
/// Built with [`ReferenceLine::new`] and refined with the builder methods; hand it to a
/// chart's `reference_line`. The default is a thin dashed line in
/// [`TextRole::Secondary`] — legible over bars without competing with them, and visibly
/// not a gridline.
#[derive(Debug, Clone)]
pub struct ReferenceLine {
    /// Where on the value axis to draw. A line outside the axis range is skipped rather
    /// than clamped to the frame, where it would claim a value the chart never reached.
    pub value: f32,
    /// Drawn at the line's leading end. Empty for an unlabelled line.
    pub label: LocalizedString,
    /// `None` uses [`TextRole::Secondary`].
    pub color: Option<ColorProp>,
    pub width: f32,
    /// `Some((dash, gap))` dashes; `None` draws solid.
    pub dash: Option<(f32, f32)>,
}

impl ReferenceLine {
    pub fn new(value: f32, label: impl Into<LocalizedString>) -> Self {
        Self {
            value,
            label: label.into(),
            color: None,
            width: 1.0,
            dash: Some((4.0, 3.0)),
        }
    }

    /// An unlabelled line — for the second and third of a set whose meaning one label
    /// already carries, or where the axis says it.
    pub fn bare(value: f32) -> Self {
        Self::new(value, bastyde_i18n::lit!(String::new()))
    }

    /// Theme role or literal colour. A role is preferable: it follows light/dark.
    pub fn color(mut self, color: impl Into<ColorProp>) -> Self {
        self.color = Some(color.into());
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn dash(mut self, dash: f32, gap: f32) -> Self {
        self.dash = Some((dash, gap));
        self
    }

    pub fn solid(mut self) -> Self {
        self.dash = None;
        self
    }

    pub(crate) fn stroke(&self) -> StrokeStyle {
        match self.dash {
            Some((dash, gap)) => StrokeStyle::dashed(self.width, dash, gap),
            None => StrokeStyle::solid(self.width),
        }
    }

    pub(crate) fn resolved_color(&self, theme: &Theme, enabled: bool) -> bastyde_tokens::Color {
        match &self.color {
            Some(c) => c.resolve(theme, enabled),
            None => TextRole::Secondary.resolve(&theme.colors),
        }
    }
}

/// Which way the value axis runs, so one drawing routine serves a vertical bar chart, a
/// horizontal one, and a line chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValueAxis {
    /// Values increase upward: the line is horizontal. Vertical bars, line charts.
    Vertical,
    /// Values increase rightward: the line is vertical. Horizontal bars.
    Horizontal,
}

/// Draw every line whose value falls inside `[lo, hi]`.
///
/// `to_pixel` maps a value to its pixel on the value axis, so the caller keeps ownership of
/// the scale it already uses for its own marks — the line and the bars cannot disagree
/// about where a value sits.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_reference_lines(
    canvas: &mut Canvas,
    theme: &Theme,
    enabled: bool,
    lines: &[ReferenceLine],
    plot: Rect,
    axis: ValueAxis,
    lo: f32,
    hi: f32,
    label_style: &TextStyle,
) {
    for line in lines {
        let (min, max) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        if line.value < min || line.value > max {
            continue;
        }
        let color = line.resolved_color(theme, enabled);
        let at = to_pixel(line.value, lo, hi, plot, axis);
        let mut path = Path::new();
        match axis {
            ValueAxis::Vertical => {
                path.move_to(Point::new(plot.x, at));
                path.line_to(Point::new(plot.right(), at));
            }
            ValueAxis::Horizontal => {
                path.move_to(Point::new(at, plot.y));
                path.line_to(Point::new(at, plot.bottom()));
            }
        }
        canvas.stroke_path(&path, color, line.stroke());

        let text = line.label.resolve_now();
        if text.is_empty() {
            continue;
        }
        let w = measure_text_width(canvas, &text, label_style);
        let h = label_style.size * 1.2;
        // At the leading end: the one place a long series of bars reliably leaves room,
        // and where the eye already is when it starts reading the axis.
        let rect = match axis {
            ValueAxis::Vertical => Rect::new(plot.x + 4.0, at - h - 2.0, w, h),
            ValueAxis::Horizontal => Rect::new(at + 4.0, plot.y + 2.0, w, h),
        };
        canvas.draw_text(&text, rect, label_style, color);
    }
}

fn to_pixel(value: f32, lo: f32, hi: f32, plot: Rect, axis: ValueAxis) -> f32 {
    let span = hi - lo;
    let t = if span.abs() < f32::EPSILON {
        0.0
    } else {
        (value - lo) / span
    };
    match axis {
        // Screen y grows downward, so a larger value sits nearer the top.
        ValueAxis::Vertical => plot.bottom() - t * plot.height,
        ValueAxis::Horizontal => plot.x + t * plot.width,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLOT: Rect = Rect {
        x: 50.0,
        y: 10.0,
        width: 400.0,
        height: 200.0,
    };

    #[test]
    fn the_default_line_is_a_thin_secondary_dash() {
        let l = ReferenceLine::new(1.0, bastyde_i18n::lit!("m"));
        assert_eq!(l.width, 1.0);
        assert_eq!(l.dash, Some((4.0, 3.0)));
        assert!(l.color.is_none(), "an unset colour must follow the theme");
    }

    #[test]
    fn a_line_can_be_recoloured_widened_and_made_solid() {
        let l = ReferenceLine::new(1.0, bastyde_i18n::lit!("m"))
            .color(bastyde_tokens::TextRole::Primary)
            .width(2.0)
            .solid();
        assert!(l.color.is_some());
        assert_eq!(l.width, 2.0);
        assert_eq!(l.dash, None);
    }

    /// The value maps through the same scale the marks use, so the line lands where a bar
    /// of that height would end.
    #[test]
    fn a_value_maps_to_its_pixel_on_the_value_axis() {
        assert_eq!(to_pixel(0.0, 0.0, 100.0, PLOT, ValueAxis::Vertical), 210.0);
        assert_eq!(to_pixel(100.0, 0.0, 100.0, PLOT, ValueAxis::Vertical), 10.0);
        assert_eq!(to_pixel(50.0, 0.0, 100.0, PLOT, ValueAxis::Vertical), 110.0);
    }

    #[test]
    fn a_horizontal_value_axis_runs_the_other_way() {
        assert_eq!(to_pixel(0.0, 0.0, 100.0, PLOT, ValueAxis::Horizontal), 50.0);
        assert_eq!(
            to_pixel(100.0, 0.0, 100.0, PLOT, ValueAxis::Horizontal),
            450.0
        );
    }

    /// A degenerate domain must not divide by zero and put the line at NaN.
    #[test]
    fn a_flat_domain_pins_the_line_to_the_baseline() {
        let at = to_pixel(5.0, 5.0, 5.0, PLOT, ValueAxis::Vertical);
        assert!(at.is_finite());
        assert_eq!(at, PLOT.bottom());
    }
}
