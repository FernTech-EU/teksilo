<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Charts

`teksilo-charts` provides bar, line, and pie/donut widgets backed by
`ChartModel<T>` or a `ChartWindow<T>` tail projection. Add `teksilo-charts` at the same version as the framework.

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
| Show the last N samples | Pass `ChartWindow::new(model.clone(), n)` to a chart |
| Aggregate samples | Compute buckets with `ChartAggregate` in application code |

Use patterns as well as color when series must remain distinguishable without
color perception. Give the chart and its series meaningful labels.

## Streaming history and scrolling

`LineChart` fits all supplied points into its current plot width. It does not
increase its width as samples arrive and has no built-in history scrollbar,
panning, or time viewport. Crowded category labels tilt and then use a stride;
this changes labels, not the number of points drawn.

The x-axis is categorical: samples are spaced by index, even when their labels
are timestamps. Unequal polling intervals are not represented as unequal gaps.

For a readable live tail, pass a `ChartWindow` directly. This constructor support
is available on `dev`; published 0.14.3 only accepts `ChartModel`.

```rust
use teksilo_charts::{AxisConfig, ChartModel, ChartWindow, LineChart};

let history = ChartModel::<String>::new();
let gpu = history.add_series("GPU");
let tail = ChartWindow::new(history.clone(), 120);
let chart = LineChart::new(tail.clone())
    .axis_y(AxisConfig::new().range(0.0, 100.0))
    .legend(true);
history.push_point(gpu, "00:01".into(), 42.0);
tail.set_window_size(60);
```

`BarChart`, `PieChart` and `ChartLegend` accept the same `ChartSource` inputs.
Window slices borrow the source points. Appends, changes to the window size,
colors, patterns and legend visibility remain reactive. Hover and selection
indices are relative to the visible window, so a point at index zero changes
identity when the tail slides. Do not use those indices as persistent sample IDs.

A window bounds rendering, not storage. The source above retains every sample.
For a long-running dashboard, choose a retention limit and remove old source
points, archive them, or aggregate them. Keep the model in app state so rebuilding
the widget tree does not reset history.

A `ScrollArea` can scroll a deliberately wider child, for example a chart wrapped
in a `FixedSize` with an explicit width. That scrolls widget geometry; it does not
provide a time-domain viewport or bound rendering work. A chart has no fixed
360 by 280 minimum: its intrinsic minimum depends on labels and legend layout.

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
