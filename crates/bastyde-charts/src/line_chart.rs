// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! LineChart — points connected by polylines, single or multi-series.
//!
//! Lines, optional point markers, axes, grid, and legend, plus optional
//! area fill (`area_fill` / `area_fill_opacity`) and an interactive
//! hover tooltip (`hover_tooltip`) with a nearest-point marker and
//! edge-flip placement so the tooltip never clips the plot rect.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use bastyde_canvas::{Canvas, Path, Point, Rect, Size, SizeProposal, TextBackend};
use bastyde_core::Theme;
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::event::{EventResponse, WidgetEvent};
use bastyde_core::gesture::TapEvent;
use bastyde_core::paint_prop::PaintProp;
use bastyde_core::signal::{Prop, Signal};
use bastyde_core::styles::{ChartFillContext, ChartStyle, SharedChartStyle};
use bastyde_core::widget::{
    EventContext, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_data::{ChartModel, ChartSelection, SeriesId, SeriesView};
use bastyde_tokens::{BorderRole, TextRole, TextStyle, TextStyleRole};

use crate::axis::AxisConfig;
use crate::hit::{self, MarkGeometry, MarkShape};
use crate::layout::{LegendPosition, PlotGeometry, PlotGeometryParams, compute_plot_geometry};
use crate::legend::{ChartLegend, legend_main_axis_size, orientation_for_position};
use crate::palette::ChartPalette;
use crate::recipe_style::RecipeChartStyle;
use crate::reference_line::{ReferenceLine, ValueAxis, draw_reference_lines};
use crate::text::measure_text_width;

/// Cache key for the memoized [`PlotGeometry`] — a miss recomputes.
#[derive(Debug, Clone, Copy, PartialEq)]
struct GeometryKey {
    bounds: Rect,
    structure_version: u64,
}

/// Text-measurement context stashed during `paint()` so `accessibility()`
/// can recompute the same geometry without a `Canvas`/`Theme`.
struct PaintSnapshot {
    backend: Option<Rc<RefCell<dyn TextBackend>>>,
    label_style: TextStyle,
}

pub struct LineChart<T: Clone + 'static> {
    model: ChartModel<T>,
    show_points: bool,
    show_area_fill: bool,
    area_fill_opacity: f32,
    show_grid: bool,
    show_hover_tooltip: bool,
    show_legend: bool,
    legend_position: LegendPosition,
    legend_interactive: bool,
    line_width: Option<f32>,
    point_radius: Option<f32>,
    axis_x: AxisConfig,
    axis_y: AxisConfig,
    palette: Prop<ChartPalette>,
    style_override: Option<SharedChartStyle>,
    selection: Option<ChartSelection>,
    reference_lines: Vec<ReferenceLine>,

    /// Live hover state; bound at `RepaintOnly` so hovering doesn't relayout.
    hover: Signal<Option<(SeriesId, usize)>>,
    marks: Rc<RefCell<Vec<MarkGeometry>>>,
    bounds: Rc<Cell<Rect>>,
    geometry_cache: Rc<RefCell<Option<(GeometryKey, PlotGeometry)>>>,
    paint_snapshot: Rc<RefCell<Option<PaintSnapshot>>>,
    legend_id: Option<WidgetId>,
}

impl<T: Clone + std::fmt::Display + 'static> LineChart<T> {
    pub fn new(model: ChartModel<T>) -> Self {
        Self {
            model,
            show_points: true,
            show_area_fill: false,
            area_fill_opacity: 0.15,
            show_grid: false,
            show_hover_tooltip: true,
            show_legend: false,
            legend_position: LegendPosition::Bottom,
            legend_interactive: false,
            line_width: None,
            point_radius: None,
            axis_x: AxisConfig::new(),
            axis_y: AxisConfig::new(),
            palette: Prop::Static(ChartPalette::FromTheme),
            style_override: None,
            selection: None,
            reference_lines: Vec::new(),
            hover: Signal::new(None),
            marks: Rc::new(RefCell::new(Vec::new())),
            bounds: Rc::new(Cell::new(Rect::ZERO)),
            geometry_cache: Rc::new(RefCell::new(None)),
            paint_snapshot: Rc::new(RefCell::new(None)),
            legend_id: None,
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

    /// Draw a labelled horizontal line across the plot at `value`. Any number of them, any
    /// colour — see [`ReferenceLine`].
    ///
    /// A chart that already plots its target as a *series* does not need one; this is for a
    /// constant the data is judged against rather than tracked toward.
    pub fn reference_line(mut self, line: ReferenceLine) -> Self {
        self.reference_lines.push(line);
        self
    }

    /// Every line at once, for a caller that already has them in hand.
    pub fn reference_lines(mut self, lines: impl IntoIterator<Item = ReferenceLine>) -> Self {
        self.reference_lines.extend(lines);
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

    /// Make the embedded legend interactive: clicking (or Space on a
    /// focused) row toggles that series' visibility. Default `false`.
    pub fn legend_interactive(mut self, on: bool) -> Self {
        self.legend_interactive = on;
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

    /// Per-call [`ChartStyle`] override. Takes precedence over
    /// `theme.style_slots.chart`.
    pub fn style(mut self, style: impl ChartStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Wire a shared [`ChartSelection`] into this chart: clicking a
    /// point selects its `(series, point)` key (Ctrl/Cmd-click toggles
    /// it in [`bastyde_data::SelectionMode::Multi`]), clicking empty
    /// space clears the selection, and every selected point paints an
    /// accent-colored ring. Pass a clone of the same `ChartSelection`
    /// to other charts/widgets to keep selection state in sync.
    pub fn selection(mut self, selection: ChartSelection) -> Self {
        self.selection = Some(selection);
        self
    }

    /// A clone of the live hover signal — the `(series, point)` key
    /// currently under the pointer, or `None`. Lets an app observe
    /// hover state from outside the chart (a synced detail panel, a
    /// custom tooltip) without re-implementing hit-testing.
    pub fn hover_signal(&self) -> Signal<Option<(SeriesId, usize)>> {
        self.hover.clone()
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
        {
            let registry = ctx.binding_registry();
            self.model
                .structure_version()
                .bind_to(id, registry, BindingLevel::Relayout);
            self.model
                .structure_version()
                .bind_to(id, registry, BindingLevel::AccessibilityOnly);
            self.model
                .style_version()
                .bind_to(id, registry, BindingLevel::RepaintOnly);
            self.palette
                .register_if_bound(id, registry, BindingLevel::RepaintOnly);
            // Hover repaints the chart but never relayouts.
            self.hover.bind_to(id, registry, BindingLevel::RepaintOnly);
            if let Some(selection) = &self.selection {
                selection
                    .selection_signal()
                    .bind_to(id, registry, BindingLevel::RepaintOnly);
            }
        }

        if self.show_hover_tooltip || self.selection.is_some() {
            let mut handlers = HandlerSet::new();

            if self.show_hover_tooltip {
                let marks = self.marks.clone();
                let bounds = self.bounds.clone();
                let geometry_cache = self.geometry_cache.clone();
                let hover = self.hover.clone();
                handlers =
                    handlers.on_pointer_event(move |event, _ctx: &mut EventContext| match event {
                        WidgetEvent::PointerMove { position } => {
                            let b = bounds.get();
                            let window_pos = Point::new(position.x + b.x, position.y + b.y);
                            let plot = geometry_cache.borrow().as_ref().map(|(_, g)| g.plot);
                            let Some(plot) = plot else {
                                return EventResponse::Ignored;
                            };
                            if !plot.contains(window_pos) {
                                if hover.get().is_some() {
                                    hover.set(None);
                                }
                                return EventResponse::Ignored;
                            }
                            let hit = hit::nearest_point(&marks.borrow(), window_pos);
                            match hit.and_then(|idx| {
                                marks.borrow().get(idx).map(|m| (m.series_id, m.point_idx))
                            }) {
                                Some(key) => {
                                    if hover.get() != Some(key) {
                                        hover.set(Some(key));
                                    }
                                }
                                None => {
                                    if hover.get().is_some() {
                                        hover.set(None);
                                    }
                                }
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
            }

            if let Some(selection) = self.selection.clone() {
                let marks = self.marks.clone();
                let bounds = self.bounds.clone();
                let geometry_cache = self.geometry_cache.clone();
                handlers = handlers.on_tap(move |tap: &TapEvent, _ctx: &mut EventContext| {
                    let b = bounds.get();
                    let window_pos = Point::new(tap.position.x + b.x, tap.position.y + b.y);
                    let Some(plot) = geometry_cache.borrow().as_ref().map(|(_, g)| g.plot) else {
                        return;
                    };
                    if !plot.contains(window_pos) {
                        selection.clear();
                        return;
                    }
                    let hit = hit::nearest_point(&marks.borrow(), window_pos);
                    match hit
                        .and_then(|idx| marks.borrow().get(idx).map(|m| (m.series_id, m.point_idx)))
                    {
                        Some((sid, idx)) => {
                            if tap.modifiers.ctrl() || tap.modifiers.super_key() {
                                selection.toggle_point(sid, idx);
                            } else {
                                selection.select_point(sid, idx);
                            }
                        }
                        None => selection.clear(),
                    }
                });
            }

            ctx.apply_self_handlers(handlers);
        }

        if self.show_legend {
            let legend = ChartLegend::new(self.model.clone())
                .palette(self.palette.clone())
                .orientation(orientation_for_position(self.legend_position))
                .interactive(self.legend_interactive);
            let legend_id = ctx.add(legend);
            self.legend_id = Some(legend_id);
            vec![legend_id]
        } else {
            self.legend_id = None;
            Vec::new()
        }
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let ideal = Size::new(
            proposal.width.unwrap_or(320.0),
            proposal.height.unwrap_or(200.0),
        );
        let min = self.compute_intrinsic_min(ctx);
        LayoutResponse::shrinkable(ideal, min, 1.0)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        self.bounds.set(bounds);
        if let Some(legend_id) = self.legend_id {
            let label_style = TextStyleRole::Tiny.resolve(&ctx.theme.typography);
            let geometry = self.ensure_geometry(bounds, ctx.text_backend, &label_style);
            for child in children.iter_mut() {
                if child.id == legend_id {
                    child.origin = Point::new(geometry.legend.x, geometry.legend.y);
                    child.size = Size::new(geometry.legend.width, geometry.legend.height);
                }
            }
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        use crate::style as cs;
        let theme = ctx.theme;
        let enabled = ctx.effective_enabled;

        if self.model.series_count() == 0 {
            return;
        }

        let style: SharedChartStyle = self
            .style_override
            .clone()
            .or_else(|| theme.style_slots.chart.clone())
            .unwrap_or_else(|| Rc::new(RecipeChartStyle));

        let label_style = TextStyleRole::Tiny.resolve(&theme.typography);
        let backend = canvas.text_backend().cloned();
        *self.paint_snapshot.borrow_mut() = Some(PaintSnapshot {
            backend: backend.clone(),
            label_style: label_style.clone(),
        });

        let geometry = self.ensure_geometry(bounds, backend.as_ref(), &label_style);
        let plot = geometry.plot;
        if plot.width <= 0.0 || plot.height <= 0.0 {
            return;
        }

        // Stash bounds for the on_pointer_event handler (window-space
        // reconstruction: `position + bounds.origin`).
        self.bounds.set(bounds);

        let marks = self.compute_marks(&geometry);
        *self.marks.borrow_mut() = marks.clone();

        // ─── Grid lines ─────────────────────────────────────────────────
        if self.show_grid {
            let recipe = style.gridline(theme);
            let stroke = hit::resolve_gridline_stroke(&recipe, self.axis_y.gridline_dash);
            let color = recipe.color.resolve(theme);
            for &t in &geometry.y_ticks {
                let y = y_to_pixel(t, geometry.y_lo, geometry.y_hi, plot);
                let mut path = Path::new();
                path.move_to(Point::new(plot.x, y));
                path.line_to(Point::new(plot.right(), y));
                canvas.stroke_path(&path, color, stroke.clone());
            }
        }

        // ─── Series (area fill first so lines paint on top) ─────────────
        // Draw directly from `marks` (series-contiguous runs, in the same
        // visible-series order `compute_marks` walked) so line/area/point
        // geometry is never recomputed — one source of truth shared with
        // the hit-test and AT paths.
        let palette = self.palette.get();
        let line_w = self.line_width.unwrap_or(cs::LINE_DEFAULT_WIDTH);
        let point_r = self.point_radius.unwrap_or(cs::POINT_DEFAULT_RADIUS);

        let mut color_lookup: HashMap<SeriesId, (usize, Option<ColorProp>)> = HashMap::new();
        self.model.with_all_series(|views| {
            let mut vi = 0usize;
            for v in views {
                if v.visible {
                    color_lookup.insert(v.id, (vi, v.color.cloned()));
                    vi += 1;
                }
            }
        });

        let baseline_y = y_to_pixel(
            0.0_f32.max(geometry.y_lo).min(geometry.y_hi),
            geometry.y_lo,
            geometry.y_hi,
            plot,
        );
        for series_marks in marks_by_series(&marks) {
            let sid = series_marks[0].series_id;
            let (si, color_prop) = color_lookup.get(&sid).cloned().unwrap_or((0, None));
            let color = color_prop
                .as_ref()
                .map(|c| c.resolve(theme, enabled))
                .unwrap_or_else(|| palette.color_for(si, theme));

            let mut path = Path::new();
            for (i, m) in series_marks.iter().enumerate() {
                let MarkShape::Point { center, .. } = m.shape else {
                    continue;
                };
                if i == 0 {
                    path.move_to(center);
                } else {
                    path.line_to(center);
                }
            }

            if self.show_area_fill && series_marks.len() > 1 {
                let MarkShape::Point { center: first, .. } = series_marks[0].shape else {
                    continue;
                };
                let MarkShape::Point { center: last, .. } =
                    series_marks[series_marks.len() - 1].shape
                else {
                    continue;
                };
                let mut filled = path.clone();
                filled.line_to(Point::new(last.x, baseline_y));
                filled.line_to(Point::new(first.x, baseline_y));
                filled.close();
                let cfg = ChartFillContext {
                    series_index: si,
                    resolved_color: color,
                    theme,
                };
                let fill = style.area_fill(&cfg, self.area_fill_opacity);
                let paint = PaintProp::from_fill(&fill, &theme.colors).resolve(
                    theme,
                    enabled,
                    filled.bounds().size(),
                );
                canvas.fill_path(&filled, paint);
            }

            canvas.stroke_path(&path, color, line_w);

            if self.show_points {
                for m in series_marks {
                    if let MarkShape::Point { center, .. } = m.shape {
                        canvas.fill_circle(center, point_r, color);
                    }
                }
            }
        }

        // ─── Selection highlight ────────────────────────────────────────
        if let Some(selection) = &self.selection {
            for m in &marks {
                if let MarkShape::Point { center, .. } = m.shape
                    && selection.is_selected(m.series_id, m.point_idx)
                {
                    canvas.stroke_circle(
                        center,
                        cs::SELECTION_POINT_RING_RADIUS,
                        theme.colors.accent,
                        cs::SELECTION_STROKE_WIDTH,
                    );
                }
            }
        }

        // ─── Reference lines ────────────────────────────────────────────
        // Over the series, because a constant the data is judged against has to stay
        // readable where the data crosses it.
        if !self.reference_lines.is_empty() {
            draw_reference_lines(
                canvas,
                theme,
                enabled,
                &self.reference_lines,
                plot,
                ValueAxis::Vertical,
                geometry.y_lo,
                geometry.y_hi,
                &label_style,
            );
        }

        // ─── Axes ───────────────────────────────────────────────────────
        let x_labels: Vec<String> = self.model.with_all_series(|views| {
            let visible: Vec<&SeriesView<'_, T>> = views.iter().filter(|v| v.visible).collect();
            if visible.is_empty() {
                return Vec::new();
            }
            let n = visible[0].points.len();
            visible[0]
                .points
                .iter()
                .take(n)
                .map(|d| format!("{}", d.category))
                .collect()
        });
        self.draw_axes(
            canvas,
            theme,
            plot,
            &geometry.y_ticks,
            &x_labels,
            geometry.y_lo,
            geometry.y_hi,
            &label_style,
        );

        // ─── Hover marker + tooltip ─────────────────────────────────────
        if self.show_hover_tooltip
            && let Some((sid, idx)) = self.hover.get()
            && let Some(m) = marks
                .iter()
                .find(|m| m.series_id == sid && m.point_idx == idx)
            && let MarkShape::Point { center, .. } = m.shape
        {
            let (si, color_prop) = color_lookup.get(&sid).cloned().unwrap_or((0, None));
            let marker_color = color_prop
                .as_ref()
                .map(|c| c.resolve(theme, enabled))
                .unwrap_or_else(|| palette.color_for(si, theme));
            canvas.stroke_circle(center, 6.0, marker_color, 2.0);
            canvas.fill_circle(center, 3.0, marker_color);

            let text = format!(
                "{}: {} = {}",
                m.series_name,
                m.category_label,
                self.axis_y.format(m.value)
            );
            hit::draw_mark_tooltip(canvas, theme, plot, center, &text, &label_style);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::GraphicsDocument);
        let n_series = self.model.series_count();
        let n_points = self
            .model
            .series_id_at(0)
            .map(|s| self.model.point_count(s))
            .unwrap_or(0);
        builder.set_name(format!(
            "Line chart: {} series, {} points",
            n_series, n_points
        ));

        let bounds = self.bounds.get();
        let (backend, label_style) = match self.paint_snapshot.borrow().as_ref() {
            Some(s) => (s.backend.clone(), s.label_style.clone()),
            None => (
                None,
                TextStyle {
                    size: 11.0,
                    ..TextStyle::default()
                },
            ),
        };
        let geometry = self.ensure_geometry(bounds, backend.as_ref(), &label_style);
        let marks = self.compute_marks(&geometry);
        for m in &marks {
            hit::emit_mark_node(builder, m);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.legend_id.into_iter().collect()
    }
}

impl<T: Clone + std::fmt::Display + 'static> LineChart<T> {
    /// Memoized [`PlotGeometry`] for `bounds` — see `BarChart::ensure_geometry`.
    fn ensure_geometry(
        &self,
        bounds: Rect,
        backend: Option<&Rc<RefCell<dyn TextBackend>>>,
        label_style: &TextStyle,
    ) -> PlotGeometry {
        let key = GeometryKey {
            bounds,
            structure_version: self.model.structure_version().get(),
        };
        if let Some((cached_key, geometry)) = self.geometry_cache.borrow().as_ref()
            && *cached_key == key
        {
            return geometry.clone();
        }

        let legend_orientation = orientation_for_position(self.legend_position);
        let legend_size = if self.show_legend {
            legend_main_axis_size(backend, &self.model, label_style, legend_orientation)
        } else {
            0.0
        };
        let y_domain = self
            .model
            .with_all_series(|views| y_domain_from_views(&self.axis_y, views));
        // Gathered here as well as at paint time: the carve needs them to size a tilted
        // label band, and the same first-visible-series rule must pick them both times.
        let x_labels: Vec<String> = self.model.with_all_series(|views| {
            let visible: Vec<&SeriesView<'_, T>> = views.iter().filter(|v| v.visible).collect();
            if visible.is_empty() {
                return Vec::new();
            }
            visible[0]
                .points
                .iter()
                .map(|d| format!("{}", d.category))
                .collect()
        });
        let geometry = compute_plot_geometry(&PlotGeometryParams {
            bounds,
            axis_x: &self.axis_x,
            axis_y: &self.axis_y,
            y_domain,
            legend_size,
            legend_position: if self.show_legend {
                Some(self.legend_position)
            } else {
                None
            },
            text_backend: backend,
            label_style,
            x_labels: &x_labels,
        });
        *self.geometry_cache.borrow_mut() = Some((key, geometry.clone()));
        geometry
    }

    fn compute_intrinsic_min(&self, ctx: &LayoutContext) -> Size {
        let plot_floor = Size::new(40.0, 40.0);
        let label_style = TextStyleRole::Tiny.resolve(&ctx.theme.typography);
        let legend_orientation = orientation_for_position(self.legend_position);
        let legend_size = if self.show_legend {
            legend_main_axis_size(
                ctx.text_backend,
                &self.model,
                &label_style,
                legend_orientation,
            )
        } else {
            0.0
        };
        let y_domain = self
            .model
            .with_all_series(|views| y_domain_from_views(&self.axis_y, views));
        crate::layout::compute_intrinsic_min(
            &self.axis_x,
            &self.axis_y,
            y_domain,
            legend_size,
            if self.show_legend {
                Some(self.legend_position)
            } else {
                None
            },
            ctx.text_backend,
            &label_style,
            plot_floor,
        )
    }

    /// Compute every visible point's geometry + identity, in series-major
    /// order (all of series 0's points, then series 1's, …) — `paint()`
    /// relies on this contiguity via [`marks_by_series`] to draw each
    /// series' polyline/area/points without recomputing pixel positions.
    /// Pure — reads only the model and layout config, so it's shared
    /// verbatim by `paint()`, the pointer hit-test (`hit::nearest_point`),
    /// and `accessibility()` (per-mark AT nodes).
    fn compute_marks(&self, geometry: &PlotGeometry) -> Vec<MarkGeometry> {
        use crate::style as cs;
        let plot = geometry.plot;
        let y_lo = geometry.y_lo;
        let y_hi = geometry.y_hi;
        let point_r = self.point_radius.unwrap_or(cs::POINT_DEFAULT_RADIUS);
        let mut marks = Vec::new();

        self.model.with_all_series(|views| {
            let visible: Vec<&SeriesView<'_, T>> = views.iter().filter(|v| v.visible).collect();
            if visible.is_empty() {
                return;
            }
            let n = visible[0].points.len();
            if n == 0 {
                return;
            }
            for series in visible.iter() {
                let count = series.points.len().min(n);
                for (i, datum) in series.points.iter().take(count).enumerate() {
                    let x = x_for_index(i, count, plot);
                    let y = y_to_pixel(datum.value, y_lo, y_hi, plot);
                    marks.push(MarkGeometry {
                        series_id: series.id,
                        point_idx: i,
                        series_name: series.name.to_string(),
                        category_label: format!("{}", datum.category),
                        value: datum.value,
                        shape: MarkShape::Point {
                            center: Point::new(x, y),
                            radius: point_r,
                        },
                    });
                }
            }
        });

        marks
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_axes(
        &self,
        canvas: &mut Canvas,
        theme: &Theme,
        plot: Rect,
        y_ticks: &[f32],
        x_labels: &[String],
        y_lo: f32,
        y_hi: f32,
        label_style: &TextStyle,
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
            let slot_w = plot.width / n as f32;
            let widest = x_labels
                .iter()
                .map(|l| measure_text_width(canvas, l, label_style))
                .fold(0.0f32, f32::max);
            let layout = crate::axis::resolve_label_layout(
                n,
                plot.width,
                widest,
                label_style.size * 1.2,
                self.axis_x.label_angle,
            );
            for (i, label) in x_labels.iter().enumerate() {
                if i % layout.stride != 0 {
                    continue;
                }
                let w = measure_text_width(canvas, label, label_style);
                let h = label_style.size * 1.2;
                let center_x = plot.x + slot_w * (i as f32 + 0.5);
                let top = plot.bottom() + cs::AXIS_TICK_LENGTH + cs::AXIS_LABEL_GAP;
                crate::axis::draw_category_label(
                    canvas,
                    label,
                    layout,
                    center_x,
                    top,
                    w,
                    h,
                    label_style,
                    label_color,
                );
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
}

/// Split a series-major-ordered mark list into contiguous per-series
/// runs. Relies on [`LineChart::compute_marks`] always appending marks
/// series-by-series (never interleaved).
fn marks_by_series(marks: &[MarkGeometry]) -> Vec<&[MarkGeometry]> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < marks.len() {
        let sid = marks[i].series_id;
        let mut j = i + 1;
        while j < marks.len() && marks[j].series_id == sid {
            j += 1;
        }
        out.push(&marks[i..j]);
        i = j;
    }
    out
}

/// Y-domain from a slice of `SeriesView`s, with a small padding so points
/// at the extremes don't touch the axis edge (line charts, unlike bars,
/// don't force a zero baseline into the domain).
fn y_domain_from_views<T>(axis_y: &AxisConfig, views: &[SeriesView<'_, T>]) -> (f32, f32) {
    let mut min = axis_y.min.unwrap_or(f32::INFINITY);
    let mut max = axis_y.max.unwrap_or(f32::NEG_INFINITY);
    if axis_y.min.is_none() || axis_y.max.is_none() {
        for v in views.iter().filter(|v| v.visible) {
            for d in v.points {
                if axis_y.min.is_none() {
                    min = min.min(d.value);
                }
                if axis_y.max.is_none() {
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
    let pad = (max - min) * 0.05;
    (min - pad, max + pad)
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

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_data::{ChartDatum, ChartSeries};

    fn one_series() -> ChartModel<String> {
        ChartModel::from_series_vec(vec![ChartSeries::new("Trend").data(vec![
            ChartDatum::new("A".to_string(), 5.0),
            ChartDatum::new("B".to_string(), 12.0),
            ChartDatum::new("C".to_string(), 8.0),
            ChartDatum::new("D".to_string(), 20.0),
        ])])
    }

    fn two_series() -> ChartModel<String> {
        ChartModel::from_series_vec(vec![
            ChartSeries::new("Foo").data(vec![
                ChartDatum::new("A".to_string(), 1.0),
                ChartDatum::new("B".to_string(), 2.0),
                ChartDatum::new("C".to_string(), 3.0),
            ]),
            ChartSeries::new("Bar").data(vec![
                ChartDatum::new("A".to_string(), 4.0),
                ChartDatum::new("B".to_string(), 1.0),
                ChartDatum::new("C".to_string(), 5.0),
            ]),
        ])
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
        assert_eq!(frame.paths.len(), 4);
    }

    #[test]
    fn points_emit_circles() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(LineChart::new(one_series()).points(true));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let frame = tree.render();
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
        assert!(frame.shapes.is_empty());
    }

    #[test]
    fn empty_data_does_not_panic() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(LineChart::<String>::new(ChartModel::new()));
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
        let chart = LineChart::new(one_series()).hover_tooltip(true);
        let hover_signal = chart.hover.clone();
        let marks_handle = chart.marks.clone();
        tree.add(chart);
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let _ = tree.render();
        let baseline_shapes = tree.render().shapes.len();
        let (sid, idx) = {
            let marks = marks_handle.borrow();
            let m = &marks[1];
            (m.series_id, m.point_idx)
        };
        hover_signal.set(Some((sid, idx)));
        let after = tree.render();
        assert!(
            after.shapes.len() > baseline_shapes,
            "expected more shapes after hover (baseline {}, after {})",
            baseline_shapes,
            after.shapes.len()
        );
    }

    #[test]
    fn pointer_move_over_point_sets_hover() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let chart = LineChart::new(one_series());
        let marks_handle = chart.marks.clone();
        let hover_handle = chart.hover.clone();
        tree.add(chart);
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let _ = tree.render();

        let target = {
            let marks = marks_handle.borrow();
            let m = marks.first().expect("at least one mark");
            let MarkShape::Point { center, .. } = m.shape else {
                panic!("expected point mark")
            };
            (center, m.series_id, m.point_idx)
        };
        tree.pointer_move(target.0);
        assert_eq!(hover_handle.get(), Some((target.1, target.2)));
    }

    #[test]
    fn hidden_series_dropped() {
        let model = ChartModel::from_series_vec(vec![
            ChartSeries::new("Visible").data(vec![
                ChartDatum::new("A".to_string(), 1.0),
                ChartDatum::new("B".to_string(), 2.0),
            ]),
            ChartSeries::new("Hidden")
                .data(vec![
                    ChartDatum::new("A".to_string(), 3.0),
                    ChartDatum::new("B".to_string(), 4.0),
                ])
                .visibility(false),
        ]);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(LineChart::new(model));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let frame = tree.render();
        assert_eq!(frame.paths.len(), 1);
    }

    #[test]
    fn per_datum_mark_count_matches_visible_points() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let chart = LineChart::new(two_series());
        let marks_handle = chart.marks.clone();
        tree.add(chart);
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let _ = tree.render();
        assert_eq!(marks_handle.borrow().len(), 6);
    }

    #[test]
    fn layout_min_shrinks_below_ideal_under_over_constraint() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(LineChart::new(one_series()));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let b = tree.bounds(id);
        assert!(b.width <= 50.0 + 0.01);
        assert!(b.height <= 50.0 + 0.01);
    }

    #[test]
    fn tap_on_point_selects_point() {
        use bastyde_core::event::PointerButton;
        use bastyde_data::SelectionMode;

        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sel = ChartSelection::new(SelectionMode::Single);
        let chart = LineChart::new(one_series()).selection(sel.clone());
        let marks_handle = chart.marks.clone();
        tree.add(chart);
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let _ = tree.render();

        let (target, sid, idx) = {
            let marks = marks_handle.borrow();
            let m = marks.first().expect("at least one mark");
            let MarkShape::Point { center, .. } = m.shape else {
                panic!("expected point mark")
            };
            (center, m.series_id, m.point_idx)
        };
        tree.pointer_down_button(target, PointerButton::Primary);
        tree.pointer_up_button(target, PointerButton::Primary);
        assert!(sel.is_selected(sid, idx));
    }

    #[test]
    fn tap_outside_plot_clears_selection() {
        use bastyde_core::event::PointerButton;
        use bastyde_data::SelectionMode;

        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sel = ChartSelection::new(SelectionMode::Single);
        let chart = LineChart::new(one_series()).selection(sel.clone());
        let marks_handle = chart.marks.clone();
        tree.add(chart);
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let _ = tree.render();

        let target = {
            let marks = marks_handle.borrow();
            let m = marks.first().expect("at least one mark");
            let MarkShape::Point { center, .. } = m.shape else {
                panic!("expected point mark")
            };
            center
        };
        tree.pointer_down_button(target, PointerButton::Primary);
        tree.pointer_up_button(target, PointerButton::Primary);
        assert_eq!(sel.count(), 1);

        // Top-left corner: outside the plot rect (axis-label margins).
        let outside = Point::new(1.0, 1.0);
        tree.pointer_down_button(outside, PointerButton::Primary);
        tree.pointer_up_button(outside, PointerButton::Primary);
        assert_eq!(
            sel.count(),
            0,
            "tap outside the plot should clear selection"
        );
    }

    #[test]
    fn selected_point_paints_highlight_ring() {
        use bastyde_data::SelectionMode;

        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sel = ChartSelection::new(SelectionMode::Single);
        let chart = LineChart::new(one_series()).selection(sel.clone());
        let marks_handle = chart.marks.clone();
        tree.add(chart);
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let baseline_shapes = tree.render().shapes.len();

        let (sid, idx) = {
            let marks = marks_handle.borrow();
            let m = marks.first().expect("at least one mark");
            (m.series_id, m.point_idx)
        };
        sel.select_point(sid, idx);
        let after = tree.render();
        assert!(
            after.shapes.len() > baseline_shapes,
            "expected an extra highlight ring after selecting a point (baseline {}, after {})",
            baseline_shapes,
            after.shapes.len()
        );
    }
}
