// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! BarChart — vertical or horizontal bars, one or more series.
//!
//! Bound to a [`ChartModel`]. Supports grouped multi-series, horizontal
//! orientation, value labels, grid lines, axis titles, an embedded
//! interactive legend, a per-datum readout with a shared tooltip card —
//! raised by a hover, a held finger, keyboard traversal or an assistive
//! technology — and per-datum accessibility marks.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use teksilo_canvas::{Canvas, Path, Point, Rect, Size, SizeProposal, TextBackend};
use teksilo_core::Theme;
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::event::WidgetEvent;
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
    /// The density the legend band was reserved at. An interactive legend's
    /// rows are sized from `InputTokens::target_size`, and a density switch
    /// changes neither the bounds nor the model version — so without this
    /// the memoized geometry would keep the band the previous density
    /// reserved.
    density: teksilo_tokens::TargetDensity,
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
    /// The datum keyboard traversal is sitting on, and the ring the chart
    /// paints while it has focus. Separate from `hover` because the
    /// readout is a *value* the reader is looking at and the focus is a
    /// *position* they are moving from: an assistive-technology Click, or
    /// a pointer, can move the readout without moving the keyboard
    /// position, and vice versa.
    focus_mark: Signal<Option<(SeriesId, usize)>>,
    /// Whether the chart itself holds keyboard focus, so the focus ring is
    /// not painted on a chart that has lost it.
    focused: Signal<bool>,
    readout: hit::ReadoutState,
    marks: Rc<RefCell<Vec<MarkGeometry>>>,
    bounds: Rc<Cell<Rect>>,
    /// The density tokens in force, captured in `build()` — the only place
    /// a widget can read them — so `accessibility()`, which has no theme,
    /// reserves the same legend band `paint()` does.
    input: teksilo_tokens::InputTokens,
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
            focus_mark: Signal::new(None),
            focused: Signal::new(false),
            readout: hit::ReadoutState::new(),
            marks: Rc::new(RefCell::new(Vec::new())),
            bounds: Rc::new(Cell::new(Rect::ZERO)),
            input: teksilo_tokens::InputTokens::default(),
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

    /// Draw a labelled line across the plot at `value` on the value axis.
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

    /// Whether consulting a bar raises a tooltip card and updates the readout
    /// state (also observable via `hover_signal`) — by a hover, a held finger,
    /// keyboard traversal or an assistive technology's `Click`. Default `true`.
    ///
    /// It does not govern `selection`: the tap that selects is gated
    /// separately, so a chart with a selection still selects with this off.
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

    /// A clone of the live readout signal — the `(series, point)` key the
    /// readout currently names, or `None`. Whichever route set it: a pointer
    /// hover or press, keyboard traversal, or an assistive technology's
    /// `Click`. Lets an app observe
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
        self.input = ctx.theme().input;
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
            self.readout
                .contact
                .bind_to(id, registry, BindingLevel::RepaintOnly);
            self.focused
                .bind_to(id, registry, BindingLevel::RepaintOnly);
            self.focus_mark
                .bind_to(id, registry, BindingLevel::RepaintOnly);
            // The focused datum is published as the chart's
            // `active_descendant`, so the AT tree has to be rebuilt when it
            // moves — a repaint alone leaves a screen reader on the datum
            // the user has arrowed away from.
            self.focus_mark
                .bind_to(id, registry, BindingLevel::AccessibilityOnly);
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
                let readout = self.readout.clone();
                // The density's tokens, read where every recipe reads
                // them: at build time, in the crate that owns the
                // dimension. `set_input_density` rebuilds the tree, so a
                // density switch re-enters this closure's construction.
                let tokens = ctx.theme().input;
                handlers = handlers.on_pointer_event(move |event, ctx: &mut EventContext| {
                    let tolerance = hit::mark_tolerance(ctx.pointer_kind(), &tokens);
                    let b = bounds.get();
                    let plot = geometry_cache.borrow().as_ref().map(|(_, g)| g.plot);
                    let resolve = |local: Point| -> Option<hit::MarkKey> {
                        let plot = plot?;
                        let window_pos = Point::new(local.x + b.x, local.y + b.y);
                        // The plot gate grows with the tolerance too, or a
                        // coarse near-miss at the plot's own edge would be
                        // refused before any bar was consulted. Zero for a
                        // precise pointer, so the gate is unchanged there.
                        if !plot.expand(tolerance).contains(window_pos) {
                            return None;
                        }
                        let marks = marks.borrow();
                        hit::rect_hit_within(&marks, window_pos, tolerance)
                            .and_then(|idx| marks.get(idx).map(|m| (m.series_id, m.point_idx)))
                    };
                    let describe = |key: hit::MarkKey| {
                        let marks = marks.borrow();
                        hit::mark_index_of(&marks, key)
                            .and_then(|idx| marks.get(idx))
                            .map(hit::mark_description)
                    };
                    hit::drive_readout(event, ctx, &hover, &readout, resolve, describe)
                });
            }

            if let Some(selection) = self.selection.clone() {
                let marks = self.marks.clone();
                let bounds = self.bounds.clone();
                let geometry_cache = self.geometry_cache.clone();
                let tap_tokens = ctx.theme().input;
                handlers = handlers.on_tap(move |tap: &TapEvent, ctx: &mut EventContext| {
                    let b = bounds.get();
                    let window_pos = Point::new(tap.position.x + b.x, tap.position.y + b.y);
                    let Some(plot) = geometry_cache.borrow().as_ref().map(|(_, g)| g.plot) else {
                        return;
                    };
                    if !plot
                        .expand(hit::mark_tolerance(ctx.pointer_kind(), &tap_tokens))
                        .contains(window_pos)
                    {
                        selection.clear();
                        return;
                    }
                    let hit = hit::rect_hit_within(
                        &marks.borrow(),
                        window_pos,
                        hit::mark_tolerance(ctx.pointer_kind(), &tap_tokens),
                    );
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

            // Keyboard datum traversal. A chart is a tab stop exactly
            // when it has something for the keyboard to do — a readout to
            // move or a selection to commit — so a decorative chart with
            // neither adds no stop. The focus ring the chart paints is
            // what keeps that stop visible (WCAG 2.4.7).
            {
                let marks = self.marks.clone();
                let hover = self.hover.clone();
                let focus_mark = self.focus_mark.clone();
                let readout = self.readout.clone();
                let selection = self.selection.clone();
                handlers = handlers.focusable(true).on_key(
                    move |event: &WidgetEvent, ctx: &mut EventContext| {
                        let marks = marks.borrow();
                        hit::drive_readout_keys(
                            event,
                            ctx,
                            &marks,
                            &hover,
                            &focus_mark,
                            &readout,
                            selection.as_ref(),
                        )
                    },
                );
            }
            {
                let focused = self.focused.clone();
                let hover = self.hover.clone();
                let focus_mark = self.focus_mark.clone();
                let readout = self.readout.clone();
                handlers = handlers.on_focus(move |has_focus, _ctx: &mut EventContext| {
                    focused.set(has_focus);
                    if !has_focus {
                        hit::clear_readout_focus(&hover, &focus_mark, &readout);
                    }
                });
            }
            {
                let marks = self.marks.clone();
                let hover = self.hover.clone();
                let focus_mark = self.focus_mark.clone();
                let readout = self.readout.clone();
                let selection = self.selection.clone();
                handlers = handlers.on_access_action_request(
                    move |action, node, _data, ctx: &mut EventContext| {
                        let marks = marks.borrow();
                        hit::handle_mark_action(
                            action,
                            node,
                            id,
                            ctx,
                            &marks,
                            &hover,
                            &focus_mark,
                            &readout,
                            selection.as_ref(),
                        )
                    },
                );
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

            // Keyboard focus ring, drawn outside the selection outline so a
            // datum that is both focused and selected shows two rings
            // rather than one ambiguous one.
            if self.focused.get() && self.focus_mark.get() == Some((m.series_id, m.point_idx)) {
                use crate::style::{SELECTION_BAR_OUTLINE_PAD, SELECTION_STROKE_WIDTH};
                let pad = SELECTION_BAR_OUTLINE_PAD + SELECTION_STROKE_WIDTH;
                canvas.stroke_rounded_rect(
                    rect.expand(pad),
                    CornerRadius::uniform(self.bar_corner_radius.unwrap_or(0.0) + pad),
                    theme.colors.focus_ring,
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
            hit::draw_mark_tooltip(
                canvas,
                theme,
                plot,
                anchor,
                self.readout.contact.get(),
                &text,
                &label_style,
            );
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
        let focused_mark = self.focus_mark.get();
        let mut active = None;
        for m in &marks {
            let node = hit::emit_mark_node(builder, m);
            if focused_mark == Some((m.series_id, m.point_idx)) {
                active = Some(node);
            }
        }
        // Roving virtual focus: the chart keeps the real arena focus and
        // points at the datum the arrows have reached, so a screen reader
        // follows the traversal without the marks needing arena nodes of
        // their own.
        if let Some(node) = active {
            builder.set_active_descendant(node);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.legend_id.into_iter().collect()
    }
}

impl<T: Clone + std::fmt::Display + 'static> BarChart<T> {
    /// Memoized [`PlotGeometry`] for `bounds` — recomputed only when
    /// `bounds`, `model.structure_version()` or the target density changed
    /// since the last call. Shared by `paint()` and `accessibility()` so a
    /// mark's bounds never disagree between the visual tree and the AT
    /// tree, even without an intervening paint.
    fn ensure_geometry(
        &self,
        bounds: Rect,
        backend: Option<&Rc<RefCell<dyn TextBackend>>>,
        label_style: &TextStyle,
    ) -> PlotGeometry {
        let key = GeometryKey {
            bounds,
            structure_version: self.model.structure_version().get(),
            density: self.input.density,
        };
        if let Some((cached_key, geometry)) = self.geometry_cache.borrow().as_ref()
            && *cached_key == key
        {
            return geometry.clone();
        }

        let legend_orientation = orientation_for_position(self.legend_position);
        let legend_size = if self.show_legend {
            legend_main_axis_size(
                backend,
                &self.model,
                label_style,
                legend_orientation,
                self.legend_interactive,
                &self.input,
            )
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
                self.legend_interactive,
                &ctx.theme.input,
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
    /// hit-test (`hit::rect_hit_within`), and `accessibility()` (per-mark AT
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

    // ── The readout: who raises it, and who retires it ────────────────────
    //
    // These are the tests for census rows 8-10 in
    // `docs/hover-affordance-census.md`. The defect they close is not that a
    // finger cannot raise the readout — a contact's `PointerMove` always
    // could — but that nothing retired it: a contact never receives a
    // `PointerLeave`, so a readout a finger raised stayed up for the rest of
    // the program's life.

    struct Fixture {
        tree: WidgetTree,
        chart: WidgetId,
        marks: Rc<RefCell<Vec<MarkGeometry>>>,
        hover: Signal<Option<(SeriesId, usize)>>,
        readout: hit::ReadoutState,
        focus: Signal<Option<(SeriesId, usize)>>,
    }

    /// Mount `chart`, lay it out at 400×200 and paint once so the mark
    /// vector every hit test reads is populated.
    fn fixture(chart: BarChart<String>) -> Fixture {
        let marks = chart.marks.clone();
        let hover = chart.hover.clone();
        let readout = chart.readout.clone();
        let focus = chart.focus_mark.clone();
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(chart);
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let _ = tree.render();
        Fixture {
            tree,
            chart: id,
            marks,
            hover,
            readout,
            focus,
        }
    }

    /// Window-space centre of the mark at `idx`, and its key.
    fn bar_center(
        marks: &Rc<RefCell<Vec<MarkGeometry>>>,
        idx: usize,
    ) -> (Point, (SeriesId, usize)) {
        let marks = marks.borrow();
        let m = marks.get(idx).expect("mark");
        let MarkShape::Rect(r) = m.shape else {
            panic!("expected a bar")
        };
        (
            Point::new(r.x + r.width * 0.5, r.y + r.height * 0.5),
            (m.series_id, m.point_idx),
        )
    }

    #[test]
    fn a_finger_press_raises_the_readout_before_it_moves() {
        let f = fixture(BarChart::new(sample_model()));
        let (target, key) = bar_center(&f.marks, 1);
        let mut tree = f.tree;
        let c = tree.new_contact();
        tree.touch_down(c, target);
        assert_eq!(
            f.hover.get(),
            Some(key),
            "a still finger produces no move, so the press has to raise it"
        );
        assert_eq!(
            f.readout.contact.get(),
            Some(target),
            "and the card is held clear of the contact, not of the mark"
        );
    }

    #[test]
    fn a_finger_tap_on_a_bar_pins_the_readout_and_anchors_it_on_the_mark() {
        let f = fixture(BarChart::new(sample_model()));
        let (target, key) = bar_center(&f.marks, 1);
        let mut tree = f.tree;
        let c = tree.new_contact();
        tree.touch_down(c, target);
        tree.touch_up(c, target);
        assert_eq!(f.hover.get(), Some(key), "a tap pins the datum it named");
        assert_eq!(
            f.readout.contact.get(),
            None,
            "the finger has gone, so the card returns to the mark"
        );
    }

    #[test]
    fn a_finger_lifting_after_a_scrub_retires_the_readout() {
        let f = fixture(BarChart::new(sample_model()));
        let (first, _) = bar_center(&f.marks, 0);
        let (second, second_key) = bar_center(&f.marks, 2);
        let mut tree = f.tree;
        let c = tree.new_contact();
        tree.touch_down(c, first);
        tree.touch_move(c, second);
        assert_eq!(
            f.hover.get(),
            Some(second_key),
            "the readout follows the contact while it is down"
        );
        tree.touch_up(c, second);
        assert_eq!(
            f.hover.get(),
            None,
            "and the lift ends the scrub — this is the retire path a contact \
             never had, because it receives no PointerLeave"
        );
        assert_eq!(f.readout.contact.get(), None);
    }

    #[test]
    fn a_finger_tapping_away_from_the_data_retires_a_pinned_readout() {
        let f = fixture(BarChart::new(sample_model()));
        let (target, key) = bar_center(&f.marks, 1);
        let mut tree = f.tree;
        let c = tree.new_contact();
        tree.touch_down(c, target);
        tree.touch_up(c, target);
        assert_eq!(f.hover.get(), Some(key));
        // Top-left corner: inside the widget, outside the plot.
        let away = Point::new(1.0, 1.0);
        let c2 = tree.new_contact();
        tree.touch_down(c2, away);
        tree.touch_up(c2, away);
        assert_eq!(f.hover.get(), None);
    }

    /// A cancel is routed to the pointer's **captor**, or failing that to a
    /// widget that answered `Handled` to one of its positional events
    /// (`cancel_recipient` in `teksilo-core`'s cancel funnel). A chart's
    /// readout claims nothing — it observes the pointer stream and returns
    /// `Ignored` so a pan claim or a tap above it still works — so the chart
    /// is told about a cancel only when something else on it holds the
    /// pointer. A selectable chart's own tap recognizer is exactly that.
    #[test]
    fn a_cancelled_contact_retires_the_readout_of_a_selectable_chart() {
        use teksilo_data::SelectionMode;
        let f = fixture(
            BarChart::new(sample_model()).selection(ChartSelection::new(SelectionMode::Single)),
        );
        let (target, key) = bar_center(&f.marks, 1);
        let mut tree = f.tree;
        let c = tree.new_contact();
        tree.touch_down(c, target);
        assert_eq!(f.hover.get(), Some(key));
        tree.touch_cancel(c, target);
        assert_eq!(
            f.hover.get(),
            None,
            "a revoked interaction is not an inspection"
        );
    }

    /// The one readout rule that is NOT kind-aware, and the one deliberate
    /// change to what a mouse does: a cancel retires the readout for every
    /// pointer kind, where the raw handler used to ignore `PointerCancel`
    /// entirely. A revoked interaction is not an inspection. Listed for the
    /// CHANGELOG and recorded in docs/charts.md §9.
    #[test]
    fn a_cancel_retires_a_mouses_readout_too() {
        use teksilo_core::event::PointerButton;
        use teksilo_data::SelectionMode;
        let f = fixture(
            BarChart::new(sample_model()).selection(ChartSelection::new(SelectionMode::Single)),
        );
        let (target, key) = bar_center(&f.marks, 1);
        let mut tree = f.tree;
        tree.pointer_move(target);
        // Pressed, so the chart's own tap recognizer holds the pointer and the
        // cancel funnel has somebody to address.
        tree.pointer_down_button(target, PointerButton::Primary);
        assert_eq!(f.hover.get(), Some(key));
        let mut ops = teksilo_core::window::NoopWindowOps;
        tree.cancel_all_pointers(
            teksilo_core::pointer::CancelReason::WindowDeactivated,
            &mut ops,
        );
        assert_eq!(f.hover.get(), None);
    }

    /// The other side of that, stated rather than hidden: a read-only chart
    /// holds no pointer, so a cancel does not reach it and its readout
    /// survives the revoked press. The next press is what retires it. This is
    /// a bounded staleness, not a leak — but it is a real limitation and it is
    /// recorded in docs/charts.md §9.
    #[test]
    fn a_cancel_a_read_only_chart_never_hears_is_retired_by_the_next_press() {
        let f = fixture(BarChart::new(sample_model()));
        let (target, key) = bar_center(&f.marks, 1);
        let mut tree = f.tree;
        let c = tree.new_contact();
        tree.touch_down(c, target);
        tree.touch_cancel(c, target);
        assert_eq!(
            f.hover.get(),
            Some(key),
            "the framework addressed the cancel to nobody"
        );
        let away = Point::new(1.0, 1.0);
        let c2 = tree.new_contact();
        tree.touch_down(c2, away);
        assert_eq!(f.hover.get(), None);
    }

    #[test]
    fn a_finger_tap_announces_the_datum_once() {
        let f = fixture(BarChart::new(sample_model()));
        let (target, _) = bar_center(&f.marks, 1);
        let expected = {
            let marks = f.marks.borrow();
            hit::mark_description(&marks[1])
        };
        let mut tree = f.tree;
        let c = tree.new_contact();
        tree.touch_down(c, target);
        tree.touch_up(c, target);
        let _ = tree.sync_accessibility();
        let spoken: Vec<String> = tree
            .announcements_since(0)
            .into_iter()
            .map(|a| a.text)
            .collect();
        assert_eq!(
            spoken,
            vec![expected],
            "a pinned readout announces the datum, exactly once"
        );
    }

    #[test]
    fn a_mouse_release_leaves_its_readout_standing_and_says_nothing() {
        let f = fixture(BarChart::new(sample_model()));
        let (target, key) = bar_center(&f.marks, 1);
        let mut tree = f.tree;
        tree.pointer_move(target);
        assert_eq!(f.hover.get(), Some(key));
        assert_eq!(
            f.readout.contact.get(),
            None,
            "a mouse anchors the card on the mark, never on the cursor"
        );
        tree.pointer_down_button(target, teksilo_core::event::PointerButton::Primary);
        tree.pointer_up_button(target, teksilo_core::event::PointerButton::Primary);
        assert_eq!(
            f.hover.get(),
            Some(key),
            "the cursor is still over the bar, so the readout stays"
        );
        let _ = tree.sync_accessibility();
        assert!(
            tree.announcements_since(0).is_empty(),
            "and a mouse click is not a pinning gesture, so nothing is spoken"
        );
    }

    #[test]
    fn a_mouse_leaving_the_plot_still_retires_its_readout() {
        let f = fixture(BarChart::new(sample_model()));
        let (target, _) = bar_center(&f.marks, 1);
        let mut tree = f.tree;
        tree.pointer_move(target);
        assert!(f.hover.get().is_some());
        tree.pointer_move(Point::new(1.0, 1.0));
        assert_eq!(f.hover.get(), None);
    }

    // ── kind-aware tolerance, end to end ──────────────────────────────────

    /// A point in the gap between two adjacent bars, closer to the second.
    fn point_in_gap(marks: &Rc<RefCell<Vec<MarkGeometry>>>) -> (Point, (SeriesId, usize)) {
        let marks = marks.borrow();
        let (MarkShape::Rect(a), MarkShape::Rect(b)) = (marks[0].shape, marks[1].shape) else {
            panic!("expected bars")
        };
        let gap = b.x - a.right();
        assert!(
            gap > 4.0,
            "fixture needs a gap wide enough that the nearer bar is unambiguous, got {gap}"
        );
        (
            Point::new(b.x - 2.0, b.y + b.height * 0.5),
            (marks[1].series_id, marks[1].point_idx),
        )
    }

    #[test]
    fn a_finger_just_past_a_bars_edge_still_reads_it_and_a_mouse_does_not() {
        let f = fixture(BarChart::new(sample_model()));
        let (target, key) = point_in_gap(&f.marks);
        let mut tree = f.tree;
        // The mouse: strictly inside or nothing, exactly as before.
        tree.pointer_move(target);
        assert_eq!(f.hover.get(), None);
        // The finger: the same point reaches the bar it nearly landed on.
        let c = tree.new_contact();
        tree.touch_down(c, target);
        assert_eq!(f.hover.get(), Some(key));
    }

    #[test]
    fn a_finger_just_past_a_bars_edge_selects_it_and_a_mouse_clears_instead() {
        use teksilo_core::event::PointerButton;
        use teksilo_data::SelectionMode;
        let sel = ChartSelection::new(SelectionMode::Single);
        let f = fixture(BarChart::new(sample_model()).selection(sel.clone()));
        let (target, key) = point_in_gap(&f.marks);
        let mut tree = f.tree;
        let c = tree.new_contact();
        tree.touch_down(c, target);
        tree.touch_up(c, target);
        assert!(
            sel.is_selected(key.0, key.1),
            "the tap and the readout share one hit test, tolerance included"
        );
        // A mouse in the same gap is a press on nothing, which clears.
        tree.pointer_down_button(target, PointerButton::Primary);
        tree.pointer_up_button(target, PointerButton::Primary);
        assert_eq!(sel.count(), 0);
    }

    // ── keyboard traversal ────────────────────────────────────────────────

    #[test]
    fn arrowing_a_focused_chart_moves_the_readout_and_announces_each_datum() {
        use teksilo_core::event::{Key, Modifiers};
        let f = fixture(BarChart::new(sample_model()));
        let mut tree = f.tree;
        let chart = f.chart;
        tree.focus(chart);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        let first = f.focus.get().expect("arrow right lands on the first datum");
        assert_eq!(
            f.hover.get(),
            Some(first),
            "the focused datum is the readout"
        );
        // Two syncs per message: the framework's announcer exposes a live
        // region carrying the text, then retracts it, because retracting is
        // what makes the next message a re-entry into the filtered tree (the
        // only thing AT-SPI announces at all). See `teksilo_core::announcer`.
        let _ = tree.sync_accessibility();
        let _ = tree.sync_accessibility();
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        let second = f.focus.get().expect("and moves on");
        assert_ne!(first, second);
        let _ = tree.sync_accessibility();
        let _ = tree.sync_accessibility();
        let spoken: Vec<String> = tree
            .announcements_since(0)
            .into_iter()
            .map(|a| a.text)
            .collect();
        assert_eq!(
            spoken.len(),
            2,
            "one utterance per traversal step: {spoken:?}"
        );
        let expected_second = {
            let marks = f.marks.borrow();
            hit::mark_index_of(&marks, second)
                .and_then(|i| marks.get(i))
                .map(hit::mark_description)
                .expect("the datum arrowed to")
        };
        assert_eq!(spoken[1], expected_second);
    }

    #[test]
    fn home_and_end_reach_the_first_and_last_datum() {
        use teksilo_core::event::{Key, Modifiers};
        let f = fixture(BarChart::new(sample_model()));
        let (_, first_key) = bar_center(&f.marks, 0);
        let last = f.marks.borrow().len() - 1;
        let (_, last_key) = bar_center(&f.marks, last);
        let mut tree = f.tree;
        tree.focus(f.chart);
        tree.press_key(Key::End, Modifiers::NONE);
        assert_eq!(f.focus.get(), Some(last_key));
        tree.press_key(Key::Home, Modifiers::NONE);
        assert_eq!(f.focus.get(), Some(first_key));
    }

    #[test]
    fn enter_commits_the_focused_datum_to_the_selection() {
        use teksilo_core::event::{Key, Modifiers};
        use teksilo_data::SelectionMode;
        let sel = ChartSelection::new(SelectionMode::Single);
        let f = fixture(BarChart::new(sample_model()).selection(sel.clone()));
        let mut tree = f.tree;
        tree.focus(f.chart);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        let key = f.focus.get().expect("focused datum");
        tree.press_key(Key::Enter, Modifiers::NONE);
        assert!(sel.is_selected(key.0, key.1));
    }

    #[test]
    fn losing_focus_forgets_the_focused_datum_and_its_readout() {
        use teksilo_core::event::{Key, Modifiers};
        let f = fixture(BarChart::new(sample_model()));
        let mut tree = f.tree;
        // A second chart, so focus has somewhere else to go.
        let elsewhere = tree.add(BarChart::new(sample_model()));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        tree.focus(f.chart);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert!(f.focus.get().is_some());
        tree.focus(elsewhere);
        assert_eq!(f.focus.get(), None, "no focus ring on an unfocused chart");
        assert_eq!(
            f.hover.get(),
            None,
            "and the keyboard's readout goes with it"
        );
    }

    #[test]
    fn a_chart_with_nothing_to_inspect_is_not_a_tab_stop() {
        let interactive = fixture(BarChart::new(sample_model()));
        assert!(
            interactive
                .tree
                .tab_stops_within(interactive.chart)
                .contains(&interactive.chart),
            "a chart with a readout is reachable by keyboard"
        );
        let inert = fixture(BarChart::new(sample_model()).hover_tooltip(false));
        assert!(
            !inert
                .tree
                .tab_stops_within(inert.chart)
                .contains(&inert.chart),
            "a chart with no readout and no selection adds no stop"
        );
    }

    // ── assistive-technology actions on a datum ───────────────────────────

    #[test]
    fn every_mark_advertises_click_and_scroll_into_view() {
        let f = fixture(BarChart::new(sample_model()));
        let snapshot = f.tree.accessibility_tree_snapshot();
        let mut marks_seen = 0;
        for (_, node) in snapshot.nodes.iter() {
            if node.role() == teksilo_core::accesskit::Role::GraphicsObject {
                marks_seen += 1;
                assert!(
                    node.supports_action(teksilo_core::accesskit::Action::Click),
                    "a datum a screen reader cannot click is a datum it cannot inspect"
                );
                assert!(node.supports_action(teksilo_core::accesskit::Action::ScrollIntoView));
            }
        }
        assert_eq!(marks_seen, 4, "one node per datum");
    }

    #[test]
    fn an_assistive_click_on_a_datum_selects_it_and_moves_the_readout() {
        use teksilo_data::SelectionMode;
        let sel = ChartSelection::new(SelectionMode::Single);
        let f = fixture(BarChart::new(sample_model()).selection(sel.clone()));
        let (_, key) = bar_center(&f.marks, 2);
        let node = teksilo_core::accessibility::synthetic_node_id(
            f.chart,
            hit::mark_element_id(key.0, key.1),
            teksilo_core::accessibility::SyntheticKind::ChartMark,
        );
        let mut tree = f.tree;
        // The synthetic-node → owner map is built by the accessibility walk,
        // which is how the platform adapter routes an action back to us.
        let _ = tree.sync_accessibility();
        let mut ops = teksilo_core::window::NoopWindowOps;
        let handled = tree.dispatch_access_action(
            node,
            teksilo_core::accesskit::Action::Click,
            None,
            &mut ops,
        );
        assert!(handled, "the chart owns its marks' actions");
        assert!(sel.is_selected(key.0, key.1));
        assert_eq!(f.hover.get(), Some(key));
        assert_eq!(f.focus.get(), Some(key));
    }

    #[test]
    fn the_focused_datum_is_published_as_the_active_descendant() {
        use teksilo_core::event::{Key, Modifiers};
        let f = fixture(BarChart::new(sample_model()));
        let mut tree = f.tree;
        let chart = f.chart;
        assert!(
            active_descendant_of(&mut tree, chart).is_none(),
            "nothing is focused yet"
        );
        tree.focus(chart);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        let key = f.focus.get().expect("focused datum");
        let expected = teksilo_core::accessibility::synthetic_node_id(
            chart,
            hit::mark_element_id(key.0, key.1),
            teksilo_core::accessibility::SyntheticKind::ChartMark,
        );
        assert_eq!(
            active_descendant_of(&mut tree, chart),
            Some(expected),
            "roving virtual focus: the marks have no arena nodes of their own"
        );
    }

    fn active_descendant_of(
        tree: &mut WidgetTree,
        chart: WidgetId,
    ) -> Option<teksilo_core::accesskit::NodeId> {
        let snapshot = tree.accessibility_tree_snapshot();
        let chart_node = teksilo_core::accessibility::widget_id_to_node_id(chart);
        snapshot
            .nodes
            .iter()
            .find(|(id, _)| *id == chart_node)
            .and_then(|(_, node)| node.active_descendant())
    }

    // ── the touch-action decision, pinned ─────────────────────────────────

    #[test]
    fn a_chart_does_not_forbid_a_finger_scrolling_the_page_over_it() {
        use teksilo_core::pointer::touch_action::TouchAction;
        let f = fixture(BarChart::new(sample_model()));
        assert_eq!(
            f.tree.touch_action_for(f.chart),
            TouchAction::AUTO,
            "a chart consumes no drag: it taps, it reads moves and it holds. \
             Declaring NONE would make a 400x200 tile a dead zone for a page \
             scroll, and would additionally resolve DragActivation::Auto to \
             Immediate — destroying the hold-then-scrub the readout depends on."
        );
    }

    #[test]
    fn an_embedded_interactive_legend_gets_the_band_its_rows_need() {
        // The third of the three sites that must agree — what the CHART
        // reserves. A band reserved at the painted line height would clip
        // rows the legend has grown to target size.
        use teksilo_tokens::{InputTokens, TargetDensity};
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.set_input_density(TargetDensity::Touch);
        let id = tree.add(
            BarChart::new(sample_model())
                .legend(true)
                .legend_interactive(true),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let legend = tree.children(id)[0];
        let row = tree.children(legend)[0];
        let want = InputTokens::for_density(TargetDensity::Touch).target_size;
        assert!(
            tree.bounds(legend).height >= want,
            "reserved band {} < {want}",
            tree.bounds(legend).height
        );
        assert!(
            tree.bounds(row).height >= want,
            "row {} < {want}",
            tree.bounds(row).height
        );
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

    /// A mark's AT node reports the mark's **own** rectangle, unscaled.
    ///
    /// AccessKit answers `AXBoundsForRange` / UIA `GetBoundingRectangles` and
    /// resolves its own hit tests from these numbers, so a magnifier tracks
    /// them and touch-explore routes a probe by them. Any transform applied on
    /// the way out — a scale "to make a mark easier to hit", a padding outset —
    /// is a lie an AT client cannot detect and cannot correct: it draws its
    /// focus rectangle somewhere the mark is not.
    ///
    /// The rule had no enforcement in this crate at all. Doubling the emitted
    /// rect in `hit::emit_mark_node` left every chart test passing, because
    /// nothing compared an emitted rect against the geometry it came from.
    /// This does, for a real laid-out chart's real first mark.
    #[test]
    fn a_mark_emits_its_own_rectangle_and_not_a_scaled_one() {
        use teksilo_core::accessibility::AccessNodeBuilder;

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let chart = BarChart::new(sample_model());
        let marks_handle = chart.marks.clone();
        let id = tree.add(chart);
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let _ = tree.render();

        let marks = marks_handle.borrow();
        let mark = marks.first().expect("a laid-out bar chart has marks");
        let expected = mark.shape.bounding_rect();

        let mut builder = AccessNodeBuilder::for_widget(id);
        crate::hit::emit_mark_node(&mut builder, mark);
        let (_self_id, _self_node, children, _local_bounds) = builder.build(id);
        let (_child_id, node) = children
            .first()
            .expect("emit_mark_node pushes exactly one synthetic child");
        let bounds = node
            .bounds()
            .expect("a mark node without bounds is invisible to every AT hit test");

        assert_eq!(bounds.x0, expected.x as f64, "left edge");
        assert_eq!(bounds.y0, expected.y as f64, "top edge");
        assert_eq!(
            bounds.x1,
            (expected.x + expected.width) as f64,
            "right edge",
        );
        assert_eq!(
            bounds.y1,
            (expected.y + expected.height) as f64,
            "bottom edge",
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
