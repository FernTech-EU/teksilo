<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ChartAggregate

`ChartAggregate<T>` — a bucket/rollup projection over a `crate::ChartModel`.

Wraps a `ChartModel<T>` and exposes each series
reduced into fixed-size buckets of `bucket_size` source points, each
bucket collapsed to one `crate::ChartDatum` via a `ChartAggregateFn`
(`Mean` / `Sum` / `Min` / `Max` / `First` / `Last` / `Custom`) — the
"downsample a long series for display" pattern (a year of daily
sensor readings shown as weekly means, a tick feed shown as 1-minute
bars). Bucket `b` covers source indices `[b*bucket_size,
min((b+1)*bucket_size, n))`; a trailing partial bucket is included. A
bucket's category is its first member's category.

Unlike `crate::ChartWindow` (which reads straight through to the
source), `ChartAggregate` **materializes** its buckets — a bucket's
category is a *clone* of a source point's category, so constructing or
rebuilding a `ChartAggregate<T>` requires `T: Clone`. Once built,
read-only queries (`point_count`, `with_point`, …) need only `T:
'static`.

## Reactivity

A tail append that doesn't change the bucket count updates the
now-not-yet-full last bucket in place (`PointUpdated`); a tail append
that starts a new bucket finalizes the previous last bucket
(`PointUpdated`) and appends the new one(s) (`PointsInserted`).
Symmetrically, a **tail removal** that doesn't eliminate the last bucket
recomputes it in place (`PointUpdated`, since it lost some of its
points); one that eliminates one or more trailing buckets recomputes the
new last bucket the same way and then drops the buckets beyond it
(`PointsRemoved`). A mid-series insert or removal (front or interior)
falls back to a full per-series rebuild reported as
`SeriesDataReplaced`. A `PointUpdated` recomputes just its own bucket.

```ignore
use teksilo_data::{ChartModel, ChartAggregate, ChartAggregateFn};
let model: ChartModel<i32> = ChartModel::new();
let s = model.add_series("daily");
for i in 0..70 {
    model.push_point(s, i, i as f32);
}
let weekly = ChartAggregate::new(model, 7, ChartAggregateFn::Mean);
assert_eq!(weekly.point_count(s), 10); // 70 / 7
```

## Builder methods at a glance

`set_bucket_size`, `set_aggregate_fn`, `bucket_size`, `series_count`, `series_ids`, `point_count`, `with_series`, `with_point`, `observe_changes`, `first_changed_index`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-data/latest/teksilo_data/chart_aggregate/index.html)

## `pub enum ChartAggregateFn`

A reduction applied to the numeric values within one bucket.

```rust
pub enum ChartAggregateFn { /* variants */ }
```

### Variants

- **`Mean`** — Arithmetic mean of the bucket's values (`0.0` for an empty bucket).
- **`Sum`** — Sum of the bucket's values.
- **`Min`** — Smallest value in the bucket.
- **`Max`** — Largest value in the bucket.
- **`First`** — The bucket's first value.
- **`Last`** — The bucket's last value.
- **`Custom`** — A caller-supplied reduction.

### Methods

#### `pub fn apply(&self, values: &[f32]) -> f32`

Apply the reduction to a bucket's values.

On an empty slice, every built-in variant returns `0.0`
(`Mean`/`Min`/`Max`/`First`/`Last`) or the empty sum (`Sum`, also
`0.0`) — a uniform, unsurprising convention rather than `Min`/`Max`
leaking their fold seed (`±INFINITY`) into a chart value. `Custom`
returns whatever the supplied closure computes for `&[]`. No internal
caller actually passes an empty slice — `compute_bucket_datum` bails
out before calling `apply` for an empty bucket — so this only bites a
direct caller.

## `pub struct ChartAggregate`

A bucket/rollup projection over a `ChartModel<T>`.

See the module documentation for semantics.

```rust
pub struct ChartAggregate<T: 'static> { /* fields */ }
```

### Methods

#### `pub fn new(source: ChartModel<T>, bucket_size: usize, aggregate_fn: ChartAggregateFn) -> Self`

Wrap `source`, bucketing every series into groups of `bucket_size`
source points reduced via `aggregate_fn`. `bucket_size` is clamped
to a minimum of 1.

#### `pub fn set_bucket_size(&self, bucket_size: usize)`

Change the bucket size, rebuilding every series and emitting
`ChartChange::Reset`. Clamped to a minimum of 1.

#### `pub fn set_aggregate_fn(&self, aggregate_fn: ChartAggregateFn)`

Change the aggregate reduction, rebuilding every series and emitting
`ChartChange::Reset`.

#### `pub fn bucket_size(&self) -> usize`

The configured bucket size.

#### `pub fn series_count(&self) -> usize`

Number of series (same set as the source).

#### `pub fn series_ids(&self) -> Vec<SeriesId>`

The series ids, in the source's display order.

#### `pub fn point_count(&self, series: SeriesId) -> usize`

Number of buckets currently materialized for `series`.

#### `pub fn with_series<R>( &self, series: SeriesId, f: impl FnOnce(&str, Option<&ColorProp>, bool) -> R, ) -> Option<R>`

Access a series' metadata (delegates straight through to the
source). Returns `None` if `series` is unknown.

#### `pub fn with_point<R>( &self, series: SeriesId, index: usize, f: impl FnOnce(&ChartDatum<T>) -> R, ) -> Option<R>`

Access the bucket at `index` within `series`. Returns `None` if
`series` or `index` is unknown.

#### `pub fn observe_changes(&self, f: impl Fn(&ChartChange) + 'static) -> ObserverHandle`

Register an observer for translated bucket changes. Returns an
`ObserverHandle` — dropping it removes the callback.

#### `pub fn first_changed_index(&self, series: SeriesId) -> Option<usize>`

First bucket index of `series` whose content may differ since the
latest translated change. `None` if `series` is unknown or
unaffected yet.
