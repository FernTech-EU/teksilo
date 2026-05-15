//! Shared plot-area layout math used by BarChart and LineChart.
//!
//! Computes the inner plot rect after carving off space for axis labels,
//! axis titles, and (optionally) a legend band. Charts read this once
//! per paint and use the result for both axis rendering and series
//! placement.

use fern_canvas::Rect;

use crate::axis::AxisConfig;
use crate::style as cs;

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
        (rect.width - y_band_w - cs::PLOT_PADDING_LEADING - cs::PLOT_PADDING_RIGHT)
            .max(0.0),
        (rect.height - x_band_h - cs::PLOT_PADDING_TOP - cs::PLOT_PADDING_BOTTOM).max(0.0),
    );

    PlotArea { plot, legend }
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
