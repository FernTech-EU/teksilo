<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ChartSelection

`ChartSelection` — point-level selection state for chart widgets.

`ChartSelection` manages which `(series, point index)` pairs are
selected across a `crate::ChartModel` — the chart counterpart of
`crate::SelectionModel` (flat lists) and
`crate::KeyedSelectionModel` (keyed collections). It is a
share-by-clone handle: pass a clone to each chart that should share
selection state. The current selection is exposed as a reactive
`Signal<HashSet<(SeriesId, usize)>>` so widgets can bind to it without
polling.

`HashSet` (not `BTreeSet`) is used because `SeriesId` is intentionally
**not** `Ord` (it's an opaque SlotMap key, mirroring `crate::NodeId`) —
there is no natural ordering across series, only within one series'
point indices. This is the same rationale as
`crate::KeyedSelectionModel`, which uses `HashSet<K>` for the same
reason.

Three selection behaviours are available via
(toggle + anchor-based range extension). `ChartSelection::extend_to`
only extends within the anchor's own series — a cross-series "range" has
no natural order, so it falls back to a single-point select.
`ChartSelection::adjust` keeps selected points consistent as the
source model mutates (series removed, points inserted/removed) — call it
from your own model observer, or skip the wiring entirely with
`ChartSelection::attached` (equivalently, `ChartSelection::attach` on
an existing selection), which subscribes internally and calls `adjust`
for you, the same way `crate::ChartWindow`/`crate::ChartAggregate`
self-wire in their own constructors. Forgetting to wire `adjust` up
manually otherwise leaves the selection silently stale after a mutation.

```rust
# use teksilo_data::{ChartModel, ChartSelection, SelectionMode};
let model: ChartModel<i32> = ChartModel::new();
let s = model.add_series("s");
for i in 0..5 {
    model.push_point(s, i, i as f32);
}

let sel = ChartSelection::attached(SelectionMode::Multi, &model);
sel.select_point(s, 1);
sel.extend_to(s, 3);
assert_eq!(sel.count(), 3); // (s,1), (s,2), (s,3)

model.remove_point(s, 0); // upstream mutation — no manual adjust() call
assert_eq!(sel.count(), 3); // (s,0), (s,1), (s,2) — shifted down

sel.clear();
assert_eq!(sel.count(), 0);
```

## Builder methods at a glance

`attached`, `attach`, `mode`, `selection_signal`, `is_selected`, `selected_points`, `count`, `select_point`, `toggle_point`, `extend_to`, `select_points`, `clear`, `adjust`, `prune`, `debug_named`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-data/latest/teksilo_data/chart_selection/index.html)

## `pub struct ChartSelection`

Point-level selection state for a chart, keyed by `(series, point
index)`. See module documentation for semantics.

```rust
pub struct ChartSelection { /* fields */ }
```

### Methods

#### `pub fn new(mode: SelectionMode) -> Self`

Create a new chart selection with the given mode.

#### `pub fn attached<T: 'static>(mode: SelectionMode, model: &ChartModel<T>) -> Self`

Create a selection that self-wires to `model`: every `ChartChange`
the model emits is automatically routed through `Self::adjust`, so
a point removed or shifted upstream never leaves a stale selected
index behind. Equivalent to `ChartSelection::new(mode)` plus
`model.observe_changes(|c| sel.adjust(c))`, minus the easy-to-forget
wiring — mirrors how `crate::ChartWindow` and
`crate::ChartAggregate` self-wire in their own constructors. The
manual `Self::adjust` path still works — call it yourself instead
if you'd rather relay through a custom change pipeline.

#### `pub fn attach<T: 'static>(&self, model: &ChartModel<T>)`

Subscribe this selection to `model`'s changes, applying
`Self::adjust` on every `ChartChange`. The subscription is held
internally (shared across clones — see `Clone`), so it stays alive
as long as any handle to this selection does; calling `attach`
again (on this handle or any clone) drops the previous subscription
and installs the new one.

The subscription closure captures only `selection` + `anchor`, not a
full `Self` — capturing `Self` would pull in `attach_handle` too,
which holds this very `ObserverHandle`, forming an `Rc` cycle that
would leak the subscription instead of tearing down when every
`ChartSelection` handle drops.

#### `pub fn mode(&self) -> SelectionMode`

The selection mode.

#### `pub fn selection_signal(&self) -> Signal<HashSet<(SeriesId, usize)>>`

A clone of the selection signal for reactive binding.

#### `pub fn is_selected(&self, series: SeriesId, index: usize) -> bool`

Whether `(series, index)` is currently selected.

#### `pub fn selected_points(&self) -> Vec<(SeriesId, usize)>`

The currently selected points (unordered snapshot).

#### `pub fn count(&self) -> usize`

Number of selected points.

#### `pub fn select_point(&self, series: SeriesId, index: usize)`

Select a single point, clearing the previous selection and setting
the anchor.

#### `pub fn toggle_point(&self, series: SeriesId, index: usize)`

Toggle a point (Ctrl+click in Multi mode; acts as `select_point` in
Single mode).

#### `pub fn extend_to(&self, series: SeriesId, target: usize)`

Extend the selection from the anchor to `(series, target)` (for
Shift+click). Only extends **within the anchor's own series** — if
the anchor is unset or belongs to a different series, falls back to
a single-point select of `(series, target)`.

#### `pub fn select_points( &self, points: impl IntoIterator<Item = (SeriesId, usize)>, additive: bool, )`

Replace the selection with `points` (or, when `additive`, union
them into the current selection). Used by rubber-band / marquee
selection. In `Single` mode an arbitrary one wins; `None` mode is a
no-op.

#### `pub fn clear(&self)`

Clear the selection and anchor.

#### `pub fn adjust(&self, change: &ChartChange)`

React to an upstream `ChartChange`, keeping selection consistent
with the model: a removed or wholesale-replaced series drops its
selected points (and the anchor, if it pointed there); point
insertions/removals shift or drop indices within their series.
Series metadata changes (rename/recolor/visibility/move/insert) and
in-place point updates never affect which points are selected.

#### `pub fn prune(&self, exists: impl Fn(SeriesId, usize) -> bool)`

Drop any selected point for which `exists` returns false.

#### `pub fn debug_named(self, _name: impl Into<String>) -> Self`

Register this selection with the debug inspector under `name`. In
release builds (`!cfg(debug_assertions)`) this is a no-op
pass-through so call sites stay free of `#[cfg]` lines.

Idempotent on repeated calls — the latest registration wins. The
registration drops automatically when the last `ChartSelection`
handle is freed (the strong adapter `Rc` lives inside a shared
holder; the registry holds only a `Weak`).
