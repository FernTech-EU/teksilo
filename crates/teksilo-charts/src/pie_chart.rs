// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! PieChart — circular value slices.
//!
//! Pie and donut share the same widget. `inner_radius_ratio == 0.0` is a
//! pie; `> 0.0` is a donut. The optional center widget slot is only used
//! when `inner_radius_ratio > 0.0` (the slot is silently ignored for pies).
//!
//! Bound to a [`ChartModel`] — a pie's slices are the points of ONE
//! series (`new` picks `ChartModel::only_series()` or the first series;
//! `from_series` wraps a single [`ChartSeries`] into its own model).
//!
//! Slot integration follows the existing `Option<PendingChild>` pattern
//! used by [`Card`](https://docs.rs/teksilo-widgets) /
//! [`DialogContent`](https://docs.rs/teksilo-widgets) / `GroupBox`: two
//! fields (`pending_center` + `center_id`), two builders
//! (`.center(impl Widget)` / `.center_id(WidgetId)`), and `build()`
//! resolves the pending child via `ctx.add_boxed`.

use std::cell::{Cell, RefCell};
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
use teksilo_core::styles::{ChartFillContext, ChartStyle, FillRecipe, SharedChartStyle};
use teksilo_core::widget::{
    EventContext, LayoutContext, LayoutResponse, PaintContext, PendingChild, Widget,
    WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_data::{ChartModel, ChartSelection, ChartSeries, SeriesId};
use teksilo_tokens::{CornerRadius, TextRole, TextStyle, TextStyleRole};

use crate::hit::{self, MarkGeometry, MarkShape};
use crate::layout::{LegendPosition, PieGeometry, PieGeometryParams, compute_pie_geometry};
use crate::legend::{LegendOrientation, orientation_for_position};
use crate::palette::ChartPalette;
use crate::recipe_style::RecipeChartStyle;
use crate::text::measure_text_width;

/// How slice labels are placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PieLabelMode {
    #[default]
    None,
    /// Inside each slice along the bisector. Slices below
    /// `PIE_MIN_SLICE_LABEL_DEGREES` are skipped.
    Inside,
    /// Outside each slice with a leader line.
    Outside,
    /// Inside if it fits, otherwise leader-out.
    InsideWithLeaders,
}

/// Cache key for the memoized [`PieGeometry`] — a miss recomputes.
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

pub struct PieChart<T: Clone + 'static> {
    model: ChartModel<T>,
    /// The series whose points become slices. `None` only for an empty
    /// model (nothing to draw).
    series_id: Option<SeriesId>,
    inner_radius_ratio: f32,
    start_angle_degrees: f32,
    clockwise: bool,
    slice_gap_degrees: f32,
    label_mode: PieLabelMode,
    show_percentages: bool,
    show_legend: bool,
    legend_position: LegendPosition,
    show_hover_tooltip: bool,
    palette: Prop<ChartPalette>,
    explicit_colors: Vec<Option<ColorProp>>,
    pending_center: Option<PendingChild>,
    center_id: Option<WidgetId>,
    style_override: Option<SharedChartStyle>,
    selection: Option<ChartSelection>,

    hover: Signal<Option<(SeriesId, usize)>>,
    marks: Rc<RefCell<Vec<MarkGeometry>>>,
    bounds: Rc<Cell<Rect>>,
    geometry_cache: Rc<RefCell<Option<(GeometryKey, PieGeometry)>>>,
    paint_snapshot: Rc<RefCell<Option<PaintSnapshot>>>,
}

impl<T: Clone + std::fmt::Display + 'static> PieChart<T> {
    pub fn new(model: ChartModel<T>) -> Self {
        let series_id = model.only_series().or_else(|| model.series_id_at(0));
        Self {
            model,
            series_id,
            inner_radius_ratio: 0.0,
            start_angle_degrees: -90.0, // 12 o'clock
            clockwise: true,
            slice_gap_degrees: 0.0,
            label_mode: PieLabelMode::None,
            show_percentages: false,
            show_legend: false,
            legend_position: LegendPosition::Bottom,
            show_hover_tooltip: true,
            palette: Prop::Static(ChartPalette::FromTheme),
            explicit_colors: Vec::new(),
            pending_center: None,
            center_id: None,
            style_override: None,
            selection: None,
            hover: Signal::new(None),
            marks: Rc::new(RefCell::new(Vec::new())),
            bounds: Rc::new(Cell::new(Rect::ZERO)),
            geometry_cache: Rc::new(RefCell::new(None)),
            paint_snapshot: Rc::new(RefCell::new(None)),
        }
    }

    /// Adapter: take a single `ChartSeries<T>` and use its data points as
    /// pie slices (the series's name and color are ignored for the pie).
    pub fn from_series(series: ChartSeries<T>) -> Self {
        Self::new(ChartModel::from_series_vec(vec![series]))
    }

    pub fn donut(mut self, inner_radius_ratio: f32) -> Self {
        self.inner_radius_ratio = inner_radius_ratio.clamp(0.0, 0.95);
        self
    }

    pub fn start_angle_degrees(mut self, deg: f32) -> Self {
        self.start_angle_degrees = deg;
        self
    }

    pub fn clockwise(mut self, on: bool) -> Self {
        self.clockwise = on;
        self
    }

    pub fn slice_gap_degrees(mut self, deg: f32) -> Self {
        self.slice_gap_degrees = deg.max(0.0);
        self
    }

    pub fn label_mode(mut self, mode: PieLabelMode) -> Self {
        self.label_mode = mode;
        self
    }

    pub fn show_percentages(mut self, on: bool) -> Self {
        self.show_percentages = on;
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

    pub fn hover_tooltip(mut self, on: bool) -> Self {
        self.show_hover_tooltip = on;
        self
    }

    pub fn palette(mut self, p: impl Into<Prop<ChartPalette>>) -> Self {
        self.palette = p.into();
        self
    }

    pub fn slice_color(mut self, index: usize, c: impl Into<ColorProp>) -> Self {
        if self.explicit_colors.len() <= index {
            self.explicit_colors.resize(index + 1, None);
        }
        self.explicit_colors[index] = Some(c.into());
        self
    }

    /// Set the donut center widget. Silently ignored if
    /// `inner_radius_ratio == 0.0` (pie mode).
    pub fn center(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_center = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Use a pre-registered widget id as the donut center.
    pub fn center_id(mut self, id: WidgetId) -> Self {
        self.pending_center = Some(PendingChild::Id(id));
        self
    }

    /// Per-call [`ChartStyle`] override. Takes precedence over
    /// `theme.style_slots.chart`.
    pub fn style(mut self, style: impl ChartStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Wire a shared [`ChartSelection`] into this chart: clicking a
    /// slice selects its `(series, point)` key (Ctrl/Cmd-click toggles
    /// it in [`teksilo_data::SelectionMode::Multi`]), clicking empty
    /// space (outside the ring, or the donut hole) clears the
    /// selection, and every selected slice paints an accent-colored
    /// outline. Pass a clone of the same `ChartSelection` to other
    /// charts/widgets to keep selection state in sync.
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

impl<T: Clone + 'static> std::fmt::Debug for PieChart<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PieChart")
            .field("inner_radius_ratio", &self.inner_radius_ratio)
            .field("start_angle_degrees", &self.start_angle_degrees)
            .field("clockwise", &self.clockwise)
            .field("label_mode", &self.label_mode)
            .finish()
    }
}

impl<T: Clone + std::fmt::Display + 'static> Widget for PieChart<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        {
            let registry = ctx.binding_registry();
            // Data swap → relayout (slice angles change, labels change)
            // AND the AT mark list must refresh.
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
            self.hover.bind_to(id, registry, BindingLevel::RepaintOnly);
            if let Some(selection) = &self.selection {
                selection
                    .selection_signal()
                    .bind_to(id, registry, BindingLevel::RepaintOnly);
            }
        }

        // Resolve the center slot via ctx.add_boxed.
        if let Some(c) = self.pending_center.take() {
            self.center_id = Some(match c {
                PendingChild::Id(cid) => cid,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }

        // Hover + tap hit-test handlers. `marks` store already-resolved
        // screen-space slice angles (see `compute_marks`), so the test
        // angle is just the raw pointer bearing — no separate
        // "logical vs. clockwise" conversion needed.
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
                            let (center, outer, inner) = geometry_cache
                                .borrow()
                                .as_ref()
                                .map(|(_, g)| (g.center, g.outer_radius, g.inner_radius))
                                .unwrap_or((Point::ZERO, 0.0, 0.0));
                            if outer <= 0.0 {
                                return EventResponse::Ignored;
                            }
                            let dx = window_pos.x - center.x;
                            let dy = window_pos.y - center.y;
                            let dist = (dx * dx + dy * dy).sqrt();
                            if dist < inner || dist > outer {
                                if hover.get().is_some() {
                                    hover.set(None);
                                }
                                return EventResponse::Ignored;
                            }
                            let raw = dy.atan2(dx);
                            let hit = hit::slice_hit(&marks.borrow(), raw);
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
                    let (center, outer, inner) = geometry_cache
                        .borrow()
                        .as_ref()
                        .map(|(_, g)| (g.center, g.outer_radius, g.inner_radius))
                        .unwrap_or((Point::ZERO, 0.0, 0.0));
                    if outer <= 0.0 {
                        return;
                    }
                    let dx = window_pos.x - center.x;
                    let dy = window_pos.y - center.y;
                    let dist = (dx * dx + dy * dy).sqrt();
                    if dist < inner || dist > outer {
                        selection.clear();
                        return;
                    }
                    let raw = dy.atan2(dx);
                    let hit = hit::slice_hit(&marks.borrow(), raw);
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

        // The center widget is the only child we expose to the tree.
        self.center_id.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let ideal = Size::new(
            proposal.width.unwrap_or(320.0),
            proposal.height.unwrap_or(220.0),
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
        if children.is_empty() {
            return;
        }
        let label_style = TextStyleRole::Tiny.resolve(&ctx.theme.typography);
        let geometry = self.ensure_geometry(bounds, ctx.text_backend, &label_style);
        let side = (geometry.inner_radius * std::f32::consts::FRAC_1_SQRT_2 * 2.0).max(0.0);
        for child in children.iter_mut() {
            child.origin = Point::new(
                geometry.center.x - side * 0.5,
                geometry.center.y - side * 0.5,
            );
            child.size = Size::new(side, side);
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let theme = ctx.theme;
        let enabled = ctx.effective_enabled;
        let Some(_series_id) = self.series_id else {
            return;
        };

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
        self.bounds.set(bounds);
        if geometry.outer_radius <= 0.0 {
            return;
        }

        let marks = self.compute_marks(&geometry);
        if marks.is_empty() {
            *self.marks.borrow_mut() = marks;
            return;
        }
        *self.marks.borrow_mut() = marks.clone();

        let palette = self.palette.get();
        let total: f32 = marks.iter().map(|m| m.value).sum();
        let disc_bounds = Rect::new(
            geometry.center.x - geometry.outer_radius,
            geometry.center.y - geometry.outer_radius,
            geometry.outer_radius * 2.0,
            geometry.outer_radius * 2.0,
        );

        for (i, m) in marks.iter().enumerate() {
            let MarkShape::Slice {
                center,
                inner_radius,
                outer_radius,
                start_rad,
                sweep_rad,
            } = m.shape
            else {
                continue;
            };

            let color = self
                .explicit_colors
                .get(i)
                .and_then(|c| c.clone())
                .map(|c| c.resolve(theme, enabled))
                .unwrap_or_else(|| palette.color_for(i, theme));

            let path = build_slice_path(center, outer_radius, inner_radius, start_rad, sweep_rad);
            let cfg = ChartFillContext {
                series_index: i,
                resolved_color: color,
                theme,
            };
            let fill = style.donut_fill(&cfg);
            let wedge_bounds = path.bounds();
            let projected = project_gradient_to_wedge_local(&fill, disc_bounds, wedge_bounds);
            let paint = PaintProp::from_fill(&projected, &theme.colors).resolve(
                theme,
                enabled,
                wedge_bounds.size(),
            );
            canvas.fill_path(&path, paint);

            if self
                .selection
                .as_ref()
                .is_some_and(|s| s.is_selected(m.series_id, m.point_idx))
            {
                use crate::style::SELECTION_STROKE_WIDTH;
                canvas.stroke_path(&path, theme.colors.accent, SELECTION_STROKE_WIDTH);
            }

            let bisector = start_rad + sweep_rad * 0.5;
            let percent = if total > 0.0 {
                m.value / total * 100.0
            } else {
                0.0
            };
            self.draw_slice_label(
                canvas,
                theme,
                center,
                outer_radius,
                bisector,
                percent,
                &m.category_label,
                &label_style,
            );
        }

        // Embedded legend (Pie synthesizes one entry per SLICE, i.e. per
        // point of the single displayed series — a different granularity
        // than Bar/Line's per-SERIES `ChartLegend`, so it isn't reused
        // here; see the module-level note in the deviations report).
        if self.show_legend && geometry.legend.width > 0.0 && geometry.legend.height > 0.0 {
            self.paint_legend(canvas, geometry.legend, theme, enabled, &label_style);
        }

        // Hover marker + tooltip.
        if self.show_hover_tooltip
            && let Some((sid, idx)) = self.hover.get()
            && let Some(m) = marks
                .iter()
                .find(|m| m.series_id == sid && m.point_idx == idx)
            && let MarkShape::Slice {
                center,
                outer_radius,
                start_rad,
                sweep_rad,
                ..
            } = m.shape
        {
            let bisector = start_rad + sweep_rad * 0.5;
            let r_anchor = outer_radius + 12.0;
            let anchor = Point::new(
                center.x + r_anchor * bisector.cos(),
                center.y + r_anchor * bisector.sin(),
            );
            let percent = if total > 0.0 {
                m.value / total * 100.0
            } else {
                0.0
            };
            let text = format!(
                "{}: {} ({:.1}%)",
                m.category_label,
                format_pie_value(m.value),
                percent
            );
            hit::draw_mark_tooltip(canvas, theme, geometry.plot, anchor, &text, &label_style);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::GraphicsDocument);
        let n = self
            .series_id
            .map(|s| self.model.point_count(s))
            .unwrap_or(0);
        builder.set_name(format!("Pie chart: {} slices", n));

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
        self.center_id.into_iter().collect()
    }
}

impl<T: Clone + std::fmt::Display + 'static> PieChart<T> {
    /// Memoized [`PieGeometry`] for `bounds` — see `BarChart::ensure_geometry`.
    fn ensure_geometry(
        &self,
        bounds: Rect,
        backend: Option<&Rc<RefCell<dyn TextBackend>>>,
        label_style: &TextStyle,
    ) -> PieGeometry {
        let key = GeometryKey {
            bounds,
            structure_version: self.model.structure_version().get(),
        };
        if let Some((cached_key, geometry)) = self.geometry_cache.borrow().as_ref()
            && *cached_key == key
        {
            return *geometry;
        }
        let legend_size = self.compute_legend_size(backend, label_style);
        let geometry = compute_pie_geometry(&PieGeometryParams {
            bounds,
            legend_size,
            legend_position: if self.show_legend {
                Some(self.legend_position)
            } else {
                None
            },
            inner_radius_ratio: self.inner_radius_ratio,
        });
        *self.geometry_cache.borrow_mut() = Some((key, geometry));
        geometry
    }

    fn compute_intrinsic_min(&self, ctx: &LayoutContext) -> Size {
        use crate::style as cs;
        const DISC_FLOOR: f32 = 60.0;
        let label_style = TextStyleRole::Tiny.resolve(&ctx.theme.typography);
        let legend_size = self.compute_legend_size(ctx.text_backend, &label_style);
        let extra_w = if self.show_legend
            && matches!(
                self.legend_position,
                LegendPosition::Leading | LegendPosition::Trailing
            ) {
            legend_size + cs::LEGEND_TO_PLOT_GAP
        } else {
            0.0
        };
        let extra_h = if self.show_legend
            && matches!(
                self.legend_position,
                LegendPosition::Top | LegendPosition::Bottom
            ) {
            legend_size + cs::LEGEND_TO_PLOT_GAP
        } else {
            0.0
        };
        Size::new(
            DISC_FLOOR + cs::PIE_PADDING * 2.0 + extra_w,
            DISC_FLOOR + cs::PIE_PADDING * 2.0 + extra_h,
        )
    }

    /// Compute every slice's geometry + identity from the displayed
    /// series' points. `start_rad`/`sweep_rad` are resolved to ACTUAL
    /// screen-space angles (accounting for `start_angle_degrees` AND
    /// `clockwise`) — unlike the pre-refactor `SliceHit`, which stored
    /// angles in an unrotated "logical" space and required the pointer
    /// handler to convert into it. Storing the resolved angle directly
    /// means [`hit::slice_hit`] and [`build_slice_path`] both consume a
    /// plain `f32.cos()/.sin()` with no extra clockwise branch, and the
    /// bounding-box math in `hit::MarkShape::Slice::bounding_rect` is
    /// correct regardless of chart configuration.
    fn compute_marks(&self, geometry: &PieGeometry) -> Vec<MarkGeometry> {
        let Some(series_id) = self.series_id else {
            return Vec::new();
        };
        let mut marks = Vec::new();
        self.model.with_series_view(series_id, |view| {
            let total: f32 = view.points.iter().map(|d| d.value.max(0.0)).sum();
            if total <= 0.0 {
                return;
            }
            let start_angle_rad = self.start_angle_degrees.to_radians();
            let half_gap = self.slice_gap_degrees.to_radians() * 0.5;
            let mut accum = 0.0_f32;
            for (i, datum) in view.points.iter().enumerate() {
                let v = datum.value.max(0.0);
                let sweep_full = v / total * std::f32::consts::TAU;
                let usable_sweep = (sweep_full - half_gap * 2.0).max(0.0);
                let slice_start = accum + half_gap;
                let start_rad_total = start_angle_rad + slice_start;
                let (screen_start, screen_sweep) = if self.clockwise {
                    (start_rad_total, usable_sweep)
                } else {
                    (-start_rad_total, -usable_sweep)
                };
                marks.push(MarkGeometry {
                    series_id,
                    point_idx: i,
                    series_name: view.name.to_string(),
                    category_label: format!("{}", datum.category),
                    value: v,
                    shape: MarkShape::Slice {
                        center: geometry.center,
                        inner_radius: geometry.inner_radius,
                        outer_radius: geometry.outer_radius,
                        start_rad: screen_start,
                        sweep_rad: screen_sweep,
                    },
                });
                accum += sweep_full;
            }
        });
        marks
    }

    /// Compute the size the embedded per-slice legend would reserve along
    /// its main axis (height for Top/Bottom, width for Leading/Trailing).
    /// Returns 0 when the legend is disabled. `Vertical` orientation
    /// measures the widest slice label via `backend` so the reservation
    /// matches what will later be drawn.
    fn compute_legend_size(
        &self,
        backend: Option<&Rc<RefCell<dyn TextBackend>>>,
        label_style: &TextStyle,
    ) -> f32 {
        use crate::style as cs;
        if !self.show_legend {
            return 0.0;
        }
        let Some(series_id) = self.series_id else {
            return 0.0;
        };
        let orientation = orientation_for_position(self.legend_position);
        match orientation {
            LegendOrientation::Horizontal => cs::LEGEND_SWATCH_SIZE.max(label_style.size * 1.2),
            LegendOrientation::Vertical => {
                let max_w = self
                    .model
                    .with_series_view(series_id, |view| {
                        view.points
                            .iter()
                            .map(|d| {
                                let name = format!("{}", d.category);
                                crate::text::measure_text_width_via(backend, &name, label_style)
                            })
                            .fold(0.0_f32, f32::max)
                    })
                    .unwrap_or(0.0);
                cs::LEGEND_SWATCH_SIZE + 4.0 + max_w
            }
        }
    }

    fn paint_legend(
        &self,
        canvas: &mut Canvas,
        band: Rect,
        theme: &Theme,
        enabled: bool,
        label_style: &TextStyle,
    ) {
        use crate::style as cs;
        let Some(series_id) = self.series_id else {
            return;
        };
        let orientation = orientation_for_position(self.legend_position);
        let label_color = TextRole::Primary.resolve(&theme.colors);
        let line_height = cs::LEGEND_SWATCH_SIZE.max(label_style.size * 1.2);
        let palette = self.palette.get();

        self.model
            .with_series_view(series_id, |view| match orientation {
                LegendOrientation::Horizontal => {
                    let names: Vec<String> = view
                        .points
                        .iter()
                        .map(|d| format!("{}", d.category))
                        .collect();
                    let label_widths: Vec<f32> = names
                        .iter()
                        .map(|n| measure_text_width(canvas, n, label_style))
                        .collect();
                    let item_widths: Vec<f32> = label_widths
                        .iter()
                        .map(|w| cs::LEGEND_SWATCH_SIZE + 4.0 + w)
                        .collect();
                    let total_w: f32 = item_widths.iter().sum::<f32>()
                        + cs::LEGEND_ITEM_GAP * (item_widths.len() as f32 - 1.0).max(0.0);
                    let mut x = band.x + (band.width - total_w) * 0.5;
                    let center_y = band.y + line_height * 0.5;
                    for (i, name) in names.iter().enumerate() {
                        let color = self
                            .explicit_colors
                            .get(i)
                            .and_then(|c| c.clone())
                            .map(|c| c.resolve(theme, enabled))
                            .unwrap_or_else(|| palette.color_for(i, theme));
                        let swatch = Rect::new(
                            x,
                            center_y - cs::LEGEND_SWATCH_SIZE * 0.5,
                            cs::LEGEND_SWATCH_SIZE,
                            cs::LEGEND_SWATCH_SIZE,
                        );
                        canvas.fill_rounded_rect(swatch, CornerRadius::uniform(2.0), color);
                        x += cs::LEGEND_SWATCH_SIZE + 4.0;
                        canvas.draw_text(
                            name,
                            Rect::new(
                                x,
                                center_y - label_style.size * 0.6,
                                label_widths[i],
                                label_style.size * 1.2,
                            ),
                            label_style,
                            label_color,
                        );
                        x += label_widths[i] + cs::LEGEND_ITEM_GAP;
                    }
                }
                LegendOrientation::Vertical => {
                    for (i, datum) in view.points.iter().enumerate() {
                        let name = format!("{}", datum.category);
                        let color = self
                            .explicit_colors
                            .get(i)
                            .and_then(|c| c.clone())
                            .map(|c| c.resolve(theme, enabled))
                            .unwrap_or_else(|| palette.color_for(i, theme));
                        let row_y = band.y + i as f32 * line_height;
                        let center_y = row_y + line_height * 0.5;
                        let swatch = Rect::new(
                            band.x,
                            center_y - cs::LEGEND_SWATCH_SIZE * 0.5,
                            cs::LEGEND_SWATCH_SIZE,
                            cs::LEGEND_SWATCH_SIZE,
                        );
                        canvas.fill_rounded_rect(swatch, CornerRadius::uniform(2.0), color);
                        let label_w = measure_text_width(canvas, &name, label_style);
                        canvas.draw_text(
                            &name,
                            Rect::new(
                                band.x + cs::LEGEND_SWATCH_SIZE + 4.0,
                                center_y - label_style.size * 0.6,
                                label_w,
                                label_style.size * 1.2,
                            ),
                            label_style,
                            label_color,
                        );
                    }
                }
            });
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_slice_label(
        &self,
        canvas: &mut Canvas,
        theme: &Theme,
        center: Point,
        outer: f32,
        bisector_rad: f32,
        percent: f32,
        category: &str,
        label_style: &TextStyle,
    ) {
        use crate::style as cs;
        let label_color = TextRole::Primary.resolve(&theme.colors);
        let min_deg = cs::PIE_MIN_SLICE_LABEL_DEGREES;
        let label_text = if self.show_percentages {
            format!("{} ({:.0}%)", category, percent)
        } else {
            category.to_string()
        };
        let approx_w = measure_text_width(canvas, &label_text, label_style);
        let height = label_style.size * 1.2;
        let (cos, sin) = (bisector_rad.cos(), bisector_rad.sin());

        match self.label_mode {
            PieLabelMode::None => {}
            PieLabelMode::Inside => {
                if percent >= min_deg / 360.0 * 100.0 {
                    let r = outer * 0.65;
                    let lx = center.x + r * cos - approx_w * 0.5;
                    let ly = center.y + r * sin - height * 0.5;
                    canvas.draw_text(
                        &label_text,
                        Rect::new(lx, ly, approx_w, height),
                        label_style,
                        label_color,
                    );
                }
            }
            PieLabelMode::Outside => {
                self.draw_outside_label(
                    canvas,
                    center,
                    outer,
                    cos,
                    sin,
                    &label_text,
                    approx_w,
                    height,
                    label_style,
                    label_color,
                );
            }
            PieLabelMode::InsideWithLeaders => {
                if percent >= min_deg / 360.0 * 100.0 {
                    let r = outer * 0.65;
                    let lx = center.x + r * cos - approx_w * 0.5;
                    let ly = center.y + r * sin - height * 0.5;
                    canvas.draw_text(
                        &label_text,
                        Rect::new(lx, ly, approx_w, height),
                        label_style,
                        label_color,
                    );
                } else {
                    self.draw_outside_label(
                        canvas,
                        center,
                        outer,
                        cos,
                        sin,
                        &label_text,
                        approx_w,
                        height,
                        label_style,
                        label_color,
                    );
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_outside_label(
        &self,
        canvas: &mut Canvas,
        center: Point,
        outer: f32,
        cos: f32,
        sin: f32,
        label_text: &str,
        approx_w: f32,
        height: f32,
        label_style: &TextStyle,
        label_color: teksilo_tokens::Color,
    ) {
        use crate::style as cs;
        let r_inner = outer * 0.95;
        let r_outer = outer + cs::PIE_LEADER_LENGTH;
        let p1 = Point::new(center.x + r_inner * cos, center.y + r_inner * sin);
        let p2 = Point::new(center.x + r_outer * cos, center.y + r_outer * sin);
        canvas.draw_line(p1, p2, label_color, 1.0);
        let lx_anchor = center.x + (r_outer + cs::PIE_LABEL_GAP) * cos;
        let lx = if cos >= 0.0 {
            lx_anchor
        } else {
            lx_anchor - approx_w
        };
        let ly = center.y + (r_outer + cs::PIE_LABEL_GAP) * sin - height * 0.5;
        canvas.draw_text(
            label_text,
            Rect::new(lx, ly, approx_w, height),
            label_style,
            label_color,
        );
    }
}

/// Build a path for a single slice. For pie (`inner == 0`) this is a
/// pie wedge; for donut (`inner > 0`) it's a hollow ring segment.
/// `start_rad`/`sweep_rad` are in the same actual-screen-space angle
/// convention as [`MarkShape::Slice`] (standard `atan2`; `sweep_rad` may
/// be negative), so plain `cos`/`sin` place every point with no extra
/// clockwise handling (the pre-refactor version needed a `clockwise`
/// parameter plus a `bisector_direction` mirror helper — folded into
/// `PieChart::compute_marks`'s angle resolution instead).
fn build_slice_path(center: Point, outer: f32, inner: f32, start_rad: f32, sweep_rad: f32) -> Path {
    let start_deg = start_rad.to_degrees();
    let sweep_deg = sweep_rad.to_degrees();
    let end_rad = start_rad + sweep_rad;

    let sx_o = center.x + outer * start_rad.cos();
    let sy_o = center.y + outer * start_rad.sin();
    let outer_rect = Rect::new(center.x - outer, center.y - outer, outer * 2.0, outer * 2.0);

    let mut path = Path::new();
    if inner <= 0.0 {
        path.move_to(center);
        path.line_to(Point::new(sx_o, sy_o));
        path.arc_to(outer_rect, start_deg, sweep_deg);
        path.close();
    } else {
        let ex_i = center.x + inner * end_rad.cos();
        let ey_i = center.y + inner * end_rad.sin();
        let inner_rect = Rect::new(center.x - inner, center.y - inner, inner * 2.0, inner * 2.0);
        path.move_to(Point::new(sx_o, sy_o));
        path.arc_to(outer_rect, start_deg, sweep_deg);
        path.line_to(Point::new(ex_i, ey_i));
        path.arc_to(inner_rect, start_deg + sweep_deg, -sweep_deg);
        path.close();
    }
    path
}

/// Remap a [`FillRecipe`]'s gradient coordinates from disc-normalized
/// (`center`/`radius` fractions of the FULL disc bounds) to the given
/// wedge's own local bounds, so every wedge samples one continuous
/// radial field instead of restarting the gradient at its own bounding
/// box (which would show as visible seams between adjacent slices).
///
/// `Solid`/`None`/`StateLayer` pass through unchanged (no coordinates to
/// remap). `LinearGradient` also passes through unchanged: its angle+size
/// model draws the gradient axis through each rect's OWN center
/// (`PaintProp::resolve`'s `angle_to_endpoints`), which has no
/// wedge-bounds-only remapping that reconstructs one straight line shared
/// by every wedge (their bounding rects differ in both size and aspect
/// ratio) — a caller supplying a `LinearGradient` donut_fill gets a
/// per-wedge angle-only gradient, not a disc-continuous field.
/// `RadialGradient` is the recommended (and only fully continuous)
/// donut-fill gradient shape.
fn project_gradient_to_wedge_local(
    fill: &FillRecipe,
    disc_bounds: Rect,
    wedge_bounds: Rect,
) -> FillRecipe {
    match fill {
        FillRecipe::Solid(_) | FillRecipe::None | FillRecipe::StateLayer { .. } => fill.clone(),
        FillRecipe::LinearGradient { .. } => fill.clone(),
        FillRecipe::RadialGradient {
            stops,
            center,
            radius,
        } => {
            let abs_center = Point::new(
                disc_bounds.x + center.0 * disc_bounds.width,
                disc_bounds.y + center.1 * disc_bounds.height,
            );
            let abs_radius = radius * disc_bounds.width.max(disc_bounds.height);
            let new_center = (
                if wedge_bounds.width > 0.0 {
                    (abs_center.x - wedge_bounds.x) / wedge_bounds.width
                } else {
                    0.5
                },
                if wedge_bounds.height > 0.0 {
                    (abs_center.y - wedge_bounds.y) / wedge_bounds.height
                } else {
                    0.5
                },
            );
            let wedge_longer = wedge_bounds
                .width
                .max(wedge_bounds.height)
                .max(f32::EPSILON);
            let new_radius = abs_radius / wedge_longer;
            FillRecipe::RadialGradient {
                stops: stops.clone(),
                center: new_center,
                radius: new_radius,
            }
        }
    }
}

/// Pie has no formal y-axis but we want consistent number formatting in
/// tooltips.
fn format_pie_value(v: f32) -> String {
    if v.fract() == 0.0 {
        format!("{:.0}", v)
    } else {
        format!("{:.2}", v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_canvas::SizeProposal;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_data::ChartDatum;
    use teksilo_i18n::lit;

    fn three_slices_model() -> ChartModel<String> {
        ChartModel::from_points(vec![
            ChartDatum::new("A".to_string(), 30.0),
            ChartDatum::new("B".to_string(), 50.0),
            ChartDatum::new("C".to_string(), 20.0),
        ])
    }

    #[test]
    fn three_slices_three_paths() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(PieChart::new(three_slices_model()));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let frame = tree.render();
        assert_eq!(
            frame.paths.len(),
            3,
            "expected 3 wedge paths, got {}",
            frame.paths.len()
        );
    }

    #[test]
    fn donut_inner_radius_creates_hollow_wedges() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(PieChart::new(three_slices_model()).donut(0.5));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let frame = tree.render();
        assert_eq!(frame.paths.len(), 3);
        let cmds_pie = {
            let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
            tree.add(PieChart::new(three_slices_model()));
            tree.layout(SizeProposal::exact(400.0, 300.0));
            let f = tree.render();
            f.paths[0].path.commands.len()
        };
        assert!(
            frame.paths[0].path.commands.len() > cmds_pie,
            "donut path should have more commands than pie path"
        );
    }

    #[test]
    fn empty_data_does_not_panic() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(PieChart::<String>::new(ChartModel::new()));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = tree.render();
    }

    #[test]
    fn all_zero_values_do_not_panic() {
        let model = ChartModel::from_points(vec![
            ChartDatum::new("A".to_string(), 0.0),
            ChartDatum::new("B".to_string(), 0.0),
        ]);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(PieChart::new(model));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = tree.render();
    }

    #[test]
    fn accessibility_role_is_graphics_document() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(PieChart::new(three_slices_model()));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), teksilo_core::accesskit::Role::GraphicsDocument);
    }

    #[test]
    fn pie_ignores_center_slot() {
        use teksilo_widgets::TextWidget;
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let pie = PieChart::new(three_slices_model()).center(TextWidget::new(lit!("$100")));
        let id = tree.add(pie);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let kids = tree.children(id);
        assert!(!kids.is_empty(), "center widget child registered");
        let child_bounds = tree.bounds(kids[0]);
        assert_eq!(child_bounds.width, 0.0);
        assert_eq!(child_bounds.height, 0.0);
    }

    #[test]
    fn donut_center_slot_has_inscribed_size() {
        use teksilo_widgets::TextWidget;
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let donut = PieChart::new(three_slices_model())
            .donut(0.6)
            .center(TextWidget::new(lit!("$100")));
        let id = tree.add(donut);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let kids = tree.children(id);
        let bounds = tree.bounds(kids[0]);
        assert!(
            bounds.width > 0.0 && bounds.height > 0.0,
            "donut center slot should be inscribed in inner radius (got {:?})",
            bounds
        );
    }

    #[test]
    fn donut_center_slot_centered_with_legend() {
        use teksilo_widgets::TextWidget;
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            PieChart::new(three_slices_model())
                .donut(0.6)
                .legend(true)
                .legend_position(LegendPosition::Bottom)
                .center(TextWidget::new(lit!("100"))),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let kids = tree.children(id);
        let child = tree.bounds(kids[0]);
        let chart_center_y = 150.0;
        let child_center_y = child.y + child.height * 0.5;
        assert!(
            child_center_y < chart_center_y,
            "child center y {} should be above geometric mid {} when legend at bottom",
            child_center_y,
            chart_center_y
        );
    }

    #[test]
    fn slice_color_overrides_palette() {
        use teksilo_tokens::Color;
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(
            PieChart::new(three_slices_model())
                .slice_color(0, Color::RED)
                .slice_color(2, Color::BLUE),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let frame = tree.render();
        assert_eq!(frame.paths[0].color, Color::RED.to_array());
        assert_eq!(frame.paths[2].color, Color::BLUE.to_array());
        assert_ne!(frame.paths[1].color, Color::RED.to_array());
        assert_ne!(frame.paths[1].color, Color::BLUE.to_array());
    }

    #[test]
    fn pointer_move_over_slice_sets_hover() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let chart = PieChart::new(three_slices_model());
        let marks_handle = chart.marks.clone();
        let hover_handle = chart.hover.clone();
        tree.add(chart);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = tree.render();

        let (target, sid, idx) = {
            let marks = marks_handle.borrow();
            let m = marks.first().expect("at least one mark");
            let MarkShape::Slice {
                center,
                outer_radius,
                start_rad,
                sweep_rad,
                ..
            } = m.shape
            else {
                panic!("expected slice mark")
            };
            let bisector = start_rad + sweep_rad * 0.5;
            let r = outer_radius * 0.5;
            let p = Point::new(center.x + r * bisector.cos(), center.y + r * bisector.sin());
            (p, m.series_id, m.point_idx)
        };
        tree.pointer_move(target);
        assert_eq!(hover_handle.get(), Some((sid, idx)));
    }

    #[test]
    fn slice_bounding_rects_cover_full_circle_at_default_config() {
        // Regression guard for the screen-space angle resolution: with
        // the default start_angle=-90 (12 o'clock) + clockwise=true, the
        // union of all slice bounding rects should span (approximately)
        // the full disc, not be rotated/mirrored off it.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let chart = PieChart::new(three_slices_model());
        let marks_handle = chart.marks.clone();
        tree.add(chart);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = tree.render();
        let marks = marks_handle.borrow();
        let MarkShape::Slice {
            center,
            outer_radius,
            ..
        } = marks[0].shape
        else {
            panic!("expected slice")
        };
        let mut min_x = f32::MAX;
        let mut max_x = f32::MIN;
        for m in marks.iter() {
            let r = m.shape.bounding_rect();
            min_x = min_x.min(r.x);
            max_x = max_x.max(r.right());
        }
        assert!((min_x - (center.x - outer_radius)).abs() < 1.0);
        assert!((max_x - (center.x + outer_radius)).abs() < 1.0);
    }

    #[test]
    fn counter_clockwise_slices_still_hit_test_correctly() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let chart = PieChart::new(three_slices_model()).clockwise(false);
        let marks_handle = chart.marks.clone();
        let hover_handle = chart.hover.clone();
        tree.add(chart);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = tree.render();

        let (target, sid, idx) = {
            let marks = marks_handle.borrow();
            let m = &marks[1];
            let MarkShape::Slice {
                center,
                outer_radius,
                start_rad,
                sweep_rad,
                ..
            } = m.shape
            else {
                panic!("expected slice mark")
            };
            let bisector = start_rad + sweep_rad * 0.5;
            let r = outer_radius * 0.5;
            let p = Point::new(center.x + r * bisector.cos(), center.y + r * bisector.sin());
            (p, m.series_id, m.point_idx)
        };
        tree.pointer_move(target);
        assert_eq!(hover_handle.get(), Some((sid, idx)));
    }

    #[test]
    fn gradient_donut_fill_produces_gradient_path_data() {
        use teksilo_canvas::render_frame::PaintData;
        use teksilo_core::styles::{GradientStop, RecipeColor, Theme};
        use teksilo_tokens::Color;

        #[derive(Debug)]
        struct GradientStyle;
        impl ChartStyle for GradientStyle {
            fn bar_fill(&self, cfg: &ChartFillContext) -> FillRecipe {
                FillRecipe::Solid(RecipeColor::Static(cfg.resolved_color))
            }
            fn area_fill(&self, cfg: &ChartFillContext, _opacity: f32) -> FillRecipe {
                FillRecipe::Solid(RecipeColor::Static(cfg.resolved_color))
            }
            fn donut_fill(&self, cfg: &ChartFillContext) -> FillRecipe {
                FillRecipe::RadialGradient {
                    stops: vec![
                        GradientStop {
                            offset: 0.0,
                            color: RecipeColor::Static(cfg.resolved_color),
                        },
                        GradientStop {
                            offset: 1.0,
                            color: RecipeColor::Static(Color::WHITE),
                        },
                    ],
                    center: (0.5, 0.5),
                    radius: 0.5,
                }
            }
            fn gridline(&self, _theme: &Theme) -> teksilo_core::styles::BorderRecipe {
                teksilo_core::styles::BorderRecipe::none()
            }
        }

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(PieChart::new(three_slices_model()).style(GradientStyle));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let frame = tree.render();
        assert!(
            frame
                .paths
                .iter()
                .all(|p| matches!(p.paint_data, PaintData::RadialGradient { .. })),
            "expected every wedge to carry radial-gradient paint data"
        );
    }

    #[test]
    fn default_style_produces_solid_path_data() {
        use teksilo_canvas::render_frame::PaintData;
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(PieChart::new(three_slices_model()));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let frame = tree.render();
        assert!(
            frame.paths.iter().all(|p| p.paint_data == PaintData::Solid),
            "default RecipeChartStyle should be flat-colored"
        );
    }

    #[test]
    fn tap_on_slice_selects_point() {
        use teksilo_core::event::PointerButton;
        use teksilo_data::SelectionMode;

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let sel = ChartSelection::new(SelectionMode::Single);
        let chart = PieChart::new(three_slices_model()).selection(sel.clone());
        let marks_handle = chart.marks.clone();
        tree.add(chart);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = tree.render();

        let (target, sid, idx) = {
            let marks = marks_handle.borrow();
            let m = marks.first().expect("at least one mark");
            let MarkShape::Slice {
                center,
                outer_radius,
                start_rad,
                sweep_rad,
                ..
            } = m.shape
            else {
                panic!("expected slice mark")
            };
            let bisector = start_rad + sweep_rad * 0.5;
            let r = outer_radius * 0.5;
            let p = Point::new(center.x + r * bisector.cos(), center.y + r * bisector.sin());
            (p, m.series_id, m.point_idx)
        };
        tree.pointer_down_button(target, PointerButton::Primary);
        tree.pointer_up_button(target, PointerButton::Primary);
        assert!(sel.is_selected(sid, idx));
    }

    #[test]
    fn tap_outside_ring_clears_selection() {
        use teksilo_core::event::PointerButton;
        use teksilo_data::SelectionMode;

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let sel = ChartSelection::new(SelectionMode::Single);
        let chart = PieChart::new(three_slices_model()).selection(sel.clone());
        let marks_handle = chart.marks.clone();
        tree.add(chart);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = tree.render();

        let target = {
            let marks = marks_handle.borrow();
            let m = marks.first().expect("at least one mark");
            let MarkShape::Slice {
                center,
                outer_radius,
                start_rad,
                sweep_rad,
                ..
            } = m.shape
            else {
                panic!("expected slice mark")
            };
            let bisector = start_rad + sweep_rad * 0.5;
            let r = outer_radius * 0.5;
            Point::new(center.x + r * bisector.cos(), center.y + r * bisector.sin())
        };
        tree.pointer_down_button(target, PointerButton::Primary);
        tree.pointer_up_button(target, PointerButton::Primary);
        assert_eq!(sel.count(), 1);

        // Top-left corner of the widget: outside the disc entirely.
        let outside = Point::new(1.0, 1.0);
        tree.pointer_down_button(outside, PointerButton::Primary);
        tree.pointer_up_button(outside, PointerButton::Primary);
        assert_eq!(
            sel.count(),
            0,
            "tap outside the pie/donut ring should clear selection"
        );
    }

    #[test]
    fn selected_slice_paints_highlight_outline() {
        use teksilo_data::SelectionMode;

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let sel = ChartSelection::new(SelectionMode::Single);
        let chart = PieChart::new(three_slices_model()).selection(sel.clone());
        let marks_handle = chart.marks.clone();
        tree.add(chart);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let baseline_paths = tree.render().paths.len();

        let (sid, idx) = {
            let marks = marks_handle.borrow();
            let m = marks.first().expect("at least one mark");
            (m.series_id, m.point_idx)
        };
        sel.select_point(sid, idx);
        let after = tree.render();
        assert!(
            after.paths.len() > baseline_paths,
            "expected an extra outline path after selecting a slice (baseline {}, after {})",
            baseline_paths,
            after.paths.len()
        );
    }
}
