//! LineChart — points connected by polylines, single or multi-series.
//!
//! PR 3 ships the core widget (lines, optional points, axes, grid, legend).
//! Area fill, hover tooltip, and edge-flip placement land in PR 4.

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::{Canvas, Path, Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, WidgetEvent};
use bastyde_core::signal::{Prop, Signal};
use bastyde_core::widget::{LayoutContext, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{BorderRole, CornerRadius, TextRole, TextStyleRole};

use crate::axis::{AxisConfig, auto_tick_count, nice_ticks};
use crate::layout::{CarveParams, LegendPosition, carve_plot_area};
use crate::legend::{legend_main_axis_size, orientation_for_position, paint_embedded_legend};
use crate::palette::ChartPalette;
use crate::series::ChartSeries;
use crate::text::measure_text_width;

/// One screen-space data point cached during paint, used by the hover
/// hit-test.
#[derive(Debug, Clone)]
struct PointHit {
    series_idx: usize,
    datum_idx: usize,
    screen: Point,
    series_name: String,
    category_label: String,
    value: f32,
}

#[derive(Debug, Clone, Copy)]
struct HoveredPoint {
    series_idx: usize,
    datum_idx: usize,
}

pub struct LineChart<T: Clone + 'static> {
    series: Prop<Vec<ChartSeries<T>>>,
    show_points: bool,
    show_area_fill: bool,
    area_fill_opacity: f32,
    show_grid: bool,
    show_hover_tooltip: bool,
    show_legend: bool,
    legend_position: LegendPosition,
    line_width: Option<f32>,
    point_radius: Option<f32>,
    axis_x: AxisConfig,
    axis_y: AxisConfig,
    palette: Prop<ChartPalette>,
    /// Live hover state; bound at `RepaintOnly` so hovering doesn't relayout.
    hover: Signal<Option<HoveredPoint>>,
    /// Snapshot of all visible data points in screen coordinates, written
    /// during paint and read by the on_pointer_event handler.
    hit_index: Rc<RefCell<Vec<PointHit>>>,
    /// Plot rectangle (window-space), written during paint.
    plot_rect: Rc<RefCell<Rect>>,
}

impl<T: Clone + std::fmt::Display + 'static> LineChart<T> {
    pub fn new(series: impl Into<Prop<Vec<ChartSeries<T>>>>) -> Self {
        Self {
            series: series.into(),
            show_points: true,
            show_area_fill: false,
            area_fill_opacity: 0.15,
            show_grid: false,
            show_hover_tooltip: true,
            show_legend: false,
            legend_position: LegendPosition::Bottom,
            line_width: None,
            point_radius: None,
            axis_x: AxisConfig::new(),
            axis_y: AxisConfig::new(),
            palette: Prop::Static(ChartPalette::FromTheme),
            hover: Signal::new(None),
            hit_index: Rc::new(RefCell::new(Vec::new())),
            plot_rect: Rc::new(RefCell::new(Rect::ZERO)),
        }
    }

    pub fn points(mut self, on: bool) -> Self {
        self.show_points = on;
        self
    }

    pub fn area_fill(mut self, on: bool) -> Self {
        self.show_area_fill = on;
        self
    }

    pub fn area_fill_opacity(mut self, alpha: f32) -> Self {
        self.area_fill_opacity = alpha.clamp(0.0, 1.0);
        self
    }

    pub fn grid(mut self, on: bool) -> Self {
        self.show_grid = on;
        self
    }

    pub fn hover_tooltip(mut self, on: bool) -> Self {
        self.show_hover_tooltip = on;
        self
    }

    pub fn legend(mut self, on: bool) -> Self {
        self.show_legend = on;
        self
    }

    pub fn legend_position(mut self, pos: LegendPosition) -> Self {
        self.legend_position = pos;
        self
    }

    pub fn line_width(mut self, w: f32) -> Self {
        self.line_width = Some(w);
        self
    }

    pub fn point_radius(mut self, r: f32) -> Self {
        self.point_radius = Some(r);
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
}

impl<T: Clone + 'static> std::fmt::Debug for LineChart<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LineChart")
            .field("show_points", &self.show_points)
            .field("show_area_fill", &self.show_area_fill)
            .finish()
    }
}

impl<T: Clone + std::fmt::Display + 'static> Widget for LineChart<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.series
            .register_if_bound(id, registry, BindingLevel::Relayout);
        self.palette
            .register_if_bound(id, registry, BindingLevel::RepaintOnly);
        // Hover repaints the chart but never relayouts.
        self.hover.bind_to(id, registry, BindingLevel::RepaintOnly);

        if self.show_hover_tooltip {
            let hit_index = self.hit_index.clone();
            let plot_rect = self.plot_rect.clone();
            let hover = self.hover.clone();
            let handlers = HandlerSet::new().on_pointer_event(move |event, _ctx| match event {
                WidgetEvent::PointerMove { position } => {
                    let plot = *plot_rect.borrow();
                    if !plot.contains(*position) {
                        if hover.get().is_some() {
                            hover.set(None);
                        }
                        return EventResponse::Ignored;
                    }
                    let hits = hit_index.borrow();
                    if hits.is_empty() {
                        return EventResponse::Ignored;
                    }
                    let mut best_idx = 0_usize;
                    let mut best_d2 = f32::INFINITY;
                    for (i, h) in hits.iter().enumerate() {
                        let dx = h.screen.x - position.x;
                        let dy = h.screen.y - position.y;
                        let d2 = dx * dx + dy * dy;
                        if d2 < best_d2 {
                            best_d2 = d2;
                            best_idx = i;
                        }
                    }
                    let h = &hits[best_idx];
                    let hp = HoveredPoint {
                        series_idx: h.series_idx,
                        datum_idx: h.datum_idx,
                    };
                    let prev = hover.get();
                    if prev.map(|p| (p.series_idx, p.datum_idx))
                        != Some((hp.series_idx, hp.datum_idx))
                    {
                        hover.set(Some(hp));
                    }
                    EventResponse::Ignored
                }
                WidgetEvent::PointerLeave => {
                    if hover.get().is_some() {
                        hover.set(None);
                    }
                    EventResponse::Ignored
                }
                _ => EventResponse::Ignored,
            });
            ctx.apply_self_handlers(handlers);
        }
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
        use crate::style as cs;

        let series_vec = self.series.get();
        if series_vec.is_empty() {
            return;
        }

        let visible: Vec<&ChartSeries<T>> = series_vec.iter().filter(|s| s.visible.get()).collect();
        if visible.is_empty() {
            return;
        }

        // Y-domain (auto from data unless user pinned it).
        let (y_min, y_max) = self.compute_y_domain(&visible);
        if (y_max - y_min).abs() < f32::EPSILON {
            return;
        }

        let label_style = TextStyleRole::Tiny.resolve(&theme.typography);
        let label_height = if self.axis_x.show_labels || self.axis_y.show_labels {
            label_style.size * 1.2
        } else {
            0.0
        };
        let title_height = label_style.size * 1.2;

        // Provisional ticks for label-width measurement.
        let provisional = nice_ticks(y_min, y_max, auto_tick_count(bounds.height));
        let y_label_max_width = if self.axis_y.show_labels {
            measure_max_label_width(canvas, &provisional, &self.axis_y, &label_style)
        } else {
            0.0
        };

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
        // Stash plot rect for the on_pointer_event handler.
        *self.plot_rect.borrow_mut() = plot;

        // Final ticks fitted to the carved plot rect.
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

        // ─── Series (area fill first so lines paint on top) ─────────────
        let palette = self.palette.get();
        let line_w = self.line_width.unwrap_or(cs::LINE_DEFAULT_WIDTH);
        let point_r = self.point_radius.unwrap_or(cs::POINT_DEFAULT_RADIUS);

        // Use the first visible series to determine x categories. PR 3
        // assumes all visible series share the same x categories — multi-x
        // is a follow-up.
        let n = visible[0].data.len();
        if n == 0 {
            return;
        }

        let mut new_hits: Vec<PointHit> = Vec::new();
        for (si, series) in visible.iter().enumerate() {
            let color = series
                .color
                .as_ref()
                .map(|c| c.resolve(theme))
                .unwrap_or_else(|| palette.color_for(si, theme));
            let count = series.data.len().min(n);
            if count == 0 {
                continue;
            }

            // Build the polyline path.
            let mut path = Path::new();
            for (i, datum) in series.data.iter().take(count).enumerate() {
                let x = x_for_index(i, count, plot);
                let y = y_to_pixel(datum.value, y_lo, y_hi, plot);
                if i == 0 {
                    path.move_to(Point::new(x, y));
                } else {
                    path.line_to(Point::new(x, y));
                }
                new_hits.push(PointHit {
                    series_idx: si,
                    datum_idx: i,
                    screen: Point::new(x, y),
                    series_name: series.name.clone(),
                    category_label: format!("{}", datum.category),
                    value: datum.value,
                });
            }

            // Area fill: close the polyline down to the baseline.
            if self.show_area_fill {
                let baseline_y = y_to_pixel(0.0_f32.max(y_lo).min(y_hi), y_lo, y_hi, plot);
                let mut filled = path.clone();
                let last_x = x_for_index(count - 1, count, plot);
                let first_x = x_for_index(0, count, plot);
                filled.line_to(Point::new(last_x, baseline_y));
                filled.line_to(Point::new(first_x, baseline_y));
                filled.close();
                canvas.fill_path(&filled, color.with_alpha(self.area_fill_opacity));
            }

            canvas.stroke_path(&path, color, line_w);

            if self.show_points {
                for (i, datum) in series.data.iter().take(count).enumerate() {
                    let x = x_for_index(i, count, plot);
                    let y = y_to_pixel(datum.value, y_lo, y_hi, plot);
                    canvas.fill_circle(Point::new(x, y), point_r, color);
                }
            }
        }

        // Update the hover hit index now that we have all screen-space
        // points (replace, never append, so a data swap can shrink it).
        *self.hit_index.borrow_mut() = new_hits;

        // ─── Embedded legend ────────────────────────────────────────────
        if self.show_legend && area.legend.width > 0.0 && area.legend.height > 0.0 {
            paint_embedded_legend(
                canvas,
                area.legend,
                &series_vec,
                &palette,
                legend_orientation,
                theme,
            );
        }

        // ─── Axes ───────────────────────────────────────────────────────
        let x_labels: Vec<String> = visible[0]
            .data
            .iter()
            .take(n)
            .map(|d| format!("{}", d.category))
            .collect();
        self.draw_axes(
            canvas,
            theme,
            plot,
            &y_ticks,
            &x_labels,
            y_lo,
            y_hi,
            &label_style,
        );

        // ─── Hover marker + tooltip ─────────────────────────────────────
        if self.show_hover_tooltip
            && let Some(hp) = self.hover.get()
        {
            let hits = self.hit_index.borrow();
            if let Some(hit) = hits
                .iter()
                .find(|h| h.series_idx == hp.series_idx && h.datum_idx == hp.datum_idx)
            {
                self.draw_hover(canvas, theme, plot, hit, &label_style);
            }
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::GraphicsDocument);
        let series_vec = self.series.get();
        let n_series = series_vec.len();
        let n_points = series_vec.first().map(|s| s.data.len()).unwrap_or(0);
        builder.set_name(format!(
            "Line chart: {} series, {} points",
            n_series, n_points
        ));
    }
}

impl<T: Clone + std::fmt::Display + 'static> LineChart<T> {
    fn compute_y_domain(&self, visible: &[&ChartSeries<T>]) -> (f32, f32) {
        let mut min = self.axis_y.min.unwrap_or(f32::INFINITY);
        let mut max = self.axis_y.max.unwrap_or(f32::NEG_INFINITY);
        if self.axis_y.min.is_none() || self.axis_y.max.is_none() {
            for s in visible {
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
        if !min.is_finite() || !max.is_finite() {
            return (0.0, 1.0);
        }
        if (max - min).abs() < f32::EPSILON {
            return (min - 1.0, max + 1.0);
        }
        // Tiny padding so points at the extremes don't touch the axis edge.
        let pad = (max - min) * 0.05;
        (min - pad, max + pad)
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_axes(
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

        if self.axis_y.show_axis_line {
            canvas.draw_line(
                Point::new(plot.x, plot.y),
                Point::new(plot.x, plot.bottom()),
                axis_color,
                1.0,
            );
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

        if self.axis_x.show_axis_line {
            canvas.draw_line(
                Point::new(plot.x, plot.bottom()),
                Point::new(plot.right(), plot.bottom()),
                axis_color,
                1.0,
            );
        }

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

        if self.axis_x.show_labels && !x_labels.is_empty() {
            let n = x_labels.len();
            for (i, label) in x_labels.iter().enumerate() {
                let w = measure_text_width(canvas, label, label_style);
                let center_x = x_for_index(i, n, plot);
                let rect = Rect::new(
                    center_x - w * 0.5,
                    plot.bottom() + cs::AXIS_TICK_LENGTH + cs::AXIS_LABEL_GAP,
                    w,
                    label_style.size * 1.2,
                );
                canvas.draw_text(label, rect, label_style, label_color);
            }
        }

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

    fn draw_hover(
        &self,
        canvas: &mut Canvas,
        theme: &bastyde_core::Theme,
        plot: Rect,
        hit: &PointHit,
        label_style: &bastyde_tokens::TextStyle,
    ) {
        use crate::style as cs;
        let palette = self.palette.get();
        // Marker color follows the series color.
        let marker_color = self
            .series
            .get()
            .get(hit.series_idx)
            .and_then(|s| s.color.clone())
            .map(|c| c.resolve(theme))
            .unwrap_or_else(|| palette.color_for(hit.series_idx, theme));
        // Outer ring: lighter, then inner dot in the series color.
        canvas.stroke_circle(hit.screen, 6.0, marker_color, 2.0);
        canvas.fill_circle(hit.screen, 3.0, marker_color);

        // Tooltip text: "<series>: <category>: <value>".
        let label = format!(
            "{}: {} = {}",
            hit.series_name,
            hit.category_label,
            self.axis_y.format(hit.value)
        );
        let text_w = measure_text_width(canvas, &label, label_style);
        let approx_w = text_w + cs::TOOLTIP_PADDING * 2.0;
        let height = label_style.size * 1.4 + cs::TOOLTIP_PADDING;

        // Place the tooltip above the marker by default; flip below if it
        // would clip; flip horizontally if it would clip left/right.
        let mut tx = hit.screen.x - approx_w * 0.5;
        let mut ty = hit.screen.y - height - 8.0;
        if ty < plot.y {
            ty = hit.screen.y + 8.0;
        }
        if tx < plot.x {
            tx = plot.x;
        }
        if tx + approx_w > plot.right() {
            tx = plot.right() - approx_w;
        }

        let tip = Rect::new(tx, ty, approx_w, height);
        let bg = theme.colors.tooltip_bg;
        let border = theme.colors.tooltip_border;
        canvas.fill_rounded_rect(tip, CornerRadius::uniform(4.0), bg);
        canvas.stroke_rounded_rect(tip, CornerRadius::uniform(4.0), border, 1.0);

        let text_color = theme.colors.tooltip_text;
        let label_rect = Rect::new(
            tip.x + cs::TOOLTIP_PADDING,
            tip.y + (tip.height - label_style.size * 1.2) * 0.5,
            tip.width - cs::TOOLTIP_PADDING * 2.0,
            label_style.size * 1.2,
        );
        canvas.draw_text(&label, label_rect, label_style, text_color);
    }
}

pub(crate) fn x_for_index(i: usize, n: usize, plot: Rect) -> f32 {
    if n <= 1 {
        plot.x + plot.width * 0.5
    } else {
        plot.x + (i as f32) / (n as f32 - 1.0) * plot.width
    }
}

pub(crate) fn y_to_pixel(value: f32, y_lo: f32, y_hi: f32, plot: Rect) -> f32 {
    let span = (y_hi - y_lo).max(f32::EPSILON);
    let frac = (value - y_lo) / span;
    plot.bottom() - frac * plot.height
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

    fn one_series() -> Vec<ChartSeries<String>> {
        let mut s = ChartSeries::<String>::new("Trend");
        s.push("A".into(), 5.0);
        s.push("B".into(), 12.0);
        s.push("C".into(), 8.0);
        s.push("D".into(), 20.0);
        vec![s]
    }

    fn two_series() -> Vec<ChartSeries<String>> {
        let mut a = ChartSeries::<String>::new("Foo");
        a.push("A".into(), 1.0);
        a.push("B".into(), 2.0);
        a.push("C".into(), 3.0);
        let mut b = ChartSeries::<String>::new("Bar");
        b.push("A".into(), 4.0);
        b.push("B".into(), 1.0);
        b.push("C".into(), 5.0);
        vec![a, b]
    }

    #[test]
    fn size_fills_proposal() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(LineChart::new(one_series()));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let b = tree.bounds(id);
        assert!((b.width - 400.0).abs() < 0.01);
        assert!((b.height - 200.0).abs() < 0.01);
    }

    #[test]
    fn one_path_per_series() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(LineChart::new(two_series()));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let frame = tree.render();
        // Two stroke paths, no area fill yet.
        assert_eq!(
            frame.paths.len(),
            2,
            "expected 2 stroked paths for 2 series, got {}",
            frame.paths.len()
        );
    }

    #[test]
    fn area_fill_doubles_path_count() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(LineChart::new(two_series()).area_fill(true));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let frame = tree.render();
        // 2 strokes + 2 fills = 4 paths
        assert_eq!(frame.paths.len(), 4);
    }

    #[test]
    fn points_emit_circles() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(LineChart::new(one_series()).points(true));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let frame = tree.render();
        // 4 data points → 4 SDF circle shapes.
        assert!(
            frame.shapes.len() >= 4,
            "expected ≥ 4 point shapes, got {}",
            frame.shapes.len()
        );
    }

    #[test]
    fn points_off_yields_no_circles() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(LineChart::new(one_series()).points(false));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let frame = tree.render();
        // Without legend, no shapes should render (only paths + decorations).
        assert!(frame.shapes.is_empty());
    }

    #[test]
    fn empty_data_does_not_panic() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(LineChart::<String>::new(Vec::<ChartSeries<String>>::new()));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let _ = tree.render();
    }

    #[test]
    fn accessibility_role_is_graphics_document() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(LineChart::new(one_series()));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), bastyde_core::accesskit::Role::GraphicsDocument);
    }

    #[test]
    fn hover_signal_default_none() {
        let chart = LineChart::new(one_series());
        assert!(chart.hover.get().is_none());
    }

    #[test]
    fn hover_marker_renders_when_hover_set() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        // Build the chart first.
        let chart = LineChart::new(one_series()).hover_tooltip(true);
        // Capture the hover signal before we hand the chart to the tree.
        let hover_signal = chart.hover.clone();
        tree.add(chart);
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let _ = tree.render();
        let baseline_shapes = tree.render().shapes.len();
        // Force a hovered point and re-render.
        hover_signal.set(Some(HoveredPoint {
            series_idx: 0,
            datum_idx: 1,
        }));
        let after = tree.render();
        // Hover marker = 1 stroked circle + 1 filled circle (≥2 extra
        // shapes). Tooltip background also adds 2 more shapes. Either
        // way the count grows.
        assert!(
            after.shapes.len() > baseline_shapes,
            "expected more shapes after hover (baseline {}, after {})",
            baseline_shapes,
            after.shapes.len()
        );
    }

    #[test]
    fn hidden_series_dropped() {
        let mut s1 = ChartSeries::<String>::new("Visible");
        s1.push("A".into(), 1.0);
        s1.push("B".into(), 2.0);
        let mut s2 = ChartSeries::<String>::new("Hidden");
        s2.push("A".into(), 3.0);
        s2.push("B".into(), 4.0);
        s2.visible.set(false);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(LineChart::new(vec![s1, s2]));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let frame = tree.render();
        // Only the visible series produces a stroked path.
        assert_eq!(frame.paths.len(), 1);
    }
}
