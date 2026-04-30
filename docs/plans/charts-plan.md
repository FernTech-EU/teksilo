# Charts Plan — `fern-charts` crate

## 1. Scope

A new dedicated crate housing three widgets: `BarChart`, `LineChart`, and
`PieChart` (with a donut variant exposed via builder method). Extends
[docs/plans/widgets-plan.md](widgets-plan.md) Gap 3.6.2 — pie/donut were
originally excluded but pulled back in because they're the one chart type
desktop-app users routinely expect.

**Out of scope:** scatter, heatmap, candlestick, radar, sankey, sunburst,
treemap. These are visualization-toolkit territory, not desktop-app
territory. Hard line — keeps the crate focused.

Both widgets render through the existing Canvas API (`fill_path`,
`stroke_path`, `fill_rect`, `draw_text`, `draw_line`) and the SDF/PathAtlas
pipelines. No new rendering primitives.

---

## 2. Crate setup

**Location:** `crates/fern-charts/`

**Dependency tier:** `tokens + canvas + core + data + i18n → charts`. Sits at
the **same layering tier** as `fern-widgets`, not on top of it. Apps depend
on `fern-charts` directly when they need charts. This deliberately avoids
pulling the ~54-widget catalog into chart-only consumers and leaves room
for `fern-widgets` to depend on `fern-charts` later if a status-strip
sparkline ever wants it.

**`Cargo.toml`:**

```toml
[package]
name = "fern-charts"
edition = "2024"
version.workspace = true
license-file.workspace = true
publish.workspace = true
authors.workspace = true
repository.workspace = true

[features]
preview = ["dep:fern-preview"]

[dependencies]
fern-core    = { workspace = true }
fern-data    = { workspace = true }
fern-tokens  = { workspace = true }
fern-canvas  = { workspace = true }
fern-i18n    = { workspace = true }
fern-preview = { workspace = true, optional = true }
```

Does **not** depend on `fern-widgets`. The hover tooltip is drawn inline by
the chart itself (see §6) — no need for `TooltipWidget`.

**Workspace registration:**

- Workspace root [Cargo.toml](../../Cargo.toml) already uses
  `members = ["crates/*"]` glob — no member edit needed.
- Add to `[workspace.dependencies]`:
  `fern-charts = { path = "crates/fern-charts", version = "0.1.0" }`.
- Re-export from `fern-ui` umbrella behind a `charts` feature flag so apps
  can `use fern_ui::charts::BarChart;` (mirrors how `rich-text` is gated).

**Module layout** (no `mod.rs`, per coding conventions):

```
crates/fern-charts/src/
  lib.rs          // public re-exports + crate doc
  series.rs       // ChartSeries<T>, ChartDatum<T>
  bar_chart.rs    // BarChart
  line_chart.rs   // LineChart
  pie_chart.rs    // PieChart (pie + donut, with center slot)
  legend.rs       // ChartLegend (standalone widget) + inline draw helper
  axis.rs         // AxisConfig, nice_ticks
  palette.rs      // Okabe-Ito default + theme-driven resolution
  hover.rs        // hit-test helpers + inline tooltip painter
  layout.rs       // shared plot-area math (margins, axis space reservation)
```

Tests live inline in each module under `#[cfg(test)] mod tests`, matching
[crates/fern-widgets/src/progress_bar.rs](../../crates/fern-widgets/src/progress_bar.rs)
— **not** in `tests/`. Convention in this codebase.

---

## 3. Public API surface

### Data model (`series.rs`)

```rust
/// One numeric value at a category/x-axis position.
pub struct ChartDatum<T> {
    pub category: T,      // x-axis position: String, date, enum, …
    pub value: f32,       // y-axis value
}

/// One named series of data points with an optional explicit color.
pub struct ChartSeries<T> {
    pub name: String,
    pub color: Option<ColorProp>,    // None → palette assigns
    pub visible: Signal<bool>,       // toggleable from a legend
    pub data: Vec<ChartDatum<T>>,
}

impl<T> ChartSeries<T> {
    pub fn new(name: impl Into<String>) -> Self;
    pub fn color(mut self, color: impl Into<ColorProp>) -> Self;
    pub fn data(mut self, data: Vec<ChartDatum<T>>) -> Self;
    pub fn push(&mut self, category: T, value: f32);
    pub fn visibility(mut self, signal: Signal<bool>) -> Self;
}
```

`T` is the **category/x-axis** type — typically `String`, an enum, or a
date type later. `value` is always `f32`. This shape lets line charts host
sparse per-series x positions (each series carries its own categories) and
bar charts treat the union of categories as the x-axis (or accept that
all series share the same categories — simplest case).

The unit of binding is `Prop<Vec<ChartSeries<T>>>` (static or
`Signal<Vec<ChartSeries<T>>>`). Whole-vec replace on update — chart
datasets are small enough that an incremental `ChartListModel` analog
isn't worth it. Revisit if measurable.

### Bar chart (`bar_chart.rs`)

```rust
pub enum BarOrientation { Vertical, Horizontal }
pub enum BarGrouping    { Single, Grouped }   // Stacked deferred

pub struct BarChart<T: Clone + 'static> {
    series: Prop<Vec<ChartSeries<T>>>,
    orientation: BarOrientation,
    grouping: BarGrouping,
    show_value_labels: bool,
    show_grid: bool,
    show_legend: bool,                       // baked-in inline legend
    legend_position: LegendPosition,
    axis_x: AxisConfig,
    axis_y: AxisConfig,
    palette: Prop<ChartPalette>,             // overridable per-chart
    bar_corner_radius: Option<f32>,
    min_bar_gap: f32,
    group_gap: f32,
}

impl<T: Clone + std::fmt::Display + 'static> BarChart<T> {
    pub fn new(series: impl Into<Prop<Vec<ChartSeries<T>>>>) -> Self;
    pub fn orientation(self, o: BarOrientation) -> Self;
    pub fn grouping(self, g: BarGrouping) -> Self;
    pub fn value_labels(self, show: bool) -> Self;
    pub fn grid(self, show: bool) -> Self;
    pub fn legend(self, show: bool) -> Self;
    pub fn legend_position(self, pos: LegendPosition) -> Self;
    pub fn axis_x(self, cfg: AxisConfig) -> Self;
    pub fn axis_y(self, cfg: AxisConfig) -> Self;
    pub fn palette(self, p: impl Into<Prop<ChartPalette>>) -> Self;
    pub fn bar_corner_radius(self, r: f32) -> Self;
}
```

### Line chart (`line_chart.rs`)

```rust
pub struct LineChart<T: Clone + 'static> {
    series: Prop<Vec<ChartSeries<T>>>,
    show_points: bool,
    show_area_fill: bool,
    area_fill_opacity: f32,         // default 0.15
    show_grid: bool,
    show_hover_tooltip: bool,       // default true
    show_legend: bool,
    legend_position: LegendPosition,
    line_width: f32,
    point_radius: f32,
    axis_x: AxisConfig,
    axis_y: AxisConfig,
    palette: Prop<ChartPalette>,
}

impl<T: Clone + std::fmt::Display + 'static> LineChart<T> {
    pub fn new(series: impl Into<Prop<Vec<ChartSeries<T>>>>) -> Self;
    pub fn points(self, show: bool) -> Self;
    pub fn area_fill(self, show: bool) -> Self;
    pub fn area_fill_opacity(self, alpha: f32) -> Self;
    pub fn grid(self, show: bool) -> Self;
    pub fn hover_tooltip(self, show: bool) -> Self;
    pub fn legend(self, show: bool) -> Self;
    pub fn legend_position(self, pos: LegendPosition) -> Self;
    pub fn line_width(self, w: f32) -> Self;
    pub fn point_radius(self, r: f32) -> Self;
    pub fn axis_x(self, cfg: AxisConfig) -> Self;
    pub fn axis_y(self, cfg: AxisConfig) -> Self;
    pub fn palette(self, p: impl Into<Prop<ChartPalette>>) -> Self;
}
```

### Pie chart (`pie_chart.rs`)

Pie and donut share one widget. `inner_radius_ratio` controls the hole:
`0.0` = pie, `0.4`–`0.6` = donut.

Pie data is naturally one-dimensional (N slices, no axes), so the widget
accepts a flatter constructor in addition to the standard
`Vec<ChartSeries<T>>` shape:

```rust
pub enum PieLabelMode {
    None,                // no slice labels
    Inside,              // label inside each slice (skipped if too small)
    Outside,             // label with leader line outside the slice
    InsideWithLeaders,   // inside if it fits, leader-out if not
}

pub struct PieChart<T: Clone + 'static> {
    data: Prop<Vec<ChartDatum<T>>>,
    inner_radius_ratio: f32,         // 0.0 = pie, >0 = donut
    start_angle_degrees: f32,        // default -90.0 (12 o'clock start)
    clockwise: bool,                 // default true
    slice_gap_degrees: f32,          // default 0.0
    label_mode: PieLabelMode,
    show_percentages: bool,
    show_legend: bool,
    legend_position: LegendPosition,
    show_hover_tooltip: bool,
    palette: Prop<ChartPalette>,
    explicit_colors: Vec<Option<ColorProp>>,  // index-aligned with data

    // Center slot — only meaningful when inner_radius_ratio > 0.
    // Hand-rolled per the slot pattern in card.rs / dialog.rs.
    pending_center: Option<PendingChild>,
    center_id: Option<WidgetId>,
}

impl<T: Clone + std::fmt::Display + 'static> PieChart<T> {
    pub fn new(data: impl Into<Prop<Vec<ChartDatum<T>>>>) -> Self;
    pub fn from_series(series: ChartSeries<T>) -> Self;     // adapter

    pub fn donut(self, inner_radius_ratio: f32) -> Self;    // 0.4..=0.7 typical
    pub fn start_angle_degrees(self, deg: f32) -> Self;
    pub fn clockwise(self, on: bool) -> Self;
    pub fn slice_gap_degrees(self, deg: f32) -> Self;       // visual separation
    pub fn label_mode(self, mode: PieLabelMode) -> Self;
    pub fn show_percentages(self, on: bool) -> Self;
    pub fn legend(self, show: bool) -> Self;
    pub fn legend_position(self, pos: LegendPosition) -> Self;
    pub fn hover_tooltip(self, show: bool) -> Self;
    pub fn palette(self, p: impl Into<Prop<ChartPalette>>) -> Self;
    pub fn slice_color(self, index: usize, c: impl Into<ColorProp>) -> Self;

    // Center slot — Card / Dialog / GroupBox pattern.
    pub fn center(mut self, widget: impl Widget + 'static) -> Self;
    pub fn center_id(mut self, id: WidgetId) -> Self;
}
```

**Center slot semantics.** Only meaningful when `inner_radius_ratio > 0`
(donut). For pie (ratio = 0) the slot is silently ignored — no panic, no
warning, just unused. Documented behavior so users can swap pie ↔ donut
freely.

The slot follows the existing pattern in
[card.rs](../../crates/fern-widgets/src/card.rs:13-24) and
[dialog.rs](../../crates/fern-widgets/src/dialog.rs:206-212): two fields
(`pending_center: Option<PendingChild>` and `center_id: Option<WidgetId>`),
two builder methods (`.center(impl Widget)` and `.center_id(WidgetId)`),
consumed in `build()`:

```rust
fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
    if let Some(c) = self.pending_center.take() {
        self.center_id = Some(match c {
            PendingChild::Id(id) => id,
            PendingChild::Deferred(w) => ctx.add_boxed(w),
        });
    }
    self.center_id.into_iter().collect()
}
```

The center widget is placed into a square inscribed in the inner radius
circle (`size = inner_radius * sqrt(2)`, centered on the donut center)
during `place_children`. Common payloads: a `TextWidget` showing the
total, a stacked `VStack(label + value)`, an `IconWidget`, or a small
nested `PieChart` for drill-down. Anything that fits.

### Legend (`legend.rs`)

Standalone widget **and** inline-draw helper:

```rust
pub enum LegendPosition { Top, Bottom, Leading, Trailing }

/// Standalone legend — compose anywhere alongside any chart that shares
/// the same series prop.
pub struct ChartLegend<T: Clone + 'static> {
    series: Prop<Vec<ChartSeries<T>>>,
    palette: Prop<ChartPalette>,
    orientation: LegendOrientation, // Horizontal | Vertical
    interactive: bool,              // click toggles series.visible Signal
}

impl<T: Clone + 'static> ChartLegend<T> {
    pub fn new(series: impl Into<Prop<Vec<ChartSeries<T>>>>) -> Self;
    pub fn palette(self, p: impl Into<Prop<ChartPalette>>) -> Self;
    pub fn orientation(self, o: LegendOrientation) -> Self;
    pub fn interactive(self, on: bool) -> Self;
}
```

When a chart is constructed with `.legend(true)`, it instantiates a
private `ChartLegend` internally and lays it out at `legend_position`,
sharing the same `series` and `palette` props. When a user wants the
legend in a different container or with custom layout, they build it
explicitly and the chart stays `.legend(false)`.

### Axis config (`axis.rs`)

```rust
pub struct AxisConfig {
    pub label: Option<String>,
    pub show_labels: bool,           // tick labels on/off (default true)
    pub show_axis_line: bool,        // baseline (default true)
    pub tick_count_hint: Option<usize>,
    pub min: Option<f32>,            // explicit override
    pub max: Option<f32>,
    pub formatter: Option<Rc<dyn Fn(f32) -> String>>,
}
```

`AxisConfig::default()` for the common path.

### Crate-level re-exports (`lib.rs`)

```rust
pub use bar_chart::{BarChart, BarOrientation, BarGrouping};
pub use line_chart::LineChart;
pub use pie_chart::{PieChart, PieLabelMode};
pub use legend::{ChartLegend, LegendPosition, LegendOrientation};
pub use series::{ChartDatum, ChartSeries};
pub use palette::ChartPalette;
pub use axis::AxisConfig;
```

---

## 4. Token additions

Extend [crates/fern-tokens/src/components.rs](../../crates/fern-tokens/src/components.rs)
with one new style struct:

```rust
pub struct ChartStyle {
    pub plot_padding_top:    f32,   // 12.0
    pub plot_padding_right:  f32,   // 12.0
    pub plot_padding_bottom: f32,   // 4.0
    pub plot_padding_leading: f32,  // 4.0
    pub axis_tick_length:    f32,   // 4.0
    pub axis_label_gap:      f32,   // 4.0
    pub axis_title_gap:      f32,   // 8.0
    pub gridline_width:      f32,   // 1.0
    pub bar_min_width:       f32,   // 4.0
    pub line_default_width:  f32,   // 1.5
    pub point_default_radius: f32,  // 3.0
    pub legend_swatch_size:  f32,   // 10.0
    pub legend_item_gap:     f32,   // 12.0
    pub legend_to_plot_gap:  f32,   // 8.0
    pub tooltip_padding:     f32,   // 8.0
    pub pie_padding:         f32,   // 8.0   (margin around the pie/donut disc)
    pub pie_label_gap:       f32,   // 4.0   (gap between slice and outside label)
    pub pie_leader_length:   f32,   // 12.0  (leader-line length for outside labels)
    pub pie_min_slice_label_degrees: f32,  // 12.0 (slices smaller than this get no inside label)
    pub donut_default_inner_ratio:   f32,  // 0.55 (when .donut() called without arg in future)
}
```

Add `pub chart: ChartStyle` to `ComponentStyles`.

**Dedicated palette token** (per user decision). Add to `ColorTokens`:

```rust
pub chart_palette: Vec<Color>,   // 8-color sequence, theme-overridable
```

The default palette for both light and dark themes is **Okabe-Ito**, the
colorblind-safe sequence from Okabe & Ito 2008 (used by ggplot2, seaborn).
Eight colors:

| # | Name      | Hex     |
|---|-----------|---------|
| 1 | Orange    | #E69F00 |
| 2 | Sky blue  | #56B4E9 |
| 3 | Bluish green | #009E73 |
| 4 | Yellow    | #F0E442 |
| 5 | Blue      | #0072B2 |
| 6 | Vermilion | #D55E00 |
| 7 | Reddish purple | #CC79A7 |
| 8 | Black/white  | #000000 (light theme) / #FFFFFF (dark theme) |

Themes can override `chart_palette` to brand-match. Per-chart override via
`BarChart::palette(ChartPalette::Custom(vec![...]))`. When a series sets
`color: Some(_)` explicitly, that wins over the palette.

**Other colors are role-driven** (no chart-specific entries beyond palette):

- Axis lines → `BorderRole::Default`
- Grid lines → `BorderRole::Default` with reduced alpha (or a new
  `BorderRole::Subtle` if it doesn't already exist — check during impl)
- Axis tick labels → `TextRole::Secondary`, `TextStyleRole::Tiny`
- Axis title → `TextRole::Primary`, `TextStyleRole::Caption`
- Legend label text → `TextRole::Primary`, `TextStyleRole::Caption`
- Tooltip background/text → reuse existing `TooltipStyle` colors

---

## 5. Layout and paint strategy

### `size_that_fits`

Charts are **proposal-driven**. Fill whatever the parent proposes; fallback
`Size::new(320.0, 200.0)` when unbounded. Same pattern as `ProgressBar`,
`ScrollArea`.

### Plot-area carve (shared, in `layout.rs`)

Given total `bounds: Rect` and the chart's axis configs + legend config,
compute a `PlotArea { rect, x_axis_band, y_axis_band, legend_band }`:

1. If `show_legend`, reserve the legend band on the requested edge
   (`Top` / `Bottom` / `Leading` / `Trailing`). Legend size is computed
   from series count + label widths in a single pass.
2. Reserve y-axis band on the leading edge: `max_tick_label_width +
   tick_length + axis_label_gap + (axis_title_height if set)`.
3. Reserve x-axis band on the bottom: `tick_label_line_height +
   tick_length + axis_label_gap + (axis_title_height if set)`.
4. Add `plot_padding_*` to inner edges.

Y-tick labels need values to measure widths, so the order is:
domain → `nice_ticks` → measure widest label string → carve y-band →
recompute tick positions to fit the carved plot rect. Single pass.

### Tick generation (`axis.rs`)

**Wilkinson / Heckbert nice-numbers algorithm** (`nice_ticks(min, max,
target_count)`) — produces tick spacings 1/2/5 × 10^k for the smallest
k that yields ≤ `target_count` intervals covering `[min, max]`. Industry
standard (matplotlib, d3). ~30 LOC, no deps.

Default `target_count` = `(axis_pixels / 60).clamp(2, 10)`. Categorical
x-axes (BarChart, LineChart over discrete categories): one tick per
category, no nice-numbers.

### Bar layout

- **Vertical, single-series:** N bars across plot width, each
  `(plot_w - (N+1)*min_bar_gap) / N` wide, height ∝ `value / y_max`.
- **Vertical, grouped:** group of S series-bars per category, group width
  `(plot_w - (N+1)*group_gap) / N`, bars within share group width minus
  inter-bar `min_bar_gap`.
- **Horizontal:** swap x/y in the same math.

Bars: `canvas.fill_rect` (or `fill_rounded_rect` if `bar_corner_radius`
set). Value labels: `canvas.draw_text` above/right of each bar.

### Line layout

Per visible series: build a `Path` via `move_to` + `line_to` over projected
points, then `canvas.stroke_path(&path, color, line_width)`. Area fill:
clone the path, `line_to` down to baseline at last x, `line_to` to
baseline at first x, `close()`, then `canvas.fill_path(&closed,
color.with_alpha(area_fill_opacity))`. Points: `canvas.fill_circle` per
datum if `show_points`.

Grid lines: per y-tick, `canvas.draw_line(plot_left, y, plot_right, y,
gridline_color, 1.0)`.

### Pie / donut layout

No axis bands — only the legend band (if shown) is carved off. Remaining
rect: inscribe the largest centered square minus `pie_padding`, then take
the inscribed circle as the disc. Center = disc center. Outer radius =
`disc_diameter / 2 - pie_label_gap - pie_leader_length` if any slice
needs an outside label, else `disc_diameter / 2`.

Per-slice angle = `(value / sum_of_values) * 360°`, accumulated from
`start_angle_degrees` in `clockwise` direction. Each slice is a `Path`:
`move_to(center) + line_to(arc_start) + arc_to(arc_end, radius) +
close()`. For donut: a hollow wedge — `move_to(outer_arc_start) +
arc_to(outer_arc_end, outer_radius) + line_to(inner_arc_end) +
arc_to(inner_arc_start, inner_radius, reversed) + close()`.

`slice_gap_degrees > 0` shrinks each wedge angle by `slice_gap_degrees / 2`
on each side, leaving a transparent ring of background visible between
slices (no manual stroke needed).

Label placement:

- **Inside / InsideWithLeaders:** label text centered along the angle
  bisector at `radius * 0.65`. Skip if slice arc < `pie_min_slice_label_degrees`.
- **Outside (or InsideWithLeaders fallback):** label centered at
  `radius + pie_leader_length + pie_label_gap` along the bisector,
  preceded by a leader line from `radius * 0.95` to `radius +
  pie_leader_length` along the same bisector.

`show_percentages` appends `(N%)` to each label using `formatter` if set,
else default `"{:.0}%"` formatting.

**Center slot placement** (donut only): square of side `inner_radius *
sqrt(2)` centered on disc center, proposed to the center widget via
`SizeProposal::exact(side, side)`. The center widget owns clipping —
typical content is small text or an icon that comfortably fits.

---

## 6. Hover tooltip — inline overlay

Per user decision: **inline draw inside the chart's own `paint()`**, clipped
to the plot rect. Matches Excel / Google Sheets behavior.

1. LineChart owns a `Signal<Option<HoveredPoint>>` where
   `HoveredPoint { series_idx, datum_idx, screen_pos }`.
2. `on_pointer_event` handler: on pointer move within plot bounds, find
   nearest visible point in screen-space (O(N·S), fine for N<10k); update
   the signal. On pointer leave / pointer down, set to `None`.
3. Bind at `BindingLevel::RepaintOnly` — geometry doesn't change.
4. In `paint`, if `Some`: draw a marker ring at the hovered point and a
   tooltip rect above it (`name`: `formatted_value`, optional category
   string). Tooltip uses `ChartStyle::tooltip_padding` and reuses
   `theme.colors.tooltip_bg` / `tooltip_text`.
5. Tooltip is clipped to plot rect — won't escape the chart bounds. If
   the marker is near an edge, the tooltip flips to the opposite side
   (above ↔ below, leading ↔ trailing) so it doesn't clip.
6. Optional `.on_hover(callback)` builder method for users who want to
   react in addition to (not instead of) the built-in tooltip.

BarChart can adopt the same pattern in a follow-up PR — bars are easy to
hit-test (point-in-rect), but the deferred Stacked grouping should land
first so the tooltip knows which sub-bar to label.

PieChart adopts the same pattern in PR 5. Hit-test = polar coordinates:
convert pointer position to `(angle, distance)` from disc center; the
slice is found by which accumulated-angle bracket the angle falls into,
and the hit is valid only if `inner_radius ≤ distance ≤ outer_radius`.
Cheap (one atan2 + one binary search per move). The tooltip shows
`category: value (percentage%)`.

---

## 7. Reactivity

| Change | Binding level | Why |
|---|---|---|
| `Vec<ChartSeries<T>>` swap (data, count, names) | `Relayout` | Y-domain may shift → tick positions change → label widths change → carve changes |
| Per-series `color: ColorProp::Bound(signal)` | `RepaintOnly` | Geometry unchanged |
| `series.visible` toggled via legend | `Relayout` | Visible set affects auto-domain and bar widths |
| Hover state `Signal<Option<HoveredPoint>>` | `RepaintOnly` | Marker + tooltip only |
| Theme change | Auto via `ctx.theme_signal()` dirty-mark | Colors/fonts re-resolved |
| `palette: Prop<ChartPalette>` change | `RepaintOnly` | Color-only |
| PieChart center-slot child swap | handled by child's own dirty path | Slot is a regular child widget |
| PieChart `inner_radius_ratio` change | `Relayout` | Center-slot inscribed-square size depends on it |

Wiring example in `BarChart::build`:

```rust
let id = ctx.self_id();
let registry = ctx.binding_registry();
self.series.register_if_bound(id, registry, BindingLevel::Relayout);
self.palette.register_if_bound(id, registry, BindingLevel::RepaintOnly);
```

`Vec<ChartSeries<T>>` cloning per `.get()` is acceptable phase-1 (small
datasets). If profiling shows it, wrap in `Rc<...>` or introduce a
`ChartDataSource` trait analogous to `ListDataSource`.

---

## 8. Accessibility

- Both charts: `Role::GraphicsDocument`.
- Each visible series contributes a `Role::GraphicsObject` child node with:
  - `Name` = series name
  - `Description` = "N data points, range [min, max], color: <color name>"
- Optional `aria-summary` synthesized from data: "BarChart: 3 series,
  5 categories, max value 142 in Q3 (Revenue series)."
- Legend items expose `Role::Switch` when `interactive: true`, with the
  bound `series.visible` signal as their checked state.
- Hover tooltip while visible flips a11y to expose the focused datum's
  name and value (similar to how `TooltipWidget` does it).

Full a11y polish is a phase-3 concern — phase-1 ships the role + name
and a basic description.

---

## 9. Phasing — 5 PRs

**PR 1 — Foundation + simplest chart** (~3 days)

- Crate skeleton, `Cargo.toml`, workspace registration.
- `ChartStyle` token + theme defaults.
- `ColorTokens::chart_palette` with Okabe-Ito defaults for light + dark.
- `ChartSeries<T>`, `ChartDatum<T>`, `AxisConfig`.
- `axis.rs::nice_ticks` + tests.
- `palette.rs` resolution.
- `BarChart` — vertical, single-series only. No grouping, no value labels,
  no legend yet. Basic axis line + tick labels.
- `examples/chart_demo` showing one bar chart with a `Signal`-driven
  refresh button.

**PR 2 — BarChart completion + legend** (~4 days)

- Grouped multi-series.
- Horizontal orientation.
- Value labels.
- Grid lines.
- Axis titles.
- `ChartLegend` standalone widget.
- `BarChart::legend(true)` baked-in variant at all four positions.
- Demo extended with a 3-series grouped bar chart + legend.

**PR 3 — LineChart core** (~3 days)

- Single + multi-series line, with points.
- Grid + axis titles inherited from PR 2.
- Legend (reuses PR 2 widget).
- No area fill, no hover tooltip yet.

**PR 4 — LineChart finish** (~3 days)

- Area fill (closed path + alpha).
- Inline hover tooltip with edge-flip placement.
- `on_hover` callback escape hatch.
- A11y pass: `GraphicsDocument` + `GraphicsObject` per series.

**PR 5 — PieChart + donut + center slot** (~3 days)

- `PieChart` widget with arc-path slice rendering.
- `from_series` + flat `Vec<ChartDatum<T>>` constructors.
- Label modes: `None`, `Inside`, `Outside`, `InsideWithLeaders` with
  leader lines.
- `show_percentages`.
- `.donut(inner_ratio)` builder.
- Center slot via `pending_center` / `center_id` + `.center()` /
  `.center_id()` builders, following the
  [card.rs](../../crates/fern-widgets/src/card.rs) pattern.
- Inline hover tooltip via polar hit-test.
- Legend integration (reuses PR 2 widget — slice-as-series).
- Demo extended with a donut showing total in center slot.

**Total: ~16 days (≈3.5 weeks).** Stacked bars, log axis, time-axis
formatters, animation on data change, hover for BarChart, exploded
slices, slice-pull-on-hover — explicit follow-ups, not in the five-PR
scope.

---

## 10. Tests

All tests headless, inline `#[cfg(test)] mod tests`, using `WidgetTree` +
`MockTextBackend` (see [crates/fern-widgets/src/progress_bar.rs](../../crates/fern-widgets/src/progress_bar.rs)
for the shape).

**`axis.rs`:**
- `nice_ticks(0.0, 100.0, 5)` → `[0, 25, 50, 75, 100]`.
- Negative ranges with zero crossing.
- Tiny range (0.0..0.003) produces sub-decimal ticks.
- Zero range (min == max) returns sensible single-tick fallback.

**`palette.rs`:**
- Okabe-Ito defaults present in both light and dark theme.
- Per-series explicit color overrides palette assignment.
- Palette wraps for series count > palette length.

**`bar_chart.rs`:**
- `size_that_fits` returns proposal verbatim.
- N data points → N bar decorations rendered.
- Mutating bound `Signal<Vec<ChartSeries>>` dirties tree at `Relayout`.
- Horizontal orientation swaps bar dimensions.
- Default palette picks distinct colors.
- Hidden series (`visible=false`) drop from layout and paint.

**`line_chart.rs`:**
- One stroked path per visible series.
- `area_fill(true)` adds N filled paths in addition to N strokes.
- Synthetic pointer move updates hover signal.
- Empty series renders axes with no paths, no panic.
- Edge-flip: hover near right edge places tooltip on left of marker.

**`pie_chart.rs`:**

- N data points → N slice paths rendered.
- Slice angles sum to 360° (within float epsilon).
- `start_angle_degrees(0.0)` rotates first slice to 3 o'clock; default
  starts at 12 o'clock.
- `clockwise(false)` reverses sweep direction.
- `slice_gap_degrees(2.0)` shrinks each slice angle by 2° total
  (1° each side).
- `donut(0.5)` draws hollow wedges; `inner_radius_ratio = 0` draws solid
  pie.
- Center slot widget receives `SizeProposal::exact(side, side)` matching
  inner-radius inscribed square.
- Center slot is silently ignored when `inner_radius_ratio == 0` (no
  panic, no render).
- `inner_radius_ratio` change dirties at `Relayout`.
- Polar hit-test selects correct slice for a given pointer position.
- `label_mode(Inside)` skips labels for slices < `pie_min_slice_label_degrees`.
- `slice_color(2, Color::RED)` overrides palette for that index.
- Empty data renders nothing (no panic).

**`legend.rs`:**
- Renders one swatch + label per series.
- Interactive click toggles `series.visible`.
- Hidden series swatch dims (visual cue).

**`integration` (cross-module, still inline):**
- BarChart with baked-in legend → legend band carved correctly at all 4
  positions, plot rect shrinks accordingly.

---

## 11. Critical files to reference

- [crates/fern-canvas/src/canvas.rs](../../crates/fern-canvas/src/canvas.rs) — Path API
- [crates/fern-tokens/src/components.rs](../../crates/fern-tokens/src/components.rs) — where `ChartStyle` slots in
- [crates/fern-tokens/src/color.rs](../../crates/fern-tokens/src/color.rs) — where `chart_palette` slots in
- [crates/fern-widgets/src/progress_bar.rs](../../crates/fern-widgets/src/progress_bar.rs) — proposal-driven sizing pattern + test shape
- [crates/fern-widgets/src/badge.rs](../../crates/fern-widgets/src/badge.rs) — minimal builder/paint widget reference
- [crates/fern-data/src/list_model.rs](../../crates/fern-data/src/list_model.rs) — reactive data source patterns
- [docs/plans/widgets-plan.md §3.6.2](widgets-plan.md) — source spec

---

## 12. Verification

Per-PR verification:

- **PR 1:** `cargo test -p fern-charts` green, `cargo run -p chart_demo`
  shows a single rendering bar chart that updates when refresh is pressed.
- **PR 2:** Demo shows a 3-series grouped bar chart with legend in all four
  positions toggled by a SegmentedControl.
- **PR 3:** Demo gains a tab switching between bar and line charts, both
  reading the same `ChartSeries` data.
- **PR 4:** Demo line chart shows area fill and follows the cursor with
  the hover tooltip; AccessKit inspector confirms `GraphicsDocument`
  role.
- **PR 5:** Demo gains a tab showing a donut with the total value and
  unit rendered in the center slot via a `VStack(TextWidget + TextWidget)`;
  hovering a slice highlights it and shows category/value/percentage in
  the inline tooltip; toggling between pie (`inner_ratio = 0`) and donut
  via a slider hides/shows the center slot accordingly.

Survey deliverable accepted by user — implementation can begin with PR 1.
