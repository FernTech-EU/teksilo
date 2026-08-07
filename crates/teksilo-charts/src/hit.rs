// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared per-datum mark geometry, pointer hit-testing, synthetic
//! accessibility-node emission, and hover-tooltip rendering for
//! `BarChart` / `LineChart` / `PieChart`.
//!
//! Each chart's `paint()` computes a fresh `Vec<MarkGeometry>` describing
//! every visible datum's on-screen shape (a bar rect, a line-point, or a
//! pie/donut slice). The same vector drives three consumers: pointer
//! hover hit-testing ([`nearest_point`] / [`rect_hit`] / [`slice_hit`]),
//! the shared hover-tooltip card ([`draw_mark_tooltip`]), and per-datum
//! AT nodes ([`emit_mark_node`]). `(series_id, point_idx)` is the natural
//! key into a [`teksilo_data::ChartSelection`] — no separate lookup
//! structure is needed.

use teksilo_canvas::{Canvas, Point, Rect, StrokeStyle};
use teksilo_core::accessibility::{AccessNodeBuilder, SyntheticKind};
use teksilo_core::accesskit;
use teksilo_core::styles::{BorderRecipe, BorderStyle, Theme};
use teksilo_data::SeriesId;
use teksilo_tokens::{CornerRadius, TextStyle};

use crate::style as cs;
use crate::text::measure_text_width;

/// One datum's on-screen shape + identity, recomputed fresh on every
/// paint by every chart kind.
#[derive(Debug, Clone)]
pub struct MarkGeometry {
    pub series_id: SeriesId,
    pub point_idx: usize,
    pub series_name: String,
    pub category_label: String,
    pub value: f32,
    pub shape: MarkShape,
}

/// The on-screen shape of one mark. All coordinates are window-space
/// (the same space `paint()` draws into).
#[derive(Debug, Clone, Copy)]
pub enum MarkShape {
    /// A line-chart data point.
    Point { center: Point, radius: f32 },
    /// A bar-chart bar.
    Rect(Rect),
    /// A pie/donut slice. `start_rad`/`sweep_rad` are in the same
    /// screen-space angle convention as `f32::atan2(dy, dx)` (0 = the
    /// screen +x / 3 o'clock direction, increasing angle sweeps toward
    /// +y — visually clockwise on a y-down screen). `sweep_rad` may be
    /// negative for a counter-clockwise sweep.
    Slice {
        center: Point,
        inner_radius: f32,
        outer_radius: f32,
        start_rad: f32,
        sweep_rad: f32,
    },
}

impl MarkShape {
    /// Axis-aligned bounding rect in window space. Used both for AT node
    /// bounds ([`emit_mark_node`]) and as a general-purpose fallback.
    pub fn bounding_rect(&self) -> Rect {
        match *self {
            MarkShape::Point { center, radius } => Rect::new(
                center.x - radius,
                center.y - radius,
                radius * 2.0,
                radius * 2.0,
            ),
            MarkShape::Rect(r) => r,
            MarkShape::Slice {
                center,
                inner_radius,
                outer_radius,
                start_rad,
                sweep_rad,
            } => slice_bounding_rect(center, inner_radius, outer_radius, start_rad, sweep_rad),
        }
    }
}

/// Bounding box of an annular (or pie, when `inner_radius <= 0`) wedge.
/// Samples both endpoints of the sweep plus every cardinal angle
/// (0, π/2, π, 3π/2) the sweep crosses, at both radii — the exact set of
/// points that can extend the AA bbox beyond the endpoints alone.
fn slice_bounding_rect(
    center: Point,
    inner_radius: f32,
    outer_radius: f32,
    start_rad: f32,
    sweep_rad: f32,
) -> Rect {
    let two_pi = std::f32::consts::TAU;
    // Normalize to a non-negative sweep so cardinal-angle membership is
    // a simple `s <= a <= s + sw` test.
    let (s, sw) = if sweep_rad < 0.0 {
        (start_rad + sweep_rad, -sweep_rad)
    } else {
        (start_rad, sweep_rad)
    };
    if sw >= two_pi - 1e-4 {
        // Full circle/annulus — the outer radius alone bounds it.
        return Rect::new(
            center.x - outer_radius,
            center.y - outer_radius,
            outer_radius * 2.0,
            outer_radius * 2.0,
        );
    }

    let mut angles: Vec<f32> = vec![s, s + sw];
    for k in 0..4 {
        let cardinal = k as f32 * std::f32::consts::FRAC_PI_2;
        let mut a = cardinal;
        while a < s {
            a += two_pi;
        }
        if a <= s + sw {
            angles.push(a);
        }
    }

    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    let mut include = |p: Point| {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    };
    let radii = [inner_radius.max(0.0), outer_radius.max(0.0)];
    for &a in &angles {
        for &r in &radii {
            if r <= 0.0 {
                include(center);
            } else {
                include(Point::new(center.x + r * a.cos(), center.y + r * a.sin()));
            }
        }
    }
    Rect::new(
        min_x,
        min_y,
        (max_x - min_x).max(0.0),
        (max_y - min_y).max(0.0),
    )
}

/// Nearest [`MarkShape::Point`] mark to `p` by squared distance — no
/// radius cutoff (matches `LineChart`'s pre-refactor "always pick the
/// closest point" semantics; the caller gates on plot-rect containment
/// separately).
pub fn nearest_point(marks: &[MarkGeometry], p: Point) -> Option<usize> {
    let mut best_idx = None;
    let mut best_d2 = f32::INFINITY;
    for (i, m) in marks.iter().enumerate() {
        if let MarkShape::Point { center, .. } = m.shape {
            let dx = center.x - p.x;
            let dy = center.y - p.y;
            let d2 = dx * dx + dy * dy;
            if d2 < best_d2 {
                best_d2 = d2;
                best_idx = Some(i);
            }
        }
    }
    best_idx
}

/// First [`MarkShape::Rect`] mark containing `p` (`None` in a gap
/// between bars).
pub fn rect_hit(marks: &[MarkGeometry], p: Point) -> Option<usize> {
    marks
        .iter()
        .position(|m| matches!(m.shape, MarkShape::Rect(r) if r.contains(p)))
}

/// First [`MarkShape::Slice`] mark whose sweep contains `test_angle`
/// (same screen-space angle convention as [`MarkShape::Slice`]).
pub fn slice_hit(marks: &[MarkGeometry], test_angle: f32) -> Option<usize> {
    marks.iter().position(|m| {
        if let MarkShape::Slice {
            start_rad,
            sweep_rad,
            ..
        } = m.shape
        {
            let (s, sw) = if sweep_rad < 0.0 {
                (start_rad + sweep_rad, -sweep_rad)
            } else {
                (start_rad, sweep_rad)
            };
            angle_in_sweep(test_angle, s, sw)
        } else {
            false
        }
    })
}

/// Whether `angle` (in 0..2π) lies inside `[start, start + sweep]`, both
/// normalized to 0..2π. `sweep` must be non-negative — callers with a
/// signed sweep normalize before calling (see [`slice_hit`]).
pub fn angle_in_sweep(angle: f32, start: f32, sweep: f32) -> bool {
    let two_pi = std::f32::consts::TAU;
    let s = start.rem_euclid(two_pi);
    let mut e = (start + sweep).rem_euclid(two_pi);
    let a = angle.rem_euclid(two_pi);
    if (sweep - two_pi).abs() < 1e-4 {
        return true;
    }
    if e < s {
        e += two_pi;
    }
    let a_lifted = if a < s { a + two_pi } else { a };
    a_lifted >= s && a_lifted <= e
}

/// Stable per-`(series, point)` synthetic-node element id. `SeriesId`
/// has no public raw-integer accessor (it's an opaque SlotMap key
/// wrapper, deliberately not `Ord`/numeric), so this hashes the
/// `(SeriesId, usize)` pair with `DefaultHasher` — deterministic within
/// a process run (which is all `synthetic_node_id`'s stability contract
/// requires: the same mark must keep the same AT node id across repeated
/// `accessibility()` walks in one program execution).
pub(crate) fn mark_element_id(series_id: SeriesId, point_idx: usize) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    series_id.hash(&mut hasher);
    point_idx.hash(&mut hasher);
    hasher.finish()
}

/// Emit one synthetic `SyntheticKind::ChartMark` AT child node for `m` on
/// `builder` (the chart widget's own accessibility builder).
pub(crate) fn emit_mark_node(builder: &mut AccessNodeBuilder, m: &MarkGeometry) {
    let element_id = mark_element_id(m.series_id, m.point_idx);
    let bounds = m.shape.bounding_rect();
    let name = format!("{}, {}: {}", m.series_name, m.category_label, m.value);
    builder.push_scene_child(element_id, SyntheticKind::ChartMark, |child| {
        child.set_role(accesskit::Role::GraphicsObject);
        child.set_name(name);
        child.set_numeric_value(m.value as f64);
        child.inner_mut().set_bounds(accesskit::Rect {
            x0: bounds.x as f64,
            y0: bounds.y as f64,
            x1: (bounds.x + bounds.width) as f64,
            y1: (bounds.y + bounds.height) as f64,
        });
    });
}

/// Draw the shared hover-tooltip card used by all three chart kinds:
/// `text` centered above `anchor`, flipped below / clamped horizontally
/// and vertically so it never clips outside `plot`. Extracted from
/// `LineChart::draw_hover`'s pre-refactor tooltip-card painting.
pub(crate) fn draw_mark_tooltip(
    canvas: &mut Canvas,
    theme: &Theme,
    plot: Rect,
    anchor: Point,
    text: &str,
    label_style: &TextStyle,
) {
    let text_w = measure_text_width(canvas, text, label_style);
    let approx_w = text_w + cs::TOOLTIP_PADDING * 2.0;
    let height = label_style.size * 1.4 + cs::TOOLTIP_PADDING;

    let mut tx = anchor.x - approx_w * 0.5;
    let mut ty = anchor.y - height - 8.0;
    if ty < plot.y {
        ty = anchor.y + 8.0;
    }
    if tx < plot.x {
        tx = plot.x;
    }
    if tx + approx_w > plot.right() {
        tx = plot.right() - approx_w;
    }
    if ty + height > plot.bottom() {
        ty = plot.bottom() - height;
    }

    let tip = Rect::new(tx, ty, approx_w, height);
    canvas.fill_rounded_rect(tip, CornerRadius::uniform(4.0), theme.colors.tooltip_bg);
    canvas.stroke_rounded_rect(
        tip,
        CornerRadius::uniform(4.0),
        theme.colors.tooltip_border,
        1.0,
    );

    let label_rect = Rect::new(
        tip.x + cs::TOOLTIP_PADDING,
        tip.y + (tip.height - label_style.size * 1.2) * 0.5,
        tip.width - cs::TOOLTIP_PADDING * 2.0,
        label_style.size * 1.2,
    );
    canvas.draw_text(text, label_rect, label_style, theme.colors.tooltip_text);
}

/// Resolve a [`BorderRecipe`] (from `ChartStyle::gridline`) plus an
/// optional per-axis dash override (`AxisConfig::gridline_dash`) into a
/// concrete [`StrokeStyle`] ready for `Canvas::stroke_path`. The dash
/// override wins over the recipe's own [`BorderStyle`] when set.
///
/// Gridlines are drawn via `stroke_path` (Tier 3, CPU-rasterized through
/// tiny-skia) rather than `Canvas::draw_line` (Tier 1) because
/// `draw_line`'s `StrokeSpace::Logical` branch bakes a plain
/// `DecorationRect` and never reads `StrokeStyle::dash_pattern` — dashing
/// would be silently dropped. `stroke_path`'s `PathEntry.stroke_style` is
/// carried through to `tiny_skia::StrokeDash` by the path atlas
/// rasterizer, so it is the only draw path that actually honors dashing.
pub(crate) fn resolve_gridline_stroke(
    recipe: &BorderRecipe,
    dash_override: Option<(f32, f32)>,
) -> StrokeStyle {
    if let Some((dash, gap)) = dash_override {
        return StrokeStyle::dashed(recipe.width, dash, gap);
    }
    match recipe.style {
        BorderStyle::Solid => StrokeStyle::solid(recipe.width),
        BorderStyle::Dashed { dash, gap } => StrokeStyle::dashed(recipe.width, dash, gap),
        BorderStyle::Dotted { gap } => StrokeStyle::dotted(recipe.width, gap),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(series_id: SeriesId, point_idx: usize, shape: MarkShape) -> MarkGeometry {
        MarkGeometry {
            series_id,
            point_idx,
            series_name: "S".into(),
            category_label: "C".into(),
            value: 1.0,
            shape,
        }
    }

    fn fake_series_id() -> SeriesId {
        let model: teksilo_data::ChartModel<i32> = teksilo_data::ChartModel::new();
        model.add_series("s")
    }

    #[test]
    fn point_bounding_rect_is_square_around_center() {
        let shape = MarkShape::Point {
            center: Point::new(10.0, 20.0),
            radius: 3.0,
        };
        let r = shape.bounding_rect();
        assert_eq!((r.x, r.y, r.width, r.height), (7.0, 17.0, 6.0, 6.0));
    }

    #[test]
    fn rect_bounding_rect_is_self() {
        let rect = Rect::new(1.0, 2.0, 3.0, 4.0);
        let shape = MarkShape::Rect(rect);
        assert_eq!(shape.bounding_rect(), rect);
    }

    #[test]
    fn slice_bounding_rect_crosses_cardinal_angle() {
        // Sweep from -10deg to +10deg crosses the 0-rad cardinal angle
        // (screen +x / 3 o'clock). The bbox must reach exactly
        // center.x + outer_radius on the right edge — a sample at only
        // the two endpoints would fall short (cos(10deg) < 1.0).
        let center = Point::new(100.0, 100.0);
        let outer = 50.0;
        let start = (-10.0_f32).to_radians();
        let sweep = (20.0_f32).to_radians();
        let shape = MarkShape::Slice {
            center,
            inner_radius: 0.0,
            outer_radius: outer,
            start_rad: start,
            sweep_rad: sweep,
        };
        let r = shape.bounding_rect();
        assert!(
            (r.right() - (center.x + outer)).abs() < 1e-3,
            "bbox right edge should reach the cardinal-angle radius, got {:?}",
            r
        );
    }

    #[test]
    fn slice_bounding_rect_handles_negative_sweep() {
        let center = Point::new(0.0, 0.0);
        let shape = MarkShape::Slice {
            center,
            inner_radius: 0.0,
            outer_radius: 10.0,
            start_rad: 0.0,
            sweep_rad: (-20.0_f32).to_radians(),
        };
        let r = shape.bounding_rect();
        // Sweeping -20deg from 0 covers [-20deg, 0], right edge still at
        // full radius (0 rad is one endpoint).
        assert!((r.right() - 10.0).abs() < 1e-3);
    }

    #[test]
    fn nearest_point_ignores_non_point_shapes_and_no_radius_cutoff() {
        let id = fake_series_id();
        let marks = vec![
            mark(id, 0, MarkShape::Rect(Rect::new(0.0, 0.0, 5.0, 5.0))),
            mark(
                id,
                1,
                MarkShape::Point {
                    center: Point::new(100.0, 100.0),
                    radius: 3.0,
                },
            ),
        ];
        // Far away — still returns the only Point mark (no radius cutoff).
        let idx = nearest_point(&marks, Point::new(0.0, 0.0));
        assert_eq!(idx, Some(1));
    }

    #[test]
    fn rect_hit_finds_containing_bar_and_none_in_gap() {
        let id = fake_series_id();
        let marks = vec![
            mark(id, 0, MarkShape::Rect(Rect::new(0.0, 0.0, 10.0, 10.0))),
            mark(id, 1, MarkShape::Rect(Rect::new(20.0, 0.0, 10.0, 10.0))),
        ];
        assert_eq!(rect_hit(&marks, Point::new(5.0, 5.0)), Some(0));
        assert_eq!(rect_hit(&marks, Point::new(25.0, 5.0)), Some(1));
        assert_eq!(rect_hit(&marks, Point::new(15.0, 5.0)), None);
    }

    #[test]
    fn slice_hit_finds_slice_and_handles_negative_sweep() {
        let id = fake_series_id();
        let marks = [
            mark(
                id,
                0,
                MarkShape::Slice {
                    center: Point::ZERO,
                    inner_radius: 0.0,
                    outer_radius: 10.0,
                    start_rad: 0.0,
                    sweep_rad: std::f32::consts::FRAC_PI_2,
                },
            ),
            mark(
                id,
                1,
                MarkShape::Slice {
                    center: Point::ZERO,
                    inner_radius: 0.0,
                    outer_radius: 10.0,
                    start_rad: 0.0,
                    sweep_rad: -std::f32::consts::FRAC_PI_2,
                },
            ),
        ];
        assert_eq!(slice_hit(&marks[..1], std::f32::consts::FRAC_PI_4), Some(0));
        assert_eq!(
            slice_hit(&marks[1..2], -std::f32::consts::FRAC_PI_4),
            Some(0)
        );
    }

    #[test]
    fn angle_in_sweep_wraps_across_zero() {
        // start close to TAU, sweep pushes past the wrap point.
        let start = std::f32::consts::TAU - 0.1;
        let sweep = 0.3;
        assert!(angle_in_sweep(0.05, start, sweep));
        assert!(!angle_in_sweep(1.0, start, sweep));
    }

    #[test]
    fn mark_element_id_is_stable_and_differs_by_point() {
        let id = fake_series_id();
        let a = mark_element_id(id, 0);
        let b = mark_element_id(id, 0);
        let c = mark_element_id(id, 1);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn gridline_stroke_dash_override_wins_over_recipe_style() {
        let recipe = BorderRecipe {
            width: 1.0,
            color: teksilo_core::styles::RecipeColor::Static(teksilo_tokens::Color::BLACK),
            style: BorderStyle::Solid,
            position: teksilo_core::styles::BorderPosition::Center,
            sides: None,
        };
        let stroke = resolve_gridline_stroke(&recipe, Some((4.0, 2.0)));
        assert_eq!(stroke.dash_pattern, Some(vec![4.0, 2.0]));
    }

    #[test]
    fn gridline_stroke_falls_back_to_recipe_dashed_style() {
        let recipe = BorderRecipe {
            width: 1.0,
            color: teksilo_core::styles::RecipeColor::Static(teksilo_tokens::Color::BLACK),
            style: BorderStyle::Dashed {
                dash: 3.0,
                gap: 1.0,
            },
            position: teksilo_core::styles::BorderPosition::Center,
            sides: None,
        };
        let stroke = resolve_gridline_stroke(&recipe, None);
        assert_eq!(stroke.dash_pattern, Some(vec![3.0, 1.0]));
    }

    #[test]
    fn gridline_stroke_solid_has_no_dash_pattern() {
        let recipe = BorderRecipe {
            width: 1.0,
            color: teksilo_core::styles::RecipeColor::Static(teksilo_tokens::Color::BLACK),
            style: BorderStyle::Solid,
            position: teksilo_core::styles::BorderPosition::Center,
            sides: None,
        };
        let stroke = resolve_gridline_stroke(&recipe, None);
        assert_eq!(stroke.dash_pattern, None);
    }
}
