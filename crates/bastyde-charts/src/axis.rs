// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Axis configuration and tick generation.
//!
//! `nice_ticks` implements the Wilkinson / Heckbert nice-numbers algorithm
//! used by matplotlib, d3, and most data-viz libraries. Tick spacings are
//! 1, 2, or 5 × 10^k for the smallest k that yields ≤ `target_count`
//! intervals covering `[min, max]`.

use std::rc::Rc;

/// Axis configuration shared by BarChart and LineChart for both x and y.
#[derive(Clone, Default)]
pub struct AxisConfig {
    pub label: Option<String>,
    pub show_labels: bool,
    pub show_axis_line: bool,
    pub tick_count_hint: Option<usize>,
    pub min: Option<f32>,
    pub max: Option<f32>,
    /// Custom value-to-string formatter. `None` → default `format!("{}", v)`
    /// with a sensible decimal cap.
    pub formatter: Option<Rc<dyn Fn(f32) -> String>>,
    /// Dash pattern `(dash, gap)` for this axis's gridlines, in logical
    /// pixels. `None` (default) draws solid gridlines.
    pub gridline_dash: Option<(f32, f32)>,
    /// Tick-label rotation in degrees, measured clockwise from horizontal.
    ///
    /// `None` (default) lets the chart choose: horizontal while the labels fit, tilted once
    /// they stop fitting. `Some(0.0)` forces horizontal, `Some(90.0)` forces vertical.
    ///
    /// Rotation alone does not make labels fit at arbitrary density — it buys roughly a
    /// factor of `1/sin(angle)` — so whatever angle is in force, the chart still drops every
    /// n-th label if that is what it takes. See [`resolve_label_layout`].
    pub label_angle: Option<f32>,
}

impl AxisConfig {
    pub fn new() -> Self {
        Self {
            label: None,
            show_labels: true,
            show_axis_line: true,
            tick_count_hint: None,
            min: None,
            max: None,
            formatter: None,
            gridline_dash: None,
            label_angle: None,
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn show_labels(mut self, on: bool) -> Self {
        self.show_labels = on;
        self
    }

    pub fn show_axis_line(mut self, on: bool) -> Self {
        self.show_axis_line = on;
        self
    }

    /// Force a tick-label rotation, in degrees clockwise from horizontal. Omit to let the
    /// chart decide (see [`AxisConfig::label_angle`]).
    pub fn label_angle(mut self, degrees: f32) -> Self {
        self.label_angle = Some(degrees);
        self
    }

    pub fn tick_count_hint(mut self, n: usize) -> Self {
        self.tick_count_hint = Some(n);
        self
    }

    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    pub fn formatter(mut self, f: impl Fn(f32) -> String + 'static) -> Self {
        self.formatter = Some(Rc::new(f));
        self
    }

    /// Draw this axis's gridlines dashed, `dash` logical pixels on and
    /// `gap` logical pixels off.
    pub fn gridline_dash(mut self, dash: f32, gap: f32) -> Self {
        self.gridline_dash = Some((dash, gap));
        self
    }

    /// Format `v` for display using the configured formatter, or a default
    /// that drops trailing zeros and caps at 4 decimal places.
    pub fn format(&self, v: f32) -> String {
        if let Some(f) = &self.formatter {
            f(v)
        } else {
            default_format(v)
        }
    }
}

impl std::fmt::Debug for AxisConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AxisConfig")
            .field("label", &self.label)
            .field("show_labels", &self.show_labels)
            .field("show_axis_line", &self.show_axis_line)
            .field("tick_count_hint", &self.tick_count_hint)
            .field("label_angle", &self.label_angle)
            .field("min", &self.min)
            .field("max", &self.max)
            .field("formatter", &self.formatter.as_ref().map(|_| "<fn>"))
            .field("gridline_dash", &self.gridline_dash)
            .finish()
    }
}

fn default_format(v: f32) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let abs = v.abs();
    let s = if abs >= 1000.0 {
        format!("{:.0}", v)
    } else if abs >= 10.0 {
        format!("{:.1}", v)
    } else if abs >= 1.0 {
        format!("{:.2}", v)
    } else {
        format!("{:.3}", v)
    };
    // Trim trailing zeros after the decimal point but keep the integer
    // part intact.
    if s.contains('.') {
        let trimmed = s.trim_end_matches('0').trim_end_matches('.');
        trimmed.to_string()
    } else {
        s
    }
}

/// Generate a "nice" set of tick values covering `[min, max]` using the
/// Wilkinson / Heckbert algorithm. Returns ticks in ascending order, each
/// at a spacing of 1/2/5 × 10^k.
///
/// `target_count` is a hint for the desired number of intervals (so
/// `target_count + 1` ticks). The algorithm picks the smallest spacing
/// from the {1, 2, 5} set times a power of ten that produces no more
/// than `target_count` intervals.
pub fn nice_ticks(min: f32, max: f32, target_count: usize) -> Vec<f32> {
    let target_count = target_count.max(2);

    if !min.is_finite() || !max.is_finite() {
        return vec![0.0];
    }

    if (max - min).abs() < f32::EPSILON {
        // Degenerate range — emit a single tick at the value.
        return vec![min];
    }

    let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
    let range = hi - lo;
    let raw_step = range / target_count as f32;

    // Find magnitude (10^k) of raw_step.
    let exp = raw_step.log10().floor();
    let pow10 = 10.0_f32.powf(exp);
    let frac = raw_step / pow10;

    // Pick nice fraction from {1, 2, 2.5, 5, 10} — the d3 / matplotlib
    // standard set. The 2.5 entry catches 0..100 / target=4 → step=25.
    let nice_frac = if frac < 1.5 {
        1.0
    } else if frac < 2.25 {
        2.0
    } else if frac < 3.5 {
        2.5
    } else if frac < 7.0 {
        5.0
    } else {
        10.0
    };
    let step = nice_frac * pow10;

    // Snap min down and max up to step-aligned positions.
    let nice_min = (lo / step).floor() * step;
    let nice_max = (hi / step).ceil() * step;

    let mut ticks = Vec::new();
    let n = ((nice_max - nice_min) / step).round() as i32 + 1;
    for i in 0..n {
        // Compute via i*step to limit floating drift, then snap tiny
        // residues that arise around zero.
        let v = nice_min + (i as f32) * step;
        let v = if v.abs() < step * 1e-6 { 0.0 } else { v };
        ticks.push(v);
    }
    ticks
}

/// Pick a target number of ticks given an axis pixel length. Roughly
/// `pixels / 60` ticks, clamped to `[2, 10]`.
pub fn auto_tick_count(axis_pixels: f32) -> usize {
    ((axis_pixels / 60.0) as usize).clamp(2, 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_range() {
        // target_count=5 → raw step 20, nice step 20, intervals 5, ticks 6.
        let t = nice_ticks(0.0, 100.0, 5);
        assert_eq!(t, vec![0.0, 20.0, 40.0, 60.0, 80.0, 100.0]);
        // Lower target gives wider step.
        let t = nice_ticks(0.0, 100.0, 4);
        assert_eq!(t, vec![0.0, 25.0, 50.0, 75.0, 100.0]);
    }

    #[test]
    fn negative_zero_crossing() {
        let t = nice_ticks(-30.0, 70.0, 5);
        assert!(t.iter().any(|&v| (v - 0.0).abs() < 1e-4));
        assert!(t.first().copied().unwrap() <= -30.0);
        assert!(t.last().copied().unwrap() >= 70.0);
    }

    #[test]
    fn tiny_range_sub_decimal_ticks() {
        let t = nice_ticks(0.0, 0.003, 5);
        assert!(t.last().copied().unwrap() >= 0.003);
        // The step should be below 0.001
        let step = t[1] - t[0];
        assert!(step <= 0.001 + 1e-7);
    }

    #[test]
    fn zero_range_returns_single_tick() {
        let t = nice_ticks(5.0, 5.0, 5);
        assert_eq!(t, vec![5.0]);
    }

    #[test]
    fn reversed_min_max_handled() {
        let t1 = nice_ticks(0.0, 10.0, 5);
        let t2 = nice_ticks(10.0, 0.0, 5);
        assert_eq!(t1, t2);
    }

    #[test]
    fn format_default_caps_decimals() {
        let cfg = AxisConfig::new();
        assert_eq!(cfg.format(0.0), "0");
        assert_eq!(cfg.format(100.0), "100");
        assert_eq!(cfg.format(0.5), "0.5");
        assert_eq!(cfg.format(0.123456), "0.123");
    }

    #[test]
    fn custom_formatter_overrides_default() {
        let cfg = AxisConfig::new().formatter(|v| format!("${:.0}", v));
        assert_eq!(cfg.format(42.0), "$42");
    }

    #[test]
    fn auto_tick_count_clamps() {
        assert_eq!(auto_tick_count(60.0), 2);
        assert_eq!(auto_tick_count(120.0), 2);
        assert_eq!(auto_tick_count(180.0), 3);
        assert_eq!(auto_tick_count(2000.0), 10);
        assert_eq!(auto_tick_count(0.0), 2);
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Category-label fitting
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// How a chart should draw its category labels so they stay readable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabelLayout {
    /// Rotation in radians, clockwise from horizontal. `0.0` for upright.
    pub angle: f32,
    /// Draw every `stride`-th label. Always `>= 1`.
    pub stride: usize,
}

impl LabelLayout {
    pub fn upright() -> Self {
        Self {
            angle: 0.0,
            stride: 1,
        }
    }
}

/// The tilt tried before dropping labels. 45° is the conventional choice and keeps text
/// readable without the neck-craning of full vertical.
const AUTO_ANGLE_DEGREES: f32 = 45.0;

/// Breathing room between neighbouring labels, as a multiple of the text line height.
const LABEL_CLEARANCE: f32 = 1.15;

/// Decide the angle and stride that keep `n` category labels legible in `plot_width`.
///
/// Two mechanisms, applied in that order, because they cost different things. Rotating
/// costs the reader a head-tilt and the chart some vertical room, and it buys a factor of
/// `1/sin(angle)` in label density — at 45°, about 1.4×. Dropping labels costs information
/// outright. So: stay upright while everything fits, then tilt, and only drop labels when
/// tilting still is not enough.
///
/// It matters that dropping is the last resort *and* that it exists. Rotation is often
/// presented as the fix for a crowded axis, but it only ever multiplies the ceiling — a
/// 37-chapter book in a 400 px pane gives each label an 11 px slot, and no angle makes a
/// 60 px label fit that. Both mechanisms together always converge; either alone does not.
///
/// `forced` overrides the angle choice (from [`AxisConfig::label_angle`]) but never the
/// stride: a caller asking for horizontal labels on a crowded axis gets horizontal labels,
/// thinned enough to read.
pub fn resolve_label_layout(
    n: usize,
    plot_width: f32,
    max_label_width: f32,
    label_height: f32,
    forced: Option<f32>,
) -> LabelLayout {
    if n == 0 || plot_width <= 0.0 {
        return LabelLayout::upright();
    }
    let slot = plot_width / n as f32;

    // Smallest stride that clears neighbours at `angle`. Upright labels are limited by their
    // *width*; tilted ones by their line height measured perpendicular to the baseline.
    let stride_for = |angle: f32| -> usize {
        let needed = if angle.abs() < f32::EPSILON {
            max_label_width
        } else {
            (label_height * LABEL_CLEARANCE) / angle.sin().abs()
        };
        if needed <= slot || slot <= 0.0 {
            1
        } else {
            (needed / slot).ceil() as usize
        }
    };

    if let Some(degrees) = forced {
        let angle = degrees.to_radians();
        return LabelLayout {
            angle,
            stride: stride_for(angle).max(1),
        };
    }

    if stride_for(0.0) == 1 {
        return LabelLayout::upright();
    }
    let angle = AUTO_ANGLE_DEGREES.to_radians();
    LabelLayout {
        angle,
        stride: stride_for(angle).max(1),
    }
}

/// Vertical room a row of labels needs, given the layout [`resolve_label_layout`] chose.
///
/// A tilted label's bounding box is taller than its line height — it is the rotated extent
/// of the whole string — which is why the plot has to give back space when the axis tilts.
pub fn label_band_height(layout: LabelLayout, max_label_width: f32, label_height: f32) -> f32 {
    if layout.angle.abs() < f32::EPSILON {
        label_height
    } else {
        max_label_width * layout.angle.sin().abs() + label_height * layout.angle.cos().abs()
    }
}

/// Draw one category label under the tick at `center_x`, `top` being the top edge of the
/// label band. Shared by both charts so the tilted case exists once.
///
/// An upright label is centred on the tick. A tilted one is anchored at the tick and runs
/// down-left, so the label's *end* sits under the bar it names — the reading order a
/// tilted axis needs.
///
/// ## The transform
///
/// [`Canvas::translate`](bastyde_canvas::Canvas::translate) and
/// [`rotate`](bastyde_canvas::Canvas::rotate) **post-multiply**: they compose in output
/// space, so `translate(x, y)` then `rotate(θ)` spins the already-positioned result about
/// the canvas origin rather than turning the label about its own anchor. Every label lands
/// on a diagonal through the origin, nowhere near its bar.
///
/// [`Canvas::apply_transform`](bastyde_canvas::Canvas::apply_transform) is the
/// pre-multiplying half of that pair — it takes a transform expressed in the coordinates
/// of the content about to be drawn, which is what a local rotate-then-place is. So the
/// local transform is built explicitly and pushed in one go, the same way
/// `bastyde-scene`'s rotated text item does it.
#[allow(clippy::too_many_arguments)]
pub fn draw_category_label(
    canvas: &mut bastyde_canvas::Canvas,
    label: &str,
    layout: LabelLayout,
    center_x: f32,
    top: f32,
    width: f32,
    height: f32,
    style: &bastyde_tokens::TextStyle,
    color: bastyde_tokens::Color,
) {
    use bastyde_canvas::Rect;

    if layout.angle.abs() < f32::EPSILON {
        let rect = Rect::new(center_x - width * 0.5, top, width, height);
        canvas.draw_text(label, rect, style, color);
        return;
    }
    canvas.save();
    canvas.apply_transform(tilted_label_transform(layout.angle, center_x, top));
    canvas.draw_text(
        label,
        Rect::new(-width, -height * 0.5, width, height),
        style,
        color,
    );
    canvas.restore();
}

/// The local transform for a tilted label: turn about the anchor, *then* move the anchor
/// onto the tick. Split out from [`draw_category_label`] because the composition order is
/// the whole subtlety, and this way it can be asserted without a canvas.
pub fn tilted_label_transform(angle: f32, center_x: f32, top: f32) -> bastyde_canvas::Transform2D {
    use bastyde_canvas::Transform2D;
    Transform2D::rotate(-angle).then(&Transform2D::translate(center_x, top))
}

#[cfg(test)]
mod label_layout_tests {
    use super::*;

    #[test]
    fn labels_that_fit_are_left_upright() {
        // 5 labels of 40px in 600px — 120px slots, no crowding.
        let l = resolve_label_layout(5, 600.0, 40.0, 12.0, None);
        assert_eq!(l, LabelLayout::upright());
    }

    /// The case from the bug report: 37 chapters in a 400px pane.
    #[test]
    fn a_crowded_axis_tilts_and_thins() {
        let l = resolve_label_layout(37, 400.0, 60.0, 12.0, None);
        assert!(
            l.angle > 0.0,
            "it must tilt before it starts dropping labels"
        );
        assert!(
            l.stride > 1,
            "tilting alone cannot fit a 60px label in an 11px slot"
        );
    }

    /// Rotation raises the ceiling rather than removing it — the property that makes
    /// "just rotate the labels" an incomplete answer.
    #[test]
    fn tilting_needs_a_smaller_stride_than_staying_upright() {
        let upright = resolve_label_layout(37, 400.0, 60.0, 12.0, Some(0.0));
        let tilted = resolve_label_layout(37, 400.0, 60.0, 12.0, Some(45.0));
        assert!(
            tilted.stride < upright.stride,
            "45° should drop fewer labels than horizontal: {} vs {}",
            tilted.stride,
            upright.stride
        );
    }

    /// Vertical is limited only by line height, so it is the densest option.
    #[test]
    fn vertical_labels_fit_the_most() {
        let l = resolve_label_layout(37, 400.0, 60.0, 12.0, Some(90.0));
        assert_eq!(
            l.stride, 2,
            "an 11px slot fits a 13.8px clearance every other label"
        );
    }

    #[test]
    fn a_forced_angle_is_honoured_but_still_thinned() {
        let l = resolve_label_layout(50, 300.0, 80.0, 12.0, Some(0.0));
        assert_eq!(l.angle, 0.0, "the caller asked for horizontal");
        assert!(l.stride > 1, "...and still gets a readable axis");
    }

    #[test]
    fn a_tilted_band_is_taller_than_an_upright_one() {
        let upright = label_band_height(LabelLayout::upright(), 60.0, 12.0);
        let tilted = label_band_height(
            LabelLayout {
                angle: std::f32::consts::FRAC_PI_4,
                stride: 1,
            },
            60.0,
            12.0,
        );
        assert_eq!(upright, 12.0);
        assert!(
            tilted > upright * 2.0,
            "a 60px label at 45° needs real vertical room"
        );
    }

    #[test]
    fn degenerate_inputs_do_not_divide_by_zero() {
        assert_eq!(
            resolve_label_layout(0, 100.0, 10.0, 12.0, None),
            LabelLayout::upright()
        );
        assert_eq!(
            resolve_label_layout(5, 0.0, 10.0, 12.0, None),
            LabelLayout::upright()
        );
        assert!(resolve_label_layout(5, 100.0, 10.0, 12.0, Some(0.0)).stride >= 1);
    }
}

#[cfg(test)]
mod tilted_label_tests {
    use super::*;
    use bastyde_canvas::{Point, Transform2D};

    const ANGLE: f32 = std::f32::consts::FRAC_PI_4; // 45°
    const TICK_X: f32 = 240.0;
    const BAND_TOP: f32 = 300.0;
    const LABEL_W: f32 = 60.0;

    /// The label's trailing end is its anchor, and the anchor belongs on the tick.
    ///
    /// This is what the bug destroyed: `Canvas::translate` / `rotate` post-multiply in
    /// output space, so translating to the tick and *then* rotating spins the already-
    /// placed label about the canvas origin. Every label landed on one diagonal near the
    /// top-left of the plot instead of under its own bar.
    #[test]
    fn the_anchor_lands_on_the_tick() {
        let t = tilted_label_transform(ANGLE, TICK_X, BAND_TOP);
        let anchor = t.apply_point(Point::new(0.0, 0.0));
        assert!(
            (anchor.x - TICK_X).abs() < 0.01 && (anchor.y - BAND_TOP).abs() < 0.01,
            "anchor landed at {anchor:?}, not on the tick at ({TICK_X}, {BAND_TOP})"
        );
    }

    /// …and the text runs down-LEFT from there, so it reads up towards the bar it names.
    #[test]
    fn the_label_runs_down_and_to_the_left() {
        let t = tilted_label_transform(ANGLE, TICK_X, BAND_TOP);
        let far_end = t.apply_point(Point::new(-LABEL_W, 0.0));
        assert!(
            far_end.x < TICK_X - 1.0,
            "label head is at x={}, not left of the tick",
            far_end.x
        );
        assert!(
            far_end.y > BAND_TOP + 1.0,
            "label head is at y={}, not below the band top (it ran upwards)",
            far_end.y
        );
        // 45° puts it equally far left as down.
        assert!((TICK_X - far_end.x - (far_end.y - BAND_TOP)).abs() < 0.01);
    }

    /// The head must stay inside the band [`label_band_height`] reserved for it, or the
    /// tilted text spills over whatever is drawn beneath the chart.
    #[test]
    fn the_label_stays_within_the_reserved_band() {
        let layout = LabelLayout {
            angle: ANGLE,
            stride: 1,
        };
        let line_h = 12.0;
        let band = label_band_height(layout, LABEL_W, line_h);
        let t = tilted_label_transform(ANGLE, TICK_X, BAND_TOP);
        // Bottom-left corner of the label box is the deepest point.
        let deepest = t.apply_point(Point::new(-LABEL_W, line_h * 0.5));
        assert!(
            deepest.y <= BAND_TOP + band + 0.01,
            "label reaches {} but the band only reserves {}",
            deepest.y - BAND_TOP,
            band
        );
    }

    /// Guards the composition order directly: the output-space order the canvas's own
    /// `translate` + `rotate` produce does NOT keep the anchor on the tick.
    #[test]
    fn the_output_space_order_is_the_one_that_was_wrong() {
        let wrong = Transform2D::translate(TICK_X, BAND_TOP).then(&Transform2D::rotate(-ANGLE));
        let anchor = wrong.apply_point(Point::new(0.0, 0.0));
        assert!(
            (anchor.x - TICK_X).abs() > 1.0 || (anchor.y - BAND_TOP).abs() > 1.0,
            "the fixture no longer distinguishes the two orders"
        );
    }
}
