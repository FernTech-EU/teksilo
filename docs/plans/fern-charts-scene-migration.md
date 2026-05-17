# `fern-charts` on `fern-scene` — Migration Plan

**Status:** Proposed — no code landed yet. This document is the
implementation plan for migrating the three chart widgets in
[`fern-charts`](../../crates/fern-charts/) onto the
[`fern-scene`](../../crates/fern-scene/) viewport model. Authored
2026-05-17, after the fern-scene fixes-and-upgrades work
(`~/.claude/plans/groovy-mapping-cake.md`) — every blocker called
out in the audit is now resolved.

**Companion docs:**
- [docs/fern-scene.md](../fern-scene.md) — scene crate user reference.
- [docs/fern-scene-a11y.md](../fern-scene-a11y.md) — scene accessibility.
- [docs/charts.md](../charts.md) — current fern-charts reference.

---

## 1. Why migrate

The three current chart widgets ([`BarChart`](../../crates/fern-charts/src/bar_chart.rs),
[`LineChart`](../../crates/fern-charts/src/line_chart.rs),
[`PieChart`](../../crates/fern-charts/src/pie_chart.rs)) each
implement `Widget::paint(&self, bounds, canvas, ctx)` and do the
entire chart in one method: compute domain, generate ticks,
measure labels, carve plot area, draw axes, draw the data shape,
overlay the legend. Roughly 1k lines per chart.

That works for a static one-shot chart. It struggles with:

| Capability | Current state | Why |
|---|---|---|
| Pan / zoom the data area while the axis stays fixed | Not supported | Single `paint` repaints everything together. |
| Hit-test individual bars / slices / points | Not supported | No per-element identity; the whole chart is one widget. |
| Per-element tooltips, click handlers, drag | Not supported | Same root cause. |
| Minimap overview of huge datasets | Not supported | Nothing to feed thumbnails from. |
| GPU cache when only the axis changed | All-or-nothing repaint | One paint method, no per-element invalidation. |
| Animate a single series independently | Whole-chart repaint | Same. |

Every one of these falls out naturally on `fern-scene`:
- Pan / zoom: `SceneView` ships them, with the per-(sub-)scene
  constraint independence (Unit 3) and reactive `DragMode` (Unit 6)
  needed to layer "pan the data" on top of "the axis doesn't move".
- Per-element hit-test: each bar / slice / point becomes its own
  `SceneItem` with a real `shape_contains`. Unit 4 wired the
  shape-aware hit-test through dispatch — wedges, stroke-only
  segments, hollow doughnuts all hit-test correctly.
- Hover / click / drag: Unit 7's `SceneTapEvent` carries modifiers
  + button at the scene-item level. Shift-click to add to a brushed
  selection, Ctrl-click to toggle, middle-click for context-menu —
  all standard interactions.
- Axis labels at fixed pixel size as the data zooms: Unit 2 wired
  `IGNORES_TRANSFORMATIONS` through paint and hit-test, with the
  anchor following the data point and the size staying constant.
- Minimap: Unit 5 shipped `viewport_in_scene_signal()` and
  `Scene::item_thumbnails()`. The minimap widget already exists.

---

## 2. Architecture overview

The migration uses **two layered SceneViews** for the chart's
interactive region, plus the existing widget tree for everything
outside it:

```
  ┌──────────────────────────────────────────────────────────────┐
  │ ChartContainer (Widget)                                      │
  │  ┌────────────────────────────────────────────────────────┐  │
  │  │ ChartTitle (Widget — TextWidget)                       │  │
  │  └────────────────────────────────────────────────────────┘  │
  │  ┌────────────────────────────────────────────────────────┐  │
  │  │ Outer SceneView — axis chrome, fixed                   │  │
  │  │  pan_axes: None  |  zoomable: false  |  interactive: false│  │
  │  │  ┌──────────────────────────────────────────────────┐  │  │
  │  │  │ Inner SceneView — data area, free pan + zoom     │  │  │
  │  │  │  pan_axes per chart shape (None / X / Y / Both)  │  │  │
  │  │  │  pan_bounds: data extent                         │  │  │
  │  │  │  zoom_range: Some(0.25..=10.0) (tunable per chart) │  │  │
  │  │  │                                                  │  │  │
  │  │  │  SceneItem per bar / slice / line series         │  │  │
  │  │  │  IGNORES_TRANSFORMATIONS items: tick labels      │  │  │
  │  │  └──────────────────────────────────────────────────┘  │  │
  │  │  Axis-frame SceneItems (gridlines, axis lines, title)  │  │
  │  └────────────────────────────────────────────────────────┘  │
  │  ┌────────────────────────────────────────────────────────┐  │
  │  │ ChartLegend (Widget — sibling, NOT in the scene)       │  │
  │  └────────────────────────────────────────────────────────┘  │
  └──────────────────────────────────────────────────────────────┘
```

**Why two SceneViews:**
- The outer one holds the axis chrome and is locked (no user pan
  / zoom). It's `interactive: false` so events pass through to the
  inner view.
- The inner one holds the data series and is freely pannable /
  zoomable. Its `pan_bounds` clamp to the data extent; its
  `zoom_range` is constrained so users can't zoom into f32-precision
  trouble.
- Tick labels live in the OUTER view, anchored at scene-coord
  positions (where the tick line lands), flagged
  `IGNORES_TRANSFORMATIONS` so they stay at readable pixel size
  regardless of the inner view's zoom. The anchor moves with the
  data point underneath (Unit 2 semantic). Inner zooms → the
  numeric label tracks the data point on the screen but stays at
  the same point size.

**Why the legend is a sibling, not in the scene:**
- Legends don't pan / zoom with data. Putting them in the scene
  would require IGNORES_TRANSFORMATIONS for every legend item, plus
  positioning math against viewport corners, plus careful event
  routing to keep clicks reaching the legend instead of the data.
- A sibling `ChartLegend` widget (today's [`legend.rs`](../../crates/fern-charts/src/legend.rs))
  composes via `VStack`/`HStack` like any other UI, gets free
  reflow, and its click handlers go through the standard widget
  dispatch.

---

## 3. Per-chart mapping

### 3.1 BarChart

Current: one widget. Each bar is a `canvas.fill_rect` call inside
the `paint_single` / `paint_grouped` helpers.

After migration: one `BarItem` per visible bar inside the data
SceneView.

| Today | After |
|---|---|
| `paint_single` loops over data, calls `fill_rect` per bar | `BarItem` per bar, each `paint` is one `fill_rect` |
| Bar hit-test: none | `shape_contains` over the bar's `local_bounds` (default AABB — exact for rect bars) |
| Per-bar hover: none | `SceneItemHandlerSet::on_hover` per bar, drives a tooltip overlay sibling |
| Per-bar click: none | `on_tap_event` → emit `Intent::BarSelected { series_idx, datum_idx }` |
| Tick labels | `IGNORES_TRANSFORMATIONS` `TextItem`s in the outer view |
| Gridlines | `PathItem`s in the outer view, drawn with stroke-only paths and the line color from `cs::grid_color(theme)` |
| Axis lines | Same — outer-view `PathItem`s |

Key sub-decisions:

- **Bar geometry**: with `IGNORES_TRANSFORMATIONS` off, the bars
  scale with zoom — which is the natural "zoom to see narrow time
  segments in detail" behavior. Bars stay rect-shaped (their
  scene-space rect transforms cleanly under uniform zoom).
- **Bar width vs zoom**: at high zoom, bars become wide stripes —
  this is the desired behavior for timeline-style data. At low
  zoom, bars become hair-thin — this is when GPU cache pays off
  (cached frames replay without re-running `paint`).
- **Category axis**: pan / zoom only on Y axis (vertical bars) or
  only on X (horizontal). Set
  `Scene::pan_axes(PanAxes::Horizontal)` or `Vertical` accordingly.
  Categorical X axes typically pin: `pan_axes(PanAxes::None)` and
  `zoomable(false)` on the inner scene.

### 3.2 LineChart

| Today | After |
|---|---|
| `paint` builds one `Path` per series and strokes it | One `PathItem` per series (stroke-only) |
| Point markers: `fill_circle` per point | Either: (a) one shape per visible point as separate items, or (b) keep them inside the `PathItem`'s `paint`. (a) gives per-point hover; (b) is cheaper for dense plots. Pick per-app. |
| Area fill: filled `Path` below the polyline | Second `PathItem` per series, fill-only, behind the stroke |
| Line hit-test: none | `PathItem::shape_contains` does per-segment distance test (Unit 4 wired this through dispatch) |

Notable wins:
- The audit-flagged "PathItem stroke-only hit-test corner" bug fix
  (Unit 4) makes connector-line hit-test actually work — a 1px-thick
  stroked line is now genuinely clickable along its real path, not
  along its AABB.
- Dense plots (10k+ points): batch into a single `PathItem` per
  series with `CacheMode::ItemCoordinate`. The cached `RenderFrame`
  is item-local; pan / zoom of the data view shifts it without
  re-running `paint`.

### 3.3 PieChart

| Today | After |
|---|---|
| `paint` builds wedge paths and fills | One `PathItem` per wedge (filled, with `shape_contains` overridden to do real angular containment) |
| Outer / inner radius for donuts | Wedge `Path` is built from arcs; the donut hole comes from the inner-radius arc in the same path |
| Label placement (inside / outside) | Heavyweight `TextWidget`s placed via `Scene::add_widget` at the label scene position, OR `IGNORES_TRANSFORMATIONS` `TextItem`s if the label should stay at fixed pixel size |
| Center widget slot (donut) | Heavyweight `Widget` placed at the pie center via `add_widget` — unchanged from today's pattern |

**Per-wedge `shape_contains` is the migration's biggest UX
upgrade for pie charts**. Today, hovering over a pie chart gives
nothing — the whole chart is one widget. With per-wedge SceneItems,
hover, click, animated explode-on-hover (`local_pos` shift on
the wedge item), drilldown — all standard.

The `shape_contains` impl on the wedge item: point-in-annular-
sector test (angle within wedge span AND radius between inner /
outer). Custom override on the per-app `WedgeItem` type; cheap
arithmetic.

---

## 4. The axis-label tradeoff: IGNORES_TRANSFORMATIONS vs heavyweight widgets

Two ways to render tick labels:

### Option A — `IGNORES_TRANSFORMATIONS` lightweight `TextItem`s

```rust
let label = TextItem::new(format!("{:.0}", tick_value))
    .text_style(TextStyleRole::Tiny);
let id = scene.add_item(label, Point::new(tick_x_scene, tick_y_scene));
scene.set_flag(id, ItemFlags::IGNORES_TRANSFORMATIONS, true);
```

**Pros:**
- Lightweight — no arena entry per label. Charts with hundreds of
  ticks (time series across years) stay cheap.
- Anchor follows the tick's scene position. Inner view pans →
  labels visually slide with the data; inner view zooms → labels
  stay at the same pixel size but their anchor slides further
  through the screen.
- One paint call per label per frame. With `CacheMode::ItemCoordinate`,
  even that is amortized across frames where the label text
  doesn't change.

**Cons:**
- Labels don't participate in standard widget i18n /
  hot-reload — `TextItem` carries a string, not a `LocalizedString`
  signal. Acceptable for numeric tick labels; awkward if the chart
  needs locale-reactive formatted dates ("Jan", "Feb" in current
  locale).
- Accessibility: lightweight items get synthetic AT nodes (per
  [docs/fern-scene-a11y.md](../fern-scene-a11y.md)), but the
  semantics for "this is an axis label" are weaker than what a
  heavyweight `TextWidget` carries.

### Option B — Heavyweight `TextWidget`s placed at scene coords

```rust
scene.add_widget(
    TextWidget::new(tr_signal!(tick_label(value = tick_value_signal))),
    Rect::new(label_x, label_y, label_w, label_h),
);
```

**Pros:**
- Full widget machinery: locale-reactive labels via `tr_signal!`,
  proper a11y role + name, theme-driven text style.
- Standard text layout (line-wrap, text-shaping, hot-reload).

**Cons:**
- Arena entry per label. A time-series with 100 daily ticks would
  pay 100 arena entries. The lightweight-vs-heavyweight tradeoff
  is documented in [docs/fern-scene.md](../fern-scene.md).
- Position scales with zoom (heavyweight items DON'T support
  IGNORES_TRANSFORMATIONS — that flag is lightweight-only). At
  high zoom, the labels' bounding boxes inflate; layout pressure on
  the arena grows.

### Recommendation

- **Numeric axis** (BarChart Y, LineChart X+Y): **Option A**.
  Numeric formatting can use a simple `format!` string; locale-aware
  numbers feed in via `NumberFormatter::format(value_signal).get()`
  at item-construction time. Re-create the item on locale change
  (rare).
- **Categorical / date axis** (BarChart X with named categories,
  LineChart X with month names): **Option B**. The label is a
  proper translated string and benefits from `tr_signal!` reactivity.

Charts can mix the two — the inner SceneView holds the data
items, the outer SceneView holds the axis lines and gridlines as
lightweight items, the labels mount as a mix of heavyweight (named
category labels) and lightweight `IGNORES_TRANSFORMATIONS`
(numeric tick labels) per axis.

---

## 5. PNG export

Today's [tools/bench_examples.py](../../tools/bench_examples.py) drives
the chart widgets through the standard widget paint path. The
migrated charts need an offscreen-render path that produces the
same PNG output at a target resolution.

The previewer crate already does this for individual widgets via
[`png_export.rs`](../../crates/fern-preview-ui/src/png_export.rs).
The chart-export helper would mirror that flow:

1. Build the `ChartContainer` widget tree (title + outer scene +
   legend).
2. Lay out at the target export size.
3. Render to a fresh canvas via the widget tree's normal `render()`
   path.
4. Pass the resulting `RenderFrame` to a software-rasterized PNG
   encoder.

**Gotcha**: the inner SceneView's pan / zoom state must be reset
to the export anchor (typically `fit_to_content()` for "show the
whole dataset"). The export helper either:
- Programmatically calls `view.fit_to_content()` before render, or
- Accepts a `(pan, zoom)` tuple from the caller for custom framing.

The `set_pan` / `set_zoom` are not animated (immediate snap), so a
single layout + render pass suffices.

---

## 6. Dep-graph implications

`fern-charts` today depends on `fern-core`, `fern-canvas`,
`fern-tokens`, `fern-widgets` (for `TextWidget`). It does NOT
depend on `fern-scene`.

`fern-scene` depends on `fern-widgets` (the heavyweight tier
reuses the full widget catalog as scene-item content). Adding
`fern-scene` as a dep of `fern-charts` creates:

```
  fern-charts → fern-scene → fern-widgets ← fern-charts
                                     ▲
                                     └─── existing dep
```

No cycle — `fern-widgets` doesn't depend on `fern-charts`. The
addition just means `fern-charts` pulls in the entire scene crate.
For apps using charts without the scene viewer (static
dashboard tile, exported PNG, terminal preview), that's wasted
compile time and binary size.

**Three options**, ranked by recommendation:

1. **`scene-charts` feature flag on `fern-charts`** (recommended).
   Add `fern-scene` as an optional dep gated behind `scene-charts`.
   Apps that want the interactive variant enable the feature; apps
   that just want the static `Widget::paint` chart don't. The new
   scene-based types are gated under `#[cfg(feature = "scene-charts")]`.
   - Pros: One crate to depend on; existing apps unaffected.
   - Cons: Bigger `fern-charts` crate, more `cfg` noise.

2. **Sibling crate `fern-charts-scene`**. New crate depends on
   `fern-charts` (for type imports) + `fern-scene`. Defines
   `BarChartScene`, `LineChartScene`, `PieChartScene` as parallel
   types.
   - Pros: Cleanest separation; existing `fern-charts` doesn't move.
   - Cons: Two crates to coordinate; users have to pick which
     family they want; type-name parallelism breeds confusion.

3. **Replace `Widget::paint` with `SceneView`-based**. Drop the
   one-shot chart variant entirely.
   - Pros: Single API; smaller crate after the dust settles.
   - Cons: Apps doing static dashboard tiles or terminal previews
     pay for the entire scene machinery they don't need. Breaks
     anything currently using the `Widget::paint` variant.

Pick **option 1** for the initial migration; revisit after one
release cycle to see if the static-chart use case actually
justifies the dual implementation.

---

## 7. Migration phases

### Phase 1: BarChart proof of concept

- Add `fern-scene` as an optional dep on `fern-charts` behind
  `scene-charts` feature.
- Create `BarChartScene` (new type, not a rename of `BarChart`).
- Inner-view items: one `BarItem` (custom `SceneItem`) per bar,
  per series. `BarItem::shape_contains` is AABB-default since bars
  ARE rects.
- Outer-view items: tick label `TextItem`s with
  `IGNORES_TRANSFORMATIONS`, axis-line `PathItem`s, gridline
  `PathItem`s.
- Sibling: existing `ChartLegend` widget.
- Pan / zoom: `PanAxes::Horizontal` (categorical X stays, numeric
  Y zooms), `zoom_range: Some(0.5..=4.0)` to keep things sane.
- Verify against the existing BarChart pixel-output via the
  previewer's snapshot harness.

### Phase 2: LineChart

- One `PathItem` per series; stroke-only with
  `CacheMode::ItemCoordinate` for the polyline; second filled
  `PathItem` per series for the optional area fill.
- Point markers: opt-in flag. When on, additional per-point
  `RectItem` (or custom `CircleItem` once
  [fern-scene#circle-item](#) lands) per visible datum.
- Hover tooltip: app-side `Popover` triggered by `on_hover` on a
  per-point item (when markers are on) or by a synthetic
  nearest-point recognizer when markers are off (custom Scene
  drag handler).

### Phase 3: PieChart

- Per-wedge `WedgeItem` with custom `shape_contains` doing
  point-in-annular-sector.
- Donut center widget slot: unchanged from today (heavyweight
  `Widget` placed at the pie center via `Scene::add_widget`).
- Label placement: heavyweight `TextWidget`s for inside / outside
  labels; angular layout computed once per data update.
- Explode-on-hover: shift `WedgeItem.local_pos` outward along
  the wedge's centerline angle when hovered. Standard scene-item
  drag-and-move pattern.

### Phase 4: Cross-chart polish

- Shared tooltip system that all three chart types feed into.
- Consistent legend ↔ data linking: legend click toggles series
  visibility via the series's `IS_VISIBLE` flag.
- Shared brushed-selection model: Shift-click extends, Ctrl-click
  toggles, marquee selects (now correctly filtered by
  `IS_SELECTABLE` after Unit 9's fix).
- Animated data transitions: `Signal<Vec<ChartSeries<T>>>` →
  re-derive per-item local_bounds, drive transitions through
  `animated_signal` per bar height, slice angle, line vertex.

---

## 8. Open questions

1. **Drag-to-pan on categorical X**. With `PanAxes::Horizontal`
   and a categorical X axis, the user expects to snap to whole
   categories at scroll rest, not stop between two bars. The
   inner-scene `gate_pan_target` doesn't know about categories.
   Options: (a) post-pan snap in an `effect` listening to
   `pan_x.animation_target`; (b) custom `pan_to_category(idx)`
   helper on the chart wrapper.

2. **Hover tooltip mounting**. Per-bar hover sets a Signal that
   the chart's tooltip overlay reads. Where does the tooltip
   widget live? Likely as a `Popover` overlay anchored to the
   item's screen-projected center (via Unit 5's `map_from_scene`).
   Need a worked example.

3. **Animated data updates** where the data length changes (a new
   bar appears). Scene items are persistent; we'd need to
   add/remove items per data-vec diff. Crossfade animations? Pop-in
   from zero height? Defer to Phase 4.

4. **Legend ↔ data series link**. The legend is OUT of the scene.
   How does clicking the legend's "hide this series" toggle reach
   the chart? The natural answer: chart wrapper owns a
   `Signal<HashSet<SeriesIdx>>` of hidden series; legend mutates
   it, chart's per-series item visibility binds to it. Standard
   `Signal` plumbing, but worth a doc example.

5. **Coordinate-system test coverage**. Unit 5 added `map_to_scene` /
   `map_from_scene`, but the chart-axis use case (a tick at
   data value 3.7 → screen pixel at outer-view x=?) goes through
   a category-or-numeric scale mapping that lives in the chart,
   plus the inner-scene's view transform, plus the outer-scene's
   bounds_origin. End-to-end coordinate tests under pan + zoom
   would be valuable.

6. **CPU vs GPU cost on dense plots**. Today's `LineChart` builds
   one `Path` per series and the whole chart redraws on any
   change. After migration, `CacheMode::ItemCoordinate` on each
   series item means the polyline replays from a cached
   `RenderFrame` instead of re-stroking. Need to measure the
   crossover point (when does the cache win vs. when does the
   memory cost of N cached frames hurt).

7. **PNG export with a non-default zoom**. `view.set_zoom(target)`
   before render works, but at extreme zoom the bars may render
   off-screen. The export helper should `fit_to_content()` after
   any zoom set unless the caller explicitly asked for a custom
   crop.

---

## 9. What Units 1-9 enabled (recap)

| Need | Enabled by |
|---|---|
| `IGNORES_TRANSFORMATIONS` actually works | Unit 2 |
| Per-bar / per-slice / per-segment shape-aware hit-test | Unit 4 |
| Per-(sub-)scene constraint independence (chart-shaped layouts) | Unit 3 |
| Reactive pan_axes / zoom_range for runtime mode switching | Unit 3 |
| Pan-bounds clamping (don't scroll past the data) | Unit 3 |
| Runtime `DragMode` switching (Hand vs Select tools on a chart) | Unit 6 |
| Modifier-aware scene-item taps (Shift-click selection, etc.) | Unit 7 |
| `map_to_scene` / `map_from_scene` for tooltip anchor projection | Unit 5 |
| `viewport_in_scene_signal()` for chart minimap | Unit 5 |
| `Scene::item_thumbnails()` for minimap data | Unit 5 |
| Hand-drag axis-locking (so charts with PanAxes::Horizontal pan correctly) | Unit 1 (ScrollHandDrag gate fix) |
| `adopt_scene_size` for static / one-shot chart renders | Unit 1 (right/bottom fix) |
| Marquee-select for brushing data in a chart, filtered by IS_SELECTABLE | Unit 9 (commit_marquee fix) |
| Narrow public surface — chart code reaches scene types as `fern_scene::Foo` | Unit 8 |

The plan is concretely actionable today.

---

## 10. Non-goals

- Real-time streaming chart updates at 60Hz (separate concern; the
  scene tier is already idle-aware, so static charts that update
  on data change are free; high-frequency streaming wants its own
  ring-buffer SceneItem with shader-driven updates).
- 3D charts (`fern-scene` is 2D-only by design).
- Non-rectangular chart types (radar / chord / Sankey) — these
  need separate item types but the same migration pattern.
- Cross-chart synchronization (brushing one chart highlights
  selected items in another) — falls out of standard
  `Signal<Selection>` sharing once Phase 4 lands.

---

## 11. Estimated effort

| Phase | Files | New tests | Estimate |
|---|---|---|---|
| 1 — BarChart | `crates/fern-charts/src/bar_chart_scene.rs` (~600 lines), per-bar / tick-label items in same file | ~15 tests | 2-3 days |
| 2 — LineChart | `crates/fern-charts/src/line_chart_scene.rs` (~500 lines) | ~10 tests | 2 days |
| 3 — PieChart | `crates/fern-charts/src/pie_chart_scene.rs` (~600 lines) + custom `WedgeItem` `shape_contains` | ~12 tests | 2-3 days |
| 4 — Polish | tooltips, legend↔data link, brushed selection, animations | ~15 tests | 3-4 days |

Total: ~9-12 working days. Fits in a normal release cycle.

---

## 12. Decision log placeholders

To be filled in as implementation lands:

- [ ] Picked between three dep-graph options? (Section 6)
- [ ] Snap-to-category behavior on categorical X axes? (Section 8, Q1)
- [ ] Tooltip widget mount strategy chosen? (Section 8, Q2)
- [ ] PNG export resolution / framing API shape? (Section 5)
- [ ] Animated data transitions in scope for Phase 4 or deferred? (Section 7)
