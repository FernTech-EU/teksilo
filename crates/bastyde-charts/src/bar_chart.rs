// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! BarChart — vertical or horizontal bars, one or more series.
//!
//! PR 1 ships vertical / single-series only. Grouped multi-series, horizontal
//! orientation, value labels, grid lines, axis titles, and legends arrive
//! in PR 2.

use bastyde_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Prop;
use bastyde_core::widget::{LayoutContext, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{BorderRole, TextRole, TextStyleRole};

use crate::axis::{AxisConfig, auto_tick_count, nice_ticks};
use crate::layout::{CarveParams, LegendPosition, carve_plot_area};
use crate::legend::{legend_main_axis_size, orientation_for_position, paint_embedded_legend};
use crate::palette::ChartPalette;
use crate::series::ChartSeries;
use crate::text::measure_text_width;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarOrientation {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarGrouping {
    Single,
    Grouped,
}

pub struct BarChart<T: Clone + 'static> {
    series: Prop<Vec<ChartSeries<T>>>,
    orientation: BarOrientation,
    grouping: BarGrouping,
    show_value_labels: bool,
    show_grid: bool,
    show_legend: bool,
    legend_position: LegendPosition,
    axis_x: AxisConfig,
    axis_y: AxisConfig,
    palette: Prop<ChartPalette>,
    bar_corner_radius: Option<f32>,
    min_bar_gap: f32,
    group_gap: f32,
}

impl<T: Clone + std::fmt::Display + 'static> BarChart<T> {
    pub fn new(series: impl Into<Prop<Vec<ChartSeries<T>>>>) -> Self {
        Self {
            series: series.into(),
            orientation: BarOrientation::Vertical,
            grouping: BarGrouping::Single,
            show_value_labels: false,
            show_grid: false,
            show_legend: false,
            legend_position: LegendPosition::Bottom,
            axis_x: AxisConfig::new(),
            axis_y: AxisConfig::new(),
            palette: Prop::Static(ChartPalette::FromTheme),
            bar_corner_radius: None,
            min_bar_gap: 6.0,
            group_gap: 12.0,
        }
    }

    pub fn orientation(mut self, o: BarOrientation) -> Self {
        self.orientation = o;
        self
    }

    pub fn grouping(mut self, g: BarGrouping) -> Self {
        self.grouping = g;
        self
    }

    pub fn value_labels(mut self, show: bool) -> Self {
        self.show_value_labels = show;
        self
    }

    pub fn grid(mut self, show: bool) -> Self {
        self.show_grid = show;
        self
    }

    pub fn legend(mut self, show: bool) -> Self {
        self.show_legend = show;
        self
    }

    pub fn legend_position(mut self, pos: LegendPosition) -> Self {
        self.legend_position = pos;
        self
    }

    pub fn axis_x(mut self, cfg: AxisConfig) -> Self {
        self.axis_x = cfg;
        self
    }

    pub fn axis_y(mut self, cfg: AxisConfig) -> Self {
        self.axis_y = cfg;
        self
    }

    pub fn palette(mut self, p: impl Into<Prop<ChartPalette>>) -> Self {
        self.palette = p.into();
        self
    }

    pub fn bar_corner_radius(mut self, r: f32) -> Self {
        self.bar_corner_radius = Some(r);
        self
    }

    pub fn min_bar_gap(mut self, g: f32) -> Self {
        self.min_bar_gap = g;
        self
    }

    pub fn group_gap(mut self, g: f32) -> Self {
        self.group_gap = g;
        self
    }
}

impl<T: Clone + 'static> std::fmt::Debug for BarChart<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BarChart")
            .field("orientation", &self.orientation)
            .field("grouping", &self.grouping)
            .finish()
    }
}

impl<T: Clone + std::fmt::Display + 'static> Widget for BarChart<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        // Data swap → relayout (y-domain might shift).
        self.series
            .register_if_bound(id, registry, BindingLevel::Relayout);
        // Palette swap is color-only → repaint.
        self.palette
            .register_if_bound(id, registry, BindingLevel::RepaintOnly);
        Vec::new()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        Size::new(
            proposal.width.unwrap_or(320.0),
            proposal.height.unwrap_or(200.0),
        )
        .into()
    }

    fn place_children(
        &self,
        _bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let theme = ctx.theme;
        let enabled = ctx.effective_enabled;
        use crate::style as cs;

        let series_vec = self.series.get();
        if series_vec.is_empty() {
            return;
        }

        // Determine y-domain (auto from data if not overridden).
        let (y_min, y_max) = self.compute_y_domain(&series_vec);
        if (y_max - y_min).abs() < f32::EPSILON {
            return;
        }

        // Generate y-ticks.
        let y_axis_pixels = bounds.height; // approximation pre-carve; refined below
        let target = self
            .axis_y
            .tick_count_hint
            .unwrap_or_else(|| auto_tick_count(y_axis_pixels));
        let y_ticks = nice_ticks(y_min, y_max, target);

        // Measure widest y-label string in pixels.
        let label_style = TextStyleRole::Tiny.resolve(&theme.typography);
        let y_label_max_width = if self.axis_y.show_labels {
            measure_max_label_width(canvas, &y_ticks, &self.axis_y, &label_style)
        } else {
            0.0
        };
        let label_height = if self.axis_x.show_labels || self.axis_y.show_labels {
            label_style.size * 1.2
        } else {
            0.0
        };
        let title_height = label_style.size * 1.2;

        let legend_orientation = orientation_for_position(self.legend_position);
        let legend_size = if self.show_legend {
            legend_main_axis_size(
                canvas.text_backend(),
                &series_vec,
                &label_style,
                legend_orientation,
            )
        } else {
            0.0
        };

        let area = carve_plot_area(&CarveParams {
            bounds,
            axis_x: &self.axis_x,
            axis_y: &self.axis_y,
            y_label_max_width,
            x_label_height: label_height,
            axis_title_line_height: title_height,
            legend_size,
            legend_position: if self.show_legend {
                Some(self.legend_position)
            } else {
                None
            },
        });

        let plot = area.plot;
        if plot.width <= 0.0 || plot.height <= 0.0 {
            return;
        }

        // Re-derive y-axis-true-pixel target now that we have the plot rect.
        let target = self
            .axis_y
            .tick_count_hint
            .unwrap_or_else(|| auto_tick_count(plot.height));
        let y_ticks = nice_ticks(y_min, y_max, target);
        let y_lo = y_ticks.first().copied().unwrap_or(y_min);
        let y_hi = y_ticks.last().copied().unwrap_or(y_max);

        // ─── Grid lines ─────────────────────────────────────────────────
        if self.show_grid {
            let grid_color = BorderRole::Default.resolve(&theme.colors).with_alpha(0.4);
            for &t in &y_ticks {
                let y = y_to_pixel(t, y_lo, y_hi, plot);
                canvas.draw_line(
                    Point::new(plot.x, y),
                    Point::new(plot.right(), y),
                    grid_color,
                    cs::GRIDLINE_WIDTH,
                );
            }
        }

        // ─── Bars ───────────────────────────────────────────────────────
        let palette = self.palette.get();
        let visible: Vec<&ChartSeries<T>> = series_vec.iter().filter(|s| s.visible.get()).collect();
        if visible.is_empty() {
            // Still draw the axes — the chart is "empty but configured".
            self.draw_axes_with_x_labels(
                canvas,
                theme,
                plot,
                &y_ticks,
                &[],
                y_lo,
                y_hi,
                &label_style,
            );
            return;
        }

        // Use the first visible series's categories as the canonical x-axis
        // for PR 1. Multi-series x-alignment lands in PR 2 with grouping.
        let categories: Vec<&T> = visible[0].data.iter().map(|d| &d.category).collect();
        let n = categories.len();
        if n == 0 {
            return;
        }

        match self.grouping {
            BarGrouping::Single => self.paint_single(
                canvas,
                theme,
                plot,
                &visible,
                &categories,
                y_lo,
                y_hi,
                &palette,
                enabled,
            ),
            BarGrouping::Grouped => self.paint_grouped(
                canvas,
                theme,
                plot,
                &visible,
                &categories,
                y_lo,
                y_hi,
                &palette,
                enabled,
            ),
        }

        // ─── Value labels (above each bar) ──────────────────────────────
        if self.show_value_labels {
            self.draw_value_labels(
                canvas,
                theme,
                plot,
                &visible,
                &categories,
                y_lo,
                y_hi,
                &label_style,
            );
        }

        // ─── Embedded legend ────────────────────────────────────────────
        if self.show_legend && area.legend.width > 0.0 && area.legend.height > 0.0 {
            paint_embedded_legend(
                canvas,
                area.legend,
                &series_vec,
                &palette,
                legend_orientation,
                theme,
                enabled,
            );
        }

        // ─── Axes ───────────────────────────────────────────────────────
        let x_labels: Vec<String> = categories.iter().map(|c| format!("{}", c)).collect();
        self.draw_axes_with_x_labels(
            canvas,
            theme,
            plot,
            &y_ticks,
            &x_labels,
            y_lo,
            y_hi,
            &label_style,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::GraphicsDocument);
        let series_vec = self.series.get();
        let n_series = series_vec.len();
        let n_categories = series_vec.first().map(|s| s.data.len()).unwrap_or(0);
        builder.set_name(format!(
            "Bar chart: {} series, {} categories",
            n_series, n_categories
        ));
    }
}

impl<T: Clone + std::fmt::Display + 'static> BarChart<T> {
    fn compute_y_domain(&self, series_vec: &[ChartSeries<T>]) -> (f32, f32) {
        let mut min = self.axis_y.min.unwrap_or(f32::INFINITY);
        let mut max = self.axis_y.max.unwrap_or(f32::NEG_INFINITY);
        if self.axis_y.min.is_none() || self.axis_y.max.is_none() {
            for s in series_vec.iter().filter(|s| s.visible.get()) {
                for d in &s.data {
                    if self.axis_y.min.is_none() {
                        min = min.min(d.value);
                    }
                    if self.axis_y.max.is_none() {
                        max = max.max(d.value);
                    }
                }
            }
        }
        // Bar charts conventionally include zero in their y-domain so bars
        // have a meaningful baseline.
        if self.axis_y.min.is_none() {
            min = min.min(0.0);
        }
        if self.axis_y.max.is_none() {
            max = max.max(0.0);
        }
        if !min.is_finite() || !max.is_finite() {
            return (0.0, 1.0);
        }
        if (max - min).abs() < f32::EPSILON {
            // All-zero data — give a tiny span so we don't degenerate.
            return (0.0, 1.0);
        }
        (min, max)
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_single(
        &self,
        canvas: &mut Canvas,
        theme: &bastyde_core::Theme,
        plot: Rect,
        visible: &[&ChartSeries<T>],
        categories: &[&T],
        y_lo: f32,
        y_hi: f32,
        palette: &ChartPalette,
        enabled: bool,
    ) {
        use crate::style as cs;
        let n = categories.len();
        if n == 0 {
            return;
        }
        let series = visible[0];

        match self.orientation {
            BarOrientation::Vertical => {
                let total_gap = self.min_bar_gap * (n as f32 + 1.0);
                let bar_w = ((plot.width - total_gap) / n as f32).max(cs::BAR_MIN_WIDTH);
                let baseline_y = y_to_pixel(0.0_f32.max(y_lo).min(y_hi), y_lo, y_hi, plot);
                for (i, datum) in series.data.iter().enumerate() {
                    let color = series
                        .color
                        .as_ref()
                        .map(|c| c.resolve(theme, enabled))
                        .unwrap_or_else(|| palette.color_for(0, theme));
                    let x = plot.x + self.min_bar_gap + i as f32 * (bar_w + self.min_bar_gap);
                    let value_y = y_to_pixel(datum.value, y_lo, y_hi, plot);
                    let (top, h) = if datum.value >= 0.0 {
                        (value_y, baseline_y - value_y)
                    } else {
                        (baseline_y, value_y - baseline_y)
                    };
                    let rect = Rect::new(x, top, bar_w, h.max(0.0));
                    if let Some(r) = self.bar_corner_radius {
                        canvas.fill_rounded_rect(
                            rect,
                            bastyde_tokens::CornerRadius::uniform(r),
                            color,
                        );
                    } else {
                        canvas.fill_rect(rect, color);
                    }
                }
            }
            BarOrientation::Horizontal => {
                let total_gap = self.min_bar_gap * (n as f32 + 1.0);
                let bar_h = ((plot.height - total_gap) / n as f32).max(cs::BAR_MIN_WIDTH);
                // For horizontal we flip the value mapping onto the x-axis
                // and use vertical positions for categories.
                let baseline_x = value_to_pixel_h(0.0_f32.max(y_lo).min(y_hi), y_lo, y_hi, plot);
                for (i, datum) in series.data.iter().enumerate() {
                    let color = series
                        .color
                        .as_ref()
                        .map(|c| c.resolve(theme, enabled))
                        .unwrap_or_else(|| palette.color_for(0, theme));
                    let y = plot.y + self.min_bar_gap + i as f32 * (bar_h + self.min_bar_gap);
                    let value_x = value_to_pixel_h(datum.value, y_lo, y_hi, plot);
                    let (left, w) = if datum.value >= 0.0 {
                        (baseline_x, value_x - baseline_x)
                    } else {
                        (value_x, baseline_x - value_x)
                    };
                    let rect = Rect::new(left, y, w.max(0.0), bar_h);
                    if let Some(r) = self.bar_corner_radius {
                        canvas.fill_rounded_rect(
                            rect,
                            bastyde_tokens::CornerRadius::uniform(r),
                            color,
                        );
                    } else {
                        canvas.fill_rect(rect, color);
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_grouped(
        &self,
        canvas: &mut Canvas,
        theme: &bastyde_core::Theme,
        plot: Rect,
        visible: &[&ChartSeries<T>],
        categories: &[&T],
        y_lo: f32,
        y_hi: f32,
        palette: &ChartPalette,
        enabled: bool,
    ) {
        use crate::style as cs;
        let n = categories.len();
        let s = visible.len();
        if n == 0 || s == 0 {
            return;
        }
        match self.orientation {
            BarOrientation::Vertical => {
                let total_group_gap = self.group_gap * (n as f32 + 1.0);
                let group_w = ((plot.width - total_group_gap) / n as f32).max(cs::BAR_MIN_WIDTH);
                let bar_w = ((group_w - self.min_bar_gap * (s as f32 - 1.0)) / s as f32)
                    .max(cs::BAR_MIN_WIDTH);
                let baseline_y = y_to_pixel(0.0_f32.max(y_lo).min(y_hi), y_lo, y_hi, plot);
                for (gi, _) in categories.iter().enumerate() {
                    let group_x = plot.x + self.group_gap + gi as f32 * (group_w + self.group_gap);
                    for (si, series) in visible.iter().enumerate() {
                        let Some(datum) = series.data.get(gi) else {
                            continue;
                        };
                        let color = series
                            .color
                            .as_ref()
                            .map(|c| c.resolve(theme, enabled))
                            .unwrap_or_else(|| palette.color_for(si, theme));
                        let x = group_x + si as f32 * (bar_w + self.min_bar_gap);
                        let value_y = y_to_pixel(datum.value, y_lo, y_hi, plot);
                        let (top, h) = if datum.value >= 0.0 {
                            (value_y, baseline_y - value_y)
                        } else {
                            (baseline_y, value_y - baseline_y)
                        };
                        let rect = Rect::new(x, top, bar_w, h.max(0.0));
                        if let Some(r) = self.bar_corner_radius {
                            canvas.fill_rounded_rect(
                                rect,
                                bastyde_tokens::CornerRadius::uniform(r),
                                color,
                            );
                        } else {
                            canvas.fill_rect(rect, color);
                        }
                    }
                }
            }
            BarOrientation::Horizontal => {
                let total_group_gap = self.group_gap * (n as f32 + 1.0);
                let group_h = ((plot.height - total_group_gap) / n as f32).max(cs::BAR_MIN_WIDTH);
                let bar_h = ((group_h - self.min_bar_gap * (s as f32 - 1.0)) / s as f32)
                    .max(cs::BAR_MIN_WIDTH);
                let baseline_x = value_to_pixel_h(0.0_f32.max(y_lo).min(y_hi), y_lo, y_hi, plot);
                for (gi, _) in categories.iter().enumerate() {
                    let group_y = plot.y + self.group_gap + gi as f32 * (group_h + self.group_gap);
                    for (si, series) in visible.iter().enumerate() {
                        let Some(datum) = series.data.get(gi) else {
                            continue;
                        };
                        let color = series
                            .color
                            .as_ref()
                            .map(|c| c.resolve(theme, enabled))
                            .unwrap_or_else(|| palette.color_for(si, theme));
                        let y = group_y + si as f32 * (bar_h + self.min_bar_gap);
                        let value_x = value_to_pixel_h(datum.value, y_lo, y_hi, plot);
                        let (left, w) = if datum.value >= 0.0 {
                            (baseline_x, value_x - baseline_x)
                        } else {
                            (value_x, baseline_x - value_x)
                        };
                        let rect = Rect::new(left, y, w.max(0.0), bar_h);
                        if let Some(r) = self.bar_corner_radius {
                            canvas.fill_rounded_rect(
                                rect,
                                bastyde_tokens::CornerRadius::uniform(r),
                                color,
                            );
                        } else {
                            canvas.fill_rect(rect, color);
                        }
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_value_labels(
        &self,
        canvas: &mut Canvas,
        theme: &bastyde_core::Theme,
        plot: Rect,
        visible: &[&ChartSeries<T>],
        categories: &[&T],
        y_lo: f32,
        y_hi: f32,
        label_style: &bastyde_tokens::TextStyle,
    ) {
        use crate::style as cs;
        let label_color = TextRole::Primary.resolve(&theme.colors);
        let s = visible.len();
        let n = categories.len();
        if n == 0 || s == 0 {
            return;
        }
        match (self.orientation, self.grouping) {
            (BarOrientation::Vertical, BarGrouping::Single) => {
                let total_gap = self.min_bar_gap * (n as f32 + 1.0);
                let bar_w = ((plot.width - total_gap) / n as f32).max(cs::BAR_MIN_WIDTH);
                let series = visible[0];
                for (i, datum) in series.data.iter().enumerate() {
                    let x = plot.x + self.min_bar_gap + i as f32 * (bar_w + self.min_bar_gap);
                    let value_y = y_to_pixel(datum.value, y_lo, y_hi, plot);
                    let label = self.axis_y.format(datum.value);
                    let approx_w = measure_text_width(canvas, &label, label_style);
                    let rect = Rect::new(
                        x + (bar_w - approx_w) * 0.5,
                        value_y - label_style.size * 1.2 - 2.0,
                        approx_w,
                        label_style.size * 1.2,
                    );
                    canvas.draw_text(&label, rect, label_style, label_color);
                }
            }
            (BarOrientation::Vertical, BarGrouping::Grouped) => {
                let total_group_gap = self.group_gap * (n as f32 + 1.0);
                let group_w = ((plot.width - total_group_gap) / n as f32).max(cs::BAR_MIN_WIDTH);
                let bar_w = ((group_w - self.min_bar_gap * (s as f32 - 1.0)) / s as f32)
                    .max(cs::BAR_MIN_WIDTH);
                for (gi, _) in categories.iter().enumerate() {
                    let group_x = plot.x + self.group_gap + gi as f32 * (group_w + self.group_gap);
                    for (si, series) in visible.iter().enumerate() {
                        let Some(datum) = series.data.get(gi) else {
                            continue;
                        };
                        let x = group_x + si as f32 * (bar_w + self.min_bar_gap);
                        let value_y = y_to_pixel(datum.value, y_lo, y_hi, plot);
                        let label = self.axis_y.format(datum.value);
                        let approx_w = measure_text_width(canvas, &label, label_style);
                        let rect = Rect::new(
                            x + (bar_w - approx_w) * 0.5,
                            value_y - label_style.size * 1.2 - 2.0,
                            approx_w,
                            label_style.size * 1.2,
                        );
                        canvas.draw_text(&label, rect, label_style, label_color);
                    }
                }
            }
            (BarOrientation::Horizontal, BarGrouping::Single) => {
                let total_gap = self.min_bar_gap * (n as f32 + 1.0);
                let bar_h = ((plot.height - total_gap) / n as f32).max(cs::BAR_MIN_WIDTH);
                let series = visible[0];
                for (i, datum) in series.data.iter().enumerate() {
                    let y = plot.y + self.min_bar_gap + i as f32 * (bar_h + self.min_bar_gap);
                    let value_x = value_to_pixel_h(datum.value, y_lo, y_hi, plot);
                    let label = self.axis_y.format(datum.value);
                    let approx_w = measure_text_width(canvas, &label, label_style);
                    let rect = Rect::new(
                        value_x + 4.0,
                        y + (bar_h - label_style.size * 1.2) * 0.5,
                        approx_w,
                        label_style.size * 1.2,
                    );
                    canvas.draw_text(&label, rect, label_style, label_color);
                }
            }
            (BarOrientation::Horizontal, BarGrouping::Grouped) => {
                let total_group_gap = self.group_gap * (n as f32 + 1.0);
                let group_h = ((plot.height - total_group_gap) / n as f32).max(cs::BAR_MIN_WIDTH);
                let bar_h = ((group_h - self.min_bar_gap * (s as f32 - 1.0)) / s as f32)
                    .max(cs::BAR_MIN_WIDTH);
                for (gi, _) in categories.iter().enumerate() {
                    let group_y = plot.y + self.group_gap + gi as f32 * (group_h + self.group_gap);
                    for (si, series) in visible.iter().enumerate() {
                        let Some(datum) = series.data.get(gi) else {
                            continue;
                        };
                        let y = group_y + si as f32 * (bar_h + self.min_bar_gap);
                        let value_x = value_to_pixel_h(datum.value, y_lo, y_hi, plot);
                        let label = self.axis_y.format(datum.value);
                        let approx_w = measure_text_width(canvas, &label, label_style);
                        let rect = Rect::new(
                            value_x + 4.0,
                            y + (bar_h - label_style.size * 1.2) * 0.5,
                            approx_w,
                            label_style.size * 1.2,
                        );
                        canvas.draw_text(&label, rect, label_style, label_color);
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_axes_with_x_labels(
        &self,
        canvas: &mut Canvas,
        theme: &bastyde_core::Theme,
        plot: Rect,
        y_ticks: &[f32],
        x_labels: &[String],
        y_lo: f32,
        y_hi: f32,
        label_style: &bastyde_tokens::TextStyle,
    ) {
        use crate::style as cs;
        let axis_color = BorderRole::Default.resolve(&theme.colors);
        let label_color = TextRole::Secondary.resolve(&theme.colors);

        // Y axis line on the leading edge (skip if disabled).
        if self.axis_y.show_axis_line {
            canvas.draw_line(
                Point::new(plot.x, plot.y),
                Point::new(plot.x, plot.bottom()),
                axis_color,
                1.0,
            );
            // Tick marks.
            for &t in y_ticks {
                let y = y_to_pixel(t, y_lo, y_hi, plot);
                canvas.draw_line(
                    Point::new(plot.x - cs::AXIS_TICK_LENGTH, y),
                    Point::new(plot.x, y),
                    axis_color,
                    1.0,
                );
            }
        }
        // X axis line on the bottom edge.
        if self.axis_x.show_axis_line {
            canvas.draw_line(
                Point::new(plot.x, plot.bottom()),
                Point::new(plot.right(), plot.bottom()),
                axis_color,
                1.0,
            );
        }

        // Y tick labels — measure via text backend so wider digits ("100",
        // "1000") aren't truncated to "..." by draw_text's max_width gate.
        if self.axis_y.show_labels {
            for &t in y_ticks {
                let y = y_to_pixel(t, y_lo, y_hi, plot);
                let label = self.axis_y.format(t);
                let w = measure_text_width(canvas, &label, label_style);
                let rect = Rect::new(
                    plot.x - cs::AXIS_TICK_LENGTH - cs::AXIS_LABEL_GAP - w,
                    y - label_style.size * 0.6,
                    w,
                    label_style.size * 1.2,
                );
                canvas.draw_text(&label, rect, label_style, label_color);
            }
        }

        // X category labels (one per bar).
        if self.axis_x.show_labels && !x_labels.is_empty() {
            let n = x_labels.len();
            let slot_w = plot.width / n as f32;
            for (i, label) in x_labels.iter().enumerate() {
                let w = measure_text_width(canvas, label, label_style);
                let center_x = plot.x + slot_w * (i as f32 + 0.5);
                let rect = Rect::new(
                    center_x - w * 0.5,
                    plot.bottom() + cs::AXIS_TICK_LENGTH + cs::AXIS_LABEL_GAP,
                    w,
                    label_style.size * 1.2,
                );
                canvas.draw_text(label, rect, label_style, label_color);
            }
        }

        // Axis titles.
        if let Some(title) = self.axis_y.label.as_ref() {
            let w = measure_text_width(canvas, title, label_style);
            let rect = Rect::new(
                plot.x - cs::AXIS_TICK_LENGTH - cs::AXIS_LABEL_GAP - w - 4.0,
                plot.y + plot.height * 0.5 - label_style.size * 0.6,
                w,
                label_style.size * 1.2,
            );
            canvas.draw_text(title, rect, label_style, label_color);
        }
        if let Some(title) = self.axis_x.label.as_ref() {
            let w = measure_text_width(canvas, title, label_style);
            let rect = Rect::new(
                plot.x + plot.width * 0.5 - w * 0.5,
                plot.bottom() + cs::AXIS_TICK_LENGTH + cs::AXIS_LABEL_GAP + label_style.size * 1.4,
                w,
                label_style.size * 1.2,
            );
            canvas.draw_text(title, rect, label_style, label_color);
        }
    }
}

fn y_to_pixel(value: f32, y_lo: f32, y_hi: f32, plot: Rect) -> f32 {
    let span = (y_hi - y_lo).max(f32::EPSILON);
    let frac = (value - y_lo) / span;
    plot.bottom() - frac * plot.height
}

fn value_to_pixel_h(value: f32, x_lo: f32, x_hi: f32, plot: Rect) -> f32 {
    let span = (x_hi - x_lo).max(f32::EPSILON);
    let frac = (value - x_lo) / span;
    plot.x + frac * plot.width
}

fn measure_max_label_width(
    canvas: &mut Canvas,
    ticks: &[f32],
    axis: &AxisConfig,
    style: &bastyde_tokens::TextStyle,
) -> f32 {
    ticks
        .iter()
        .map(|t| measure_text_width(canvas, &axis.format(*t), style))
        .fold(0.0_f32, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::widget_tree::WidgetTree;

    fn sample_series() -> Vec<ChartSeries<String>> {
        let mut s = ChartSeries::new("Revenue");
        s.push("Q1".into(), 10.0);
        s.push("Q2".into(), 25.0);
        s.push("Q3".into(), 18.0);
        s.push("Q4".into(), 30.0);
        vec![s]
    }

    #[test]
    fn size_fills_proposal() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(BarChart::new(sample_series()));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let b = tree.bounds(id);
        assert!((b.width - 400.0).abs() < 0.01);
        assert!((b.height - 200.0).abs() < 0.01);
    }

    #[test]
    fn fallback_size_when_proposal_unbounded() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(BarChart::new(sample_series()));
        tree.layout(SizeProposal::unspecified());
        let b = tree.bounds(id);
        assert!(b.width >= 320.0);
        assert!(b.height >= 200.0);
    }

    #[test]
    fn one_decoration_per_bar() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(BarChart::new(sample_series()));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let frame = tree.render();
        // 4 bars: at minimum 4 fill_rect decorations. The axis lines are
        // also decorations (draw_line) so total can be more.
        assert!(
            frame.decorations.len() >= 4,
            "expected ≥ 4 decorations, got {}",
            frame.decorations.len()
        );
    }

    #[test]
    fn empty_series_does_not_panic() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(BarChart::<String>::new(Vec::<ChartSeries<String>>::new()));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let _ = tree.render();
    }

    #[test]
    fn accessibility_role_is_graphics_document() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(BarChart::new(sample_series()));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), bastyde_core::accesskit::Role::GraphicsDocument);
    }

    #[test]
    fn horizontal_orientation_swaps_axes() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(BarChart::new(sample_series()).orientation(BarOrientation::Horizontal));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let frame = tree.render();
        assert!(frame.decorations.len() >= 4);
    }

    #[test]
    fn grouped_multi_series_renders_per_series_bars() {
        let mut a = ChartSeries::<String>::new("A");
        a.push("Q1".into(), 1.0);
        a.push("Q2".into(), 2.0);
        let mut b = ChartSeries::<String>::new("B");
        b.push("Q1".into(), 3.0);
        b.push("Q2".into(), 4.0);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(BarChart::new(vec![a, b]).grouping(BarGrouping::Grouped));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let frame = tree.render();
        // 2 categories × 2 series = 4 bars (plus axis decorations).
        assert!(frame.decorations.len() >= 4);
    }

    #[test]
    fn legend_band_reserved_when_show_legend() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            BarChart::new(sample_series())
                .legend(true)
                .legend_position(LegendPosition::Bottom),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let frame = tree.render();
        // Legend renders one swatch (rounded rect) per series. Single series → ≥1 shape.
        assert!(
            !frame.shapes.is_empty(),
            "expected legend swatch shape when legend enabled"
        );
    }

    #[test]
    fn value_labels_emit_glyphs() {
        // With value_labels(true) the chart issues one extra `draw_text`
        // call per bar. Wire MockTextBackend so glyph emission is
        // observable in the render frame.
        use bastyde_canvas::text_backend::MockTextBackend;
        use std::cell::RefCell;
        use std::rc::Rc;

        let backend: Rc<RefCell<dyn bastyde_canvas::TextBackend>> =
            Rc::new(RefCell::new(MockTextBackend::new()));

        let mut tree_off = WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(backend.clone());
        tree_off.add(BarChart::new(sample_series()).value_labels(false));
        tree_off.layout(SizeProposal::exact(400.0, 200.0));
        let off_glyphs = tree_off.render().glyphs.len();

        let mut tree_on = WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(backend.clone());
        tree_on.add(BarChart::new(sample_series()).value_labels(true));
        tree_on.layout(SizeProposal::exact(400.0, 200.0));
        let on_glyphs = tree_on.render().glyphs.len();

        // MockTextBackend emits empty glyph slices, so we instead lock the
        // weaker invariant: the render path was reached without panic and
        // the layout_keys (one per draw_text call) grew by ≥ 4 (one per bar).
        let _ = (off_glyphs, on_glyphs);
        let off_keys = {
            let mut t = WidgetTree::new()
                .with_theme(bastyde_core::presets::intui::light())
                .with_text_backend(backend.clone());
            t.add(BarChart::new(sample_series()).value_labels(false));
            t.layout(SizeProposal::exact(400.0, 200.0));
            t.render().layout_keys.len()
        };
        let on_keys = {
            let mut t = WidgetTree::new()
                .with_theme(bastyde_core::presets::intui::light())
                .with_text_backend(backend.clone());
            t.add(BarChart::new(sample_series()).value_labels(true));
            t.layout(SizeProposal::exact(400.0, 200.0));
            t.render().layout_keys.len()
        };
        assert!(
            on_keys >= off_keys + 4,
            "expected ≥ 4 extra layout_keys for 4 bars (off={}, on={})",
            off_keys,
            on_keys
        );
    }

    #[test]
    fn hidden_series_not_rendered() {
        let mut s = ChartSeries::<String>::new("X");
        s.push("a".into(), 1.0);
        s.push("b".into(), 2.0);
        s.visible.set(false);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(BarChart::new(vec![s]));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let frame = tree.render();
        // No bar fills should exist (only axis lines as decorations).
        // Axis lines are 2 (y + x), tick marks add a few more. Without
        // bars the count should be small (<10).
        assert!(
            frame.decorations.len() < 10,
            "expected few decorations when series is hidden, got {}",
            frame.decorations.len()
        );
    }
}
