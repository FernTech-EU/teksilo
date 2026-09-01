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

use teksilo_canvas::{Canvas, Path, Point, Rect, Size, SizeProposal, TextBackend};
use teksilo_core::Theme;
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::event::{EventResponse, WidgetEvent};
use teksilo_core::gesture::TapEvent;
use teksilo_core::paint_prop::PaintProp;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::styles::{ChartFillContext, ChartStyle, SharedChartStyle};
use teksilo_core::widget::{
    EventContext, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_data::{ChartModel, ChartSelection, SeriesId, SeriesPattern, SeriesView};
use teksilo_tokens::{BorderRole, CornerRadius, TextRole, TextStyle, TextStyleRole};

use crate::axis::AxisConfig;
use crate::hit::{self, MarkGeometry, MarkShape};
use crate::layout::{LegendPosition, PlotGeometry, PlotGeometryParams, compute_plot_geometry};
use crate::legend::{ChartLegend, legend_main_axis_size, orientation_for_position};
use crate::palette::ChartPalette;
use crate::pattern::{self, PatternPolicy};
use crate::recipe_style::RecipeChartStyle;
use crate::reference_line::{ReferenceLine, ValueAxis, draw_reference_lines};
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
    /// Whether series carry their non-colour channel (a hatch over the bar
    /// fill). See [`PatternPolicy`] — `Auto` draws it from the second visible
    /// series on, which is what keeps a grouped or stacked bar chart readable
    /// without colour (WCAG 1.4.1).
    pattern_policy: PatternPolicy,
    bar_corner_radius: Option<f32>,
    min_bar_gap: f32,
    group_gap: f32,
    style_override: Option<SharedChartStyle>,
    show_hover_tooltip: bool,
    selection: Option<ChartSelection>,
    reference_lines: Vec<ReferenceLine>,

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
            pattern_policy: PatternPolicy::default(),
            bar_corner_radius: None,
            min_bar_gap: 6.0,
            group_gap: 12.0,
            style_override: None,
            show_hover_tooltip: true,
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

    /// Whether series carry a non-colour channel — a per-series hatch laid
    /// over the bar fill, and the matching hatch in the legend swatch.
    ///
    /// Defaults to [`PatternPolicy::Auto`]: drawn from the second visible
    /// series onwards, because that is when colour starts carrying information
    /// a reader who cannot see it would otherwise lose (WCAG 1.4.1). A
    /// single-series bar chart stays plain. Read
    /// [`Never`](PatternPolicy::Never)'s docs before reaching for it.
    pub fn pattern_policy(mut self, policy: PatternPolicy) -> Self {
        self.pattern_policy = policy;
        self
    }

    pub fn bar_corner_radius(mut self, r: f32) -> Self {
        self.bar_corner_radius = Some(r);
        self
    }

    /// Draw a labelled horizontal line across the plot at `value` on the value axis.
    ///
    /// For the comparison a chart is *about* — a median, a target, a budget. Without one, a
    /// chart that tints its bars by how they compare to something leaves the something
    /// invisible, and the reader is asked to judge a distance to a line that was never
    /// drawn.
    ///
    /// Sits above the bars, because it is the thing being compared against. Call it more
    /// than once for more than one comparison; see [`ReferenceLine`] for colour, width and
    /// dash.
    pub fn reference_line(mut self, line: ReferenceLine) -> Self {
        self.reference_lines.push(line);
        self
    }

    /// Every line at once, for a caller that already has them in hand.
    pub fn reference_lines(mut self, lines: impl IntoIterator<Item = ReferenceLine>) -> Self {
        self.reference_lines.extend(lines);
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
    /// [`teksilo_data::SelectionMode::Multi`]), clicking empty space
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
                            // The accelerator-click that adds one mark to a
                            // discontiguous selection: Ctrl+click on Windows
                            // and Linux, ⌘-click on macOS — where ⌃-click is
                            // the secondary click.
                            if tap.modifiers.command() {
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
                // The swatch samples what the plot draws: a hatched chip.
                .swatch(crate::pattern::LegendSwatch::Block)
                .pattern_policy(self.pattern_policy)
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
        // The visible index drives both default channels — the palette entry
        // and the hatch — which wrap at different periods, so the pair stays
        // unique long after colour alone would have repeated.
        type SeriesStyle = (usize, Option<ColorProp>, Option<SeriesPattern>);
        let mut color_lookup: HashMap<SeriesId, SeriesStyle> = HashMap::new();
        // Per-point color overrides, keyed by `(series, point index)`. A bar
        // whose datum sets a color uses it in preference to the series color.
        let mut point_colors: HashMap<(SeriesId, usize), ColorProp> = HashMap::new();
        let mut visible_series = 0usize;
        self.model.with_all_series(|views| {
            for v in views {
                if v.visible {
                    color_lookup.insert(v.id, (visible_series, v.color.cloned(), v.pattern));
                    for (pi, d) in v.points.iter().enumerate() {
                        if let Some(c) = &d.color {
                            point_colors.insert((v.id, pi), c.clone());
                        }
                    }
                    visible_series += 1;
                }
            }
        });
        // The non-colour channel is on exactly when colour is identifying
        // something — see `PatternPolicy`. `Single` grouping draws only the
        // first series however many the model holds, so the count that matters
        // is what reaches the plot, not what the model contains: hatching a
        // chart that shows one series would be decoration carrying nothing.
        let plotted_series = match self.grouping {
            BarGrouping::Single => visible_series.min(1),
            BarGrouping::Grouped => visible_series,
        };
        let patterned = self.pattern_policy.applies(plotted_series);
        for m in &marks {
            let MarkShape::Rect(rect) = m.shape else {
                continue;
            };
            let (si, color_prop, series_pattern) = color_lookup
                .get(&m.series_id)
                .cloned()
                .unwrap_or((0, None, None));
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
            // The hatch is the series' second, non-colour channel: a grouped
            // bar chart printed in greyscale, or read by someone who does not
            // see the palette, still separates the series. Laid over the fill
            // (which may be a gradient), so it is drawn after `paint_bar`.
            if patterned {
                pattern::fill_hatch(
                    canvas,
                    rect,
                    pattern::resolve(series_pattern, si),
                    resolved_color,
                );
            }

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

        // ─── Reference lines ────────────────────────────────────────────
        // After the bars, so the thing being compared against stays legible over the thing
        // being compared. The value axis follows the orientation: a horizontal bar chart
        // measures rightward, so its reference lines are vertical.
        if !self.reference_lines.is_empty() {
            let (axis, lo, hi) = match self.orientation {
                BarOrientation::Vertical => (ValueAxis::Vertical, geometry.y_lo, geometry.y_hi),
                BarOrientation::Horizontal => (ValueAxis::Horizontal, geometry.y_lo, geometry.y_hi),
            };
            draw_reference_lines(
                canvas,
                theme,
                enabled,
                &self.reference_lines,
                plot,
                axis,
                lo,
                hi,
                &label_style,
            );
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
            geometry.x_label_band,
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
        builder.set_role(teksilo_core::accesskit::Role::GraphicsDocument);
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
        // The room the carve granted the x labels; see `PlotGeometry::x_label_band`.
        x_label_band: f32,
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
            // What the carve actually granted the labels, which is capped; see
            // `PlotGeometry::x_label_band`. A label wider than fits in it is elided
            // rather than painted off the bottom of the widget.
            let budget =
                crate::axis::label_width_budget(layout, x_label_band, label_style.size * 1.2);
            for (i, label) in x_labels.iter().enumerate() {
                if i % layout.stride != 0 {
                    continue;
                }
                let w = measure_text_width(canvas, label, label_style).min(budget);
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

/// Bar-fill helper: `Solid` + no corner radius → a plain `fill_rect`
/// (Tier 1, cheapest); `Solid` + a corner radius, or any gradient → an
/// SDF rounded rect (Tier 2) so gradients render.
fn paint_bar(
    canvas: &mut Canvas,
    rect: Rect,
    paint: teksilo_canvas::Paint,
    corner_radius: Option<f32>,
) {
    use teksilo_canvas::Paint;
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
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_data::{ChartDatum, ChartSeries};

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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(BarChart::new(sample_model()));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let b = tree.bounds(id);
        assert!((b.width - 400.0).abs() < 0.01);
        assert!((b.height - 200.0).abs() < 0.01);
    }

    #[test]
    fn fallback_size_when_proposal_unbounded() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(BarChart::new(sample_model()));
        tree.layout(SizeProposal::unspecified());
        let b = tree.bounds(id);
        assert!(b.width >= 320.0);
        assert!(b.height >= 200.0);
    }

    #[test]
    fn one_decoration_per_bar() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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

    /// Two series over the same categories — the case where colour starts
    /// carrying information.
    fn two_series_model() -> ChartModel<String> {
        ChartModel::from_series_vec(vec![
            ChartSeries::new("Revenue").data(vec![
                ChartDatum::new("Q1".to_string(), 10.0),
                ChartDatum::new("Q2".to_string(), 25.0),
            ]),
            ChartSeries::new("Cost").data(vec![
                ChartDatum::new("Q1".to_string(), 6.0),
                ChartDatum::new("Q2".to_string(), 14.0),
            ]),
        ])
    }

    /// `Single` grouping — the default — plots only the first series, so a
    /// multi-series chart must ask for `Grouped` to actually show both.
    fn grouped(model: ChartModel<String>) -> BarChart<String> {
        BarChart::new(model).grouping(BarGrouping::Grouped)
    }

    fn path_count(chart: BarChart<String>) -> usize {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(chart);
        tree.layout(SizeProposal::exact(400.0, 200.0));
        tree.render().paths.len()
    }

    // ── The non-colour series channel (WCAG 1.4.1) ──────────────────────

    #[test]
    fn a_second_series_brings_a_hatch_with_it() {
        // The finding this closes: series were told apart by fill colour and
        // nothing else, so a greyscale print, a forced-colours setting, or a
        // reader with monochrome vision lost the distinction entirely.
        let plain = path_count(BarChart::new(sample_model()));
        let patterned = path_count(grouped(two_series_model()));
        assert!(
            patterned > plain,
            "a two-series bar chart must draw hatch strokes its single-series              counterpart does not ({patterned} vs {plain} paths)"
        );
    }

    #[test]
    fn a_single_series_chart_stays_plain() {
        // Nothing to disambiguate, so the hatch would be decoration carrying
        // no information — `PatternPolicy::Auto` holds off.
        let auto = path_count(BarChart::new(sample_model()));
        let never = path_count(BarChart::new(sample_model()).pattern_policy(PatternPolicy::Never));
        assert_eq!(
            auto, never,
            "with one series, Auto must draw exactly what Never draws"
        );
    }

    #[test]
    fn the_policy_overrides_the_default_in_both_directions() {
        let auto = path_count(grouped(two_series_model()));
        let never = path_count(grouped(two_series_model()).pattern_policy(PatternPolicy::Never));
        assert!(
            never < auto,
            "Never must drop the hatch a two-series chart draws"
        );

        // `Always` on a single series draws that series' pattern — which for
        // series 0 is `Solid`, i.e. no hatch. Pinning a non-solid pattern is
        // what makes the override observable, and exercises the explicit-
        // pattern path at the same time.
        let pinned = ChartModel::from_series_vec(vec![
            ChartSeries::new("Revenue")
                .pattern(teksilo_data::SeriesPattern::Dotted)
                .data(vec![
                    ChartDatum::new("Q1".to_string(), 10.0),
                    ChartDatum::new("Q2".to_string(), 25.0),
                ]),
        ]);
        let always = path_count(BarChart::new(pinned).pattern_policy(PatternPolicy::Always));
        assert!(
            always > 0,
            "Always must hatch a single-series chart that pinned a hatched pattern"
        );
    }

    /// **A chart with data always draws it, however long its category names are.**
    ///
    /// The end-to-end half of `layout::tests::sentence_long_category_labels_never_starve_
    /// the_plot`. A tilted label band grows with the widest label and is carved off a
    /// fixed height; unbounded, a book whose chapters are titled in whole sentences left
    /// `plot.height == 0`, and `paint` returns before drawing on a zero-height plot. The
    /// result was a chart with no bars, no grid, no axis and no diagnostic: 37 scenes of
    /// measured prose rendering as a blank rectangle.
    #[test]
    fn sentence_long_categories_still_draw_their_bars() {
        let points: Vec<ChartDatum<String>> = (0..37)
            .map(|i| {
                ChartDatum::new(
                    format!(
                        "{i}. Dans lequel Phileas Fogg et Passepartout s\u{2019}acceptent \
                         réciproquement, l\u{2019}un comme maître, l\u{2019}autre comme domestique"
                    ),
                    1000.0 + i as f32 * 50.0,
                )
            })
            .collect();
        let model =
            ChartModel::from_series_vec(vec![ChartSeries::new("Words per scene").data(points)]);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(BarChart::new(model).grid(true));
        tree.layout(SizeProposal::exact(1036.0, 360.0));
        let frame = tree.render();

        assert!(
            frame.decorations.len() >= 37,
            "every measured scene must get a bar; drew {} decorations",
            frame.decorations.len()
        );
    }

    #[test]
    fn empty_series_does_not_panic() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(BarChart::<String>::new(ChartModel::new()));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let _ = tree.render();
    }

    #[test]
    fn accessibility_role_is_graphics_document() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(BarChart::new(sample_model()));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), teksilo_core::accesskit::Role::GraphicsDocument);
    }

    #[test]
    fn horizontal_orientation_swaps_axes() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(BarChart::new(model).grouping(BarGrouping::Grouped));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let frame = tree.render();
        // 2 categories × 2 series = 4 bars (plus axis decorations).
        assert!(frame.decorations.len() >= 4);
    }

    #[test]
    fn legend_band_reserved_when_show_legend() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        use teksilo_canvas::text_backend::MockTextBackend;

        let backend: Rc<RefCell<dyn teksilo_canvas::TextBackend>> =
            Rc::new(RefCell::new(MockTextBackend::new()));

        let off_keys = {
            let mut t = WidgetTree::new()
                .with_theme(teksilo_core::presets::intui::light())
                .with_text_backend(backend.clone());
            t.add(BarChart::new(sample_model()).value_labels(false));
            t.layout(SizeProposal::exact(400.0, 200.0));
            t.render().layout_keys.len()
        };
        let on_keys = {
            let mut t = WidgetTree::new()
                .with_theme(teksilo_core::presets::intui::light())
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
                let mut t = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(BarChart::new(sample_model()));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let _ = tree.render();
        // accessibility() must reuse the same cached geometry — reading
        // it again should not panic and should report the chart role.
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), teksilo_core::accesskit::Role::GraphicsDocument);
    }

    #[test]
    fn layout_min_grows_with_wider_y_labels() {
        let theme = teksilo_core::presets::intui::light();
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
        use teksilo_core::event::PointerButton;
        use teksilo_data::SelectionMode;

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        use teksilo_core::event::PointerButton;
        use teksilo_data::SelectionMode;

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        use teksilo_data::SelectionMode;

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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

#[cfg(test)]
mod reference_line_tests {
    use super::*;
    use teksilo_data::{ChartDatum, ChartSeries};
    use teksilo_i18n::lit;

    fn chart(values: &[f32]) -> BarChart<String> {
        let data: Vec<ChartDatum<String>> = values
            .iter()
            .enumerate()
            .map(|(i, v)| ChartDatum::new(format!("c{i}"), *v))
            .collect();
        BarChart::new(ChartModel::from_series_vec(vec![
            ChartSeries::new("s").data(data),
        ]))
    }

    #[test]
    fn a_chart_has_no_reference_lines_until_one_is_asked_for() {
        assert!(chart(&[1.0, 2.0]).reference_lines.is_empty());
    }

    #[test]
    fn reference_lines_keep_their_value_and_label() {
        let c = chart(&[1.0, 2.0]).reference_line(ReferenceLine::new(2495.0, lit!("median")));
        assert_eq!(c.reference_lines.len(), 1);
        assert_eq!(c.reference_lines[0].value, 2495.0);
        assert_eq!(c.reference_lines[0].label.resolve_now(), "median");
    }

    /// More than one is meaningful — a median and a target are different claims, and the
    /// API exists so they can look different.
    #[test]
    fn several_lines_stack_in_the_order_they_were_added() {
        let c = chart(&[1.0])
            .reference_line(ReferenceLine::new(10.0, lit!("a")))
            .reference_lines([
                ReferenceLine::new(20.0, lit!("b")).color(TextRole::Primary),
                ReferenceLine::bare(30.0).solid().width(2.0),
            ]);
        let values: Vec<f32> = c.reference_lines.iter().map(|r| r.value).collect();
        assert_eq!(values, vec![10.0, 20.0, 30.0]);
        assert!(c.reference_lines[1].color.is_some());
        assert_eq!(c.reference_lines[2].dash, None);
    }
}
