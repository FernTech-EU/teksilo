<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ChartWindow

`ChartWindow<T>` — a "last N points per series" streaming projection over
a `crate::ChartModel`.

Wraps a `ChartModel<T>` and exposes the tail
`window_size` points of every series — the live-scrolling-strip-chart
pattern (a sensor feed, a log-rate graph, a stock ticker). Unlike
`crate::ChartAggregate`, `ChartWindow` copies **no point data**: it
tracks, per series, the source index of the window's first visible point
(`starts`) and delegates every read straight through to the source. That
means a `ChartWindow<T>` needs no `T: Clone` bound at all.

## Reactivity

The upstream `ChartChange` stream is translated, not collapsed to a
blanket `Reset` (unlike `crate::SortFilterListModel`, where an
arbitrary sort-key move makes fine-grained translation unsafe — a
fixed-size tail window has no such hazard): a tail append into a full
window becomes a `PointsRemoved` + `PointsInserted` pair (the window
slides), a tail append into a still-growing window becomes a plain
`PointsInserted`, and symmetrically a **tail removal** (trimming the
series' own end — e.g. discarding a bad trailing reading) becomes the
mirror-image `PointsRemoved` + `PointsInserted` pair: points beyond the
new total drop out of the window, and if the window slid backward to
stay full, the newly-uncovered prefix is revealed as an insertion.
Anything that isn't a clean tail append/removal (a mid-series insert or
removal) falls back to a per-series rebuild reported as
`SeriesDataReplaced`.

```ignore
use bastyde_data::{ChartModel, ChartWindow};
let model: ChartModel<i32> = ChartModel::new();
let s = model.add_series("sensor");
for i in 0..100 {
    model.push_point(s, i, i as f32);
}
let window = ChartWindow::new(model.clone(), 10);
assert_eq!(window.point_count(s), 10); // last 10 points only
```

## Builder methods at a glance

`window_size`, `set_window_size`, `series_count`, `series_ids`, `point_count`, `with_series`, `with_point`, `observe_changes`, `first_changed_index`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_data/chart_window/index.html)

## `pub struct ChartWindow`

A "last N points per series" streaming projection over a `ChartModel<T>`.

See the module documentation for semantics.

```rust
pub struct ChartWindow<T: 'static> { /* fields */ }
```

### Methods

#### `pub fn new(source: ChartModel<T>, window_size: usize) -> Self`

Wrap `source`, showing only the last `window_size` points of every
series.

#### `pub fn window_size(&self) -> usize`

The configured window size.

#### `pub fn set_window_size(&self, window_size: usize)`

Change the window size, rebuilding every series and emitting
`ChartChange::Reset`.

#### `pub fn series_count(&self) -> usize`

Number of series (same set as the source).

#### `pub fn series_ids(&self) -> Vec<SeriesId>`

The series ids, in the source's display order.

#### `pub fn point_count(&self, series: SeriesId) -> usize`

Number of points currently visible in the window for `series`.

#### `pub fn with_series<R>( &self, series: SeriesId, f: impl FnOnce(&str, Option<&ColorProp>, bool) -> R, ) -> Option<R>`

Access a series' metadata (delegates straight through to the
source). Returns `None` if `series` is unknown.

#### `pub fn with_point<R>( &self, series: SeriesId, index: usize, f: impl FnOnce(&ChartDatum<T>) -> R, ) -> Option<R>`

Access the point at window-local `index` within `series`. Returns
`None` if `series` is unknown or `index` is outside the window.

#### `pub fn observe_changes(&self, f: impl Fn(&ChartChange) + 'static) -> ObserverHandle`

Register an observer for translated window changes. Returns an
`ObserverHandle` — dropping it removes the callback.

#### `pub fn first_changed_index(&self, series: SeriesId) -> Option<usize>`

First window-local index of `series` whose content may differ since
the latest translated change. Per-series (chart data is 2-level:
series, then points), unlike
`SortFilterListModel::first_changed_index`'s
single flat value. `None` if `series` is unknown or unaffected yet.
