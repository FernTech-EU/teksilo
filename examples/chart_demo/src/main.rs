// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Chart Demo: BarChart, LineChart, PieChart with donut + center slot,
//! migrated to the `ChartModel<T>` API.
//!
//! Demonstrates:
//! - `ChartModel<T>` construction (`from_series_vec` / `from_points`) and
//!   live mutation (`replace_series_data`) instead of a whole-vec `Signal`
//!   swap.
//! - `.legend_interactive(true)` — clicking a legend swatch toggles that
//!   series' visibility.
//! - A `ChartStyle` override (`GradientChartStyle`) producing gradient bar
//!   fills, gradient area fills, a radial donut gradient, and dashed
//!   gridlines — toggled live via a second `SegmentedControl`.
//! - `ChartWindow<T>` — a "last N points" streaming projection over an
//!   unbounded `ChartModel<T>`, driving a live-updating strip chart.
//! - `ChartSelection` — real click-to-select on all three chart kinds via
//!   `.selection(ChartSelection)`: clicking a bar/point/slice selects it
//!   (accent outline highlight), clicking empty space clears it. The
//!   donut's center slot reacts to the pie's selection live; the Bar and
//!   Line panels share one `ChartSelection` over the same series model, so
//!   switching between them keeps the highlighted point in sync.
//!
//! Run with: `cargo run -p chart-demo`

use bastyde::core::styles::{
    BorderPosition, BorderRecipe, BorderStyle, ChartFillContext, ChartStyle, FillRecipe,
    GradientStop, RecipeColor,
};
use bastyde::core::{FrameTickSubscription, WidgetPlacement};
use bastyde::data::SelectionMode;
use bastyde::prelude::*;
use bastyde::tokens::{Color, HAlignment};
use bastyde::widgets::{
    Button, ButtonVariant, Center, Expand, GroupHeader, HStack, Padding, ScrollArea,
    SegmentedControl, Spacer, Switcher, TextWidget, Toolbar, VStack,
};
use bastyde_charts::{
    AxisConfig, BarChart, BarGrouping, ChartAggregate, ChartAggregateFn, ChartDatum, ChartModel,
    ChartSelection, ChartSeries, ChartWindow, LegendPosition, LineChart, PieChart, PieLabelMode,
    SeriesId,
};

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

const SERIES_NAMES: [&str; 3] = ["Revenue", "Cost", "Profit"];
const QUARTERS: [&str; 4] = ["Q1", "Q2", "Q3", "Q4"];
const MAX_SERIES: usize = 5;
const STRIP_WINDOW: usize = 24;
const STRIP_PERIOD_MS: u64 = 600;
/// Selectable bucket widths for the live `ChartAggregate` rollup.
const AGG_BUCKETS: [usize; 3] = [2, 4, 8];

fn agg_fn_for(idx: usize) -> ChartAggregateFn {
    match idx {
        1 => ChartAggregateFn::Max,
        2 => ChartAggregateFn::Min,
        _ => ChartAggregateFn::Mean,
    }
}

/// Copy a projection's current points (window or aggregate — both keyed by
/// the *source* series id) into a render-bound display `ChartModel`. Chart
/// widgets consume a `ChartModel<T>`, not a projection, so each tick the
/// projection's computed tail is materialized here.
fn materialize(
    src: SeriesId,
    count: usize,
    read: impl Fn(usize) -> Option<ChartDatum<u32>>,
    display: &ChartModel<u32>,
    dst: SeriesId,
) {
    let _ = src;
    let points: Vec<ChartDatum<u32>> = (0..count).filter_map(read).collect();
    display.replace_series_data(dst, points);
}

fn dark_mode_toolbar() -> impl Widget {
    Toolbar::new().child(
        HStack::new()
            .child(Spacer::new())
            .child(bastyde::widgets::ThemeSwitcher::new()),
    )
}

// ─── Data generation (seeded pseudo-random, matches the pre-refactor demo) ─

fn quarter_points(seed: u32, si: usize) -> Vec<ChartDatum<String>> {
    QUARTERS
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let v = ((seed
                .wrapping_mul(31)
                .wrapping_add(si as u32 * 53)
                .wrapping_add(i as u32 * 17))
                % 60) as f32
                + 10.0;
            ChartDatum::new(label.to_string(), v)
        })
        .collect()
}

fn make_series_model(seed: u32) -> ChartModel<String> {
    let series = SERIES_NAMES
        .iter()
        .enumerate()
        .map(|(si, name)| ChartSeries::<String>::new(*name).data(quarter_points(seed, si)))
        .collect();
    ChartModel::from_series_vec(series)
}

fn pie_points(seed: u32) -> Vec<ChartDatum<String>> {
    let labels = ["Storage", "Apps", "System", "Cache", "Free"];
    labels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let v = ((seed.wrapping_mul(13).wrapping_add(i as u32 * 41)) % 50) as f32 + 5.0;
            ChartDatum::new(l.to_string(), v)
        })
        .collect()
}

// ─── ChartStyle override: gradient fills + dashed gridlines ───────────────

/// Mix `c` toward white by `k` (0 = unchanged, 1 = white), keeping it fully
/// OPAQUE. Gradients that fade via *alpha* let the page background bleed
/// through, which desaturates the (deliberately vivid, colorblind-safe)
/// Okabe-Ito palette into pastel mush and destroys each series' identity.
/// Fading toward a lighter *opaque* tint instead keeps the hue and the
/// series recognisable while still reading as a gradient.
fn tint(c: Color, k: f32) -> Color {
    Color::new(
        c.r() + (1.0 - c.r()) * k,
        c.g() + (1.0 - c.g()) * k,
        c.b() + (1.0 - c.b()) * k,
        c.a(),
    )
}

/// A `ChartStyle` demonstrating every gradient hook: a vertical bar
/// gradient, a top-to-bottom area gradient that fades toward the
/// baseline, a radial donut gradient (continuous across wedges — see
/// `PieChart`'s `project_gradient_to_wedge_local`), and dashed gridlines.
///
/// Bars and slices fade between two *opaque* tints of the series color (see
/// [`tint`]) rather than fading out via alpha, so the palette stays vivid.
/// Only the line's area fill fades to transparent — that IS the idiom for an
/// area chart (the plot must show through), so it stays alpha-based.
#[derive(Debug, Default, Clone, Copy)]
struct GradientChartStyle;

impl ChartStyle for GradientChartStyle {
    fn bar_fill(&self, cfg: &ChartFillContext) -> FillRecipe {
        FillRecipe::LinearGradient {
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: RecipeColor::Static(tint(cfg.resolved_color, 0.45)),
                },
                GradientStop {
                    offset: 1.0,
                    color: RecipeColor::Static(cfg.resolved_color),
                },
            ],
            angle_deg: 0.0, // top (lighter tint) -> bottom (full series color)
        }
    }

    fn area_fill(&self, cfg: &ChartFillContext, opacity: f32) -> FillRecipe {
        // The one place an alpha fade is right: an area fill must fade out so
        // the gridlines/other series show through. Keep the top edge strong
        // enough to read, though.
        FillRecipe::LinearGradient {
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: RecipeColor::Static(
                        cfg.resolved_color.with_alpha((opacity * 3.0).min(0.55)),
                    ),
                },
                GradientStop {
                    offset: 1.0,
                    color: RecipeColor::Static(cfg.resolved_color.with_alpha(0.0)),
                },
            ],
            angle_deg: 0.0, // top -> bottom, fading toward the baseline
        }
    }

    fn donut_fill(&self, cfg: &ChartFillContext) -> FillRecipe {
        // The radial field is centred on the DISC centre, which for a donut
        // lies inside the (invisible) hole: with a plain 0.0->1.0 ramp the
        // whole visible ring would sample only the tail of the gradient and
        // never show the true series color. Hold the full color out to the
        // inner radius (the donut ratio, 0.55) and only lighten across the
        // ring itself.
        FillRecipe::RadialGradient {
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: RecipeColor::Static(cfg.resolved_color),
                },
                GradientStop {
                    offset: 0.55, // inner radius — ring starts at FULL color
                    color: RecipeColor::Static(cfg.resolved_color),
                },
                GradientStop {
                    offset: 1.0, // outer rim — lighter, still opaque
                    color: RecipeColor::Static(tint(cfg.resolved_color, 0.40)),
                },
            ],
            center: (0.5, 0.5),
            radius: 0.5,
        }
    }

    fn gridline(&self, theme: &Theme) -> BorderRecipe {
        BorderRecipe {
            width: 1.0,
            color: RecipeColor::Static(BorderRole::Default.resolve(&theme.colors).with_alpha(0.5)),
            style: BorderStyle::Dashed {
                dash: 4.0,
                gap: 3.0,
            },
            position: BorderPosition::Center,
            sides: None,
        }
    }
}

// ─── Chart panels: a Switcher between the default style and the gradient
//     style, driven by the shared `theme_mode` signal. ────────────────────

fn bar_panel(
    model: ChartModel<String>,
    theme_mode: Signal<usize>,
    selection: ChartSelection,
) -> impl Widget {
    let default_chart = BarChart::new(model.clone())
        .grouping(BarGrouping::Grouped)
        .grid(true)
        .legend(true)
        .legend_interactive(true)
        .legend_position(LegendPosition::Bottom)
        .axis_y(
            AxisConfig::new()
                .label("USD (k)")
                .formatter(|v| format!("{:.0}", v)),
        )
        .axis_x(AxisConfig::new().label("Quarter"))
        .bar_corner_radius(2.0)
        .selection(selection.clone());

    let gradient_chart = BarChart::new(model)
        .grouping(BarGrouping::Grouped)
        .grid(true)
        .legend(true)
        .legend_interactive(true)
        .legend_position(LegendPosition::Bottom)
        .axis_y(
            AxisConfig::new()
                .label("USD (k)")
                .formatter(|v| format!("{:.0}", v)),
        )
        .axis_x(AxisConfig::new().label("Quarter"))
        .bar_corner_radius(2.0)
        .style(GradientChartStyle)
        .selection(selection);

    Switcher::new(theme_mode)
        .child(default_chart)
        .child(gradient_chart)
}

fn line_panel(
    model: ChartModel<String>,
    theme_mode: Signal<usize>,
    selection: ChartSelection,
) -> impl Widget {
    let default_chart = LineChart::new(model.clone())
        .grid(true)
        .points(true)
        .area_fill(true)
        .legend(true)
        .legend_interactive(true)
        .legend_position(LegendPosition::Bottom)
        .axis_y(
            AxisConfig::new()
                .label("USD (k)")
                .formatter(|v| format!("{:.0}", v)),
        )
        .axis_x(AxisConfig::new().label("Quarter"))
        .selection(selection.clone());

    let gradient_chart = LineChart::new(model)
        .grid(true)
        .points(true)
        .area_fill(true)
        .legend(true)
        .legend_interactive(true)
        .legend_position(LegendPosition::Bottom)
        .axis_y(
            AxisConfig::new()
                .label("USD (k)")
                .formatter(|v| format!("{:.0}", v)),
        )
        .axis_x(AxisConfig::new().label("Quarter"))
        .style(GradientChartStyle)
        .selection(selection);

    Switcher::new(theme_mode)
        .child(default_chart)
        .child(gradient_chart)
}

fn pie_panel(
    model: ChartModel<String>,
    theme_mode: Signal<usize>,
    selection: ChartSelection,
) -> impl Widget {
    let series = model
        .only_series()
        .expect("pie model has exactly one series");

    let default_chart = PieChart::new(model.clone())
        .donut(0.55)
        .label_mode(PieLabelMode::Outside)
        .show_percentages(true)
        .selection(selection.clone())
        .center(pie_center_widget(selection.clone(), model.clone(), series));

    let gradient_chart = PieChart::new(model.clone())
        .donut(0.55)
        .label_mode(PieLabelMode::Outside)
        .show_percentages(true)
        .style(GradientChartStyle)
        .selection(selection.clone())
        .center(pie_center_widget(selection.clone(), model.clone(), series));

    let switcher = Switcher::new(theme_mode)
        .child(default_chart)
        .child(gradient_chart);

    let clear_selection = selection;
    VStack::new().spacing(8.0).child(switcher).child(
        HStack::new().spacing(8.0).child(Spacer::new()).child(
            Button::new(lit!("Clear selection"))
                .variant(ButtonVariant::Ghost)
                .on_activate_fn(move |_ctx| clear_selection.clear()),
        ),
    )
}

/// The donut center slot: shows the selected slice's category + share, or
/// "Total" when nothing is selected. Reactive on both the selection and
/// the model's own data (a refresh recomputes the percentage live).
fn pie_center_widget(
    selection: ChartSelection,
    model: ChartModel<String>,
    series: SeriesId,
) -> impl Widget {
    let base = selection.selection_signal().zip(&model.structure_version());

    let heading = {
        let model = model.clone();
        base.map(move |(set, _)| {
            set.iter()
                .next()
                .and_then(|&(sid, idx)| model.with_point(sid, idx, |d| d.category.clone()))
                .unwrap_or_else(|| "Total".to_string())
        })
    };
    let value = {
        let model = model.clone();
        base.map(move |(set, _)| {
            let total: f32 = model
                .with_series_view(series, |v| v.points.iter().map(|d| d.value).sum())
                .unwrap_or(0.0);
            if let Some(&(sid, idx)) = set.iter().next()
                && let Some(v) = model.with_point(sid, idx, |d| d.value)
            {
                let pct = if total > 0.0 { v / total * 100.0 } else { 0.0 };
                format!("{:.0}%", pct)
            } else {
                format!("{:.0}", total)
            }
        })
    };

    Center::new().child(
        VStack::new()
            .spacing(0.0)
            .alignment(HAlignment::Center)
            .child(
                TextWidget::new(lit!(""))
                    .style(TextStyleRole::Tiny)
                    .text(heading),
            )
            .child(
                TextWidget::new(lit!(""))
                    .style(TextStyleRole::BodyBold)
                    .text(value),
            ),
    )
}

/// A one-line readout of the Bar/Line `ChartSelection`: the selected
/// point's series, category, and value, or a prompt when nothing is
/// selected. Reactive on both the selection and the model's structure, so
/// a data refresh re-reads the value live.
fn series_selection_readout(selection: ChartSelection, model: ChartModel<String>) -> impl Widget {
    let text = selection
        .selection_signal()
        .zip(&model.structure_version())
        .map(move |(set, _)| {
            if let Some(&(sid, idx)) = set.iter().next() {
                let name = model
                    .with_series_view(sid, |v| v.name.to_string())
                    .unwrap_or_default();
                let detail = model
                    .with_point(sid, idx, |d| format!("{} = {:.0}", d.category, d.value))
                    .unwrap_or_default();
                format!("Selected: {name} · {detail}")
            } else {
                "Click a bar or point to select it.".to_string()
            }
        });
    TextWidget::new(lit!(""))
        .style(TextStyleRole::Small)
        .text(text)
}

// ─── Live strip chart: ChartModel push_point on a periodic tick, projected
//     through a ChartWindow. Chart widgets bind to a `ChartModel<T>`, not a
//     `ChartWindow<T>`, so the window's computed tail is materialized into a
//     small render-bound model each tick — an honest bridge given that
//     constraint (see module docs). ─────────────────────────────────────────

struct LiveStripPane {
    history: ChartModel<u32>,
    history_series: SeriesId,
    // ChartWindow projection → render-bound display model.
    window: ChartWindow<u32>,
    window_display: ChartModel<u32>,
    window_series: SeriesId,
    // ChartAggregate projection → render-bound display model.
    aggregate: ChartAggregate<u32>,
    agg_display: ChartModel<u32>,
    agg_series: SeriesId,
    tick: Rc<Cell<u32>>,
    status: Signal<String>,
    bucket_idx: Signal<usize>, // index into AGG_BUCKETS
    agg_fn_idx: Signal<usize>, // 0=Mean, 1=Max, 2=Min
    feed_state: Signal<usize>, // 0=running, 1=paused
    root_id: Option<WidgetId>,
    tick_sub: Option<FrameTickSubscription>,
}

impl LiveStripPane {
    fn new() -> Self {
        let history: ChartModel<u32> = ChartModel::new();
        let history_series = history.add_series("Live");

        let window_display: ChartModel<u32> = ChartModel::new();
        let window_series = window_display.add_series("Windowed");
        let window = ChartWindow::new(history.clone(), STRIP_WINDOW);

        let agg_display: ChartModel<u32> = ChartModel::new();
        let agg_series = agg_display.add_series("Rollup");
        let aggregate =
            ChartAggregate::new(history.clone(), AGG_BUCKETS[1], ChartAggregateFn::Mean);

        Self {
            history,
            history_series,
            window,
            window_display,
            window_series,
            aggregate,
            agg_display,
            agg_series,
            tick: Rc::new(Cell::new(0)),
            status: Signal::new(format!(
                "0 samples — window: last 0 of {STRIP_WINDOW} · rollup: 0 buckets"
            )),
            bucket_idx: Signal::new(1),
            agg_fn_idx: Signal::new(0),
            feed_state: Signal::new(0),
            root_id: None,
            tick_sub: None,
        }
    }
}

impl std::fmt::Debug for LiveStripPane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveStripPane").finish()
    }
}

impl Widget for LiveStripPane {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let raw_chart = LineChart::new(self.window_display.clone())
            .grid(true)
            .points(true)
            .area_fill(true)
            .hover_tooltip(true)
            .axis_y(
                AxisConfig::new()
                    .label("Value")
                    .formatter(|v| format!("{:.0}", v)),
            )
            .axis_x(AxisConfig::new().show_labels(false));

        let agg_chart = LineChart::new(self.agg_display.clone())
            .grid(true)
            .points(true)
            .hover_tooltip(true)
            .axis_y(
                AxisConfig::new()
                    .label("Rollup")
                    .formatter(|v| format!("{:.0}", v)),
            )
            .axis_x(AxisConfig::new().show_labels(false));

        let controls = HStack::new()
            .spacing(10.0)
            .child(TextWidget::new(lit!("Feed")).style(TextStyleRole::Small))
            .child(
                SegmentedControl::new(self.feed_state.clone())
                    .segments([lit!("Running"), lit!("Paused")]),
            )
            .child(Spacer::new())
            .child(TextWidget::new(lit!("Rollup bucket")).style(TextStyleRole::Small))
            .child(SegmentedControl::new(self.bucket_idx.clone()).segments([
                lit!("×2"),
                lit!("×4"),
                lit!("×8"),
            ]))
            .child(TextWidget::new(lit!("fn")).style(TextStyleRole::Small))
            .child(SegmentedControl::new(self.agg_fn_idx.clone()).segments([
                lit!("Mean"),
                lit!("Max"),
                lit!("Min"),
            ]));

        let status_label = TextWidget::new(lit!(""))
            .style(TextStyleRole::Small)
            .text(self.status.clone());

        let raw_labeled = VStack::new()
            .spacing(2.0)
            .child(
                TextWidget::new(lit!("Raw — ChartWindow (last N samples)"))
                    .style(TextStyleRole::Tiny),
            )
            .child(raw_chart);
        let agg_labeled = VStack::new()
            .spacing(2.0)
            .child(
                TextWidget::new(lit!("Rollup — ChartAggregate over the full history"))
                    .style(TextStyleRole::Tiny),
            )
            .child(agg_chart);

        let root = ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(controls)
                .child(raw_labeled)
                .child(agg_labeled)
                .child(status_label),
        );
        self.root_id = Some(root);

        // Re-configure + re-materialize the rollup when its controls change.
        {
            let aggregate = self.aggregate.clone();
            let disp = self.agg_display.clone();
            let src = self.history_series;
            let dst = self.agg_series;
            ctx.effect(&self.bucket_idx, move |i| {
                aggregate.set_bucket_size(AGG_BUCKETS[*i]);
                let n = aggregate.point_count(src);
                materialize(
                    src,
                    n,
                    |k| aggregate.with_point(src, k, |d| ChartDatum::new(d.category, d.value)),
                    &disp,
                    dst,
                );
            });
        }
        {
            let aggregate = self.aggregate.clone();
            let disp = self.agg_display.clone();
            let src = self.history_series;
            let dst = self.agg_series;
            ctx.effect(&self.agg_fn_idx, move |i| {
                aggregate.set_aggregate_fn(agg_fn_for(*i));
                let n = aggregate.point_count(src);
                materialize(
                    src,
                    n,
                    |k| aggregate.with_point(src, k, |d| ChartDatum::new(d.category, d.value)),
                    &disp,
                    dst,
                );
            });
        }

        // Reduced motion: build the (empty) charts but don't start the timer.
        if ctx.prefers_reduced_motion() {
            return vec![root];
        }

        let history = self.history.clone();
        let history_series = self.history_series;
        let window = self.window.clone();
        let window_display = self.window_display.clone();
        let window_series = self.window_series;
        let aggregate = self.aggregate.clone();
        let agg_display = self.agg_display.clone();
        let agg_series = self.agg_series;
        let feed_state = self.feed_state.clone();
        let status = self.status.clone();
        let tick = self.tick.clone();
        let period = Duration::from_millis(STRIP_PERIOD_MS);
        let last_advance: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));

        // Absolute-time gated, throttled tick — same pattern as `Cycle`'s
        // once-per-period advance (see crates/bastyde-widgets/src/animations/cycle.rs).
        ctx.effect(&ctx.frame_tick(), move |_delta| {
            if feed_state.get() == 1 {
                return; // paused
            }
            let now = Instant::now();
            let due = match last_advance.get() {
                None => {
                    last_advance.set(Some(now));
                    false
                }
                Some(prev) if now.duration_since(prev) >= period => {
                    last_advance.set(Some(now));
                    true
                }
                Some(_) => false,
            };
            if !due {
                return;
            }

            let x = tick.get();
            tick.set(x.wrapping_add(1));
            let t = x as f32;
            let value = 50.0 + 32.0 * (t * 0.35).sin() + 8.0 * (t * 0.9).cos();
            history.push_point(history_series, x, value);

            // Materialize both projections' current tails into their
            // render-bound display models — chart widgets consume a
            // ChartModel, not a ChartWindow / ChartAggregate projection.
            let wn = window.point_count(history_series);
            materialize(
                history_series,
                wn,
                |k| window.with_point(history_series, k, |d| ChartDatum::new(d.category, d.value)),
                &window_display,
                window_series,
            );
            let an = aggregate.point_count(history_series);
            materialize(
                history_series,
                an,
                |k| {
                    aggregate
                        .with_point(history_series, k, |d| ChartDatum::new(d.category, d.value))
                },
                &agg_display,
                agg_series,
            );

            status.set(format!(
                "{} samples — window: last {} of {} · rollup: {} buckets",
                history.point_count(history_series),
                wn,
                STRIP_WINDOW,
                an,
            ));
        });
        self.tick_sub = Some(ctx.subscribe_frame_tick_throttled(period));

        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // Layout-only wrapper; the inner LineChart owns its own a11y.
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_id.into_iter().collect()
    }
}

fn main() {
    BastydeAppBuilder::new()
        .install_automation_bridge_in_debug()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — Chart Demo")
                .size(880, 860)
                .root(|tree, _state| {
                    let series_model = make_series_model(1);
                    let pie_model = ChartModel::from_points(pie_points(1));

                    let counter = Signal::new(1u32);
                    let chart_kind = Signal::new(0_usize); // 0=bar, 1=line, 2=pie
                    let theme_mode = Signal::new(0_usize); // 0=default, 1=gradient
                    // Bar and Line share one selection over the same series
                    // model, so switching panels keeps the highlighted
                    // point in sync; the donut gets its own (unrelated
                    // series).
                    let series_selection = ChartSelection::new(SelectionMode::Single);
                    let pie_selection = ChartSelection::new(SelectionMode::Single);
                    // Clones for the selection-readout row (the switcher below
                    // consumes `series_selection`).
                    let readout_selection = series_selection.clone();
                    let clear_selection = series_selection.clone();
                    let readout_model = series_model.clone();
                    let kind_for_readout = chart_kind.clone();

                    let chart_switcher = Switcher::new(chart_kind.clone())
                        .child(bar_panel(
                            series_model.clone(),
                            theme_mode.clone(),
                            series_selection.clone(),
                        ))
                        .child(line_panel(
                            series_model.clone(),
                            theme_mode.clone(),
                            series_selection,
                        ))
                        .child(pie_panel(
                            pie_model.clone(),
                            theme_mode.clone(),
                            pie_selection,
                        ));

                    // Selection readout: a one-liner for Bars/Lines, a hint for
                    // the Donut (whose selection drives the center slot).
                    let selection_readout =
                        Switcher::new(kind_for_readout.map(|k| (*k == 2) as usize))
                            .child(
                                HStack::new()
                                    .spacing(8.0)
                                    .child(series_selection_readout(
                                        readout_selection,
                                        readout_model,
                                    ))
                                    .child(Spacer::new())
                                    .child(
                                        Button::new(lit!("Clear selection"))
                                            .variant(ButtonVariant::Ghost)
                                            .on_activate_fn(move |_ctx| clear_selection.clear()),
                                    ),
                            )
                            .child(
                                TextWidget::new(lit!(
                                    "The selected slice is shown in the donut center."
                                ))
                                .style(TextStyleRole::Small),
                            );

                    let kind_selector = SegmentedControl::new(chart_kind).segments([
                        lit!("Bars"),
                        lit!("Lines"),
                        lit!("Donut"),
                    ]);
                    let theme_selector = SegmentedControl::new(theme_mode)
                        .segments([lit!("Default"), lit!("Gradient theme")]);

                    // Live structural mutation: add / remove a series in place.
                    let add_model = series_model.clone();
                    let add_series_button =
                        Button::new(lit!("Add series")).on_activate_fn(move |_ctx| {
                            let count = add_model.series_count();
                            if count < MAX_SERIES {
                                let sid = add_model.add_series(format!("Series {}", count + 1));
                                for d in quarter_points(count as u32 + 7, count) {
                                    add_model.push_point(sid, d.category, d.value);
                                }
                            }
                        });
                    let rem_model = series_model.clone();
                    let remove_series_button =
                        Button::new(lit!("Remove series")).on_activate_fn(move |_ctx| {
                            let ids = rem_model.series_ids();
                            if let Some(&last) = ids.last().filter(|_| ids.len() > 1) {
                                rem_model.remove_series(last);
                            }
                        });

                    let refresh_series_model = series_model.clone();
                    let refresh_pie_model = pie_model.clone();
                    let refresh_button = Button::new(lit!("Refresh data"))
                        .variant(ButtonVariant::Filled)
                        .on_activate_fn(move |_ctx| {
                            let next = counter.get().wrapping_add(1);
                            counter.set(next);
                            // Regenerate every current series (any count).
                            for (si, id) in
                                refresh_series_model.series_ids().into_iter().enumerate()
                            {
                                refresh_series_model
                                    .replace_series_data(id, quarter_points(next, si));
                            }
                            if let Some(pid) = refresh_pie_model.only_series() {
                                refresh_pie_model.replace_series_data(pid, pie_points(next));
                            }
                        });

                    let content = VStack::new()
                        .spacing(12.0)
                        .child(GroupHeader::new(lit!("Quarterly Performance")))
                        .child(
                            HStack::new()
                                .spacing(16.0)
                                .child(kind_selector)
                                .child(theme_selector),
                        )
                        .child(chart_switcher)
                        .child(selection_readout)
                        .child(
                            HStack::new()
                                .spacing(8.0)
                                .child(add_series_button)
                                .child(remove_series_button)
                                .child(Spacer::new())
                                .child(refresh_button),
                        )
                        .child(GroupHeader::new(lit!("Live Strip Chart")))
                        .child(LiveStripPane::new());

                    tree.add(
                        VStack::new().child(dark_mode_toolbar()).child(
                            Expand::new().child(
                                Padding::uniform(16.0).child(ScrollArea::new().child(content)),
                            ),
                        ),
                    )
                }),
        )
        .run();
}
