// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Charts tab — BarChart, LineChart, PieChart (donut + center slot), plus a
//! `ChartStyle` override showcase. Cannibalized from the `chart-demo`
//! example. Charts live in the `bastyde-charts` crate (same tier as
//! `bastyde-widgets`) and are bound to a `ChartModel<T>`.

use bastyde::core::styles::{
    BorderPosition, BorderRecipe, BorderStyle, ChartFillContext, ChartStyle, FillRecipe,
    GradientStop, RecipeColor,
};
use bastyde::prelude::*;
use bastyde::tokens::HAlignment;
use bastyde::widgets::{Center, Divider, MaxSize, TextWidget, VStack};
use bastyde_charts::{
    AxisConfig, BarChart, BarGrouping, ChartDatum, ChartModel, ChartSeries, LegendPosition,
    LineChart, PieChart, PieLabelMode,
};

use crate::shared::{Signals, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_charts_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_charts_refs())
}

/// Three deterministic series (Revenue / Cost / Profit over four
/// quarters) shared by the bar and line charts.
fn make_series() -> Vec<ChartSeries<String>> {
    let labels = ["Q1", "Q2", "Q3", "Q4"];
    let names = ["Revenue", "Cost", "Profit"];
    let mut out = Vec::new();
    for (si, name) in names.iter().enumerate() {
        let mut s = ChartSeries::<String>::new(*name);
        for (i, l) in labels.iter().enumerate() {
            let v = ((si * 53 + i * 17 + 31) % 60) as f32 + 10.0;
            s.push(l.to_string(), v);
        }
        out.push(s);
    }
    out
}

fn make_series_model() -> ChartModel<String> {
    ChartModel::from_series_vec(make_series())
}

/// Five slices for the donut chart.
fn make_pie_data() -> Vec<ChartDatum<String>> {
    let labels = ["Storage", "Apps", "System", "Cache", "Free"];
    labels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let v = ((i * 41 + 13) % 50) as f32 + 5.0;
            ChartDatum::new(l.to_string(), v)
        })
        .collect()
}

/// A `ChartStyle` override demonstrating gradient bar fills and dashed
/// gridlines — the same style used by the `chart-demo` example's
/// "Gradient theme" toggle. See `docs/styling-system.md` Tier 3.
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
        FillRecipe::Solid(RecipeColor::Static(cfg.resolved_color.with_alpha(opacity)))
    }

    fn donut_fill(&self, cfg: &ChartFillContext) -> FillRecipe {
        FillRecipe::Solid(RecipeColor::Static(cfg.resolved_color))
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

fn make_bar() -> BarChart<String> {
    BarChart::new(make_series_model())
        .grouping(BarGrouping::Grouped)
        .grid(true)
        .legend(true)
        .legend_position(LegendPosition::Bottom)
        .axis_y(
            AxisConfig::new()
                .label("USD (k)")
                .formatter(|v| format!("{v:.0}")),
        )
        .axis_x(AxisConfig::new().label("Quarter"))
        .bar_corner_radius(2.0)
}

fn make_bar_gradient() -> BarChart<String> {
    BarChart::new(make_series_model())
        .grouping(BarGrouping::Grouped)
        .grid(true)
        .legend(true)
        .legend_position(LegendPosition::Bottom)
        .axis_y(
            AxisConfig::new()
                .label("USD (k)")
                .formatter(|v| format!("{v:.0}")),
        )
        .axis_x(AxisConfig::new().label("Quarter"))
        .bar_corner_radius(2.0)
        .style(GradientChartStyle)
}

fn make_line() -> LineChart<String> {
    LineChart::new(make_series_model())
        .grid(true)
        .points(true)
        .area_fill(true)
        .legend(true)
        .legend_position(LegendPosition::Bottom)
        .axis_y(
            AxisConfig::new()
                .label("USD (k)")
                .formatter(|v| format!("{v:.0}")),
        )
        .axis_x(AxisConfig::new().label("Quarter"))
}

fn make_pie() -> PieChart<String> {
    let data = make_pie_data();
    let total: f32 = data.iter().map(|d| d.value).sum();
    PieChart::new(ChartModel::from_points(data))
        .donut(0.55)
        .label_mode(PieLabelMode::Outside)
        .show_percentages(true)
        .legend(true)
        .legend_position(LegendPosition::Trailing)
        .center(
            Center::new().child(
                VStack::new()
                    .spacing(0.0)
                    .alignment(HAlignment::Center)
                    .child(TextWidget::new(lit!("Total")).style(TextStyleRole::Tiny))
                    .child(
                        TextWidget::new(lit!(format!("{total:.0}"))).style(TextStyleRole::BodyBold),
                    ),
            ),
        )
}

// Charts fill whatever width they're given (`layout_response` proposes
// `proposal.width.unwrap_or(320.0)`), so cap rather than pin the width: at
// full viewport width the cap wins (same 560dp demo box as before), and in a
// narrow window the chart shrinks to fit instead of overshooting the tab.
// Height still comes through as an exact value on a VStack's unbounded main
// axis (`MaxSize` maps `(None, Some(max))` -> `Some(max)`), preserving each
// demo's chosen height.
fn sized(w: f32, h: f32, body: impl Widget + 'static) -> MaxSize {
    MaxSize::new(w, h).child(body)
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    let bar = section(ctx, lit!("BarChart"), sized(560.0, 260.0, make_bar()));
    let bar_gradient = section(
        ctx,
        lit!("BarChart (ChartStyle override — gradient fill + dashed grid)"),
        sized(560.0, 260.0, make_bar_gradient()),
    );
    let line = section(ctx, lit!("LineChart"), sized(560.0, 260.0, make_line()));
    let pie = section(
        ctx,
        lit!("PieChart (donut + center slot)"),
        sized(560.0, 280.0, make_pie()),
    );

    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(bar)
            .add_child(bar_gradient)
            .add_child(line)
            .add_child(pie),
    )
}

pub fn bati(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    // Charts take a `ChartModel<…>` constructor arg plus closure-bearing
    // builder chains (axis formatter, center slot, style override) that
    // bati! property syntax can't express — pre-build each and splice via
    // `#{ id }`.
    let bar_id = ctx.add(sized(560.0, 260.0, make_bar()));
    let bar_gradient_id = ctx.add(sized(560.0, 260.0, make_bar_gradient()));
    let line_id = ctx.add(sized(560.0, 260.0, make_line()));
    let pie_id = ctx.add(sized(560.0, 280.0, make_pie()));

    bati!(ctx => VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_charts_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_charts_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("BarChart")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ bar_id }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("BarChart (ChartStyle override — gradient fill + dashed grid)")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ bar_gradient_id }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("LineChart")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ line_id }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("PieChart (donut + center slot)")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ pie_id }
            }
        }
    )
}
