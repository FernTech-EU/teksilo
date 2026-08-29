<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Charts

**Companion to:** [architecture.md](architecture.md), [data-models.md](data-models.md)
**Scope:** The `teksilo-charts` crate — `BarChart`, `LineChart`, `PieChart`
(pie + donut), the `ChartModel<T>` data model (`teksilo-data`) and its
`ChartSeries<T>` / `ChartDatum<T>` construction DTOs, the Tier-3
`ChartStyle` trait, the shared axis / palette / legend infrastructure,
and the rendering and reactivity contracts that connect them to the
widget tree.

---

## 1. Why teksilo-charts is its own crate

Charts are widget-shaped — they implement
[`Widget`](../crates/teksilo-core/src/widget.rs) and live inside the
retained tree like any other view — but the catalog is large enough
that bundling it into [`teksilo-widgets`](../crates/teksilo-widgets/) would
mean every chart-free desktop app drags ~3,000 lines of axis math,
nice-numbers tick generation, polygonal slice paths, and the Okabe-Ito
palette into its binary. So `teksilo-charts` sits *at the same layering
tier* as `teksilo-widgets`, not on top of it:

```
teksilo-tokens → teksilo-canvas → teksilo-core ── teksilo-data ─┬→ teksilo-widgets
                                                    └→ teksilo-charts
```

`teksilo-charts` deliberately does **not** depend on `teksilo-widgets`. The
hover tooltip, the legend, the donut center placeholder all live inside
`teksilo-charts` and use only `teksilo-core` + `teksilo-canvas` primitives.
Tests reach for `teksilo-widgets::TextWidget` as a *dev-dependency* to
populate the donut center slot, but no production code path crosses
the boundary.

What this buys an app: depending on `teksilo-charts` brings just charts.
Depending on `teksilo-widgets` brings just widgets. The umbrella
[`teksilo`](../crates/teksilo/) crate re-exports both, so apps that
want the union pay nothing extra.

The directory layout under [crates/teksilo-charts/src/](../crates/teksilo-charts/src/)
is module-flat (no `mod.rs` per coding conventions): one file per
public widget plus shared helpers for axes, palette, legend, and
plot-area carving.

## 2. The widget catalog

Three widgets, deliberately kept that small — a focused two-chart
catalog avoids the tiny-matplotlib trap.
Pie/donut joined late because it's the one chart users routinely
expect from a desktop GUI toolkit and the implementation reuses 90% of
the bar/line infrastructure.

### 2.1 BarChart

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

The y-domain auto-includes zero — bars without a zero baseline aren't
legible, and the bar of a 100→102 series on a [100, 102] axis looks
identical to a 0→2 series on a [0, 100] axis. Override with
`AxisConfig::range(min, max)` if you really mean it.

### 2.2 LineChart

Polyline per series with optional area fill, hover tooltips, and
embedded legend. PR-3 / PR-4 territory.

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
range slightly past the padding — that's the standard data-viz
behavior and matches matplotlib / d3.

### 2.3 PieChart (and donut)

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
used by [`Card`](../crates/teksilo-widgets/src/card.rs:31),
[`DialogContent`](../crates/teksilo-widgets/src/dialog.rs:351), and
[`GroupBox`](../crates/teksilo-widgets/src/group_box.rs:29): two builders
(`.center(impl Widget)` and `.center_id(WidgetId)`), resolved in
`build()` via `ctx.add_boxed`.

The placement is the largest square inscribed in the donut hole
(`side = inner_radius * √2`). A `TextWidget` for the total / a
`VStack` of label + value / a small `IconWidget` all fit comfortably;
larger compositions need to be self-clipping.

## 3. Data model — `ChartModel<T>`

Series data lives in a [`ChartModel<T>`](../crates/teksilo-data/src/chart_model.rs)
— a concrete reactive multi-series chart data model in `teksilo-data`,
the same tier as `ListModel<T>` / `TreeModel<T>`. All three chart
widgets (`BarChart::new`, `LineChart::new`, `PieChart::new`) take a
`ChartModel<T>` directly; there is no `Prop<Vec<ChartSeries<T>>>` or
`Signal<Vec<ChartDatum<T>>>` binding path anymore — mutating the model
*is* the reactivity. Full mechanism reference:
[data-models.md §15](data-models.md).

`ChartModel<T>` is `Rc<RefCell<…>>` inside — cloning shares the same
series and points, and every clone receives the same change
notifications. Series live in a flat `SlotMap` arena keyed by
[`SeriesId`](../crates/teksilo-data/src/chart_change.rs) (a stable
handle, like `NodeId`) plus a separate `order: Vec<SeriesId>` for
display order. Every mutation method follows the mutate-then-notify
discipline (drop the borrow, then notify) and:

1. emits a [`ChartChange`](../crates/teksilo-data/src/chart_change.rs)
   describing exactly what changed (`SeriesInserted`, `SeriesRemoved`,
   `SeriesMoved`, `SeriesRenamed`, `SeriesColorChanged`,
   `SeriesVisibilityChanged`, `PointsInserted`, `PointsRemoved`,
   `PointUpdated`, `SeriesDataReplaced`, `Reset`) to every observer
   registered via `model.observe_changes(|change| …)`, and
2. bumps one of two `Signal<u64>` version counters the three chart
   widgets bind internally — see §8 for the full mapping.

`ChartSeries<T>` and `ChartDatum<T>` (the construction DTOs) now live
in `teksilo-data` alongside the model and are re-exported from
`teksilo_charts` for convenience:

```rust
pub struct ChartDatum<T> {
    pub category: T,        // x-axis position: String, enum, date, …
    pub value: f32,         // y-axis value (always f32)
}

pub struct ChartSeries<T> {
    pub name: String,
    pub color: Option<ColorProp>,    // None → palette assigns
    pub visible: bool,               // plain bool — see note below
    pub points: Vec<ChartDatum<T>>,
}
```

`ChartSeries::visible` is a **plain `bool`**, not a `Signal<bool>` —
unlike the pre-`ChartModel` shape, reactivity does not live on the
per-series DTO. `ChartSeries` only describes the *desired shape* of
one series at construction time (`ChartModel::from_series_vec`); once
a series is in the model, its visibility is toggled through
`ChartModel::set_series_visible(series, bool)`, which notifies
observers and bumps `structure_version()` like every other structural
change (§8).

Construction:

```rust
use teksilo_charts::{ChartDatum, ChartModel, ChartSeries};

// Multi-series (BarChart / LineChart):
let model = ChartModel::from_series_vec(vec![
    ChartSeries::new("Revenue").data(vec![
        ChartDatum::new("Q1".to_string(), 10.0),
        ChartDatum::new("Q2".to_string(), 20.0),
    ]),
    ChartSeries::new("Costs").data(vec![
        ChartDatum::new("Q1".to_string(), 5.0),
    ]),
]);

// Single anonymous series (PieChart's flat, one-dimensional path):
let pie_model = ChartModel::from_points(vec![
    ChartDatum::new("Storage".to_string(), 42.0),
    ChartDatum::new("Apps".to_string(), 18.0),
]);
```

Live updates mutate the model in place — no `.set()`, no vec swap:

```rust
let revenue = model.series_id_at(0).unwrap();
model.push_point(revenue, "Q3".to_string(), 30.0);   // structure_version bumps → chart relayouts
model.set_series_color(revenue, Color::from_hex("#0072B2")); // style_version bumps → repaint only
```

`T` is the **category / x-axis** type. Common choices: `String` for
human-readable labels, an `enum` for fixed buckets, `chrono::DateTime`
for time-series (the chart only requires `Display`). Numeric values
are always `f32`.

`ChartModel<T>` also underpins three companion types for the streaming
/ downsampling / selection cases — `ChartWindow<T>` (last-N-points
projection), `ChartAggregate<T>` (bucket/rollup projection), and
`ChartSelection` (point-level selection state). None of the three
chart widgets wire these in directly today; they're building blocks
for apps that need a strip-chart, a downsampled long series, or
click-to-select behavior on top of the same model. See
[data-models.md §15](data-models.md)
for the full API.

## 4. Axes — `nice_ticks` and formatting

[crates/teksilo-charts/src/axis.rs](../crates/teksilo-charts/src/axis.rs)
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

[`ChartPalette`](../crates/teksilo-charts/src/palette.rs) is the
mechanism that decides series colors when a series didn't pick its
own:

```rust
pub enum ChartPalette {
    FromTheme,             // reads theme.colors.chart_palette
    Custom(Vec<Color>),
}
```

Default is `FromTheme`, which reads
[`ColorTokens::chart_palette`](../crates/teksilo-tokens/src/theme.rs).
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

> **Inactive-window desaturation.** Like every other themed control,
> the chart palette dims when its window loses OS focus (see
> [window-activation.md](window-activation.md)). The paint walker
> swaps in
> [`ColorTokens::for_inactive_window`](../crates/teksilo-tokens/src/theme.rs),
> which desaturates `chart_palette` by
> `ColorTokens::INACTIVE_CHART_DESATURATION` (`0.35`) — deliberately
> **lighter** than `INACTIVE_ACCENT_DESATURATION` (`0.70`) used for the
> accent family. The Okabe-Ito sequence's whole purpose is inter-series
> hue separation; fully desaturating it like a single accent control
> would defeat that even in a background window. No per-chart code is
> needed — this falls out of the same theme-side swap every other
> control gets.

## 5.1 The non-colour channel — `SeriesPattern`

A palette answers *"are these colours distinguishable from one another?"*
Okabe-Ito answers it well. It does **not** answer WCAG **1.4.1 (Use of
Color)**, which is a different question: colour must not be the *only*
visual means of conveying information. A reader with monochrome vision, a
greyscale printout, a screen in direct sunlight, or a forced-colours
setting has no colour channel at all — and neither does a ninth series,
which used to repeat the first's colour exactly under the modulo wrap
described above.

So every series carries a second, orthogonal identity:
[`SeriesPattern`](../crates/teksilo-data/src/series_pattern.rs), a single
value that drives all three renderings a chart needs, so a series looks
like *itself* whether it is drawn as a line, a bar, a slice, or a legend
swatch:

| | line | marker | filled area |
| --- | --- | --- | --- |
| `Solid` | solid | circle | plain |
| `Dashed` | long dash | square | 45° hatch |
| `Dotted` | dotted | triangle | back-hatch |
| `DashDot` | dash-dot | diamond | cross-hatch |
| `ShortDash` | short dash | × | horizontal |
| `WideDash` | wide dash | + | vertical |

Six patterns against eight palette colours means the pair
`(colour, pattern)` does not repeat until the 24th series. A series with
no explicit pattern takes the one its position implies, so the channel
exists with no application code; pin one with
`ChartSeries::pattern(..)` or `ChartModel::set_series_pattern(..)` when a
series' identity must survive a reorder.

**When it is drawn** is
[`PatternPolicy`](../crates/teksilo-charts/src/pattern.rs), a builder on
all three charts (`.pattern_policy(..)`):

- **`Auto`** (default) — drawn once colour is actually doing
  identification work: from the second **plotted** series onwards for
  `BarChart` (so `BarGrouping::Single`, which draws one series however
  many the model holds, stays plain) and `LineChart`; for `PieChart`,
  when a **legend** is shown and there is more than one slice, since a
  pie's colour-to-category mapping lives in its legend. A chart showing
  one series has nothing to disambiguate, and hatching it would be
  decoration carrying no information.
- **`Always`** — draw it regardless. Use for consistency across a small
  multiple, where each panel holds one series but the set is read
  together. Note that series 0's pattern is `Solid`, so a single-series
  chart needs an explicit `.pattern(..)` for `Always` to be visible.
- **`Never`** — a deliberate accessibility regression, named plainly so
  it reads as one at the call site. Reach for it only where the design
  already carries the distinction some other way — direct series labels
  on the plot, or one series per chart.

**Legend swatches sample what the plot draws**, so there is no second
mapping to learn: `LegendSwatch::Block` (a hatched chip) for bars,
`LegendSwatch::Line` (a dashed sample with the marker at its centre) for
lines, `LegendSwatch::Marked` (a chip stamped with the marker glyph) for
pie slices. The charts set this themselves; a standalone `ChartLegend`
takes `.swatch(..)` and `.pattern_policy(..)` to match its chart.

**Why a pie gets a marker and a bar gets a hatch.** Hatches are parallel
strokes clipped to the region being filled, and the canvas clips to
rectangles only. A bar and a legend swatch are rectangles; a wedge is
not. Each slice therefore carries its pattern's marker glyph at its
centroid — in a tone derived from the slice's own fill so it contrasts
against any palette — and the matching legend swatch carries the same
glyph. Slices narrower than
`style::MIN_MARKED_SLICE_RAD` are skipped: a sliver cannot hold a glyph
without it spilling into its neighbours.

## 6. Legend

Two ways to use it:

**Embedded** — the chart instantiates `ChartLegend` internally when
constructed with `.legend(true)`, lays it out at `legend_position`
(`Top` / `Bottom` / `Leading` / `Trailing`), and shares the same
`ChartModel` and `palette` prop.

**Standalone** — build a [`ChartLegend`](../crates/teksilo-charts/src/legend.rs)
yourself and place it anywhere in your widget tree, sharing the
same `ChartModel` the chart binds to:

```rust
use teksilo_charts::{ChartLegend, ChartModel, LegendOrientation};

let model = ChartModel::from_series_vec(make_series());
let chart = LineChart::new(model.clone())
    .legend(false);                       // chart draws no legend
let legend = ChartLegend::new(model.clone())
    .orientation(LegendOrientation::Vertical);

VStack::new()
    .child(HStack::new().child(chart).child(legend))
```

**Interactive.** `ChartLegend::interactive(true)` (or the chart-level
`.legend_interactive(true)` on `BarChart` / `LineChart` — `PieChart`
does not expose it) turns every row into a real focusable/clickable
element (`Role::CheckBox`, click or Space toggles). Toggling a row
calls `ChartModel::set_series_visible(series, !visible)` directly —
there's no separate wiring; the legend mutates the same model the
chart reads. Default `false`.

Embedded legend orientation is auto-derived from position: `Top` and
`Bottom` get horizontal, `Leading` and `Trailing` get vertical.
Override with the standalone widget if you need something different.

## 7. Layout — proposal-driven plot-area carve

All three charts are **proposal-driven**: `layout_response` returns
whatever the parent proposes, with a 320×200 (line / bar) or 320×220
(pie) fallback when the proposal is unbounded. This matches
[`ProgressBar`](../crates/teksilo-widgets/src/progress_bar.rs) and
[`ScrollArea`](../crates/teksilo-widgets/src/scroll_area.rs) — charts
fit any container.

Inside `paint`, the bounds are carved into a plot rect by
[`carve_plot_area`](../crates/teksilo-charts/src/layout.rs), which:

1. Reserves the legend band on the requested edge (when shown).
2. Reserves a y-axis band on the leading edge: max tick label
   width + tick length + gap + axis-title height (when applicable).
3. Reserves an x-axis band on the bottom edge: tick label height +
   tick length + gap + axis-title height.
4. Insets the inner plot by `plot_padding_*` from the dimension
   constants in
   [crates/teksilo-charts/src/style.rs](../crates/teksilo-charts/src/style.rs)
   (not to be confused with the Tier-3 `ChartStyle` *trait* — §11 below
   — which carries paint recipes, not dimensions).

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

Every chart binds to its `ChartModel<T>`'s two version signals — see
§3 and [data-models.md §15](data-models.md)
for what bumps which. The mapping is deliberately coarse: **only a
series color change is paint-only** — everything else that can mutate
a model (including a visibility toggle, which shifts the auto
y-domain and bar widths) goes through `structure_version` and is a
full `Relayout`.

| Change | Model signal | Binding level | Why |
|---|---|---|---|
| Series add/insert/remove/move/rename | `structure_version` | `Relayout` + `AccessibilityOnly` | Y-domain, tick positions, and label widths may all shift; the per-datum AT mark list must also refresh |
| Point push/insert/remove/update, `replace_series_data`, `clear` | `structure_version` | `Relayout` + `AccessibilityOnly` | Same — any point-shape change can move the domain |
| `set_series_visible` | `structure_version` | `Relayout` + `AccessibilityOnly` | Visible set changes the auto y-domain and bar widths, not just paint |
| `set_series_color` / `clear_series_color` | `style_version` | `RepaintOnly` | Geometry unchanged — this is the **only** `ChartChange` variant that doesn't bump `structure_version` |
| Hover state (private `Signal<Option<(SeriesId, usize)>>`, all three charts) | — | `RepaintOnly` | Marker + tooltip only |
| Theme change | — | Auto via tree-wide `mark_all_dirty` | Colors/fonts re-resolved on next paint |
| `Prop<ChartPalette>` change | — | `RepaintOnly` | Color-only |
| PieChart `inner_radius_ratio` change | — | `Relayout` | Center-slot inscribed-square size depends on it |

The wiring lives in each chart's `build()` (`BarChart` shown; `LineChart`
/ `PieChart` follow the same shape):

```rust
fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
    let id = ctx.self_id();
    let registry = ctx.binding_registry();
    // Data swap → relayout (y-domain might shift) AND the AT mark
    // list must refresh.
    self.model.structure_version().bind_to(id, registry, BindingLevel::Relayout);
    self.model.structure_version().bind_to(id, registry, BindingLevel::AccessibilityOnly);
    // Color-only swap → repaint.
    self.model.style_version().bind_to(id, registry, BindingLevel::RepaintOnly);
    self.palette.register_if_bound(id, registry, BindingLevel::RepaintOnly);
    self.hover.bind_to(id, registry, BindingLevel::RepaintOnly);
    // ...
}
```

For widgets that bind via per-series `ColorProp::Bound(signal)` (a
series' `color` field holding a live `Signal<Color>` rather than a
static value), the chart palette stays untouched and the color signal
triggers a repaint without a full relayout — same effect as
`set_series_color`, driven from outside the model. This is the right
path for "pulsing" / "highlighted" colors that don't change geometry.

## 9. Hover tooltips

All three charts — `BarChart`, `LineChart`, `PieChart` — draw their
hover tooltips **inline inside their own `paint()`**, clipped to the
plot rect. This is deliberately different from
[`TooltipWidget`](../crates/teksilo-widgets/src/tooltip.rs):

- Chart tooltips track the cursor across the plot to the **nearest
  data point**, snapping per-pixel. `TooltipWidget` is anchored to a
  widget bounds box.
- Chart tooltips appear instantly. `TooltipWidget` waits ~700 ms for
  dwell.
- The content depends on which point is nearest, which can change
  within the same widget without an enter/leave event.

The implementation is straightforward:

1. The chart owns a private hover signal —
   `Signal<Option<(SeriesId, usize)>>`, the same `(series, point
   index)` shape across all three chart kinds — bound at
   `BindingLevel::RepaintOnly`.
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
[`pie_hit_test_uses_logical_angle_space`](../crates/teksilo-charts/src/pie_chart.rs)
test locks this.

Disable with `.hover_tooltip(false)` if you'd rather the chart not
react to hover at all (e.g. embedded in a tooltip itself, or behind
a busy overlay). A clone of the hover signal is also public via
`.hover_signal() -> Signal<Option<(SeriesId, usize)>>` on each chart,
for apps that want to observe hover from outside without
re-implementing the hit-test.

`ChartSelection` ([teksilo-data](../crates/teksilo-data/src/chart_selection.rs),
keyed by `(SeriesId, usize)`) is consumed by all three charts the
same way: `.selection(ChartSelection)` reuses the exact hit-test the
hover handler uses (`hit::rect_hit` / `hit::nearest_point` /
`hit::slice_hit`) to add click-to-select — a tap on a mark selects it
(Ctrl/Cmd-click toggles it in `SelectionMode::Multi`), a tap on empty
space clears the selection — and every selected mark paints an
accent-colored highlight (a bar's outline, a line point's ring, a
slice's outline) on top of its normal fill; see
[data-models.md §15.4](data-models.md).

## 10. Theming — chart style constants

[`crates/teksilo-charts/src/style.rs`](../crates/teksilo-charts/src/style.rs)
carries chart-specific dimension constants: padding (`PLOT_PADDING_TOP`,
`PLOT_PADDING_RIGHT`, `PLOT_PADDING_BOTTOM`, `PLOT_PADDING_LEADING`),
tick lengths, label gaps, gridline width, default line / point sizes,
legend swatch and item gaps, tooltip padding, and the four pie-related
constants (`PIE_PADDING`, `PIE_LABEL_GAP`, `PIE_LEADER_LENGTH`,
`PIE_MIN_SLICE_LABEL_DEGREES`, `DONUT_DEFAULT_INNER_RATIO`).

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
a theme tightening density can change the `PLOT_PADDING_*` constants
in `teksilo-charts/src/style.rs` without touching colors.

## 11. Styling — the `ChartStyle` trait

Charts sit on the same Tier-3 styling ladder as every other themable
widget (see [styling-system.md](styling-system.md)) via
[`ChartStyle`](../crates/teksilo-core/src/styles/chart_style.rs), a
trait in `teksilo-core::styles`:

```rust
pub struct ChartFillContext<'a> {
    pub series_index: usize,
    pub resolved_color: Color,     // palette / per-series color, already resolved
    pub theme: &'a Theme,
}

pub trait ChartStyle: 'static {
    fn bar_fill(&self, cfg: &ChartFillContext) -> FillRecipe;
    fn area_fill(&self, cfg: &ChartFillContext, opacity: f32) -> FillRecipe;
    fn donut_fill(&self, cfg: &ChartFillContext) -> FillRecipe;
    fn gridline(&self, theme: &Theme) -> BorderRecipe;
}
```

Unlike every other Tier-3 trait, `ChartStyle` is **all-recipe** — four
methods returning plain-data `FillRecipe` / `BorderRecipe` (Tier 2),
none returning `WidgetId`. Charts paint via `Canvas` calls inside their
own `paint()` rather than composing child widgets, so there's no
`make_*(cfg, ctx) -> WidgetId` step to hook into; the recipe is
resolved once per fill/stroke and painted directly. This is a
different trait *shape* from the widget world's `make_body` traits
and from the multi-method traits (`TabStyle`, `DialogStyle`,
`TableStyle`, `CalendarStyle`) that still return `WidgetId`s from
several named slots — `ChartStyle` returns data from all four.

**Resolution chain**, same precedence as every other themable widget:

```
per-call .style(impl ChartStyle)  >  theme.style_slots.chart  >  RecipeChartStyle::default()
```

`BarChart` / `LineChart` / `PieChart` all expose
`.style(impl ChartStyle) -> Self`. The theme-wide slot is
`theme.style_slots.chart: Option<Rc<dyn ChartStyle>>`
(`SharedChartStyle`).

**Layering note:** `RecipeChartStyle`, the shipped default, lives in
**`teksilo-charts` itself, not `teksilo-widgets/src/styles/*`** — the
one place this default breaks the convention every other `Recipe*Style`
follows (see §1 and [styling-system.md](styling-system.md)). The
reason is layering, not oversight: `teksilo-charts` deliberately does
not depend on `teksilo-widgets`, so its default style implementation
has to live where its dependencies already reach. `teksilo-core` only
holds the trait and the `Rc<dyn ChartStyle>` slot type — it has no
opinion on where the default lives.

`RecipeChartStyle` reproduces the flat-color chrome charts always
painted before Tier-3 styling landed: `bar_fill` / `donut_fill`
resolve to `FillRecipe::Solid(cfg.resolved_color)`, `area_fill` is the
same solid color at the caller-given opacity, and `gridline` is a
`BorderRole::Default`-at-40%-alpha solid `BorderRecipe`.

**Dashed gridlines.** `gridline()`'s returned `BorderRecipe` carries a
`BorderStyle` (`Solid` by default in `RecipeChartStyle`), so a custom
`ChartStyle` can theme-wide switch every chart's gridlines to
`BorderStyle::Dashed { dash, gap }`. For a one-chart override without
writing a whole `ChartStyle`, `AxisConfig::gridline_dash(dash, gap)`
sets a per-axis dash pattern that **wins** over the style's gridline
recipe. Gridlines are drawn via `Canvas::stroke_path` (Tier 3) rather
than the faster `draw_line` (Tier 1), because `draw_line` doesn't
honor dash patterns.

**Gradient area / donut fills.** `area_fill` and `donut_fill` can
return `FillRecipe::LinearGradient { .. }` / `FillRecipe::RadialGradient
{ .. }` instead of `Solid` — a custom `ChartStyle` is the only way to
opt in (`RecipeChartStyle` stays flat). Gradient fills route through
the same two recipe methods plus
[`Canvas::fill_path(path: &Path, paint: impl Into<Paint>)`](../crates/teksilo-canvas/src/canvas.rs)
(widened from a flat-color-only signature) and a new Tier-3
path-gradient GPU pipeline (`path_gradient.wgsl`). Radial gradients on
a donut are continuous across wedge boundaries (the gradient is
defined once over the whole disc, not re-evaluated per slice); a
linear gradient across a donut is a documented edge case — it reads
correctly per-wedge but the seam between wedges isn't a straight
gradient line the way a radial one is, so radial is the natural choice
for donut fills.

## 12. Accessibility

Each chart declares `Role::GraphicsDocument` with a name that
describes the shape (`"Bar chart: 3 series, 4 categories"`,
`"Line chart: 2 series, 12 points"`, `"Pie chart: 5 slices"`).

**Per-datum AT nodes.** Every visible bar / line point / pie slice is
also its own synthetic child node — `Role::GraphicsObject`, name
`"{series name}, {category}: {value}"`, and `numeric_value` set to the
datum's `f32` value — emitted via
[`hit::emit_mark_node`](../crates/teksilo-charts/src/hit.rs) under
`SyntheticKind::ChartMark` (the same synthetic-child mechanism
`teksilo-scene` uses for lightweight scene items). Node ids are
deterministic within a process run, derived from `(SeriesId, usize)`
via `DefaultHasher`, so a mark keeps the same AT id across repeated
`accessibility()` walks. Apps that need full data-table semantics
(sortable columns, cell-level navigation) should still mirror the
chart with a `TreeView` / `TableView` next to it — the per-datum marks
give a screen reader a way to inspect individual values, not a
substitute for tabular navigation.

## 13. Limits and explicit follow-ups

Closed since the initial five-PR cycle: BarChart hover tooltips,
interactive legends, per-datum accessibility nodes, the styling
ladder gap (`ChartStyle`, §11), and `ChartSelection` click-to-select
are all now implemented — see §5 (inactive-window desaturation), §6
(interactive legend), §9 (BarChart tooltip + selection), §11
(`ChartStyle`, dashed gridlines, gradient fills), and §12 (per-datum
AT nodes) above. The flat-fill limit is closed as an **opt-in**:
`RecipeChartStyle` stays flat by default (visual parity with every
chart drawn before Tier-3 styling landed) — gradients and dashed
gridlines require installing a custom `ChartStyle` or setting
`AxisConfig::gridline_dash`.

Still genuinely open:

- **No stacked bars.** Single + grouped only. Stacked needs its own
  legend + hit-test pass for the sub-bar; deferred.
- **No log axis.** `nice_ticks` is linear-only.
- **No time-axis formatters.** `T = chrono::DateTime` works
  structurally (the chart only needs `Display`), but tick generation
  doesn't snap to month/quarter/year boundaries. Deferred.
- **No animation on data change.** A model mutation (`push_point`,
  `set_series_visible`, …) relayouts/repaints instantly — there's no
  `animate_to` integration on bar height / line position / slice angle
  transitions yet.
- **Pie / donut hover for BarChart-style "follow the cursor across
  multiple slices."** The handler exists but the visual treatment
  matches Excel's "highlight one slice" — no slice-pull-on-hover yet.
- **Linear gradient on a donut is a documented edge, not a bug.** See
  §11 — reach for a radial gradient on a donut; a linear gradient
  reads correctly per-wedge but has a visible seam across wedge
  boundaries.
- **No chart widget wires `ChartWindow` / `ChartAggregate`
  internally.** Both remain `teksilo-data` building blocks (§3, and
  [data-models.md §15](data-models.md)) an app composes on top of a
  `ChartModel` for a strip-chart or a downsampled long series.
  `ChartSelection` is the one exception — see §9 — all three charts
  consume it directly via `.selection(ChartSelection)`.

For each of these, the file pattern in
[crates/teksilo-charts/src/](../crates/teksilo-charts/src/) is the place
to look — the modules are intentionally split so future work lands
in one or two files at most.

## 14. Demo

[examples/chart_demo](../examples/chart_demo/src/main.rs) ships all
three charts in one window, built throughout on the current
`ChartModel<T>` API — `ChartModel::from_series_vec` /
`ChartModel::from_points` construction plus in-place mutation
(`replace_series_data`, `push_point`, §3) — with no wholesale
`Signal<Vec<ChartSeries<T>>>` swap anywhere in the demo. Run with:

```
cargo run -p chart-demo
```

What it shows, end to end:

- **Chart-kind switcher.** A `SegmentedControl` ("Bars" / "Lines" /
  "Donut") drives a `Switcher` between the three panels. Bar and Line
  share one series `ChartModel<String>` — constructed once, cloned
  into both chart widgets, the same sharing pattern `ChartModel::clone()`
  gives for free (§3) — and one `ChartSelection`, so switching
  between the two panels keeps the highlighted point selected. The
  donut consumes a second, single-series `ChartModel<String>`.
- **Default / Gradient theme toggle.** A second `SegmentedControl`
  drives a `Switcher` between the shipped flat `RecipeChartStyle` and
  a demo-defined `GradientChartStyle` (§11): a vertical bar-fill
  gradient, a top-to-bottom area-fill gradient fading toward the
  baseline, a continuous radial donut gradient, and dashed gridlines
  via `ChartStyle::gridline`.
- **Interactive legend (§6).** Both the Bar and Line panels embed a
  `.legend_interactive(true)` legend — clicking (or pressing Space on
  a focused) row toggles that series' visibility live.
- **BarChart hover (§9, §4).** Hovering a bar shows the shared
  tooltip card, snapping to the nearest bar.
- **Click-to-select (§9, §2, [data-models.md §15.4](data-models.md)).**
  All three charts are wired with `.selection(ChartSelection)`:
  clicking a bar, line point, or donut slice paints an accent
  highlight on it and clicking empty space clears the selection. The
  donut's center slot reads the pie's own
  `ChartSelection::selection_signal()` directly and shows the
  selected category plus its share of the total, falling back to
  "Total" plus the full sum when nothing is selected — real slice
  interaction, no button-chip stand-in.
- **"Refresh data" button.** Re-seeds the pseudo-random series and
  calls `ChartModel::replace_series_data` per series (Bar/Line model)
  and per point (pie model) — an in-place data swap, not a rebuild.
- **Live strip-chart pane (§3, [data-models.md §15](data-models.md)).**
  A `LiveStripPane` widget appends one point every tick (via a
  periodic frame-tick timer) to an unbounded history `ChartModel<u32>`,
  then projects its tail through a `ChartWindow<u32>` ("last N
  points"). Since chart widgets bind to a `ChartModel`, not a
  `ChartWindow` projection directly, the window's current tail is
  materialized each tick into a small render-bound `ChartModel` the
  `LineChart` actually consumes — an honest bridge given that
  constraint. Reduced-motion builds the (empty) chart but skips the
  timer.

Useful as a sanity-check after any change to teksilo-charts;
`cargo test -p teksilo-charts` (88 headless tests, no GPU) is the
faster CI path.
