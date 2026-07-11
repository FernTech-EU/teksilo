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
use bastyde::tokens::HAlignment;
use bastyde::widgets::{
    Button, ButtonVariant, Center, Expand, GroupHeader, HStack, Padding, ScrollArea,
    SegmentedControl, Spacer, Switcher, TextWidget, Toolbar, VStack,
};
use bastyde_charts::{
    AxisConfig, BarChart, BarGrouping, ChartDatum, ChartModel, ChartSelection, ChartSeries,
    ChartWindow, LegendPosition, LineChart, PieChart, PieLabelMode, SeriesId,
};

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

const SERIES_NAMES: [&str; 3] = ["Revenue", "Cost", "Profit"];
const QUARTERS: [&str; 4] = ["Q1", "Q2", "Q3", "Q4"];
const STRIP_WINDOW: usize = 24;
const STRIP_PERIOD_MS: u64 = 600;

fn dark_mode_toolbar() -> impl Widget {
    Toolbar::new().child(
        HStack::new()
            .child(Spacer::new())
            .child(bastyde::widgets::ThemeSwitcher::new()),
    )
}

// ─── Data generation (seeded pseudo-random, matches the pre-refactor demo) ─

fn series_points(seed: u32) -> Vec<Vec<ChartDatum<String>>> {
    (0..SERIES_NAMES.len())
        .map(|si| {
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
        })
        .collect()
}

fn make_series_model(seed: u32) -> ChartModel<String> {
    let series = SERIES_NAMES
        .iter()
        .zip(series_points(seed))
        .map(|(name, points)| ChartSeries::<String>::new(*name).data(points))
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

/// A `ChartStyle` demonstrating every gradient hook: a vertical bar
/// gradient, a top-to-bottom area gradient that fades toward the
/// baseline, a radial donut gradient (continuous across wedges — see
/// `PieChart`'s `project_gradient_to_wedge_local`), and dashed gridlines.
#[derive(Debug, Default, Clone, Copy)]
struct GradientChartStyle;

impl ChartStyle for GradientChartStyle {
    fn bar_fill(&self, cfg: &ChartFillContext) -> FillRecipe {
        FillRecipe::LinearGradient {
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: RecipeColor::Static(cfg.resolved_color.with_alpha(0.55)),
                },
                GradientStop {
                    offset: 1.0,
                    color: RecipeColor::Static(cfg.resolved_color),
                },
            ],
            angle_deg: 0.0, // top -> bottom
        }
    }

    fn area_fill(&self, cfg: &ChartFillContext, opacity: f32) -> FillRecipe {
        FillRecipe::LinearGradient {
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: RecipeColor::Static(
                        cfg.resolved_color.with_alpha((opacity * 2.5).min(1.0)),
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
        FillRecipe::RadialGradient {
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: RecipeColor::Static(cfg.resolved_color),
                },
                GradientStop {
                    offset: 1.0,
                    color: RecipeColor::Static(cfg.resolved_color.with_alpha(0.6)),
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

// ─── Live strip chart: ChartModel push_point on a periodic tick, projected
//     through a ChartWindow. Chart widgets bind to a `ChartModel<T>`, not a
//     `ChartWindow<T>`, so the window's computed tail is materialized into a
//     small render-bound model each tick — an honest bridge given that
//     constraint (see module docs). ─────────────────────────────────────────

struct LiveStripPane {
    history: ChartModel<u32>,
    display: ChartModel<u32>,
    window: ChartWindow<u32>,
    history_series: SeriesId,
    display_series: SeriesId,
    tick: Rc<Cell<u32>>,
    status: Signal<String>,
    root_id: Option<WidgetId>,
    tick_sub: Option<FrameTickSubscription>,
}

impl LiveStripPane {
    fn new() -> Self {
        let history: ChartModel<u32> = ChartModel::new();
        let history_series = history.add_series("Live");
        let display: ChartModel<u32> = ChartModel::new();
        let display_series = display.add_series("Live");
        let window = ChartWindow::new(history.clone(), STRIP_WINDOW);
        Self {
            history,
            display,
            window,
            history_series,
            display_series,
            tick: Rc::new(Cell::new(0)),
            status: Signal::new(format!(
                "0 samples captured — ChartWindow shows the last 0 of {STRIP_WINDOW}"
            )),
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
        let chart = LineChart::new(self.display.clone())
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

        let status_label = TextWidget::new(lit!(""))
            .style(TextStyleRole::Small)
            .text(self.status.clone());

        let root = ctx.add(VStack::new().spacing(4.0).child(chart).child(status_label));
        self.root_id = Some(root);

        // Reduced motion: build the (empty) chart but don't start the timer.
        if ctx.prefers_reduced_motion() {
            return vec![root];
        }

        let history = self.history.clone();
        let display = self.display.clone();
        let window = self.window.clone();
        let history_series = self.history_series;
        let display_series = self.display_series;
        let status = self.status.clone();
        let tick = self.tick.clone();
        let period = Duration::from_millis(STRIP_PERIOD_MS);
        let last_advance: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));

        // Absolute-time gated, throttled tick — same pattern as `Cycle`'s
        // once-per-period advance (see crates/bastyde-widgets/src/animations/cycle.rs).
        ctx.effect(&ctx.frame_tick(), move |_delta| {
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

            // Materialize the ChartWindow's current tail into the
            // render-bound display model — the chart widget can only
            // consume a ChartModel, not the ChartWindow projection itself.
            let n = window.point_count(history_series);
            let points: Vec<ChartDatum<u32>> = (0..n)
                .filter_map(|i| {
                    window.with_point(history_series, i, |d| ChartDatum::new(d.category, d.value))
                })
                .collect();
            display.replace_series_data(display_series, points);

            status.set(format!(
                "{} samples captured — ChartWindow shows the last {} of {}",
                history.point_count(history_series),
                window.point_count(history_series),
                STRIP_WINDOW,
            ));
        });
        self.tick_sub = None;
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

                    let kind_selector = SegmentedControl::new(chart_kind).segments([
                        lit!("Bars"),
                        lit!("Lines"),
                        lit!("Donut"),
                    ]);
                    let theme_selector = SegmentedControl::new(theme_mode)
                        .segments([lit!("Default"), lit!("Gradient theme")]);

                    let refresh_series_model = series_model.clone();
                    let refresh_pie_model = pie_model.clone();
                    let refresh_button = Button::new(lit!("Refresh data"))
                        .variant(ButtonVariant::Filled)
                        .on_activate_fn(move |_ctx| {
                            let next = counter.get().wrapping_add(1);
                            counter.set(next);
                            for (id, points) in refresh_series_model
                                .series_ids()
                                .into_iter()
                                .zip(series_points(next))
                            {
                                refresh_series_model.replace_series_data(id, points);
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
                        .child(
                            HStack::new()
                                .spacing(8.0)
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
