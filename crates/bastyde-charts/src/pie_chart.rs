//! PieChart — circular value slices.
//!
//! Pie and donut share the same widget. `inner_radius_ratio == 0.0` is a
//! pie; `> 0.0` is a donut. The optional center widget slot is only used
//! when `inner_radius_ratio > 0.0` (the slot is silently ignored for pies).
//!
//! Slot integration follows the existing `Option<PendingChild>` pattern
//! used by [`Card`](https://docs.rs/bastyde-widgets) /
//! [`DialogContent`](https://docs.rs/bastyde-widgets) / `GroupBox`: two
//! fields (`pending_center` + `center_id`), two builders
//! (`.center(impl Widget)` / `.center_id(WidgetId)`), and `build()`
//! resolves the pending child via `ctx.add_boxed`.

use bastyde_i18n::lit;
use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::{Canvas, Path, Point, Rect, Size, SizeProposal, TextBackend};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::event::{EventResponse, WidgetEvent};
use bastyde_core::signal::{Prop, Signal};
use bastyde_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{CornerRadius, TextRole, TextStyleRole};

use crate::layout::{CarveParams, LegendPosition, carve_plot_area};
use crate::legend::{LegendOrientation, orientation_for_position, paint_embedded_legend};
use crate::palette::ChartPalette;
use crate::series::{ChartDatum, ChartSeries};
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

#[derive(Debug, Clone, Copy)]
struct HoveredSlice {
    slice_idx: usize,
}

#[derive(Debug, Clone)]
struct SliceHit {
    /// Cumulative start angle in radians (clockwise from start_angle).
    start_rad: f32,
    /// Sweep in radians.
    sweep_rad: f32,
    label: String,
    value: f32,
    percent: f32,
}

pub struct PieChart<T: Clone + 'static> {
    data: Prop<Vec<ChartDatum<T>>>,
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

    // hover plumbing
    hover: Signal<Option<HoveredSlice>>,
    hit_index: Rc<RefCell<Vec<SliceHit>>>,
    /// Center of the disc + outer radius, in window space; used by the
    /// pointer hit-test.
    disc: Rc<RefCell<(Point, f32, f32)>>, // (center, outer_radius, inner_radius)
}

impl<T: Clone + std::fmt::Display + 'static> PieChart<T> {
    pub fn new(data: impl Into<Prop<Vec<ChartDatum<T>>>>) -> Self {
        Self {
            data: data.into(),
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
            hover: Signal::new(None),
            hit_index: Rc::new(RefCell::new(Vec::new())),
            disc: Rc::new(RefCell::new((Point::ZERO, 0.0, 0.0))),
        }
    }

    /// Adapter: take a single `ChartSeries<T>` and use its data points as
    /// pie slices (the series's name and color are ignored for the pie).
    pub fn from_series(series: ChartSeries<T>) -> Self {
        Self::new(series.data)
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
        let registry = ctx.binding_registry();
        // Data swap → relayout (slice angles change, labels change).
        self.data
            .register_if_bound(id, registry, BindingLevel::Relayout);
        self.palette
            .register_if_bound(id, registry, BindingLevel::RepaintOnly);
        self.hover.bind_to(id, registry, BindingLevel::RepaintOnly);

        // Resolve the center slot via ctx.add_boxed.
        if let Some(c) = self.pending_center.take() {
            self.center_id = Some(match c {
                PendingChild::Id(cid) => cid,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }

        // Hover hit-test handler.
        if self.show_hover_tooltip {
            let disc = self.disc.clone();
            let hits = self.hit_index.clone();
            let hover = self.hover.clone();
            let clockwise = self.clockwise;
            let start_angle_rad = self.start_angle_degrees.to_radians();
            let handlers = HandlerSet::new().on_pointer_event(move |event, _ctx| match event {
                WidgetEvent::PointerMove { position } => {
                    let (center, outer, inner) = *disc.borrow();
                    if outer <= 0.0 {
                        return EventResponse::Ignored;
                    }
                    let dx = position.x - center.x;
                    let dy = position.y - center.y;
                    let dist = (dx * dx + dy * dy).sqrt();
                    if dist < inner || dist > outer {
                        if hover.get().is_some() {
                            hover.set(None);
                        }
                        return EventResponse::Ignored;
                    }
                    // Pointer angle in screen-space radians, measured
                    // from +x axis (3 o'clock = 0). Translate into the
                    // chart's logical angle space (rooted at
                    // `start_angle_rad`), then flip for non-clockwise.
                    // SliceHit::start_rad is stored in this same logical
                    // space, so the comparison is direct.
                    let raw = dy.atan2(dx).rem_euclid(std::f32::consts::TAU);
                    let logical = (raw - start_angle_rad).rem_euclid(std::f32::consts::TAU);
                    let test_angle = if clockwise {
                        logical
                    } else {
                        (std::f32::consts::TAU - logical) % std::f32::consts::TAU
                    };
                    let hits = hits.borrow();
                    if hits.is_empty() {
                        return EventResponse::Ignored;
                    }
                    let mut found = None;
                    for (i, h) in hits.iter().enumerate() {
                        if angle_in_sweep(test_angle, h.start_rad, h.sweep_rad) {
                            found = Some(i);
                            break;
                        }
                    }
                    match found {
                        Some(idx) => {
                            let prev = hover.get().map(|h| h.slice_idx);
                            if prev != Some(idx) {
                                hover.set(Some(HoveredSlice { slice_idx: idx }));
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
            ctx.apply_self_handlers(handlers);
        }

        // The center widget is the only child we expose to the tree.
        self.center_id.into_iter().collect()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        Size::new(
            proposal.width.unwrap_or(320.0),
            proposal.height.unwrap_or(220.0),
        )
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        // Place the center widget (if any) into the inscribed square at
        // the donut's inner radius. For pies (inner_radius_ratio == 0)
        // the side is 0 so the slot occupies no space.
        if children.is_empty() {
            return;
        }
        let label_style = TextStyleRole::Tiny.resolve(&ctx.theme.typography);
        let legend_size = self.compute_legend_size(ctx.text_backend, &label_style);
        // Carve off the legend band first so the disc placed here matches
        // the disc rendered in paint (otherwise the center widget drifts
        // off-center when a legend is shown).
        let plot_rect = self.compute_plot_rect(bounds, legend_size);
        let (center, _outer, inner) = self.compute_disc_geometry(plot_rect);
        let side = (inner * std::f32::consts::FRAC_1_SQRT_2 * 2.0).max(0.0);
        for child in children.iter_mut() {
            child.origin = Point::new(center.x - side * 0.5, center.y - side * 0.5);
            child.size = Size::new(side, side);
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let theme = ctx.theme;
        let enabled = ctx.effective_enabled;

        let data = self.data.get();
        if data.is_empty() {
            return;
        }
        let total: f32 = data.iter().map(|d| d.value.max(0.0)).sum();
        if total <= 0.0 {
            return;
        }

        let label_style = TextStyleRole::Tiny.resolve(&theme.typography);
        let legend_orientation = orientation_for_position(self.legend_position);

        let legend_size = self.compute_legend_size(canvas.text_backend(), &label_style);
        let plot = self.compute_plot_rect(bounds, legend_size);
        if plot.width <= 0.0 || plot.height <= 0.0 {
            return;
        }
        let legend_band = legend_band_rect(bounds, self.legend_position, legend_size);

        // Disc geometry.
        let (center, outer_radius, inner_radius) = self.compute_disc_geometry(plot);
        if outer_radius <= 0.0 {
            return;
        }
        *self.disc.borrow_mut() = (center, outer_radius, inner_radius);

        // Paint slices.
        let palette = self.palette.get();
        let start_rad = self.start_angle_degrees.to_radians();
        let mut accum = 0.0_f32;
        let mut new_hits: Vec<SliceHit> = Vec::new();
        let half_gap = self.slice_gap_degrees.to_radians() * 0.5;

        for (i, datum) in data.iter().enumerate() {
            let v = datum.value.max(0.0);
            let sweep_full = v / total * std::f32::consts::TAU;
            // Clip the gap so we never produce negative sweeps.
            let usable_sweep = (sweep_full - half_gap * 2.0).max(0.0);
            let slice_start = accum + half_gap;
            let slice_end = accum + sweep_full - half_gap;

            // Color resolution.
            let color = self
                .explicit_colors
                .get(i)
                .and_then(|c| c.clone())
                .map(|c| c.resolve(theme, enabled))
                .unwrap_or_else(|| palette.color_for(i, theme));

            // Build the wedge / ring-segment path. Angles are in
            // **clockwise** orientation when `self.clockwise = true`. We
            // render with clockwise sweep by default (matches start at 12
            // o'clock + clockwise convention).
            let path = build_slice_path(
                center,
                outer_radius,
                inner_radius,
                start_rad + slice_start,
                usable_sweep,
                self.clockwise,
            );
            canvas.fill_path(&path, color);

            new_hits.push(SliceHit {
                start_rad: slice_start,
                sweep_rad: usable_sweep,
                label: format!("{}", datum.category),
                value: v,
                percent: v / total * 100.0,
            });

            // Slice labels.
            self.draw_slice_label(
                canvas,
                theme,
                center,
                outer_radius,
                start_rad + (slice_start + slice_end) * 0.5,
                self.clockwise,
                v,
                v / total * 100.0,
                &format!("{}", datum.category),
                &label_style,
            );

            accum += sweep_full;
        }
        *self.hit_index.borrow_mut() = new_hits;

        // Embedded legend — synthesize a series list (one entry per slice)
        // so the embedded-legend painter works uniformly across charts.
        if self.show_legend && legend_band.width > 0.0 && legend_band.height > 0.0 {
            let pseudo_series: Vec<ChartSeries<String>> = data
                .iter()
                .map(|d| ChartSeries::new(format!("{}", d.category)))
                .collect();
            paint_embedded_legend(
                canvas,
                legend_band,
                &pseudo_series,
                &palette,
                legend_orientation,
                theme,
                enabled,
            );
        }

        // Hover marker + tooltip.
        if self.show_hover_tooltip
            && let Some(hovered) = self.hover.get()
            && let Some(hit) = self.hit_index.borrow().get(hovered.slice_idx)
        {
            self.draw_hover(canvas, theme, plot, center, outer_radius, hit, &label_style);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::GraphicsDocument);
        let data = self.data.get();
        builder.set_name(format!("Pie chart: {} slices", data.len()));
    }

    fn children(&self) -> Vec<WidgetId> {
        self.center_id.into_iter().collect()
    }
}

impl<T: Clone + std::fmt::Display + 'static> PieChart<T> {
    /// Compute the size the embedded legend would reserve along its main
    /// axis (height for Top/Bottom, width for Leading/Trailing). Returns
    /// 0 when the legend is disabled. Vertical orientation measures the
    /// widest slice label via `backend` so the reservation matches what
    /// will later be drawn.
    fn compute_legend_size(
        &self,
        backend: Option<&Rc<RefCell<dyn TextBackend>>>,
        label_style: &bastyde_tokens::TextStyle,
    ) -> f32 {
        use crate::style as cs;
        if !self.show_legend {
            return 0.0;
        }
        let orientation = orientation_for_position(self.legend_position);
        match orientation {
            LegendOrientation::Horizontal => cs::LEGEND_SWATCH_SIZE.max(label_style.size * 1.2),
            LegendOrientation::Vertical => {
                let data = self.data.get();
                let max_w = data
                    .iter()
                    .map(|d| {
                        let name = format!("{}", d.category);
                        crate::text::measure_text_width_via(backend, &name, label_style)
                    })
                    .fold(0.0_f32, f32::max);
                cs::LEGEND_SWATCH_SIZE + 4.0 + max_w
            }
        }
    }

    /// Carve the legend band off `bounds` and return the inner plot rect
    /// where the disc lives. Both `place_children` and `paint` go through
    /// this so the donut's center-slot placement matches the rendered
    /// disc when a legend is shown.
    fn compute_plot_rect(&self, bounds: Rect, legend_size: f32) -> Rect {
        let no_axis = crate::axis::AxisConfig::new()
            .show_labels(false)
            .show_axis_line(false);
        let area = carve_plot_area(&CarveParams {
            bounds,
            axis_x: &no_axis,
            axis_y: &no_axis,
            y_label_max_width: 0.0,
            x_label_height: 0.0,
            axis_title_line_height: 0.0,
            legend_size,
            legend_position: if self.show_legend {
                Some(self.legend_position)
            } else {
                None
            },
        });
        area.plot
    }

    /// Compute (center, outer_radius, inner_radius) given a bounds rect.
    fn compute_disc_geometry(&self, bounds: Rect) -> (Point, f32, f32) {
        use crate::style as cs;
        let pad = cs::PIE_PADDING;
        let usable_w = (bounds.width - pad * 2.0).max(0.0);
        let usable_h = (bounds.height - pad * 2.0).max(0.0);
        let diameter = usable_w.min(usable_h);
        if diameter <= 0.0 {
            return (
                Point::new(
                    bounds.x + bounds.width * 0.5,
                    bounds.y + bounds.height * 0.5,
                ),
                0.0,
                0.0,
            );
        }
        let outer = diameter * 0.5;
        let center = Point::new(
            bounds.x + bounds.width * 0.5,
            bounds.y + bounds.height * 0.5,
        );
        let inner = if self.inner_radius_ratio > 0.0 {
            outer * self.inner_radius_ratio
        } else {
            0.0
        };
        (center, outer, inner)
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_slice_label(
        &self,
        canvas: &mut Canvas,
        theme: &bastyde_core::Theme,
        center: Point,
        outer: f32,
        bisector_rad: f32,
        clockwise: bool,
        _value: f32,
        percent: f32,
        category: &str,
        label_style: &bastyde_tokens::TextStyle,
    ) {
        use crate::style as cs;
        let label_color = TextRole::Primary.resolve(&theme.colors);
        let min_deg = cs::PIE_MIN_SLICE_LABEL_DEGREES;
        // Compute the slice's actual sweep degrees by looking it up in
        // the live hits. (Hits include the gap-adjusted sweep.)
        let label_text = if self.show_percentages {
            format!("{} ({:.0}%)", category, percent)
        } else {
            category.to_string()
        };
        let approx_w = measure_text_width(canvas, &label_text, label_style);
        let height = label_style.size * 1.2;

        // Convert to a screen direction.
        let (cos, sin) = bisector_direction(bisector_rad, clockwise);

        // For PR 5 we keep label placement simple: skip if mode is None,
        // otherwise place inside or outside per mode.
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
                    &label_text,
                    Rect::new(lx, ly, approx_w, height),
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
                        &label_text,
                        Rect::new(lx, ly, approx_w, height),
                        label_style,
                        label_color,
                    );
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_hover(
        &self,
        canvas: &mut Canvas,
        theme: &bastyde_core::Theme,
        plot: Rect,
        center: Point,
        outer: f32,
        hit: &SliceHit,
        label_style: &bastyde_tokens::TextStyle,
    ) {
        use crate::style as cs;
        let label = format!(
            "{}: {} ({:.1}%)",
            hit.label,
            self.axis_y_format_dummy(hit.value),
            hit.percent
        );
        let text_w = measure_text_width(canvas, &label, label_style);
        let approx_w = text_w + cs::TOOLTIP_PADDING * 2.0;
        let height = label_style.size * 1.4 + cs::TOOLTIP_PADDING;

        // Anchor at the bisector midpoint between inner and outer radius.
        let bisector_rad =
            self.start_angle_degrees.to_radians() + hit.start_rad + hit.sweep_rad * 0.5;
        let (cos, sin) = bisector_direction(bisector_rad, self.clockwise);
        let r_anchor = outer + 12.0;
        let mut tx = center.x + r_anchor * cos - approx_w * 0.5;
        let mut ty = center.y + r_anchor * sin - height * 0.5;
        if tx < plot.x {
            tx = plot.x;
        }
        if tx + approx_w > plot.right() {
            tx = plot.right() - approx_w;
        }
        if ty < plot.y {
            ty = plot.y;
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
        canvas.draw_text(&label, label_rect, label_style, theme.colors.tooltip_text);
    }

    /// Pie has no formal y-axis but we want consistent number formatting
    /// in tooltips. Use a default formatter.
    fn axis_y_format_dummy(&self, v: f32) -> String {
        if v.fract() == 0.0 {
            format!("{:.0}", v)
        } else {
            format!("{:.2}", v)
        }
    }
}

/// Re-derive just the legend band rect from `bounds` for a given
/// position + size. Mirrors the carve done inside `carve_plot_area`.
fn legend_band_rect(bounds: Rect, pos: LegendPosition, size: f32) -> Rect {
    if size <= 0.0 {
        return Rect::ZERO;
    }
    match pos {
        LegendPosition::Top => Rect::new(bounds.x, bounds.y, bounds.width, size),
        LegendPosition::Bottom => Rect::new(bounds.x, bounds.bottom() - size, bounds.width, size),
        LegendPosition::Leading => Rect::new(bounds.x, bounds.y, size, bounds.height),
        LegendPosition::Trailing => Rect::new(bounds.right() - size, bounds.y, size, bounds.height),
    }
}

/// Build a path for a single slice. For pie (`inner == 0`) this is a
/// pie wedge; for donut (`inner > 0`) it's a hollow ring segment.
fn build_slice_path(
    center: Point,
    outer: f32,
    inner: f32,
    start_rad: f32,
    sweep_rad: f32,
    clockwise: bool,
) -> Path {
    // The Path::arc_to API takes degrees; convert.
    let start_deg = start_rad.to_degrees();
    // Sweep direction: positive = clockwise in screen-space (y-down).
    let sweep_deg = if clockwise {
        sweep_rad.to_degrees()
    } else {
        -sweep_rad.to_degrees()
    };

    // Use direction-aware sin/cos for endpoint placement.
    let (sx_o, sy_o) = endpoint(center, outer, start_rad, clockwise);
    let (ex_o, ey_o) = endpoint(center, outer, start_rad + sweep_rad, clockwise);

    let outer_rect = Rect::new(center.x - outer, center.y - outer, outer * 2.0, outer * 2.0);

    let mut path = Path::new();
    if inner <= 0.0 {
        // Pie wedge: center → outer arc start → arc → close back to center.
        path.move_to(center);
        path.line_to(Point::new(sx_o, sy_o));
        path.arc_to(outer_rect, start_deg, sweep_deg);
        let _ = (ex_o, ey_o);
        path.close();
    } else {
        // Donut wedge: outer arc → line to inner arc end → reversed inner
        // arc → close back.
        let (sx_i, sy_i) = endpoint(center, inner, start_rad, clockwise);
        let (ex_i, ey_i) = endpoint(center, inner, start_rad + sweep_rad, clockwise);
        let inner_rect = Rect::new(center.x - inner, center.y - inner, inner * 2.0, inner * 2.0);
        path.move_to(Point::new(sx_o, sy_o));
        path.arc_to(outer_rect, start_deg, sweep_deg);
        let _ = (ex_o, ey_o);
        path.line_to(Point::new(ex_i, ey_i));
        path.arc_to(inner_rect, start_deg + sweep_deg, -sweep_deg);
        let _ = (sx_i, sy_i);
        path.close();
    }
    path
}

/// Compute the endpoint of a radial line at angle `rad` and distance
/// `radius` from `center`. Direction respects the `clockwise` flag.
fn endpoint(center: Point, radius: f32, rad: f32, clockwise: bool) -> (f32, f32) {
    let (cos, sin) = bisector_direction(rad, clockwise);
    (center.x + radius * cos, center.y + radius * sin)
}

/// Convert a "logical" angle to a (cos, sin) screen direction. We treat
/// `start_angle_degrees = -90` as 12 o'clock and let `clockwise` flip
/// the direction. In screen coordinates y grows downward.
fn bisector_direction(rad: f32, clockwise: bool) -> (f32, f32) {
    if clockwise {
        (rad.cos(), rad.sin())
    } else {
        // Counter-clockwise: mirror y.
        (rad.cos(), -rad.sin())
    }
}

/// Whether `angle` (in 0..2π) lies inside `[start, start + sweep]`,
/// also normalized to 0..2π.
fn angle_in_sweep(angle: f32, start: f32, sweep: f32) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_canvas::SizeProposal;
    use bastyde_core::widget_tree::WidgetTree;

    fn three_slices() -> Vec<ChartDatum<String>> {
        vec![
            ChartDatum::new("A".into(), 30.0),
            ChartDatum::new("B".into(), 50.0),
            ChartDatum::new("C".into(), 20.0),
        ]
    }

    #[test]
    fn three_slices_three_paths() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(PieChart::new(three_slices()));
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
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(PieChart::new(three_slices()).donut(0.5));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let frame = tree.render();
        // Still 3 paths (one per slice), but each path now has more
        // commands (outer arc + line + inner arc + close).
        assert_eq!(frame.paths.len(), 3);
        let cmds_pie = {
            let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
            tree.add(PieChart::new(three_slices()));
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
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(PieChart::<String>::new(Vec::<ChartDatum<String>>::new()));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = tree.render();
    }

    #[test]
    fn all_zero_values_do_not_panic() {
        let data: Vec<ChartDatum<String>> = vec![
            ChartDatum::new("A".into(), 0.0),
            ChartDatum::new("B".into(), 0.0),
        ];
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(PieChart::new(data));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = tree.render();
    }

    #[test]
    fn accessibility_role_is_graphics_document() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(PieChart::new(three_slices()));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), bastyde_core::accesskit::Role::GraphicsDocument);
    }

    #[test]
    fn pie_ignores_center_slot() {
        // Pie (inner_radius_ratio = 0). We add a child via .center(...)
        // and verify the chart does not panic and the center child has
        // zero placement size when rendered.
        use bastyde_widgets::TextWidget;
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let pie = PieChart::new(three_slices()).center(TextWidget::new(lit!("$100")));
        let id = tree.add(pie);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        // child exists but its bounds should be zero (inner radius = 0).
        let kids = tree.children(id);
        assert!(!kids.is_empty(), "center widget child registered");
        let child_bounds = tree.bounds(kids[0]);
        assert_eq!(child_bounds.width, 0.0);
        assert_eq!(child_bounds.height, 0.0);
    }

    #[test]
    fn donut_center_slot_has_inscribed_size() {
        use bastyde_widgets::TextWidget;
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let donut = PieChart::new(three_slices())
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
        // Regression: when a legend was visible, place_children carved
        // against the full bounds while paint carved against the
        // legend-trimmed plot rect — the center slot drifted off-center.
        // Now both go through compute_plot_rect, so the center widget
        // sits inside the actually-rendered disc.
        use bastyde_widgets::TextWidget;
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            PieChart::new(three_slices())
                .donut(0.6)
                .legend(true)
                .legend_position(LegendPosition::Bottom)
                .center(TextWidget::new(lit!("100"))),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let kids = tree.children(id);
        let child = tree.bounds(kids[0]);
        // Vertical center of the child should be in the upper half of
        // the chart (the legend takes the bottom strip, pulling the disc
        // up). Loose check — just verify it's not at the geometric mid.
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
    fn pie_hit_test_uses_logical_angle_space() {
        // Regression: the on_pointer_event handler used to compare the
        // raw screen-space angle against SliceHit::start_rad without
        // subtracting `start_angle_degrees` first. With the default
        // `start_angle_degrees = -90` (12 o'clock), this off-by-90°
        // misalignment meant a pointer over the first slice could miss.
        // We can't easily synthesize pointer events in a unit test, but
        // we can lock the logical-space conversion math.
        use std::f32::consts::TAU;
        let start_angle_rad = (-90.0_f32).to_radians();
        // Pointer at 12 o'clock in screen space: dx ≈ 0, dy ≈ -1 → angle = -π/2.
        let raw = (-1.0_f32).atan2(0.0).rem_euclid(TAU);
        let logical = (raw - start_angle_rad).rem_euclid(TAU);
        // After subtracting the start offset, the pointer at 12 o'clock
        // sits at logical angle ≈ 0 (the start of the first slice).
        assert!(
            logical < 0.05 || (TAU - logical) < 0.05,
            "pointer at 12 o'clock should map to logical 0 (got {})",
            logical
        );
    }

    #[test]
    fn slice_color_overrides_palette() {
        use bastyde_tokens::Color;
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            PieChart::new(three_slices())
                .slice_color(0, Color::RED)
                .slice_color(2, Color::BLUE),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let frame = tree.render();
        assert_eq!(frame.paths[0].color, Color::RED.to_array());
        assert_eq!(frame.paths[2].color, Color::BLUE.to_array());
        // Slice 1 falls back to palette → not RED or BLUE.
        assert_ne!(frame.paths[1].color, Color::RED.to_array());
        assert_ne!(frame.paths[1].color, Color::BLUE.to_array());
    }
}
