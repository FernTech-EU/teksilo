<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ChartWindow

`ChartWindow<T>` — a "last N points per series" streaming projection over
a `crate::ChartModel`.

## Public functions

### `ChartWindow`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(source: ChartModel<T>, window_size: usize)`](#chartwindow-new) |
| | **Methods** |
| `usize` | [`window_size()`](#chartwindow-window_size) |
|  | [`set_window_size(window_size: usize)`](#chartwindow-set_window_size) |
| `ChartModel<T>` | [`source()`](#chartwindow-source) |
| `Signal<u64>` | [`structure_version()`](#chartwindow-structure_version) |
| `Signal<u64>` | [`style_version()`](#chartwindow-style_version) |
| `R` | [`with_all_series<R>(f: impl FnOnce(&[SeriesView<'_, T>]) -> R)`](#chartwindow-with_all_series) |
| `Option<R>` | [`with_series_view<R>(series: SeriesId, f: impl FnOnce(SeriesView<'_, T>) -> R)`](#chartwindow-with_series_view) |
| `usize` | [`series_count()`](#chartwindow-series_count) |
| `Vec<SeriesId>` | [`series_ids()`](#chartwindow-series_ids) |
| `usize` | [`point_count(series: SeriesId)`](#chartwindow-point_count) |
| `Option<R>` | [`with_series<R>(series: SeriesId, f: impl FnOnce(&str, Option<&ColorProp>, bool) -> R)`](#chartwindow-with_series) |
| `Option<R>` | [`with_point<R>(series: SeriesId, index: usize, f: impl FnOnce(&ChartDatum<T>) -> R)`](#chartwindow-with_point) |
| `ObserverHandle` | [`observe_changes(f: impl Fn(&ChartChange) + 'static)`](#chartwindow-observe_changes) |
| `Option<usize>` | [`first_changed_index(series: SeriesId)`](#chartwindow-first_changed_index) |

## Detailed description

Wraps a `ChartModel<T>` and exposes the tail
`window_size` points of every series — the live-scrolling-strip-chart
pattern (a sensor feed, a log-rate graph, a stock ticker). Unlike
`crate::ChartAggregate`, `ChartWindow` copies **no point data**: it
tracks, per series, the source index of the window's first visible point
(`starts`) and delegates every read straight through to the source. That
means a `ChartWindow<T>` needs no `T: Clone` bound at all.

#### Reactivity

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
use teksilo_data::{ChartModel, ChartWindow};
let model: ChartModel<i32> = ChartModel::new();
let s = model.add_series("sensor");
for i in 0..100 {
    model.push_point(s, i, i as f32);
}
let window = ChartWindow::new(model.clone(), 10);
assert_eq!(window.point_count(s), 10); // last 10 points only
```

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-data/latest/teksilo_data/chart_window/index.html)

<a id="chartwindow"></a>

## `pub struct ChartWindow`

A "last N points per series" streaming projection over a `ChartModel<T>`.

See the module documentation for semantics.

```rust
pub struct ChartWindow<T: 'static> { /* fields */ }
```

### Methods

<a id="chartwindow-new"></a>

#### `pub fn new(source: ChartModel<T>, window_size: usize) -> Self`

Wrap `source`, showing only the last `window_size` points of every
series.

<a id="chartwindow-window_size"></a>

#### `pub fn window_size(&self) -> usize`

The configured window size.

<a id="chartwindow-set_window_size"></a>

#### `pub fn set_window_size(&self, window_size: usize)`

Change the window size, rebuilding every series and emitting
`ChartChange::Reset`.

<a id="chartwindow-source"></a>

#### `pub fn source(&self) -> ChartModel<T>`

Access the source model. Visibility changes made by a legend are shared
with every other view of this model.

<a id="chartwindow-structure_version"></a>

#### `pub fn structure_version(&self) -> Signal<u64>`

Reactive version for changes to the visible data or window size.

<a id="chartwindow-style_version"></a>

#### `pub fn style_version(&self) -> Signal<u64>`

Reactive version for source colors and patterns.

<a id="chartwindow-with_all_series"></a>

#### `pub fn with_all_series<R>(&self, f: impl FnOnce(&[SeriesView<'_, T>]) -> R) -> R`

Read windowed series as borrowed slices, without copying point data.

<a id="chartwindow-with_series_view"></a>

#### `pub fn with_series_view<R>( &self, series: SeriesId, f: impl FnOnce(SeriesView<'_, T>) -> R, ) -> Option<R>`

Read a single windowed series. Indices are relative to the window.

<a id="chartwindow-series_count"></a>

#### `pub fn series_count(&self) -> usize`

Number of series (same set as the source).

<a id="chartwindow-series_ids"></a>

#### `pub fn series_ids(&self) -> Vec<SeriesId>`

The series ids, in the source's display order.

<a id="chartwindow-point_count"></a>

#### `pub fn point_count(&self, series: SeriesId) -> usize`

Number of points currently visible in the window for `series`.

<a id="chartwindow-with_series"></a>

#### `pub fn with_series<R>( &self, series: SeriesId, f: impl FnOnce(&str, Option<&ColorProp>, bool) -> R, ) -> Option<R>`

Access a series' metadata (delegates straight through to the
source). Returns `None` if `series` is unknown.

<a id="chartwindow-with_point"></a>

#### `pub fn with_point<R>( &self, series: SeriesId, index: usize, f: impl FnOnce(&ChartDatum<T>) -> R, ) -> Option<R>`

Access the point at window-local `index` within `series`. Returns
`None` if `series` is unknown or `index` is outside the window.

<a id="chartwindow-observe_changes"></a>

#### `pub fn observe_changes(&self, f: impl Fn(&ChartChange) + 'static) -> ObserverHandle`

Register an observer for translated window changes. Returns an
`ObserverHandle` — dropping it removes the callback.

<a id="chartwindow-first_changed_index"></a>

#### `pub fn first_changed_index(&self, series: SeriesId) -> Option<usize>`

First window-local index of `series` whose content may differ since
the latest translated change. Per-series (chart data is 2-level:
series, then points), unlike
`SortFilterListModel::first_changed_index`'s
single flat value. `None` if `series` is unknown or unaffected yet.
