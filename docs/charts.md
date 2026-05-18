# Charts

**Companion to:** [architecture.md](architecture.md)
**Scope:** The `bastyde-charts` crate — `BarChart`, `LineChart`, `PieChart`
(pie + donut), the `ChartSeries<T>` / `ChartDatum<T>` data model, the
shared axis / palette / legend infrastructure, and the rendering and
reactivity contracts that connect them to the widget tree.

---

## 1. Why bastyde-charts is its own crate

Charts are widget-shaped — they implement
[`Widget`](../crates/bastyde-core/src/widget.rs) and live inside the
retained tree like any other view — but the catalog is large enough
that bundling it into [`bastyde-widgets`](../crates/bastyde-widgets/) would
mean every chart-free desktop app drags ~3,000 lines of axis math,
nice-numbers tick generation, polygonal slice paths, and the Okabe-Ito
palette into its binary. So `bastyde-charts` sits *at the same layering
tier* as `bastyde-widgets`, not on top of it:

```
bastyde-tokens → bastyde-canvas → bastyde-core ── bastyde-data ─┬→ bastyde-widgets
                                                    └→ bastyde-charts
```

`bastyde-charts` deliberately does **not** depend on `bastyde-widgets`. The
hover tooltip, the legend, the donut center placeholder all live inside
`bastyde-charts` and use only `bastyde-core` + `bastyde-canvas` primitives.
Tests reach for `bastyde-widgets::TextWidget` as a *dev-dependency* to
populate the donut center slot, but no production code path crosses
the boundary.

What this buys an app: depending on `bastyde-charts` brings just charts.
Depending on `bastyde-widgets` brings just widgets. The umbrella
[`bastyde`](../crates/bastyde/) crate re-exports both, so apps that
want the union pay nothing extra.

The directory layout under [crates/bastyde-charts/src/](../crates/bastyde-charts/src/)
is module-flat (no `mod.rs` per coding conventions): one file per
public widget plus shared helpers for axes, palette, legend, and
plot-area carving.

## 2. The widget catalog

Three widgets, deliberately kept that small. The
[charts plan](plans/charts-plan.md) §1 spells out the
"focused two-chart catalog avoids the tiny matplotlib trap" reasoning;
pie/donut joined late because it's the one chart users routinely
expect from a desktop GUI toolkit and the implementation reuses 90% of
the bar/line infrastructure.

### 2.1 BarChart

Vertical or horizontal bars, single or grouped series. Value labels,
grid lines, axis titles, and an embedded legend are all opt-in flags
on the builder.

```rust
use bastyde_charts::{AxisConfig, BarChart, BarGrouping, ChartDatum, ChartSeries, LegendPosition};

let mut revenue = ChartSeries::<String>::new("Revenue");
revenue.push("Q1".into(), 12.5);
revenue.push("Q2".into(), 18.3);
revenue.push("Q3".into(), 9.8);
revenue.push("Q4".into(), 22.1);

BarChart::new(vec![revenue])
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

The y-domain auto-includes zero — bars without a zero baseline aren't
legible, and the bar of a 100→102 series on a [100, 102] axis looks
identical to a 0→2 series on a [0, 100] axis. Override with
`AxisConfig::range(min, max)` if you really mean it.

### 2.2 LineChart

Polyline per series with optional area fill, hover tooltips, and
embedded legend. PR-3 / PR-4 territory.

```rust
use bastyde_charts::{AxisConfig, ChartSeries, LineChart};

let mut series = ChartSeries::<String>::new("Latency p99");
series.push("Mon".into(), 142.0);
series.push("Tue".into(), 138.5);
// ...

LineChart::new(vec![series])
    .grid(true)
    .points(true)
    .area_fill(true)
    .area_fill_opacity(0.15)
    .hover_tooltip(true)
    .axis_y(AxisConfig::new().label("ms"))
```

The y-domain pads ±5% so points at the data extremes don't sit on the
axis edge. `nice_ticks` then snaps ticks outward, which can extend the
range slightly past the padding — that's the standard data-viz
behavior and matches matplotlib / d3.

### 2.3 PieChart (and donut)

One widget for both shapes. Set `inner_radius_ratio == 0.0` (the
default) for a pie; any positive value is a donut. The optional
**center widget slot** is silently ignored when the ratio is `0.0`,
so swapping pie ↔ donut at runtime is safe.

```rust
use bastyde_charts::{ChartDatum, LegendPosition, PieChart, PieLabelMode};
use bastyde::widgets::{TextWidget, VStack};

let total = data.map(|d| d.iter().map(|x| x.value).sum::<f32>())
                .map(|t| format!("${:.0}", t));

PieChart::new(data)
    .donut(0.55)
    .label_mode(PieLabelMode::Outside)
    .show_percentages(true)
    .legend(true)
    .legend_position(LegendPosition::Trailing)
    .center(
        VStack::new()
            .child(TextWidget::new_literal("Total").style(TextStyleRole::Tiny))
            .child(TextWidget::new_literal("").bind_text(total)),
    )
```

The center slot follows the existing `Option<PendingChild>` pattern
used by [`Card`](../crates/bastyde-widgets/src/card.rs:13),
[`DialogContent`](../crates/bastyde-widgets/src/dialog.rs:206), and
[`GroupBox`](../crates/bastyde-widgets/src/group_box.rs:23): two builders
(`.center(impl Widget)` and `.center_id(WidgetId)`), resolved in
`build()` via `ctx.add_boxed`.

The placement is the largest square inscribed in the donut hole
(`side = inner_radius * √2`). A `TextWidget` for the total / a
`VStack` of label + value / a small `IconWidget` all fit comfortably;
larger compositions need to be self-clipping.

## 3. Data model

`ChartSeries<T>` and `ChartDatum<T>` live in
[crates/bastyde-charts/src/series.rs](../crates/bastyde-charts/src/series.rs).

```rust
pub struct ChartDatum<T> {
    pub category: T,        // x-axis position: String, enum, date, …
    pub value: f32,         // y-axis value (always f32)
}

pub struct ChartSeries<T> {
    pub name: String,
    pub color: Option<ColorProp>,    // None → palette assigns
    pub visible: Signal<bool>,       // toggleable from a legend / settings
    pub data: Vec<ChartDatum<T>>,
}
```

The unit of binding is `Prop<Vec<ChartSeries<T>>>` — static for fixed
charts, `Signal<Vec<…>>` for live data. The whole vec is replaced on
update; data sets that fit a chart (typically <500 points across
<10 series) don't need incremental change events. If profiling shows
clone cost from `Signal<Vec<…>>::get()`, the next step is wrapping
the vec in `Rc<…>` rather than introducing a `ChartListModel` analog
of [`ListModel<T>`](../crates/bastyde-data/src/list_model.rs).

`T` is the **category / x-axis** type. Common choices: `String` for
human-readable labels, an `enum` for fixed buckets, `chrono::DateTime`
for time-series (the chart only requires `Display`). Numeric values
are always `f32`. PieChart accepts a flat `Vec<ChartDatum<T>>` directly
since it's naturally one-dimensional, with a `from_series(series)`
adapter for callers that already have a `ChartSeries`.

`series.visible` is a `Signal<bool>` so a legend (or any other UI)
can drive show/hide without reaching into the data vec. Hidden series
are dropped from the y-domain and skipped in paint.

## 4. Axes — `nice_ticks` and formatting

[crates/bastyde-charts/src/axis.rs](../crates/bastyde-charts/src/axis.rs)
implements the Wilkinson / Heckbert nice-numbers algorithm extended
with the d3 / matplotlib `2.5` step (so `0..100 / target=4` produces
`[0, 25, 50, 75, 100]` instead of degrading to step 20):

```rust
pub fn nice_ticks(min: f32, max: f32, target_count: usize) -> Vec<f32>;
```

`target_count` is the **maximum number of intervals**, not a hard
tick count. The algorithm picks the smallest "nice" step (1, 2, 2.5,
5, or 10 × 10^k) that yields `≤ target_count` intervals covering
`[min, max]`, then snaps `min` down and `max` up to step-aligned
positions. The result has at most `target_count + 1` ticks.

Tick counts auto-derive from the y-axis pixel length unless
`AxisConfig::tick_count_hint(n)` overrides:

```rust
pub fn auto_tick_count(axis_pixels: f32) -> usize {
    ((axis_pixels / 60.0) as usize).clamp(2, 10)
}
```

That's a 60-pixel-per-tick density target. It works equally well
for a 200-pixel-tall sparkline (3 ticks) and a 600-pixel-tall
dashboard chart (10 ticks).

`AxisConfig::formatter` takes any `Fn(f32) -> String` for currency,
units, locale-aware separators, time strings, etc. The default
formatter trims trailing zeros and caps at 4 decimal places — fine
for most charts; supply your own when you want `"$12k"` or
`"3.5 ms"`.

Categorical x-axes (BarChart, LineChart over discrete categories) use
one tick per category — `nice_ticks` is not invoked there. The
x-axis type stays generic over `T` exactly so a future time-axis
formatter can hook in here without API churn.

## 5. Palette

[`ChartPalette`](../crates/bastyde-charts/src/palette.rs) is the
mechanism that decides series colors when a series didn't pick its
own:

```rust
pub enum ChartPalette {
    FromTheme,             // reads theme.colors.chart_palette
    Custom(Vec<Color>),
}
```

Default is `FromTheme`, which reads
[`ColorTokens::chart_palette`](../crates/bastyde-tokens/src/theme.rs).
The built-in light and dark themes ship the **Okabe-Ito**
colorblind-safe sequence (Okabe & Ito 2008), the same palette used by
ggplot2 and seaborn:

| # | Name           | Hex     |
|---|----------------|---------|
| 1 | Orange         | #E69F00 |
| 2 | Sky blue       | #56B4E9 |
| 3 | Bluish green   | #009E73 |
| 4 | Yellow         | #F0E442 |
| 5 | Blue           | #0072B2 |
| 6 | Vermilion      | #D55E00 |
| 7 | Reddish purple | #CC79A7 |
| 8 | Black (light) / White (dark) | #000000 / #FFFFFF |

Themes can override the palette field directly to brand-match — the
`from_os_colors` derivation path inherits the default and lets the
OS colors flow through everything else. Per-chart override:
`.palette(ChartPalette::Custom(vec![...]))` on the chart builder.
Per-series override: `series.color = Some(ColorProp::Static(...))`,
which wins over both the chart palette and the theme palette.

Wrap-around is automatic: `palette.color_for(index, theme)` does
`palette[index % palette.len()]`. Eight default colors handle every
chart you should reasonably draw without a legend so dense it's
unreadable.

## 6. Legend

Two ways to use it:

**Embedded** — the chart instantiates `ChartLegend` internally when
constructed with `.legend(true)`, lays it out at `legend_position`
(`Top` / `Bottom` / `Leading` / `Trailing`), and shares the same
`series` and `palette` props.

**Standalone** — build a [`ChartLegend`](../crates/bastyde-charts/src/legend.rs)
yourself and place it anywhere in your widget tree, sharing the
same series prop the chart binds to:

```rust
use bastyde_charts::{ChartLegend, LegendOrientation};

let series_signal = Signal::new(make_series());
let chart = LineChart::new(series_signal.clone())
    .legend(false);                       // chart draws no legend
let legend = ChartLegend::new(series_signal.clone())
    .orientation(LegendOrientation::Vertical);

VStack::new()
    .child(HStack::new().child(chart).child(legend))
```

The standalone form is the right answer when you want the legend in
a different container, with a custom layout, or interactive
(click-to-hide) — currently planned as a follow-up.

Embedded legend orientation is auto-derived from position: `Top` and
`Bottom` get horizontal, `Leading` and `Trailing` get vertical.
Override with the standalone widget if you need something different.

## 7. Layout — proposal-driven plot-area carve

All three charts are **proposal-driven**: `layout_response` returns
whatever the parent proposes, with a 320×200 (line / bar) or 320×220
(pie) fallback when the proposal is unbounded. This matches
[`ProgressBar`](../crates/bastyde-widgets/src/progress_bar.rs) and
[`ScrollArea`](../crates/bastyde-widgets/src/scroll_area.rs) — charts
fit any container.

Inside `paint`, the bounds are carved into a plot rect by
[`carve_plot_area`](../crates/bastyde-charts/src/layout.rs), which:

1. Reserves the legend band on the requested edge (when shown).
2. Reserves a y-axis band on the leading edge: max tick label
   width + tick length + gap + axis-title height (when applicable).
3. Reserves an x-axis band on the bottom edge: tick label height +
   tick length + gap + axis-title height.
4. Insets the inner plot by `plot_padding_*` from the
   [`ChartStyle`](../crates/bastyde-tokens/src/components.rs) tokens.

Y-tick labels need actual values to measure widths, so the order is:
domain → `nice_ticks` → measure widest label string → carve y-band →
recompute tick positions to fit the carved plot rect. Single pass —
no iteration on label collisions.

PieChart bypasses axis bands entirely (pie has no axes) and only
carves off the legend band. The disc inscribes into the largest
centered square minus `pie_padding`.

For PieChart with a center widget, `place_children` and `paint` both
go through `compute_plot_rect` so the inscribed-square slot is
centered on the actually-rendered disc, not on the full bounds —
otherwise the slot drifts when a legend is shown.

## 8. Reactivity — binding levels

| Change | Binding level | Why |
|---|---|---|
| `Vec<ChartSeries<T>>` swap | `Relayout` | Y-domain may shift → tick positions change → label widths change → carve changes |
| `ChartSeries::color` via `Signal<Color>` | `RepaintOnly` | Geometry unchanged |
| `series.visible` toggle | `Relayout` | Visible set changes auto-domain and bar widths |
| Hover state `Signal<Option<HoveredPoint>>` | `RepaintOnly` | Marker + tooltip only |
| Theme change | Auto via tree-wide `mark_all_dirty` | Colors/fonts re-resolved on next paint |
| `Prop<ChartPalette>` change | `RepaintOnly` | Color-only |
| PieChart `inner_radius_ratio` change | `Relayout` | Center-slot inscribed-square size depends on it |

The wiring lives in each chart's `build()`:

```rust
fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
    let id = ctx.self_id();
    let registry = ctx.binding_registry();
    self.series.register_if_bound(id, registry, BindingLevel::Relayout);
    self.palette.register_if_bound(id, registry, BindingLevel::RepaintOnly);
    // ...
}
```

For widgets that bind via per-series `ColorProp::Bound(signal)`, the
chart palette stays untouched and the series color signal triggers a
repaint without a full relayout. This is the right path for
"pulsing" / "highlighted" colors that don't change geometry.

## 9. Hover tooltips

LineChart and PieChart draw their hover tooltips **inline inside
their own `paint()`**, clipped to the plot rect. This is
deliberately different from
[`TooltipWidget`](../crates/bastyde-widgets/src/tooltip.rs):

- Chart tooltips track the cursor across the plot to the **nearest
  data point**, snapping per-pixel. `TooltipWidget` is anchored to a
  widget bounds box.
- Chart tooltips appear instantly. `TooltipWidget` waits ~700 ms for
  dwell.
- The content depends on which point is nearest, which can change
  within the same widget without an enter/leave event.

The implementation is straightforward:

1. The chart owns a `Signal<Option<HoveredState>>` (point or slice)
   bound at `BindingLevel::RepaintOnly`.
2. An `on_pointer_event` handler attached via `HandlerSet` reads the
   pointer position, finds the nearest hit in a `Vec<…Hit>` snapshot
   the chart wrote during paint, and updates the signal.
3. `paint()` reads the signal — if `Some`, it draws a marker (a
   small ring + filled circle for line charts, the wedge stroke for
   pie) and a tooltip rect above the marker.
4. **Edge-flip** placement: if the tooltip would clip the plot
   rect's top edge, it flips below the marker; if it would clip
   leading/trailing, it shifts inward.

The hit snapshot is keyed by paint epoch (replaced, not appended,
each paint), so a data change shrinks the index correctly. Hit-test
cost is O(N×S) per pointer move for N points across S series —
acceptable up to ~10k points without optimization.

For pie/donut, the hit-test is polar: convert pointer position to
`(angle, distance)` from disc center, accept the hit only if
`inner_radius ≤ distance ≤ outer_radius`, then locate the slice
whose angular range covers the pointer. The angle conversion has to
subtract `start_angle_degrees` and flip for non-clockwise charts —
both are easy to forget; the
[`pie_hit_test_uses_logical_angle_space`](../crates/bastyde-charts/src/pie_chart.rs)
test locks this.

Disable with `.hover_tooltip(false)` if you'd rather the chart not
react to hover at all (e.g. embedded in a tooltip itself, or behind
a busy overlay).

## 10. Theming — the `ChartStyle` token

[`ChartStyle`](../crates/bastyde-tokens/src/components.rs) carries
chart-specific dimensions: padding, tick lengths, label gaps,
gridline width, default line / point sizes, legend swatch and item
gaps, tooltip padding, and the four pie-related fields
(`pie_padding`, `pie_label_gap`, `pie_leader_length`,
`pie_min_slice_label_degrees`, `donut_default_inner_ratio`).

Charts pull their colors from existing roles, not new fields:

- Axis lines → `BorderRole::Default`
- Grid lines → `BorderRole::Default` with reduced alpha (0.4)
- Axis tick labels → `TextRole::Secondary`, `TextStyleRole::Tiny`
- Axis title → `TextRole::Secondary`, `TextStyleRole::Tiny`
- Legend label text → `TextRole::Primary`, `TextStyleRole::Tiny`
- Tooltip background / text / border → reuse `tooltip_bg`,
  `tooltip_text`, `tooltip_border`

The only chart-specific color is the `chart_palette` (§5). A theme
overriding the palette doesn't need to touch any other chart token;
a theme tightening density can change `ChartStyle::plot_padding_*`
without touching colors.

## 11. Accessibility

Each chart declares `Role::GraphicsDocument` with a name that
describes the shape (`"Bar chart: 3 series, 4 categories"`,
`"Line chart: 2 series, 12 points"`, `"Pie chart: 5 slices"`).

The plan called for per-series `GraphicsObject` children with
descriptive metadata; that's a follow-up. Today the chart node is a
single accessible label without per-series drill-down. Apps that need
data-table semantics for screen readers should mirror the chart with
a `TreeView` or a custom data table next to it — the same pattern
matplotlib / d3 users follow.

## 12. Limits and explicit follow-ups

- **No stacked bars.** Single + grouped only. Stacked needs its own
  legend + hit-test pass for the sub-bar; deferred.
- **No log axis.** `nice_ticks` is linear-only.
- **No time-axis formatters.** `T = chrono::DateTime` works
  structurally (the chart only needs `Display`), but tick generation
  doesn't snap to month/quarter/year boundaries. Deferred.
- **No animation on data change.** Whole-vec replace is instant. A
  per-bar / per-point `animate_to` integration is planned but not in
  the initial five-PR cycle.
- **No BarChart hover tooltip.** The infrastructure is shared with
  LineChart; wiring is straightforward but waited on Stacked
  semantics so the tooltip knows which sub-bar to label.
- **No interactive legend.** The standalone `ChartLegend` widget
  exposes an `interactive(true)` flag that's currently a no-op.
  Click-to-hide via `series.visible.set(...)` is the planned wire-up.
- **Per-series `GraphicsObject` a11y nodes.** Single
  `GraphicsDocument` only today.
- **Pie / donut hover for BarChart-style "follow the cursor across
  multiple slices."** The handler exists but the visual treatment
  matches Excel's "highlight one slice" — no slice-pull-on-hover yet.

For each of these, the file pattern in
[crates/bastyde-charts/src/](../crates/bastyde-charts/src/) is the place
to look — the modules are intentionally split so future work lands
in one or two files at most.

## 13. Demo

[examples/chart_demo](../examples/chart_demo/src/main.rs) ships all
three charts in one window driven by a `SegmentedControl` switch and
a "Refresh data" button that re-rolls a `Signal<Vec<ChartSeries>>`.
Run with:

```
cargo run -p chart-demo
```

It's the smallest non-trivial integration — bar/line share the same
series prop, the donut consumes a parallel `Signal<Vec<ChartDatum>>`
with a center-slot `VStack` showing the live total. Useful as a
sanity-check after any change to bastyde-charts; `cargo test -p
bastyde-charts` (51 headless tests, no GPU) is the faster CI path.
