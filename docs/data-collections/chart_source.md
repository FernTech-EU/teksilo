<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ChartSource

Shared chart input for a complete model or a live tail projection.

## Builder methods at a glance

`with_all_series`, `with_series_view`, `with_series`, `set_series_visible`, `series_ids`, `series_count`, `series_id_at`, `only_series`, `point_count`, `structure_version`, `style_version`, `observe_changes`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-data/latest/teksilo_data/chart_source/index.html)

## `pub enum ChartSource`

A reactive chart input. Conversions retain shared handles and copy no points.

Window point indices (including hover and selection indices) are local to
the visible tail. A window bounds the displayed data, not source storage.

```rust
pub enum ChartSource<T: 'static> { /* variants */ }
```

### Variants

- **`Model`**: All points in the model.
- **`Window`**: The last configured number of points in each series.

### Methods

#### `pub fn with_all_series<R>(&self, f: impl FnOnce(&[SeriesView<'_, T>]) -> R) -> R`

Read the visible series and borrowed point slices.

#### `pub fn with_series_view<R>( &self, series: SeriesId, f: impl FnOnce(SeriesView<'_, T>) -> R, ) -> Option<R>`

Read one visible series, using window-local point indices.

#### `pub fn with_series<R>( &self, series: SeriesId, f: impl FnOnce(&str, Option<&ColorProp>, bool) -> R, ) -> Option<R>`

Read series metadata from the source.

#### `pub fn set_series_visible(&self, series: SeriesId, visible: bool)`

Change source visibility, for interactive legends.

#### `pub fn series_ids(&self) -> Vec<SeriesId>`

Ordered series identifiers.

#### `pub fn series_count(&self) -> usize`

Number of series, including hidden series.

#### `pub fn series_id_at(&self, index: usize) -> Option<SeriesId>`

Series at the given display position.

#### `pub fn only_series(&self) -> Option<SeriesId>`

Sole series, when the source has exactly one.

#### `pub fn point_count(&self, series: SeriesId) -> usize`

Number of visible points in a series.

#### `pub fn structure_version(&self) -> Signal<u64>`

Reactive version for geometry and accessible data.

#### `pub fn style_version(&self) -> Signal<u64>`

Reactive version for colors and patterns.

#### `pub fn observe_changes(&self, f: impl Fn(&ChartChange) + 'static) -> ObserverHandle`

Observe changes using indices relative to this input.
