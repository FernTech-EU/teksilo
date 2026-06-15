// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Charts tab — BarChart, LineChart, PieChart (donut + center slot).
//! Cannibalized from the `chart-demo` example. Charts live in the
//! `bastyde-charts` crate (same tier as `bastyde-widgets`).

use bastyde::prelude::*;
use bastyde::tokens::HAlignment;
use bastyde::widgets::{Center, Divider, FixedSize, TextWidget, VStack};
use bastyde_charts::{
    AxisConfig, BarChart, BarGrouping, ChartDatum, ChartSeries, LegendPosition, LineChart,
    PieChart, PieLabelMode,
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
            s.data.push(ChartDatum::new(l.to_string(), v));
        }
        out.push(s);
    }
    out
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

fn make_bar() -> BarChart<String> {
    BarChart::new(Signal::new(make_series()))
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

fn make_line() -> LineChart<String> {
    LineChart::new(Signal::new(make_series()))
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
    let pie_data = Signal::new(make_pie_data());
    let total_label = pie_data.map(|d| format!("{:.0}", d.iter().map(|x| x.value).sum::<f32>()));
    PieChart::new(pie_data)
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
                        TextWidget::new(lit!(""))
                            .style(TextStyleRole::BodyBold)
                            .bind_text(total_label),
                    ),
            ),
        )
}

fn sized(w: f32, h: f32, body: impl Widget + 'static) -> FixedSize {
    FixedSize::new().bind_width(w).bind_height(h).child(body)
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    let bar = section(ctx, lit!("BarChart"), sized(560.0, 260.0, make_bar()));
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
            .add_child(line)
            .add_child(pie),
    )
}

pub fn bati(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    // Charts take a `Signal<…>` constructor arg plus closure-bearing
    // builder chains (axis formatter, center slot) that bati! property
    // syntax can't express — pre-build each and splice via `#{ id }`.
    let bar_id = ctx.add(sized(560.0, 260.0, make_bar()));
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
