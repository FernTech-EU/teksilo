<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ChartModel

`ChartModel<T>` — concrete reactive multi-series chart data model.

`ChartModel<T>` owns an ordered collection of named series, each holding a
`Vec<ChartDatum<T>>` (a `category: T` paired with a numeric `value: f32`),
in a flat SlotMap arena — the same shape as `crate::TreeModel`. Every
mutation (series add/remove/move/rename/recolor/show-hide, point
push/insert/remove/update/replace) emits a `ChartChange` to all
registered observers *and* bumps one of two reactive version signals:
`ChartModel::style_version` (color changes only — a paint-only signal a
chart can bind at `BindingLevel::RepaintOnly`) or
`ChartModel::structure_version` (everything else — series/point shape,
bound at `BindingLevel::Relayout`/`Rebuild`). Series identity is a stable,
versioned `SeriesId` (a SlotMap key) that is never reused after removal.

Cloning produces a second handle to the **same** data — all handles share
series/points and receive the same change notifications. Register
observers via `observe_changes`; the
returned `ObserverHandle` is RAII — dropping it unregisters the callback.

For a bounded "last N points" streaming view use
`ChartWindow`. For bucketed/rolled-up display use
`ChartAggregate`. For point-level selection use
`ChartSelection`.

```rust
# use teksilo_data::{ChartModel, ChartSeries, ChartDatum};
let model = ChartModel::from_series_vec(vec![
    ChartSeries::new("Revenue").data(vec![
        ChartDatum::new("Q1".to_string(), 10.0),
        ChartDatum::new("Q2".to_string(), 20.0),
    ]),
]);
assert_eq!(model.series_count(), 1);
let s = model.series_id_at(0).unwrap();
assert_eq!(model.point_count(s), 2);

model.push_point(s, "Q3".to_string(), 30.0);
assert_eq!(model.point_count(s), 3);
```

## Builder methods at a glance

`from_series_vec`, `from_points`, `only_series`, `add_series`, `insert_series`, `remove_series`, `rename_series`, `set_series_color`, `clear_series_color`, `set_series_visible`, `move_series`, `clear`, `push_point`, `insert_point`, `remove_point`, `update_point`, `replace_series_data`, `series_count`, `series_ids`, `series_id_at`, `series_index_of`, `point_count`, `with_series`, `with_point`, `with_series_view`, `with_all_series`, `structure_version`, `style_version`, `observe_changes`, `debug_named`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_data/chart_model/index.html)

## `pub struct ChartDatum`

One numeric data point at a category/x-axis position, with an optional
per-point color that overrides the series color (bar charts only).

```rust
pub struct ChartDatum<T> { /* fields */ }
```

### Methods

#### `pub fn new(category: T, value: f32) -> Self`

#### `pub fn with_color(mut self, color: impl Into<ColorProp>) -> Self`

Override this point's color (a bar's fill). Ignored by line/pie charts,
which color by series.

## `pub struct ChartSeries`

A named series of data points with an optional explicit color and a
visibility flag, used to construct a `ChartModel` (via
`ChartModel::from_series_vec`) or to describe one series' desired
shape. Unlike the model, `visible` here is a plain `bool` — reactivity
lives in the model's `ChartModel::structure_version` /
`ChartModel::style_version` signals, not in this construction DTO.

```rust
pub struct ChartSeries<T> { /* fields */ }
```

### Methods

#### `pub fn new(name: impl Into<String>) -> Self`

#### `pub fn color(mut self, color: impl Into<ColorProp>) -> Self`

#### `pub fn visibility(mut self, visible: bool) -> Self`

#### `pub fn push(&mut self, category: T, value: f32)`

#### `pub fn data(mut self, points: Vec<ChartDatum<T>>) -> Self`

## `pub struct SeriesView`

A read-only, borrowed view over one series — returned by
`ChartModel::with_series_view` / `ChartModel::with_all_series`.

```rust
pub struct SeriesView<'a, T> { /* fields */ }
```

## `pub struct ChartModel`

A concrete reactive multi-series chart data model.

`ChartModel<T>` is `Clone` — cloning produces a second handle to the same
data. Multiple charts can hold clones and all see the same series and
points, and receive the same `ChartChange` notifications.

```rust
pub struct ChartModel<T: 'static> { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create an empty chart model with no series.

#### `pub fn from_series_vec(series: Vec<ChartSeries<T>>) -> Self`

Build a model directly from a vector of `ChartSeries` DTOs — the
primary constructor. Populates the arena in one pass with no
per-item notification (mirrors `crate::ListModel::from_vec`).

#### `pub fn from_points(points: Vec<ChartDatum<T>>) -> Self`

Build a model with a single anonymous, visible series holding
`points` — the flat/pie-chart path where series structure doesn't
matter.

#### `pub fn only_series(&self) -> Option<SeriesId>`

The model's sole series id, iff it has exactly one series.

#### `pub fn add_series(&self, name: impl Into<String>) -> SeriesId`

Append a new, empty, visible series named `name`.

#### `pub fn insert_series(&self, index: usize, name: impl Into<String>) -> SeriesId`

Insert a new, empty, visible series named `name` at `index`.

# Panics
Panics if `index > series_count()`.

#### `pub fn remove_series(&self, series: SeriesId)`

Remove a series and all of its points.

# Panics
Panics if `series` is unknown.

#### `pub fn rename_series(&self, series: SeriesId, name: impl Into<String>)`

Rename a series. A no-op (no notify, no version bump) if `name`
already matches the current value.

# Panics
Panics if `series` is unknown.

#### `pub fn set_series_color(&self, series: SeriesId, color: impl Into<ColorProp>)`

Set a series' explicit color. Bumps `Self::style_version` (not
`Self::structure_version`) — this is a paint-only change. A no-op
(no notify, no version bump) if `color` already matches the current
value.

# Panics
Panics if `series` is unknown.

#### `pub fn clear_series_color(&self, series: SeriesId)`

Clear a series' explicit color (falls back to the chart's palette).
Bumps `Self::style_version`. A no-op (no notify, no version bump)
if the series already has no explicit color.

# Panics
Panics if `series` is unknown.

#### `pub fn set_series_visible(&self, series: SeriesId, visible: bool)`

Show or hide a series. A no-op (no notify, no version bump) if
`visible` already matches the current value.

# Panics
Panics if `series` is unknown.

#### `pub fn move_series(&self, series: SeriesId, to: usize)`

Move a series to a new position among its siblings. A no-op (no
notify, no version bump) if `to` is already the series' position.

# Panics
Panics if `series` is unknown or `to` is out of bounds.

#### `pub fn clear(&self)`

Remove every series.

#### `pub fn push_point(&self, series: SeriesId, category: T, value: f32)`

Append a point to the end of `series`.

# Panics
Panics if `series` is unknown.

#### `pub fn insert_point(&self, series: SeriesId, index: usize, category: T, value: f32)`

Insert a point at `index` within `series`.

# Panics
Panics if `series` is unknown or `index > point_count(series)`.

#### `pub fn remove_point(&self, series: SeriesId, index: usize) -> ChartDatum<T>`

Remove and return the point at `index` within `series`.

# Panics
Panics if `series` is unknown or `index >= point_count(series)`.

#### `pub fn update_point(&self, series: SeriesId, index: usize, category: T, value: f32)`

Replace the point at `index` within `series`.

# Panics
Panics if `series` is unknown or `index >= point_count(series)`.

#### `pub fn replace_series_data(&self, series: SeriesId, points: Vec<ChartDatum<T>>)`

Replace `series`' entire point list.

# Panics
Panics if `series` is unknown.

#### `pub fn series_count(&self) -> usize`

Number of series.

#### `pub fn series_ids(&self) -> Vec<SeriesId>`

The series ids, in display order.

#### `pub fn series_id_at(&self, index: usize) -> Option<SeriesId>`

The series id at `index`, if any.

#### `pub fn series_index_of(&self, series: SeriesId) -> Option<usize>`

The display index of `series`, if it exists.

#### `pub fn point_count(&self, series: SeriesId) -> usize`

Number of points in `series` (0 if unknown).

#### `pub fn with_series<R>( &self, series: SeriesId, f: impl FnOnce(&str, Option<&ColorProp>, bool) -> R, ) -> Option<R>`

Access a series' metadata (name, color, visibility) via a callback.
Returns `None` if `series` is unknown.

#### `pub fn with_point<R>( &self, series: SeriesId, index: usize, f: impl FnOnce(&ChartDatum<T>) -> R, ) -> Option<R>`

Access a point within `series` via a callback. Returns `None` if the
series or index is unknown.

#### `pub fn with_series_view<R>( &self, series: SeriesId, f: impl FnOnce(SeriesView<'_, T>) -> R, ) -> Option<R>`

Access a whole-series view (metadata + points slice) via a callback.
Returns `None` if `series` is unknown.

#### `pub fn with_all_series<R>(&self, f: impl FnOnce(&[SeriesView<'_, T>]) -> R) -> R`

Access every series as an ordered slice of views via a callback.

#### `pub fn structure_version(&self) -> Signal<u64>`

Structural version signal — bumped by every mutation except a color
change (series add/remove/move/rename/show-hide, all point ops).
Bind at `BindingLevel::Relayout` or `Rebuild`.

Ordering: every mutator notifies the `ChartChange` observers
registered via `Self::observe_changes` *before* bumping this
signal — see the note on `observe_changes` for what that means for a
callback that reads the signal back synchronously.

#### `pub fn style_version(&self) -> Signal<u64>`

Style version signal — bumped only by a series color change. Bind at
`BindingLevel::RepaintOnly`. Same notify-before-bump ordering as
`Self::structure_version` — see `Self::observe_changes`.

#### `pub fn observe_changes(&self, f: impl Fn(&ChartChange) + 'static) -> ObserverHandle`

Register an observer that is called on every mutation.
Returns an `ObserverHandle` — dropping it removes the callback.

**Ordering contract:** every mutator calls this observer *before*
bumping `Self::structure_version` / `Self::style_version` (see
e.g. `rename_series`, `push_point`) — notify, then bump. This is
intentional, not an implementation accident: it lets a `ChartChange`
callback distinguish "did I get here via the change I'm reacting to"
from "did something else bump the version already", by comparing the
version signal's value inside the callback against a value captured
before the mutation. The flip side: a callback that reads
`structure_version()`/`style_version()` synchronously **inside**
itself always observes the **pre-bump** value for the mutation
currently being notified — the bump hasn't happened yet. Don't use
the version signal from inside a `ChartChange` observer as a proxy
for "has this specific mutation been applied" — the `ChartChange`
argument already tells you that; use the signal for *external*
bind-and-rerun consumers (widgets), not from within the notify path
itself.

#### `pub fn debug_named(self, _name: impl Into<String>) -> Self`

Register this model with the debug inspector under `name`. In
release builds (`!cfg(debug_assertions)`) this is a no-op
pass-through so call sites stay free of `#[cfg]` lines.

Idempotent on repeated calls — the latest registration wins.
The registration drops automatically when the last `ChartModel`
handle is freed (the adapter the registry holds is `Weak`).
