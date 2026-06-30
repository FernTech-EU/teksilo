// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Chart Demo: BarChart, LineChart, PieChart with donut + center slot.
//!
//! Run with: `cargo run -p chart-demo`

use bastyde::prelude::*;
use bastyde::tokens::HAlignment;
use bastyde::widgets::{
    Button, ButtonVariant, Center, Expand, GroupHeader, HStack, Padding, SegmentedControl, Spacer,
    Switcher, TextWidget, Toolbar, VStack,
};
use bastyde_charts::{
    AxisConfig, BarChart, BarGrouping, ChartDatum, ChartSeries, LegendPosition, LineChart,
    PieChart, PieLabelMode,
};

fn dark_mode_toolbar() -> impl Widget {
    Toolbar::new().child(
        HStack::new()
            .child(Spacer::new())
            .child(bastyde::widgets::ThemeSwitcher::new()),
    )
}

fn make_series(seed: u32) -> Vec<ChartSeries<String>> {
    let labels = ["Q1", "Q2", "Q3", "Q4"];
    let names = ["Revenue", "Cost", "Profit"];
    let mut out = Vec::new();
    for (si, name) in names.iter().enumerate() {
        let mut s = ChartSeries::<String>::new(*name);
        for (i, l) in labels.iter().enumerate() {
            let v = ((seed
                .wrapping_mul(31)
                .wrapping_add(si as u32 * 53)
                .wrapping_add(i as u32 * 17))
                % 60) as f32
                + 10.0;
            s.data.push(ChartDatum::new(l.to_string(), v));
        }
        out.push(s);
    }
    out
}

fn make_pie_data(seed: u32) -> Vec<ChartDatum<String>> {
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

fn main() {
    BastydeAppBuilder::new()
        .install_automation_bridge_in_debug()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — Chart Demo")
                .size(820, 720)
                .root(|tree, _state| {
                    let series = Signal::new(make_series(1));
                    let pie_data = Signal::new(make_pie_data(1));
                    let counter = Signal::new(1u32);

                    // Switcher state for the chart family selector.
                    let chart_kind = Signal::new(0_usize); // 0=bar, 1=line, 2=pie
                    let kind_for_btn = chart_kind.clone();

                    let series_for_btn = series.clone();
                    let pie_for_btn = pie_data.clone();
                    let counter_for_btn = counter.clone();

                    let bar = BarChart::new(series.clone())
                        .grouping(BarGrouping::Grouped)
                        .grid(true)
                        .legend(true)
                        .legend_position(LegendPosition::Bottom)
                        .axis_y(
                            AxisConfig::new()
                                .label("USD (k)")
                                .formatter(|v| format!("{:.0}", v)),
                        )
                        .axis_x(AxisConfig::new().label("Quarter"))
                        .bar_corner_radius(2.0);

                    let line = LineChart::new(series.clone())
                        .grid(true)
                        .points(true)
                        .area_fill(true)
                        .legend(true)
                        .legend_position(LegendPosition::Bottom)
                        .axis_y(
                            AxisConfig::new()
                                .label("USD (k)")
                                .formatter(|v| format!("{:.0}", v)),
                        )
                        .axis_x(AxisConfig::new().label("Quarter"));

                    let total_signal = pie_data.map(|d| d.iter().map(|x| x.value).sum::<f32>());
                    let total_label = total_signal.map(|t| format!("{:.0}", t));
                    let pie = PieChart::new(pie_data.clone())
                        .donut(0.55)
                        .label_mode(PieLabelMode::Outside)
                        .show_percentages(true)
                        .legend(true)
                        .legend_position(LegendPosition::Trailing)
                        .center(
                            // Wrap the VStack in a Center so the
                            // "Total / value" pair is centered both
                            // horizontally AND vertically inside the
                            // donut hole (VStack alone would top-align).
                            Center::new().child(
                                VStack::new()
                                    .spacing(0.0)
                                    .alignment(HAlignment::Center)
                                    .child(
                                        TextWidget::new(lit!("Total")).style(TextStyleRole::Tiny),
                                    )
                                    .child(
                                        TextWidget::new(lit!(""))
                                            .style(TextStyleRole::BodyBold)
                                            .bind_text(total_label),
                                    ),
                            ),
                        );

                    let switcher = Switcher::new(chart_kind.clone())
                        .child(bar)
                        .child(line)
                        .child(pie);

                    let segmented = SegmentedControl::new(chart_kind.clone()).segments([
                        lit!("Bars"),
                        lit!("Lines"),
                        lit!("Donut"),
                    ]);

                    let content = VStack::new()
                        .spacing(12.0)
                        .child(GroupHeader::new(lit!("Quarterly Performance")))
                        .child(segmented)
                        .child(switcher)
                        .child(
                            HStack::new().spacing(8.0).child(Spacer::new()).child(
                                Button::new(lit!("Refresh data"))
                                    .variant(ButtonVariant::Filled)
                                    .on_activate_fn(move |_ctx| {
                                        let next = counter_for_btn.get().wrapping_add(1);
                                        counter_for_btn.set(next);
                                        series_for_btn.set(make_series(next));
                                        pie_for_btn.set(make_pie_data(next));
                                        // Cycle through kinds to demo
                                        // each chart, optional.
                                        let _ = kind_for_btn.get();
                                    }),
                            ),
                        );
                    tree.add(
                        VStack::new()
                            .child(dark_mode_toolbar())
                            .child(Expand::new().child(Padding::uniform(16.0).child(content))),
                    )
                }),
        )
        .run();
}
