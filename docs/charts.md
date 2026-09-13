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
readout card, the legend, the donut center placeholder all live inside
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

Polyline per series with optional area fill, the shared datum readout
(§9), and an embedded legend. PR-3 / PR-4 territory.

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

**Interactive rows as targets.** A row *paints* a 10 dp swatch beside an
11 pt label — about 13 dp tall — and it paints that at every density.
An interactive row is also a real target, and 13 dp is under the 24 dp
WCAG 2.2 SC 2.5.8 floor, so `legend_row_extent` gives an interactive
legend's rows the density's `target_size` at `Comfortable` (32 dp) and
`Touch` (44 dp), reserving the band at the same extent so the rows are
not clipped by their own container. A non-interactive legend has no
target and is never grown.

At `Compact` the row stays at what it paints, and **that is a recorded
shortfall rather than a fixed one**: giving it 24 dp would be a Compact
layout change, which the density work does not make. Neither of the
framework's two hit-widening mechanisms can reach it where it sits —
`Widget::hit_outset` never escapes its parent, and the parent is a band
reserved at exactly one row's extent (in a vertical legend the rows are
contiguous besides, so an outset could only take space from a
neighbour); and the miss-only slop pass returns the exact hit as soon as
the bubble path has an owner at distance zero, which a chart with a
readout always provides. So the only mechanism is more room, and more
room at Compact is a layout change. An app that needs the floor at
Compact raises the whole UI to `Comfortable`, or places a standalone
legend outside the chart where it is not inside a pointer-handling
ancestor.

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

## 9. The datum readout

All three charts — `BarChart`, `LineChart`, `PieChart` — draw the same
**readout**: a tooltip card plus a highlight on the datum it describes,
painted **inline inside their own `paint()`** and clipped to the plot
rect. This is deliberately different from
[`TooltipWidget`](../crates/teksilo-widgets/src/tooltip.rs):

- The readout tracks the pointer across the plot to the datum under or
  nearest to it, changing content without an enter/leave event.
  `TooltipWidget` is anchored to a widget's bounds box.
- The readout appears with the pointer. A plain `TooltipWidget` waits
  `motion.tooltip_delay` (500 ms; the rich and composite tiers wait
  `motion.tooltip_delay_heavy`, 700 ms).
- It is not hover-gated. All three charts read
  `WidgetEvent::PointerMove` through `on_pointer_event` rather than
  `on_hover`, and a contact's bare move is dispatched to its hit target
  like any other — which is why a finger could always *raise* the
  readout, and why what it could not do was retire it.

The pieces, all in [`hit.rs`](../crates/teksilo-charts/src/hit.rs):

1. One `Signal<Option<MarkKey>>` per chart — `MarkKey` is
   `(SeriesId, usize)`, the same shape across all three kinds — bound at
   `BindingLevel::RepaintOnly`. Every input route writes that one
   signal, so the pointer, the keyboard and an assistive technology
   cannot disagree about what is being read.
2. `drive_readout` owns the pointer routes and `drive_readout_keys` the
   keyboard ones. The three chart kinds differ only in how a point
   resolves to a mark: `rect_hit_within` for bars, `nearest_point` for a
   line, `slice_hit_within` for a pie.
3. `paint()` reads the signal and draws the highlight (a ring plus a
   filled dot for a line point, the wedge stroke for a slice, nothing
   extra for a bar) plus the card.
4. `mark_tooltip_rect` places the card: centred over its anchor, above
   it by a gap, **flipped below** rather than clipping the plot's top
   edge, then clamped inside the plot on both axes. It is a pure
   function, so the placement is tested without a canvas.

The mark snapshot is replaced, not appended, each paint, so a data
change shrinks the index correctly. Hit-test cost is O(N×S) per pointer
sample for N points across S series — acceptable up to ~10k points
without optimisation.

### Kind-aware rules, and who retires the readout

A finger has no way to leave. A precise pointer sets the readout as it
moves and clears it by moving off the plot or leaving the widget, which
is what `PointerLeave` is for — and **a contact never receives a
`PointerLeave`** anywhere in the framework: the dispatch sites are all
reached only for the hover owner, which a contact can never be. So a
readout a finger raised would have stayed up for the rest of the
program's life. That is the defect rows 8-10 of
[the hover-affordance census](hover-affordance-census.md) record, and it
is a defect in the retire path, not in the set path.

A coarse pointer therefore gets two modes, and both of them end:

| gesture | what the readout does |
| --- | --- |
| press | appears at once on the datum under the contact — a still finger produces no move, so the press has to raise it |
| press, then travel | follows the contact, and the **lift retires it** |
| press and release without travelling | **pins** on the datum under the release, re-anchored on the mark now the finger has gone, and announced once |
| a later press away from the data | retires a pinned readout |
| cancel | retires it |

A precise pointer keeps the behaviour it had: it neither pins nor
announces, its card stays anchored on the mark rather than on the
cursor, and a click over a datum leaves the readout exactly as the move
left it. **One deliberate change reaches every kind**: a
`WidgetEvent::PointerCancel` now retires the readout, where the raw
handler used to ignore it. A revoked interaction is not an inspection,
and the framework's cancel taxonomy asks for state to be unwound rather
than left standing; a mouse would have cleared it on its next move
anyway, so the change costs a mouse nothing and is what gives a contact
its third retire path.

While a coarse pointer owns the readout the card is anchored on the
**contact** rather than on the mark, and the gap grows by half of
`teksilo_core::overlay::ASSUMED_CONTACT_PATCH` — the card is painted
inline by the chart, so it never passes through the overlay layer's own
contact avoidance and has to clear the finger itself.

Two things the readout deliberately does **not** do:

- **No timeout.** Nothing on `EventContext` reports the clock and a
  chart runs no timer of its own; with press, release and cancel all
  owning a retire path, a timeout could only ever fire against a
  readout the user is still reading.
- **No `on_long_press`.** A hold is not needed to raise the readout, and
  installing a widget-level long press would take the tree-owned hold
  route away from the chart *and its whole subtree* — precedence rule 1
  in [`touch_route`](../crates/teksilo-core/src/widget_tree/touch_route.rs)
  is that a widget's own `on_long_press` wins — so an app's
  `.tooltip(..)` on a chart would stop answering to a hold.

**One known limitation.** A cancel is routed to the pointer's captor,
or failing that to a widget that answered `Handled` to one of its
positional events. The readout claims nothing on purpose — it returns
`Ignored` so a pan claim or a tap above the chart still works — so a
**read-only** chart (no `.selection(..)`) is never told about a cancel,
and its readout survives the revoked press until the next one. A
selectable chart's own tap recognizer holds the pointer, so it is told.
Both halves are pinned by tests in `bar_chart.rs`.

### Coarse hit tolerance

A press that misses a mark by a few dp is a miss a finger cannot avoid,
so each chart's own hit test admits one — sized by
`hit::mark_tolerance`, which is the pointer's hit-slop radius from
`HitSlop::for_pointer`: **0 dp for a mouse**, 2 for a pen, 8 for a
finger — and the density's `slop_budget` caps only the **coarse** figure,
because the budget describes a contact patch a precise pointer does not
have. Because the mouse's is
zero by arithmetic rather than by a branch, every tolerance-aware hit
test below is the identity for a mouse.

- **Bars** (`rect_hit_within`) test containment first, so a press inside
  a bar answers exactly what a zero tolerance answers. Only on a miss
  does the **nearest** bar within the tolerance take it — nearest rather
  than first, because inflating every bar makes adjacent inflated
  rectangles overlap, and a first-match scan through an overlap resolves
  by position in the mark vector (i.e. by series and category order)
  regardless of which bar the press was closer to.
- **A line chart** already picks the nearest point with no radius
  cutoff, so its marks need no tolerance. What a coarse pointer gains is
  the plot boundary: a press a few dp outside it, beside the first or
  last point, reads as an inspection rather than as a miss.
- **Pie and donut wedges** (`slice_hit_within`) grow a **radial**
  tolerance only, so a press just past the outer arc — or just inside a
  donut hole — reaches the wedge whose bearing it falls in. There is no
  angular tolerance, and that is not an omission: inside the disc some
  slice owns every bearing already, so an angular tolerance would widen
  nothing and would make two adjacent slices both claim their shared
  boundary, with the first in paint order winning it at every radius.
  A press whose bearing lies in a neighbour's sweep reaches the
  neighbour however close to the boundary it falls.

The same tolerance is spent by the readout and by the selecting tap, in
one shared call — a wedge that highlighted near its edge but refused to
select there would be two hit tests wearing one name.

**Why not `Widget::hit_distance` or `Widget::hit_shape` on a wedge.**
Neither hook can speak for a wedge, and one of them would break a
working mouse behaviour:

- A wedge has no `WidgetId`. All of a chart's marks are paint regions
  inside the one chart node, recomputed each paint, and the framework's
  miss-only slop pass produces a *node* id — so it can never name a
  wedge.
- The slop pass would not run over a chart in any case. A node earns an
  outset of `((target_size − min(w, h)) / 2)`, which is exactly zero for
  a chart at any realistic size (a pie's default ideal is 320×220), and
  a press *inside* the chart's rectangle is not a miss: the pass returns
  the exact hit as soon as the bubble path has an owner at distance
  zero, and a chart with a readout always installs a pointer handler.
- A `hit_shape` restricted to the annulus would make the corner press in
  `tap_outside_ring_clears_selection` — a **mouse** test — miss the chart
  altogether, so the handler that clears the selection would never run.
  It would also remove the only pointer route for clearing a chart
  selection.

So the wedge's tolerance lives in the chart's own polar code, which is
where the geometry is.

### Keyboard and assistive technology

A chart is **focusable** exactly when it has something for the keyboard
to do — a readout to move (`hover_tooltip`, on by default) or a
selection to commit. A chart with neither adds no tab stop. While
focused:

| key | effect |
| --- | --- |
| `→` / `↓` | next datum |
| `←` / `↑` | previous datum |
| `Home` / `End` | first / last datum |
| `Enter` / `Space` | select the focused datum, where the chart has a `ChartSelection` |

Both axes step one sequence — the paint-order mark vector, series-major
and point-minor — because a chart's marks are not a grid: a pie has one
ring, a grouped bar chart one bar per (series, category) pair, a line
chart points per series. That is the only ordering all three share, and
it is the order the AT tree publishes its per-datum nodes in, so the
keyboard and a screen reader's own review agree. Traversal clamps at
both ends rather than wrapping. A chord carrying any modifier is left
alone, so nothing here can shadow a registered shortcut.

Each step moves the readout to the focused datum and announces it, and
the chart paints a **focus ring** on it — outside the selection
highlight, so a datum that is both focused and selected reads as both.
The ring is why the tab stop is not a WCAG 2.4.7 failure, and it is
painted only while the chart actually holds focus.

The focused datum is published as the chart node's
`active_descendant` (roving virtual focus, the same pattern
`teksilo-scene` uses), which is what lets a screen reader follow the
traversal without the marks needing arena nodes of their own.

Every per-datum AT node advertises the two actions a datum can answer,
and the chart implements both — see §12.

### touch-action

Charts stay at `TouchAction::AUTO`. A chart consumes no drag: it taps,
it reads moves and it holds. Declaring `NONE` would make a 400×200 tile
a dead zone for a page scroll, and would additionally resolve
`DragActivation::Auto` to `Immediate` for everything on the press's
path — destroying the hold-then-scrub behaviour the readout depends on.
A test pins the decision.

### Turning it off, and observing it

`.hover_tooltip(false)` stops the chart raising a readout (embedded inside
a tooltip itself, or behind a busy overlay). It does not stop
`.selection(..)`'s tap: the two are gated independently, so a chart with a
selection still selects on a click. On a chart with no selection it does
remove the last thing the keyboard could do, and so also takes the chart
out of the tab order. A clone of the
readout signal is public as `.hover_signal() -> Signal<Option<(SeriesId,
usize)>>` on each chart, for apps that want to observe it without
re-implementing the hit test. Note what it now reports: the readout key,
whichever route set it — pointer, keyboard, or an assistive-technology
click.

`ChartSelection` ([teksilo-data](../crates/teksilo-data/src/chart_selection.rs),
keyed by `(SeriesId, usize)`) is consumed by all three charts the same
way: `.selection(ChartSelection)` reuses the exact hit test the readout
uses to add click-to-select — a tap on a mark selects it (Ctrl/Cmd-click
toggles it in `SelectionMode::Multi`), a tap on empty space clears the
selection — and every selected mark paints an accent-coloured highlight
(a bar's outline, a line point's ring, a slice's outline) on top of its
normal fill; see [data-models.md §15.4](data-models.md).

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

`ChartStyle` is one of the few **all-recipe** Tier-3 traits — four
methods returning plain-data `FillRecipe` / `BorderRecipe` (Tier 2),
none returning `WidgetId`. `GridViewStyle` and `TextSelectionStyle`
have the same shape, for the same reason. Charts paint via `Canvas` calls inside their
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

**Actions on a datum.** Each mark advertises exactly the two actions the
chart implements, because an advertised action nothing implements is
worse than a missing one — the only way to discover it does nothing is
to invoke it.

| action | what the chart does |
| --- | --- |
| `Click` | moves the readout and the focused datum to that mark, and selects it where the chart has a `ChartSelection`. It is the assistive-technology equivalent of tapping the mark, which no screen-reader user can aim at otherwise — all the marks are one node to the hit test. |
| `ScrollIntoView` | moves the readout to that mark and asks any scroll container **above** the chart to reveal the mark's own rectangle. Meaningful because a chart is often a tile on a scrolling page, even though nothing scrolls inside the plot. |

Routing needs no new plumbing: the framework resolves a synthetic node
to its owning widget, and the chart recovers which mark the node named
by recomputing the ids over its live mark vector
(`hit::mark_for_node` — `mark_element_id` hashes its inputs, so the map
is only invertible that way).

**Roving virtual focus.** The chart keeps the real arena focus and
publishes the keyboard-focused datum as its `active_descendant`, so a
screen reader follows arrow-key traversal (§9) without the marks needing
arena nodes of their own.

**Emitted bounds are logical.** The device scale factor is applied once,
by the root window node's transform, so no chart multiplies a rect by
it — a test in `bar_chart.rs` fails if one starts to.

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
- **An interactive legend row is under the 24 dp target floor at
  `Compact`** — see §6 for the measurement and for why neither
  hit-widening mechanism reaches it. Fixing it at Compact means changing
  Compact layout, which is a decision this crate does not get to take
  alone.
- **A cancel does not reach a read-only chart**, so a revoked press
  leaves its readout up until the next press — see §9. Closing it needs
  either a way for a widget to observe a pointer's end without claiming
  its events, or a chart claiming events it has no other reason to
  claim.
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
- **BarChart readout (§9, §4).** Pointing at a bar shows the shared
  readout card, snapping to the nearest bar; a finger presses, scrubs and
  pins, and the keyboard arrows through the data.
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
