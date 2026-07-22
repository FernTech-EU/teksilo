<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ChartChange

ChartChange — change notifications and stable series identifiers for chart collections.

`SeriesId` is an opaque, stable handle for a series in a `crate::ChartModel`.
Because `ChartModel` is backed by a slotmap, `SeriesId` values survive arbitrary
series insertions, removals, and reorders — only removing the series itself
invalidates it. `ChartChange` describes exactly what mutated (at the series
level or the point level within a series) so that projections
(`ChartWindow`, `ChartAggregate`) and consumers (`ChartSelection`) can refresh
or adjust incrementally instead of rebuilding from scratch.

Consumers typically receive `ChartChange` values through an observer
registered via `crate::ChartModel::observe_changes`, which fires
synchronously (before the registering call returns) after each mutation.

```ignore
// ChartModel::observe_changes returns an ObserverHandle whose drop
// unregisters the callback — keep it alive for the observer's lifetime.
use bastyde_data::{ChartModel, ChartChange};
let model: ChartModel<String> = ChartModel::new();
let _handle = model.observe_changes(|change| {
    println!("{change:?}");
});
model.add_series("Revenue");
// prints: SeriesInserted { index: 0, series: SeriesId(...) }
```

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_data/chart_change/index.html)

## `pub struct SeriesId`

Opaque identifier for a series in a `ChartModel`.

`SeriesId` values are stable across mutations — inserting or removing
other series does not invalidate existing `SeriesId` handles (they are
SlotMap keys).

```rust
pub struct SeriesId(slotmap::DefaultKey);
```

## `pub enum ChartChange`

Describes a mutation to a chart's series or point data. Emitted by
`ChartModel<T>` automatically.

```rust
pub enum ChartChange { /* variants */ }
```

### Variants

- **`SeriesInserted`** — A series was inserted at the given index.
- **`SeriesRemoved`** — A series (and all of its points) was removed.
- **`SeriesMoved`** — A series was moved to a new position among its siblings.
- **`SeriesRenamed`** — A series' display name changed.
- **`SeriesColorChanged`** — A series' explicit color changed (set or cleared). The only variant that bumps `crate::ChartModel::style_version` rather than `crate::ChartModel::structure_version`.
- **`SeriesVisibilityChanged`** — A series' visibility flag changed.
- **`PointsInserted`** — Points were inserted; `range` holds the indices of the newly inserted points within `series`.
- **`PointsRemoved`** — Points were removed; `range` holds the indices they occupied *before* removal within `series`.
- **`PointUpdated`** — A single point's data changed in place without any structural shift.
- **`SeriesDataReplaced`** — A series' entire point list was replaced; consumers must discard cached state for that series and rebuild it.
- **`Reset`** — The entire chart was replaced. Consumers should discard all state and rebuild.
