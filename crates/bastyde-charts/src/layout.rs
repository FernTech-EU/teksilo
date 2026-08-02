// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared plot-area layout math used by BarChart and LineChart.
//!
//! Computes the inner plot rect after carving off space for axis labels,
//! axis titles, and (optionally) a legend band. Charts read this once
//! per paint and use the result for both axis rendering and series
//! placement.

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::{Point, Rect, Size, TextBackend};
use bastyde_tokens::TextStyle;

use crate::axis::{AxisConfig, auto_tick_count, nice_ticks};
use crate::style as cs;
use crate::text::measure_text_width_via;

/// Where the legend sits relative to the plot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegendPosition {
    Top,
    Bottom,
    Leading,
    Trailing,
}

/// Plot-area carve result.
#[derive(Debug, Clone, Copy)]
pub struct PlotArea {
    /// Inner rectangle where the data series are drawn.
    pub plot: Rect,
    /// Legend band (zero-area Rect when no legend was reserved).
    pub legend: Rect,
    /// Width of the carved y-axis band (labels + tick + axis title), in
    /// logical pixels. `0.0` when nothing was reserved on that edge.
    pub y_band_w: f32,
    /// Height of the carved x-axis band (labels + tick + axis title), in
    /// logical pixels. `0.0` when nothing was reserved on that edge.
    pub x_band_h: f32,
}

/// Inputs for plot-area computation. Charts populate this once and pass
/// to [`carve_plot_area`].
pub struct CarveParams<'a> {
    pub bounds: Rect,
    pub axis_x: &'a AxisConfig,
    pub axis_y: &'a AxisConfig,
    /// Maximum width of any y-axis tick label, in logical pixels. Pass 0 if
    /// the y axis has no labels.
    pub y_label_max_width: f32,
    /// Line height of x-axis tick labels, in logical pixels. Pass 0 if
    /// the x axis has no labels.
    pub x_label_height: f32,
    /// Line height for axis titles. Same value for x and y; pass 0 if
    /// neither axis has a title.
    pub axis_title_line_height: f32,
    /// Reserved size for the legend along the relevant axis (height for
    /// Top/Bottom, width for Leading/Trailing). Pass 0 to skip.
    pub legend_size: f32,
    pub legend_position: Option<LegendPosition>,
}

pub fn carve_plot_area(p: &CarveParams) -> PlotArea {
    let mut rect = p.bounds;
    let mut legend = Rect::ZERO;

    // 1. Legend band (carved off first so the rest of the layout fits the
    //    smaller remaining rect).
    if let Some(pos) = p.legend_position
        && p.legend_size > 0.0
    {
        match pos {
            LegendPosition::Top => {
                let h = p.legend_size + cs::LEGEND_TO_PLOT_GAP;
                legend = Rect::new(rect.x, rect.y, rect.width, p.legend_size);
                rect = Rect::new(rect.x, rect.y + h, rect.width, (rect.height - h).max(0.0));
            }
            LegendPosition::Bottom => {
                let h = p.legend_size + cs::LEGEND_TO_PLOT_GAP;
                legend = Rect::new(
                    rect.x,
                    rect.bottom() - p.legend_size,
                    rect.width,
                    p.legend_size,
                );
                rect = Rect::new(rect.x, rect.y, rect.width, (rect.height - h).max(0.0));
            }
            LegendPosition::Leading => {
                let w = p.legend_size + cs::LEGEND_TO_PLOT_GAP;
                legend = Rect::new(rect.x, rect.y, p.legend_size, rect.height);
                rect = Rect::new(rect.x + w, rect.y, (rect.width - w).max(0.0), rect.height);
            }
            LegendPosition::Trailing => {
                let w = p.legend_size + cs::LEGEND_TO_PLOT_GAP;
                legend = Rect::new(
                    rect.right() - p.legend_size,
                    rect.y,
                    p.legend_size,
                    rect.height,
                );
                rect = Rect::new(rect.x, rect.y, (rect.width - w).max(0.0), rect.height);
            }
        }
    }

    // 2. Y-axis band (leading edge).
    let mut y_band_w = 0.0;
    if p.axis_y.show_labels && p.y_label_max_width > 0.0 {
        y_band_w += p.y_label_max_width;
        if p.axis_y.show_axis_line {
            y_band_w += cs::AXIS_TICK_LENGTH;
        }
        y_band_w += cs::AXIS_LABEL_GAP;
    } else if p.axis_y.show_axis_line {
        y_band_w += cs::AXIS_TICK_LENGTH;
    }
    if p.axis_y.label.is_some() && p.axis_title_line_height > 0.0 {
        y_band_w += p.axis_title_line_height + cs::AXIS_TITLE_GAP;
    }

    // 3. X-axis band (bottom edge).
    let mut x_band_h = 0.0;
    if p.axis_x.show_labels && p.x_label_height > 0.0 {
        x_band_h += p.x_label_height;
        if p.axis_x.show_axis_line {
            x_band_h += cs::AXIS_TICK_LENGTH;
        }
        x_band_h += cs::AXIS_LABEL_GAP;
    } else if p.axis_x.show_axis_line {
        x_band_h += cs::AXIS_TICK_LENGTH;
    }
    if p.axis_x.label.is_some() && p.axis_title_line_height > 0.0 {
        x_band_h += p.axis_title_line_height + cs::AXIS_TITLE_GAP;
    }

    // 4. Inner plot padding.
    let plot = Rect::new(
        rect.x + y_band_w + cs::PLOT_PADDING_LEADING,
        rect.y + cs::PLOT_PADDING_TOP,
        (rect.width - y_band_w - cs::PLOT_PADDING_LEADING - cs::PLOT_PADDING_RIGHT).max(0.0),
        (rect.height - x_band_h - cs::PLOT_PADDING_TOP - cs::PLOT_PADDING_BOTTOM).max(0.0),
    );

    PlotArea {
        plot,
        legend,
        y_band_w,
        x_band_h,
    }
}

/// Single-pass plot geometry: carved plot rect + legend band + the
/// y-axis ticks fitted to that carved rect. Extraction of the
/// "provisional `nice_ticks` off `bounds.height` for label-width
/// measurement → `carve_plot_area` → final `nice_ticks` off `plot.height`"
/// two-pass dance previously duplicated in `bar_chart.rs` and
/// `line_chart.rs`. Both charts now share one algorithm — including
/// honoring `axis_y.tick_count_hint` in the provisional pass (previously
/// `LineChart`'s provisional measurement ignored the hint, which could
/// under/over-reserve the y-label band width when a hint diverged
/// sharply from the auto tick count).
#[derive(Debug, Clone, PartialEq)]
pub struct PlotGeometry {
    pub plot: Rect,
    pub legend: Rect,
    pub y_ticks: Vec<f32>,
    pub y_lo: f32,
    pub y_hi: f32,
}

/// Inputs for [`compute_plot_geometry`].
pub struct PlotGeometryParams<'a> {
    pub bounds: Rect,
    pub axis_x: &'a AxisConfig,
    pub axis_y: &'a AxisConfig,
    pub y_domain: (f32, f32),
    pub legend_size: f32,
    pub legend_position: Option<LegendPosition>,
    pub text_backend: Option<&'a Rc<RefCell<dyn TextBackend>>>,
    pub label_style: &'a TextStyle,
    /// The category labels the x axis will draw. Needed here, not just at paint time,
    /// because a tilted label's bounding box is taller than its line height — the plot has
    /// to hand back the difference or the labels are clipped.
    pub x_labels: &'a [String],
}

pub fn compute_plot_geometry(p: &PlotGeometryParams) -> PlotGeometry {
    let (y_min, y_max) = p.y_domain;

    // Provisional pass: nice_ticks off `bounds.height` (pre-carve) so we
    // can measure the widest y-label string and reserve a matching band.
    let provisional_target = p
        .axis_y
        .tick_count_hint
        .unwrap_or_else(|| auto_tick_count(p.bounds.height));
    let provisional_ticks = nice_ticks(y_min, y_max, provisional_target);
    let y_label_max_width = if p.axis_y.show_labels {
        provisional_ticks
            .iter()
            .map(|t| measure_text_width_via(p.text_backend, &p.axis_y.format(*t), p.label_style))
            .fold(0.0_f32, f32::max)
    } else {
        0.0
    };
    let label_height = if p.axis_x.show_labels || p.axis_y.show_labels {
        p.label_style.size * 1.2
    } else {
        0.0
    };
    let title_height = p.label_style.size * 1.2;

    // The x band's height depends on whether the labels tilt, which depends on how much
    // width they have, which depends on the band. Carved twice for the same reason the y
    // ticks are fitted twice: once provisionally to learn the width, then for real.
    let carve = |x_band: f32| {
        carve_plot_area(&CarveParams {
            bounds: p.bounds,
            axis_x: p.axis_x,
            axis_y: p.axis_y,
            y_label_max_width,
            x_label_height: x_band,
            axis_title_line_height: title_height,
            legend_size: p.legend_size,
            legend_position: p.legend_position,
        })
    };
    let mut area = carve(label_height);
    if p.axis_x.show_labels && !p.x_labels.is_empty() {
        let widest = p
            .x_labels
            .iter()
            .map(|l| measure_text_width_via(p.text_backend, l, p.label_style))
            .fold(0.0_f32, f32::max);
        let layout = crate::axis::resolve_label_layout(
            p.x_labels.len(),
            area.plot.width,
            widest,
            label_height,
            p.axis_x.label_angle,
        );
        let band = crate::axis::label_band_height(layout, widest, label_height);
        if band > label_height {
            area = carve(band);
        }
    }

    let plot = area.plot;
    // Final pass: nice_ticks refitted to the carved plot rect.
    let final_target = p
        .axis_y
        .tick_count_hint
        .unwrap_or_else(|| auto_tick_count(plot.height));
    let y_ticks = nice_ticks(y_min, y_max, final_target);
    let y_lo = y_ticks.first().copied().unwrap_or(y_min);
    let y_hi = y_ticks.last().copied().unwrap_or(y_max);

    PlotGeometry {
        plot,
        legend: area.legend,
        y_ticks,
        y_lo,
        y_hi,
    }
}

/// Pie/donut disc geometry: the carved plot rect + legend band, plus the
/// disc's center and inner/outer radii. Wraps `PieChart`'s pre-refactor
/// `compute_plot_rect` + `compute_disc_geometry` combination so
/// `place_children` and `paint` (and now `accessibility`) always agree
/// on where the disc sits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PieGeometry {
    pub plot: Rect,
    pub legend: Rect,
    pub center: Point,
    pub outer_radius: f32,
    pub inner_radius: f32,
}

/// Inputs for [`compute_pie_geometry`].
pub struct PieGeometryParams {
    pub bounds: Rect,
    pub legend_size: f32,
    pub legend_position: Option<LegendPosition>,
    /// `0.0` for a solid pie; `> 0.0` for a donut (fraction of the outer
    /// radius the hole occupies).
    pub inner_radius_ratio: f32,
}

pub fn compute_pie_geometry(p: &PieGeometryParams) -> PieGeometry {
    let no_axis = AxisConfig::new().show_labels(false).show_axis_line(false);
    let area = carve_plot_area(&CarveParams {
        bounds: p.bounds,
        axis_x: &no_axis,
        axis_y: &no_axis,
        y_label_max_width: 0.0,
        x_label_height: 0.0,
        axis_title_line_height: 0.0,
        legend_size: p.legend_size,
        legend_position: p.legend_position,
    });
    let plot = area.plot;

    let pad = cs::PIE_PADDING;
    let usable_w = (plot.width - pad * 2.0).max(0.0);
    let usable_h = (plot.height - pad * 2.0).max(0.0);
    let diameter = usable_w.min(usable_h);
    let center = Point::new(plot.x + plot.width * 0.5, plot.y + plot.height * 0.5);
    if diameter <= 0.0 {
        return PieGeometry {
            plot,
            legend: area.legend,
            center,
            outer_radius: 0.0,
            inner_radius: 0.0,
        };
    }
    let outer = diameter * 0.5;
    let inner = if p.inner_radius_ratio > 0.0 {
        outer * p.inner_radius_ratio
    } else {
        0.0
    };
    PieGeometry {
        plot,
        legend: area.legend,
        center,
        outer_radius: outer,
        inner_radius: inner,
    }
}

/// Shared intrinsic-minimum-size heuristic for `BarChart`/`LineChart`'s
/// `layout_response` (`LayoutResponse::shrinkable`'s `min`): the carved
/// axis bands — measured at a generous probe bounds so label-width
/// measurement isn't itself clipped by a tight incoming proposal — plus
/// a caller-supplied plot floor, plus any Leading/Trailing (adds width)
/// or Top/Bottom (adds height) legend reservation.
#[allow(clippy::too_many_arguments)]
pub(crate) fn compute_intrinsic_min(
    axis_x: &AxisConfig,
    axis_y: &AxisConfig,
    y_domain: (f32, f32),
    legend_size: f32,
    legend_position: Option<LegendPosition>,
    text_backend: Option<&Rc<RefCell<dyn TextBackend>>>,
    label_style: &TextStyle,
    plot_floor: Size,
) -> Size {
    let probe = Rect::new(0.0, 0.0, 2000.0, 2000.0);
    let (y_min, y_max) = y_domain;
    let target = axis_y
        .tick_count_hint
        .unwrap_or_else(|| auto_tick_count(probe.height));
    let ticks = nice_ticks(y_min, y_max, target);
    let y_label_max_width = if axis_y.show_labels {
        ticks
            .iter()
            .map(|t| measure_text_width_via(text_backend, &axis_y.format(*t), label_style))
            .fold(0.0_f32, f32::max)
    } else {
        0.0
    };
    let label_height = if axis_x.show_labels || axis_y.show_labels {
        label_style.size * 1.2
    } else {
        0.0
    };
    let title_height = label_style.size * 1.2;

    let area = carve_plot_area(&CarveParams {
        bounds: probe,
        axis_x,
        axis_y,
        y_label_max_width,
        x_label_height: label_height,
        axis_title_line_height: title_height,
        legend_size,
        legend_position,
    });

    let extra_w = if matches!(
        legend_position,
        Some(LegendPosition::Leading) | Some(LegendPosition::Trailing)
    ) {
        legend_size + cs::LEGEND_TO_PLOT_GAP
    } else {
        0.0
    };
    let extra_h = if matches!(
        legend_position,
        Some(LegendPosition::Top) | Some(LegendPosition::Bottom)
    ) {
        legend_size + cs::LEGEND_TO_PLOT_GAP
    } else {
        0.0
    };

    Size::new(
        area.y_band_w
            + cs::PLOT_PADDING_LEADING
            + cs::PLOT_PADDING_RIGHT
            + plot_floor.width
            + extra_w,
        area.x_band_h
            + cs::PLOT_PADDING_TOP
            + cs::PLOT_PADDING_BOTTOM
            + plot_floor.height
            + extra_h,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_params(bounds: Rect) -> CarveParams<'static> {
        // We deliberately leak owned configs — tests are short-lived.
        let ax: &'static AxisConfig = Box::leak(Box::new(AxisConfig::new()));
        let ay: &'static AxisConfig = Box::leak(Box::new(AxisConfig::new()));
        CarveParams {
            bounds,
            axis_x: ax,
            axis_y: ay,
            y_label_max_width: 0.0,
            x_label_height: 0.0,
            axis_title_line_height: 0.0,
            legend_size: 0.0,
            legend_position: None,
        }
    }

    #[test]
    fn plot_fills_bounds_when_no_axes_or_legend() {
        let bounds = Rect::new(0.0, 0.0, 200.0, 100.0);
        let mut p = default_params(bounds);
        // Disable axis lines too so nothing carves off space.
        let ax = AxisConfig::new().show_axis_line(false);
        let ay = AxisConfig::new().show_axis_line(false);
        p.axis_x = Box::leak(Box::new(ax));
        p.axis_y = Box::leak(Box::new(ay));
        let area = carve_plot_area(&p);
        assert!(area.plot.width > 0.0);
        assert!(area.plot.height > 0.0);
        assert!(area.plot.width < bounds.width);
        assert!(area.plot.height < bounds.height);
    }

    #[test]
    fn legend_band_top_carves_top_strip() {
        let bounds = Rect::new(0.0, 0.0, 200.0, 100.0);
        let mut p = default_params(bounds);
        p.legend_size = 20.0;
        p.legend_position = Some(LegendPosition::Top);
        let area = carve_plot_area(&p);
        assert_eq!(area.legend.y, 0.0);
        assert_eq!(area.legend.height, 20.0);
        assert!(area.plot.y > 20.0);
    }

    #[test]
    fn legend_band_trailing_shrinks_width() {
        let bounds = Rect::new(0.0, 0.0, 200.0, 100.0);
        let mut p = default_params(bounds);
        p.legend_size = 50.0;
        p.legend_position = Some(LegendPosition::Trailing);
        let area = carve_plot_area(&p);
        assert!(area.plot.right() <= 150.0 + 0.01);
        assert_eq!(area.legend.width, 50.0);
    }

    #[test]
    fn axis_label_widths_carve_y_band() {
        let bounds = Rect::new(0.0, 0.0, 200.0, 100.0);
        let mut p = default_params(bounds);
        p.y_label_max_width = 30.0;
        let area_with = carve_plot_area(&p);
        p.y_label_max_width = 0.0;
        let area_without = carve_plot_area(&p);
        assert!(area_without.plot.width > area_with.plot.width);
    }
}
