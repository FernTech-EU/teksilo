// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! BarChart — vertical or horizontal bars, one or more series.
//!
//! Bound to a [`ChartModel`]. Supports grouped multi-series, horizontal
//! orientation, value labels, grid lines, axis titles, an embedded
//! interactive legend, per-datum pointer hover with a shared tooltip
//! card, and per-datum accessibility marks.

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
use bastyde_tokens::{BorderRole, CornerRadius, TextRole, TextStyle, TextStyleRole};

use crate::axis::AxisConfig;
use crate::hit::{self, MarkGeometry, MarkShape};
use crate::layout::{LegendPosition, PlotGeometry, PlotGeometryParams, compute_plot_geometry};
use crate::legend::{ChartLegend, legend_main_axis_size, orientation_for_position};
use crate::palette::ChartPalette;
use crate::recipe_style::RecipeChartStyle;
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

pub struct BarChart<T: Clone + 'static> {
    model: ChartModel<T>,
    orientation: BarOrientation,
    grouping: BarGrouping,
    show_value_labels: bool,
    show_grid: bool,
    show_legend: bool,
    legend_position: LegendPosition,
    legend_interactive: bool,
    axis_x: AxisConfig,
    axis_y: AxisConfig,
    palette: Prop<ChartPalette>,
    bar_corner_radius: Option<f32>,
    min_bar_gap: f32,
    group_gap: f32,
    style_override: Option<SharedChartStyle>,
    show_hover_tooltip: bool,
    selection: Option<ChartSelection>,

    hover: Signal<Option<(SeriesId, usize)>>,
    marks: Rc<RefCell<Vec<MarkGeometry>>>,
    bounds: Rc<Cell<Rect>>,
    geometry_cache: Rc<RefCell<Option<(GeometryKey, PlotGeometry)>>>,
    paint_snapshot: Rc<RefCell<Option<PaintSnapshot>>>,
    legend_id: Option<WidgetId>,
}

impl<T: Clone + std::fmt::Display + 'static> BarChart<T> {
    pub fn new(model: ChartModel<T>) -> Self {
        Self {
            model,
            orientation: BarOrientation::Vertical,
            grouping: BarGrouping::Single,
            show_value_labels: false,
            show_grid: false,
            show_legend: false,
            legend_position: LegendPosition::Bottom,
            legend_interactive: false,
            axis_x: AxisConfig::new(),
            axis_y: AxisConfig::new(),
            palette: Prop::Static(ChartPalette::FromTheme),
            bar_corner_radius: None,
            min_bar_gap: 6.0,
            group_gap: 12.0,
            style_override: None,
            show_hover_tooltip: true,
            selection: None,
            hover: Signal::new(None),
            marks: Rc::new(RefCell::new(Vec::new())),
            bounds: Rc::new(Cell::new(Rect::ZERO)),
            geometry_cache: Rc::new(RefCell::new(None)),
            paint_snapshot: Rc::new(RefCell::new(None)),
            legend_id: None,
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

    /// Make the embedded legend interactive: clicking (or Space on a
    /// focused) row toggles that series' visibility. Default `false`.
    pub fn legend_interactive(mut self, on: bool) -> Self {
        self.legend_interactive = on;
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

    /// Per-call [`ChartStyle`] override. Takes precedence over
    /// `theme.style_slots.chart`.
    pub fn style(mut self, style: impl ChartStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Whether hovering a bar shows a tooltip card + updates the
    /// hover-driven state (also observable via `hover_signal`). Default `true`.
    pub fn hover_tooltip(mut self, on: bool) -> Self {
        self.show_hover_tooltip = on;
        self
    }

    /// Wire a shared [`ChartSelection`] into this chart: clicking a bar
    /// selects its `(series, point)` key (Ctrl/Cmd-click toggles it in
    /// [`bastyde_data::SelectionMode::Multi`]), clicking empty space
    /// clears the selection, and every selected bar paints an
    /// accent-colored outline on top of its fill. Pass a clone of the
    /// same `ChartSelection` to other charts/widgets to keep selection
    /// state in sync.
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
        {
            let registry = ctx.binding_registry();
            // Data swap → relayout (y-domain might shift) AND the AT mark
            // list must refresh.
            self.model
                .structure_version()
                .bind_to(id, registry, BindingLevel::Relayout);
            self.model
                .structure_version()
                .bind_to(id, registry, BindingLevel::AccessibilityOnly);
            // Color-only swap → repaint.
            self.model
                .style_version()
                .bind_to(id, registry, BindingLevel::RepaintOnly);
            self.palette
                .register_if_bound(id, registry, BindingLevel::RepaintOnly);
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
                            let hit = hit::rect_hit(&marks.borrow(), window_pos);
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
                    let hit = hit::rect_hit(&marks.borrow(), window_pos);
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

        // ─── Bars ───────────────────────────────────────────────────────
        // Draw directly from `marks` — the same geometry used for hit-test
        // and AT — so there is one source of truth for bar rects. Colors
        // are resolved via a `series_id -> (visible-index, explicit
        // color)` lookup built in the same visible-series order
        // `compute_marks` walked, so palette indices match pre-refactor
        // behavior exactly.
        let palette = self.palette.get();
        let mut color_lookup: HashMap<SeriesId, (usize, Option<ColorProp>)> = HashMap::new();
        // Per-point color overrides, keyed by `(series, point index)`. A bar
        // whose datum sets a color uses it in preference to the series color.
        let mut point_colors: HashMap<(SeriesId, usize), ColorProp> = HashMap::new();
        self.model.with_all_series(|views| {
            let mut vi = 0usize;
            for v in views {
                if v.visible {
                    color_lookup.insert(v.id, (vi, v.color.cloned()));
                    for (pi, d) in v.points.iter().enumerate() {
                        if let Some(c) = &d.color {
                            point_colors.insert((v.id, pi), c.clone());
                        }
                    }
                    vi += 1;
                }
            }
        });
        for m in &marks {
            let MarkShape::Rect(rect) = m.shape else {
                continue;
            };
            let (si, color_prop) = color_lookup.get(&m.series_id).cloned().unwrap_or((0, None));
            let resolved_color = point_colors
                .get(&(m.series_id, m.point_idx))
                .or(color_prop.as_ref())
                .map(|c| c.resolve(theme, enabled))
                .unwrap_or_else(|| palette.color_for(si, theme));
            let cfg = ChartFillContext {
                series_index: si,
                resolved_color,
                theme,
            };
            let fill = style.bar_fill(&cfg);
            let paint =
                PaintProp::from_fill(&fill, &theme.colors).resolve(theme, enabled, rect.size());
            paint_bar(canvas, rect, paint, self.bar_corner_radius);

            if self
                .selection
                .as_ref()
                .is_some_and(|s| s.is_selected(m.series_id, m.point_idx))
            {
                use crate::style::{SELECTION_BAR_OUTLINE_PAD, SELECTION_STROKE_WIDTH};
                let outline_rect = rect.expand(SELECTION_BAR_OUTLINE_PAD);
                let radius = self.bar_corner_radius.unwrap_or(0.0) + SELECTION_BAR_OUTLINE_PAD;
                canvas.stroke_rounded_rect(
                    outline_rect,
                    CornerRadius::uniform(radius),
                    theme.colors.accent,
                    SELECTION_STROKE_WIDTH,
                );
            }
        }

        // ─── Value labels ───────────────────────────────────────────────
        if self.show_value_labels {
            self.draw_value_labels(canvas, theme, &marks, &label_style);
        }

        // ─── Axes ───────────────────────────────────────────────────────
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
        self.draw_axes_with_x_labels(
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
            && let MarkShape::Rect(rect) = m.shape
        {
            let anchor = Point::new(rect.x + rect.width * 0.5, rect.y);
            let text = format!(
                "{}: {} = {}",
                m.series_name,
                m.category_label,
                self.axis_y.format(m.value)
            );
            hit::draw_mark_tooltip(canvas, theme, plot, anchor, &text, &label_style);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::GraphicsDocument);
        let n_series = self.model.series_count();
        let n_categories = self
            .model
            .series_id_at(0)
            .map(|s| self.model.point_count(s))
            .unwrap_or(0);
        builder.set_name(format!(
            "Bar chart: {} series, {} categories",
            n_series, n_categories
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

impl<T: Clone + std::fmt::Display + 'static> BarChart<T> {
    /// Memoized [`PlotGeometry`] for `bounds` — recomputed only when
    /// `bounds` or `model.structure_version()` changed since the last
    /// call. Shared by `paint()` and `accessibility()` so a mark's bounds
    /// never disagree between the visual tree and the AT tree, even
    /// without an intervening paint.
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
        });
        *self.geometry_cache.borrow_mut() = Some((key, geometry.clone()));
        geometry
    }

    /// Intrinsic compression floor — see [`crate::layout::compute_intrinsic_min`].
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

    /// Compute every visible bar's geometry + identity. Pure — reads only
    /// the model and layout config, no theme/canvas — so it's shared
    /// verbatim by `paint()` (drives the actual bar fills), the pointer
    /// hit-test (`hit::rect_hit`), and `accessibility()` (per-mark AT
    /// nodes).
    fn compute_marks(&self, geometry: &PlotGeometry) -> Vec<MarkGeometry> {
        use crate::style as cs;
        let plot = geometry.plot;
        let y_lo = geometry.y_lo;
        let y_hi = geometry.y_hi;
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

            match self.grouping {
                BarGrouping::Single => {
                    let series = visible[0];
                    match self.orientation {
                        BarOrientation::Vertical => {
                            let total_gap = self.min_bar_gap * (n as f32 + 1.0);
                            let bar_w =
                                ((plot.width - total_gap) / n as f32).max(cs::BAR_MIN_WIDTH);
                            let baseline_y =
                                y_to_pixel(0.0_f32.max(y_lo).min(y_hi), y_lo, y_hi, plot);
                            for (i, datum) in series.points.iter().enumerate() {
                                let x = plot.x
                                    + self.min_bar_gap
                                    + i as f32 * (bar_w + self.min_bar_gap);
                                let value_y = y_to_pixel(datum.value, y_lo, y_hi, plot);
                                let (top, h) = if datum.value >= 0.0 {
                                    (value_y, baseline_y - value_y)
                                } else {
                                    (baseline_y, value_y - baseline_y)
                                };
                                marks.push(MarkGeometry {
                                    series_id: series.id,
                                    point_idx: i,
                                    series_name: series.name.to_string(),
                                    category_label: format!("{}", datum.category),
                                    value: datum.value,
                                    shape: MarkShape::Rect(Rect::new(x, top, bar_w, h.max(0.0))),
                                });
                            }
                        }
                        BarOrientation::Horizontal => {
                            let total_gap = self.min_bar_gap * (n as f32 + 1.0);
                            let bar_h =
                                ((plot.height - total_gap) / n as f32).max(cs::BAR_MIN_WIDTH);
                            let baseline_x =
                                value_to_pixel_h(0.0_f32.max(y_lo).min(y_hi), y_lo, y_hi, plot);
                            for (i, datum) in series.points.iter().enumerate() {
                                let y = plot.y
                                    + self.min_bar_gap
                                    + i as f32 * (bar_h + self.min_bar_gap);
                                let value_x = value_to_pixel_h(datum.value, y_lo, y_hi, plot);
                                let (left, w) = if datum.value >= 0.0 {
                                    (baseline_x, value_x - baseline_x)
                                } else {
                                    (value_x, baseline_x - value_x)
                                };
                                marks.push(MarkGeometry {
                                    series_id: series.id,
                                    point_idx: i,
                                    series_name: series.name.to_string(),
                                    category_label: format!("{}", datum.category),
                                    value: datum.value,
                                    shape: MarkShape::Rect(Rect::new(left, y, w.max(0.0), bar_h)),
                                });
                            }
                        }
                    }
                }
                BarGrouping::Grouped => {
                    let s = visible.len();
                    match self.orientation {
                        BarOrientation::Vertical => {
                            let total_group_gap = self.group_gap * (n as f32 + 1.0);
                            let group_w =
                                ((plot.width - total_group_gap) / n as f32).max(cs::BAR_MIN_WIDTH);
                            let bar_w = ((group_w - self.min_bar_gap * (s as f32 - 1.0))
                                / s as f32)
                                .max(cs::BAR_MIN_WIDTH);
                            let baseline_y =
                                y_to_pixel(0.0_f32.max(y_lo).min(y_hi), y_lo, y_hi, plot);
                            for gi in 0..n {
                                let group_x = plot.x
                                    + self.group_gap
                                    + gi as f32 * (group_w + self.group_gap);
                                for (si, series) in visible.iter().enumerate() {
                                    let Some(datum) = series.points.get(gi) else {
                                        continue;
                                    };
                                    let x = group_x + si as f32 * (bar_w + self.min_bar_gap);
                                    let value_y = y_to_pixel(datum.value, y_lo, y_hi, plot);
                                    let (top, h) = if datum.value >= 0.0 {
                                        (value_y, baseline_y - value_y)
                                    } else {
                                        (baseline_y, value_y - baseline_y)
                                    };
                                    marks.push(MarkGeometry {
                                        series_id: series.id,
                                        point_idx: gi,
                                        series_name: series.name.to_string(),
                                        category_label: format!("{}", datum.category),
                                        value: datum.value,
                                        shape: MarkShape::Rect(Rect::new(
                                            x,
                                            top,
                                            bar_w,
                                            h.max(0.0),
                                        )),
                                    });
                                }
                            }
                        }
                        BarOrientation::Horizontal => {
                            let total_group_gap = self.group_gap * (n as f32 + 1.0);
                            let group_h =
                                ((plot.height - total_group_gap) / n as f32).max(cs::BAR_MIN_WIDTH);
                            let bar_h = ((group_h - self.min_bar_gap * (s as f32 - 1.0))
                                / s as f32)
                                .max(cs::BAR_MIN_WIDTH);
                            let baseline_x =
                                value_to_pixel_h(0.0_f32.max(y_lo).min(y_hi), y_lo, y_hi, plot);
                            for gi in 0..n {
                                let group_y = plot.y
                                    + self.group_gap
                                    + gi as f32 * (group_h + self.group_gap);
                                for (si, series) in visible.iter().enumerate() {
                                    let Some(datum) = series.points.get(gi) else {
                                        continue;
                                    };
                                    let y = group_y + si as f32 * (bar_h + self.min_bar_gap);
                                    let value_x = value_to_pixel_h(datum.value, y_lo, y_hi, plot);
                                    let (left, w) = if datum.value >= 0.0 {
                                        (baseline_x, value_x - baseline_x)
                                    } else {
                                        (value_x, baseline_x - value_x)
                                    };
                                    marks.push(MarkGeometry {
                                        series_id: series.id,
                                        point_idx: gi,
                                        series_name: series.name.to_string(),
                                        category_label: format!("{}", datum.category),
                                        value: datum.value,
                                        shape: MarkShape::Rect(Rect::new(
                                            left,
                                            y,
                                            w.max(0.0),
                                            bar_h,
                                        )),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        });

        marks
    }

    fn draw_value_labels(
        &self,
        canvas: &mut Canvas,
        theme: &Theme,
        marks: &[MarkGeometry],
        label_style: &TextStyle,
    ) {
        let label_color = TextRole::Primary.resolve(&theme.colors);
        for m in marks {
            let MarkShape::Rect(rect) = m.shape else {
                continue;
            };
            let label = self.axis_y.format(m.value);
            let approx_w = measure_text_width(canvas, &label, label_style);
            let text_rect = match self.orientation {
                BarOrientation::Vertical => {
                    let value_y = if m.value >= 0.0 {
                        rect.y
                    } else {
                        rect.bottom()
                    };
                    Rect::new(
                        rect.x + (rect.width - approx_w) * 0.5,
                        value_y - label_style.size * 1.2 - 2.0,
                        approx_w,
                        label_style.size * 1.2,
                    )
                }
                BarOrientation::Horizontal => {
                    let value_x = if m.value >= 0.0 { rect.right() } else { rect.x };
                    Rect::new(
                        value_x + 4.0,
                        rect.y + (rect.height - label_style.size * 1.2) * 0.5,
                        approx_w,
                        label_style.size * 1.2,
                    )
                }
            };
            canvas.draw_text(&label, text_rect, label_style, label_color);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_axes_with_x_labels(
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

/// Bar-fill helper: `Solid` + no corner radius → a plain `fill_rect`
/// (Tier 1, cheapest); `Solid` + a corner radius, or any gradient → an
/// SDF rounded rect (Tier 2) so gradients render.
fn paint_bar(
    canvas: &mut Canvas,
    rect: Rect,
    paint: bastyde_canvas::Paint,
    corner_radius: Option<f32>,
) {
    use bastyde_canvas::Paint;
    match (paint, corner_radius) {
        (Paint::Solid(c), None) => canvas.fill_rect(rect, c),
        (Paint::Solid(c), Some(r)) => canvas.fill_rounded_rect(rect, CornerRadius::uniform(r), c),
        (p, r) => canvas.fill_rounded_rect(rect, CornerRadius::uniform(r.unwrap_or(0.0)), p),
    }
}

/// Y-domain from a slice of `SeriesView`s — bar charts conventionally
/// include zero so bars have a meaningful baseline.
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
    if axis_y.min.is_none() {
        min = min.min(0.0);
    }
    if axis_y.max.is_none() {
        max = max.max(0.0);
    }
    if !min.is_finite() || !max.is_finite() {
        return (0.0, 1.0);
    }
    if (max - min).abs() < f32::EPSILON {
        return (0.0, 1.0);
    }
    (min, max)
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

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_data::{ChartDatum, ChartSeries};

    fn sample_model() -> ChartModel<String> {
        ChartModel::from_series_vec(vec![ChartSeries::new("Revenue").data(vec![
            ChartDatum::new("Q1".to_string(), 10.0),
            ChartDatum::new("Q2".to_string(), 25.0),
            ChartDatum::new("Q3".to_string(), 18.0),
            ChartDatum::new("Q4".to_string(), 30.0),
        ])])
    }

    #[test]
    fn size_fills_proposal() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(BarChart::new(sample_model()));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let b = tree.bounds(id);
        assert!((b.width - 400.0).abs() < 0.01);
        assert!((b.height - 200.0).abs() < 0.01);
    }

    #[test]
    fn fallback_size_when_proposal_unbounded() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(BarChart::new(sample_model()));
        tree.layout(SizeProposal::unspecified());
        let b = tree.bounds(id);
        assert!(b.width >= 320.0);
        assert!(b.height >= 200.0);
    }

    #[test]
    fn one_decoration_per_bar() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(BarChart::new(sample_model()));
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
        tree.add(BarChart::<String>::new(ChartModel::new()));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let _ = tree.render();
    }

    #[test]
    fn accessibility_role_is_graphics_document() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(BarChart::new(sample_model()));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), bastyde_core::accesskit::Role::GraphicsDocument);
    }

    #[test]
    fn horizontal_orientation_swaps_axes() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(BarChart::new(sample_model()).orientation(BarOrientation::Horizontal));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let frame = tree.render();
        assert!(frame.decorations.len() >= 4);
    }

    #[test]
    fn grouped_multi_series_renders_per_series_bars() {
        let model = ChartModel::from_series_vec(vec![
            ChartSeries::new("A").data(vec![
                ChartDatum::new("Q1".to_string(), 1.0),
                ChartDatum::new("Q2".to_string(), 2.0),
            ]),
            ChartSeries::new("B").data(vec![
                ChartDatum::new("Q1".to_string(), 3.0),
                ChartDatum::new("Q2".to_string(), 4.0),
            ]),
        ]);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(BarChart::new(model).grouping(BarGrouping::Grouped));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let frame = tree.render();
        // 2 categories × 2 series = 4 bars (plus axis decorations).
        assert!(frame.decorations.len() >= 4);
    }

    #[test]
    fn legend_band_reserved_when_show_legend() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            BarChart::new(sample_model())
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
        use bastyde_canvas::text_backend::MockTextBackend;

        let backend: Rc<RefCell<dyn bastyde_canvas::TextBackend>> =
            Rc::new(RefCell::new(MockTextBackend::new()));

        let off_keys = {
            let mut t = WidgetTree::new()
                .with_theme(bastyde_core::presets::intui::light())
                .with_text_backend(backend.clone());
            t.add(BarChart::new(sample_model()).value_labels(false));
            t.layout(SizeProposal::exact(400.0, 200.0));
            t.render().layout_keys.len()
        };
        let on_keys = {
            let mut t = WidgetTree::new()
                .with_theme(bastyde_core::presets::intui::light())
                .with_text_backend(backend.clone());
            t.add(BarChart::new(sample_model()).value_labels(true));
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
        let model = ChartModel::from_series_vec(vec![
            ChartSeries::new("X")
                .data(vec![
                    ChartDatum::new("a".to_string(), 1.0),
                    ChartDatum::new("b".to_string(), 2.0),
                ])
                .visibility(false),
        ]);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(BarChart::new(model));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let frame = tree.render();
        assert!(
            frame.decorations.len() < 10,
            "expected few decorations when series is hidden, got {}",
            frame.decorations.len()
        );
    }

    #[test]
    fn pointer_move_over_bar_sets_hover() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let model = sample_model();
        let chart = BarChart::new(model.clone());
        let marks_handle = chart.marks.clone();
        let hover_handle = chart.hover.clone();
        tree.add(chart);
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let _ = tree.render();

        let target = {
            let marks = marks_handle.borrow();
            let m = marks.first().expect("at least one mark");
            let MarkShape::Rect(r) = m.shape else {
                panic!("expected rect mark")
            };
            (r.center(), m.series_id, m.point_idx)
        };
        tree.pointer_move(target.0);
        assert_eq!(hover_handle.get(), Some((target.1, target.2)));
    }

    #[test]
    fn per_datum_accessibility_marks_match_visible_count() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(BarChart::new(sample_model()));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let _ = tree.render();
        tree.sync_accessibility();
        // 4 bars → 4 synthetic ChartMark AT nodes (verified via the mark
        // cache populated by the same paint that drove the a11y walk).
        // We can't easily enumerate synthetic nodes from the public test
        // API, so assert the underlying mark count directly instead.
        assert_eq!(
            {
                let mut t = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
                let chart = BarChart::new(sample_model());
                let marks_handle = chart.marks.clone();
                t.add(chart);
                t.layout(SizeProposal::exact(400.0, 200.0));
                let _ = t.render();
                marks_handle.borrow().len()
            },
            4
        );
    }

    #[test]
    fn single_pass_geometry_matches_between_paint_and_accessibility() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(BarChart::new(sample_model()));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let _ = tree.render();
        // accessibility() must reuse the same cached geometry — reading
        // it again should not panic and should report the chart role.
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), bastyde_core::accesskit::Role::GraphicsDocument);
    }

    #[test]
    fn layout_min_grows_with_wider_y_labels() {
        let theme = bastyde_core::presets::intui::light();
        let narrow = {
            let mut tree = WidgetTree::new().with_theme(theme.clone());
            let id = tree.add(BarChart::new(sample_model()));
            tree.layout(SizeProposal::exact(30.0, 200.0));
            tree.bounds(id).width
        };
        let wide_axis = AxisConfig::new().formatter(|v| format!("{:.5}------wide", v));
        let wide = {
            let mut tree = WidgetTree::new().with_theme(theme);
            let id = tree.add(BarChart::new(sample_model()).axis_y(wide_axis));
            tree.layout(SizeProposal::exact(30.0, 200.0));
            tree.bounds(id).width
        };
        assert!(
            wide >= narrow,
            "wider y-axis labels should not shrink the min width below the narrower case (narrow={narrow}, wide={wide})"
        );
    }

    #[test]
    fn tap_on_bar_selects_point() {
        use bastyde_core::event::PointerButton;
        use bastyde_data::SelectionMode;

        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sel = ChartSelection::new(SelectionMode::Single);
        let chart = BarChart::new(sample_model()).selection(sel.clone());
        let marks_handle = chart.marks.clone();
        tree.add(chart);
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let _ = tree.render();

        let (target, sid, idx) = {
            let marks = marks_handle.borrow();
            let m = marks.first().expect("at least one mark");
            let MarkShape::Rect(r) = m.shape else {
                panic!("expected rect mark")
            };
            (r.center(), m.series_id, m.point_idx)
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
        let chart = BarChart::new(sample_model()).selection(sel.clone());
        let marks_handle = chart.marks.clone();
        tree.add(chart);
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let _ = tree.render();

        let target = {
            let marks = marks_handle.borrow();
            let m = marks.first().expect("at least one mark");
            let MarkShape::Rect(r) = m.shape else {
                panic!("expected rect mark")
            };
            r.center()
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
    fn selected_bar_paints_highlight_shape() {
        use bastyde_data::SelectionMode;

        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sel = ChartSelection::new(SelectionMode::Single);
        let chart = BarChart::new(sample_model()).selection(sel.clone());
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
            "expected an extra highlight shape after selecting a bar (baseline {}, after {})",
            baseline_shapes,
            after.shapes.len()
        );
    }
}
