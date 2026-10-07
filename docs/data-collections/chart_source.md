<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ChartSource

Shared chart input for a complete model or a live tail projection.

## Public functions

### `ChartSource`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `R` | [`with_all_series<R>(f: impl FnOnce(&[SeriesView<'_, T>]) -> R)`](#chartsource-with_all_series) |
| `Option<R>` | [`with_series_view<R>(series: SeriesId, f: impl FnOnce(SeriesView<'_, T>) -> R)`](#chartsource-with_series_view) |
| `Option<R>` | [`with_series<R>(series: SeriesId, f: impl FnOnce(&str, Option<&ColorProp>, bool) -> R)`](#chartsource-with_series) |
|  | [`set_series_visible(series: SeriesId, visible: bool)`](#chartsource-set_series_visible) |
| `Vec<SeriesId>` | [`series_ids()`](#chartsource-series_ids) |
| `usize` | [`series_count()`](#chartsource-series_count) |
| `Option<SeriesId>` | [`series_id_at(index: usize)`](#chartsource-series_id_at) |
| `Option<SeriesId>` | [`only_series()`](#chartsource-only_series) |
| `usize` | [`point_count(series: SeriesId)`](#chartsource-point_count) |
| `Signal<u64>` | [`structure_version()`](#chartsource-structure_version) |
| `Signal<u64>` | [`style_version()`](#chartsource-style_version) |
| `ObserverHandle` | [`observe_changes(f: impl Fn(&ChartChange) + 'static)`](#chartsource-observe_changes) |

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-data/latest/teksilo_data/chart_source/index.html)

<a id="chartsource"></a>

## `pub enum ChartSource`

A reactive chart input. Conversions retain shared handles and copy no points.

Window point indices (including hover and selection indices) are local to
the visible tail. A window bounds the displayed data, not source storage.

```rust
pub enum ChartSource<T: 'static> { /* variants */ }
```

### Variants

- **`Model`** — All points in the model.
- **`Window`** — The last configured number of points in each series.

### Methods

<a id="chartsource-with_all_series"></a>

#### `pub fn with_all_series<R>(&self, f: impl FnOnce(&[SeriesView<'_, T>]) -> R) -> R`

Read the visible series and borrowed point slices.

<a id="chartsource-with_series_view"></a>

#### `pub fn with_series_view<R>( &self, series: SeriesId, f: impl FnOnce(SeriesView<'_, T>) -> R, ) -> Option<R>`

Read one visible series, using window-local point indices.

<a id="chartsource-with_series"></a>

#### `pub fn with_series<R>( &self, series: SeriesId, f: impl FnOnce(&str, Option<&ColorProp>, bool) -> R, ) -> Option<R>`

Read series metadata from the source.

<a id="chartsource-set_series_visible"></a>

#### `pub fn set_series_visible(&self, series: SeriesId, visible: bool)`

Change source visibility, for interactive legends.

<a id="chartsource-series_ids"></a>

#### `pub fn series_ids(&self) -> Vec<SeriesId>`

Ordered series identifiers.

<a id="chartsource-series_count"></a>

#### `pub fn series_count(&self) -> usize`

Number of series, including hidden series.

<a id="chartsource-series_id_at"></a>

#### `pub fn series_id_at(&self, index: usize) -> Option<SeriesId>`

Series at the given display position.

<a id="chartsource-only_series"></a>

#### `pub fn only_series(&self) -> Option<SeriesId>`

Sole series, when the source has exactly one.

<a id="chartsource-point_count"></a>

#### `pub fn point_count(&self, series: SeriesId) -> usize`

Number of visible points in a series.

<a id="chartsource-structure_version"></a>

#### `pub fn structure_version(&self) -> Signal<u64>`

Reactive version for geometry and accessible data.

<a id="chartsource-style_version"></a>

#### `pub fn style_version(&self) -> Signal<u64>`

Reactive version for colors and patterns.

<a id="chartsource-observe_changes"></a>

#### `pub fn observe_changes(&self, f: impl Fn(&ChartChange) + 'static) -> ObserverHandle`

Observe changes using indices relative to this input.
