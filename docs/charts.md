<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Charts

`teksilo-charts` provides bar, line, and pie/donut widgets backed by
`ChartModel<T>`. Add `teksilo-charts` at the same version as the framework.

## Minimal example

```rust
use teksilo::prelude::*;
use teksilo_charts::{BarChart, ChartModel, ChartSeries};

fn main() {
    TeksiloAppBuilder::new()
        .theme(intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Revenue")
                .size(640, 400)
                .root(|tree, _| {
                    let mut revenue = ChartSeries::<String>::new("Revenue");
                    revenue.push("Q1".into(), 12.5);
                    revenue.push("Q2".into(), 18.3);
                    let model = ChartModel::from_series_vec(vec![revenue]);
                    tree.add(BarChart::new(model).grid(true).value_labels(true))
                }),
        )
        .run();
}
```

## Common operations

| Task | API |
| --- | --- |
| Show bars, lines, or slices | `BarChart`, `LineChart`, `PieChart` |
| Set labels and number formatting | `AxisConfig::label` and `formatter` |
| Show a legend | `legend(true)` and `legend_position(...)` |
| Update data | Mutate the shared `ChartModel` |
| Share a selected datum | Pass a `ChartSelection` to `.selection(...)` |
| Change chart appearance | Install a `ChartStyle` |
| Project a streaming series | Compose `ChartWindow` or `ChartAggregate` in application code |

Use patterns as well as color when series must remain distinguishable without
color perception. Give the chart and its series meaningful labels.

## BarChart

Vertical or horizontal bars, single or grouped series. Value labels,
grid lines, axis titles, and an embedded legend are all opt-in flags
on the builder.

```rust
use teksilo_charts::{AxisConfig, BarChart, BarGrouping, ChartModel, ChartSeries, LegendPosition};

let mut revenue = ChartSeries::<String>::new("Revenue");
revenue.push("Q1".into(), 12.5);
revenue.push("Q2".into(), 18.3);
revenue.push("Q3".into(), 9.8);
revenue.push("Q4".into(), 22.1);

let model = ChartModel::from_series_vec(vec![revenue]);

BarChart::new(model)
    .grid(true)
    .value_labels(true)
    .legend(true)
    .legend_position(LegendPosition::Bottom)
    .axis_y(
        AxisConfig::new()
            .label("USD (k)")
            .formatter(|v| format!("${:.0}", v)),
    )
    .axis_x(AxisConfig::new().label("Quarter"))
    .bar_corner_radius(2.0)
```

The y-domain auto-includes zero, bars without a zero baseline aren't
legible, and the bar of a 100→102 series on a [100, 102] axis looks
identical to a 0→2 series on a [0, 100] axis. Override with
`AxisConfig::range(min, max)` if you really mean it.

## LineChart

Polyline per series with optional area fill, the shared datum readout
and an embedded legend.

```rust
use teksilo_charts::{AxisConfig, ChartModel, ChartSeries, LineChart};

let mut series = ChartSeries::<String>::new("Latency p99");
series.push("Mon".into(), 142.0);
series.push("Tue".into(), 138.5);
// ...

let model = ChartModel::from_series_vec(vec![series]);

LineChart::new(model)
    .grid(true)
    .points(true)
    .area_fill(true)
    .area_fill_opacity(0.15)
    .hover_tooltip(true)
    .axis_y(AxisConfig::new().label("ms"))
```

The y-domain pads ±5% so points at the data extremes don't sit on the
axis edge. `nice_ticks` then snaps ticks outward, which can extend the
range slightly past the padding, that's the standard data-viz
behavior and matches matplotlib / d3.

## PieChart (and donut)

One widget for both shapes. Set `inner_radius_ratio == 0.0` (the
default) for a pie; any positive value is a donut. The optional
**center widget slot** is silently ignored when the ratio is `0.0`,
so swapping pie ↔ donut at runtime is safe.

```rust
use teksilo_charts::{ChartDatum, ChartModel, LegendPosition, PieChart, PieLabelMode};
use teksilo::widgets::{TextWidget, VStack};

let data: Vec<ChartDatum<String>> = /* … */;
let total = format!("${:.0}", data.iter().map(|d| d.value).sum::<f32>());
let model = ChartModel::from_points(data);

PieChart::new(model)
    .donut(0.55)
    .label_mode(PieLabelMode::Outside)
    .show_percentages(true)
    .legend(true)
    .legend_position(LegendPosition::Trailing)
    .center(
        VStack::new()
            .child(TextWidget::new(lit!("Total")).style(TextStyleRole::Tiny))
            .child(TextWidget::new(lit!(total))),
    )
```

The center slot follows the existing `Option<PendingChild>` pattern
used by [`Card`](../crates/teksilo-widgets/src/card.rs),
[`DialogContent`](../crates/teksilo-widgets/src/dialog.rs), and
[`GroupBox`](../crates/teksilo-widgets/src/group_box.rs): one builder,
`.center(impl IntoTeksiChild)`, taking either a widget or a `WidgetId`,
resolved in `build()` via `ctx.add_boxed`.

The placement is the largest square inscribed in the donut hole
(`side = inner_radius * √2`). A `TextWidget` for the total / a
`VStack` of label + value / a small `IconWidget` all fit comfortably;
larger compositions need to be self-clipping.


## Constraints

- Bars support single and grouped series; stacked bars are not implemented.
- Axes use linear tick generation. There is no logarithmic axis or calendar-aware
  tick spacing. A custom label formatter changes labels, not tick placement.
- Data changes update layout and paint without animated data transitions.
- `ChartWindow` and `ChartAggregate` are application-side building blocks;
  chart widgets do not install them automatically.
- Check legend target sizes at Compact density and test keyboard and
  assistive-technology navigation in the application.

## Reference

- [Chart source and API](../crates/teksilo-charts/src/lib.rs)
- [Data collections API](data-collections/index.md)
- [Data models](data-models.md)


## Engineering reference

[Implementation details and review history](https://github.com/ferntech-eu/teksilo/blob/main/engineering/docs/charts.md)
are retained in the repository.
