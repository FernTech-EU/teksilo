// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `ChartAggregate<T>` — a bucket/rollup projection over a [`crate::ChartModel`].
//!
//! Wraps a [`ChartModel<T>`](crate::ChartModel) and exposes each series
//! reduced into fixed-size buckets of `bucket_size` source points, each
//! bucket collapsed to one [`crate::ChartDatum`] via a [`ChartAggregateFn`]
//! (`Mean` / `Sum` / `Min` / `Max` / `First` / `Last` / `Custom`) — the
//! "downsample a long series for display" pattern (a year of daily
//! sensor readings shown as weekly means, a tick feed shown as 1-minute
//! bars). Bucket `b` covers source indices `[b*bucket_size,
//! min((b+1)*bucket_size, n))`; a trailing partial bucket is included. A
//! bucket's category is its first member's category.
//!
//! Unlike [`crate::ChartWindow`] (which reads straight through to the
//! source), `ChartAggregate` **materializes** its buckets — a bucket's
//! category is a *clone* of a source point's category, so constructing or
//! rebuilding a `ChartAggregate<T>` requires `T: Clone`. Once built,
//! read-only queries (`point_count`, `with_point`, …) need only `T:
//! 'static`.
//!
//! ## Reactivity
//!
//! A tail append that doesn't change the bucket count updates the
//! now-not-yet-full last bucket in place (`PointUpdated`); a tail append
//! that starts a new bucket finalizes the previous last bucket
//! (`PointUpdated`) and appends the new one(s) (`PointsInserted`). A
//! mid-series insert or any removal falls back to a full per-series rebuild
//! reported as `SeriesDataReplaced`. A `PointUpdated` recomputes just its
//! own bucket.
//!
//! ```ignore
//! use bastyde_data::{ChartModel, ChartAggregate, ChartAggregateFn};
//! let model: ChartModel<i32> = ChartModel::new();
//! let s = model.add_series("daily");
//! for i in 0..70 {
//!     model.push_point(s, i, i as f32);
//! }
//! let weekly = ChartAggregate::new(model, 7, ChartAggregateFn::Mean);
//! assert_eq!(weekly.point_count(s), 10); // 70 / 7
//! ```

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use bastyde_core::ObserverHandle;
use bastyde_core::color_prop::ColorProp;

use crate::chart_change::{ChartChange, SeriesId};
use crate::chart_model::{ChartDatum, ChartModel};

type SeriesIdsFn = Rc<dyn Fn() -> Vec<SeriesId>>;
type PointCountFn = Rc<dyn Fn(SeriesId) -> usize>;
type WithPointFn<T> = Rc<dyn Fn(SeriesId, usize, &dyn Fn(&ChartDatum<T>))>;
type WithSeriesFn = Rc<dyn Fn(SeriesId, &dyn Fn(&str, Option<&ColorProp>, bool))>;
type ObserveChartFn = Rc<dyn Fn(Box<dyn Fn(&ChartChange)>) -> ObserverHandle>;

/// A reduction applied to the numeric values within one bucket.
pub enum ChartAggregateFn {
    /// Arithmetic mean of the bucket's values (`0.0` for an empty bucket).
    Mean,
    /// Sum of the bucket's values.
    Sum,
    /// Smallest value in the bucket.
    Min,
    /// Largest value in the bucket.
    Max,
    /// The bucket's first value.
    First,
    /// The bucket's last value.
    Last,
    /// A caller-supplied reduction.
    Custom(Rc<dyn Fn(&[f32]) -> f32>),
}

impl ChartAggregateFn {
    /// Apply the reduction to a bucket's values.
    ///
    /// On an empty slice, every built-in variant returns `0.0`
    /// (`Mean`/`Min`/`Max`/`First`/`Last`) or the empty sum (`Sum`, also
    /// `0.0`) — a uniform, unsurprising convention rather than `Min`/`Max`
    /// leaking their fold seed (`±INFINITY`) into a chart value. `Custom`
    /// returns whatever the supplied closure computes for `&[]`. No internal
    /// caller actually passes an empty slice — `compute_bucket_datum` bails
    /// out before calling `apply` for an empty bucket — so this only bites a
    /// direct caller.
    pub fn apply(&self, values: &[f32]) -> f32 {
        match self {
            ChartAggregateFn::Mean => {
                if values.is_empty() {
                    0.0
                } else {
                    values.iter().sum::<f32>() / values.len() as f32
                }
            }
            ChartAggregateFn::Sum => values.iter().sum(),
            ChartAggregateFn::Min => {
                if values.is_empty() {
                    0.0
                } else {
                    values.iter().copied().fold(f32::INFINITY, f32::min)
                }
            }
            ChartAggregateFn::Max => {
                if values.is_empty() {
                    0.0
                } else {
                    values.iter().copied().fold(f32::NEG_INFINITY, f32::max)
                }
            }
            ChartAggregateFn::First => values.first().copied().unwrap_or(0.0),
            ChartAggregateFn::Last => values.last().copied().unwrap_or(0.0),
            ChartAggregateFn::Custom(f) => f(values),
        }
    }
}

impl Clone for ChartAggregateFn {
    fn clone(&self) -> Self {
        match self {
            ChartAggregateFn::Mean => ChartAggregateFn::Mean,
            ChartAggregateFn::Sum => ChartAggregateFn::Sum,
            ChartAggregateFn::Min => ChartAggregateFn::Min,
            ChartAggregateFn::Max => ChartAggregateFn::Max,
            ChartAggregateFn::First => ChartAggregateFn::First,
            ChartAggregateFn::Last => ChartAggregateFn::Last,
            ChartAggregateFn::Custom(f) => ChartAggregateFn::Custom(f.clone()),
        }
    }
}

impl std::fmt::Debug for ChartAggregateFn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChartAggregateFn::Mean => f.write_str("Mean"),
            ChartAggregateFn::Sum => f.write_str("Sum"),
            ChartAggregateFn::Min => f.write_str("Min"),
            ChartAggregateFn::Max => f.write_str("Max"),
            ChartAggregateFn::First => f.write_str("First"),
            ChartAggregateFn::Last => f.write_str("Last"),
            ChartAggregateFn::Custom(_) => f.write_str("Custom(..)"),
        }
    }
}

struct ObserverEntry {
    id: u64,
    callback: Rc<dyn Fn(&ChartChange)>,
}

struct ChartAggregateInner<T: 'static> {
    series_ids_fn: SeriesIdsFn,
    point_count_fn: PointCountFn,
    with_point_fn: WithPointFn<T>,
    with_series_fn: WithSeriesFn,
    bucket_size: usize,
    aggregate_fn: ChartAggregateFn,
    /// Materialized bucket data, per series — NOT a view into the source.
    buckets: HashMap<SeriesId, Vec<ChartDatum<T>>>,
    /// First bucket index that may have changed, per series.
    divergence: HashMap<SeriesId, usize>,
    observers: Vec<ObserverEntry>,
    next_observer_id: u64,
    _upstream_handle: Option<ObserverHandle>,
}

impl<T: 'static> ChartAggregateInner<T> {
    fn snapshot_callbacks(&self) -> Vec<Rc<dyn Fn(&ChartChange)>> {
        self.observers.iter().map(|e| e.callback.clone()).collect()
    }
}

/// A bucket/rollup projection over a [`ChartModel<T>`].
///
/// See the module documentation for semantics.
pub struct ChartAggregate<T: 'static> {
    inner: Rc<RefCell<ChartAggregateInner<T>>>,
}

/// Compute one bucket's reduced datum from source indices `[start, end)`.
/// Returns `None` for an empty range (no bucket to emit).
fn compute_bucket_datum<T: Clone>(
    with_point_fn: &WithPointFn<T>,
    series: SeriesId,
    aggregate_fn: &ChartAggregateFn,
    start: usize,
    end: usize,
) -> Option<ChartDatum<T>> {
    if start >= end {
        return None;
    }
    let values: RefCell<Vec<f32>> = RefCell::new(Vec::with_capacity(end - start));
    let category: RefCell<Option<T>> = RefCell::new(None);
    for i in start..end {
        (with_point_fn)(series, i, &|d: &ChartDatum<T>| {
            if category.borrow().is_none() {
                *category.borrow_mut() = Some(d.category.clone());
            }
            values.borrow_mut().push(d.value);
        });
    }
    let category = category.into_inner()?;
    let values = values.into_inner();
    Some(ChartDatum::new(category, aggregate_fn.apply(&values)))
}

fn rebuild_series<T: Clone + 'static>(inner: &mut ChartAggregateInner<T>, series: SeriesId) {
    let bs = inner.bucket_size;
    let n = (inner.point_count_fn)(series);
    let bucket_count = n.div_ceil(bs);
    let mut list = Vec::with_capacity(bucket_count);
    for b in 0..bucket_count {
        let start = b * bs;
        let end = (start + bs).min(n);
        if let Some(d) = compute_bucket_datum(
            &inner.with_point_fn,
            series,
            &inner.aggregate_fn,
            start,
            end,
        ) {
            list.push(d);
        }
    }
    inner.buckets.insert(series, list);
    inner.divergence.insert(series, 0);
}

fn rebuild_all<T: Clone + 'static>(inner: &mut ChartAggregateInner<T>) {
    inner.buckets.clear();
    inner.divergence.clear();
    for series in (inner.series_ids_fn)() {
        rebuild_series(inner, series);
    }
}

/// Translate one upstream `ChartChange` into zero or more local changes.
/// See module docs.
fn translate<T: Clone + 'static>(
    inner: &mut ChartAggregateInner<T>,
    change: &ChartChange,
) -> Vec<ChartChange> {
    match change {
        ChartChange::SeriesInserted { index, series } => {
            let (index, series) = (*index, *series);
            inner.buckets.insert(series, Vec::new());
            inner.divergence.insert(series, 0);
            vec![ChartChange::SeriesInserted { index, series }]
        }
        ChartChange::SeriesRemoved { series } => {
            let series = *series;
            inner.buckets.remove(&series);
            inner.divergence.remove(&series);
            vec![ChartChange::SeriesRemoved { series }]
        }
        ChartChange::SeriesMoved { series, from, to } => vec![ChartChange::SeriesMoved {
            series: *series,
            from: *from,
            to: *to,
        }],
        ChartChange::SeriesRenamed { series } => {
            vec![ChartChange::SeriesRenamed { series: *series }]
        }
        ChartChange::SeriesColorChanged { series } => {
            vec![ChartChange::SeriesColorChanged { series: *series }]
        }
        ChartChange::SeriesVisibilityChanged { series } => {
            vec![ChartChange::SeriesVisibilityChanged { series: *series }]
        }
        ChartChange::PointsInserted { series, range } => {
            let series = *series;
            let range = range.clone();
            let bs = inner.bucket_size;
            let new_total = (inner.point_count_fn)(series);
            let inserted = range.end - range.start;
            let old_total = new_total - inserted;

            if range.start != old_total {
                // Not a tail append — a mid-series insert. Rebuild.
                rebuild_series(inner, series);
                return vec![ChartChange::SeriesDataReplaced { series }];
            }

            let old_bc = old_total.div_ceil(bs);
            let new_bc = new_total.div_ceil(bs);
            let mut out = Vec::new();

            if old_bc >= 1 {
                let start = (old_bc - 1) * bs;
                let end = (start + bs).min(new_total);
                let mut wrote = false;
                if let Some(d) = compute_bucket_datum(
                    &inner.with_point_fn,
                    series,
                    &inner.aggregate_fn,
                    start,
                    end,
                ) && let Some(list) = inner.buckets.get_mut(&series)
                    && old_bc - 1 < list.len()
                {
                    list[old_bc - 1] = d;
                    wrote = true;
                }
                // Only notify/diverge if the write actually happened — a
                // failed guard (stale `buckets` entry, an empty computed
                // range) must be a silent no-op, not a claim that a bucket
                // changed when it didn't.
                if wrote {
                    inner.divergence.insert(series, old_bc - 1);
                    out.push(ChartChange::PointUpdated {
                        series,
                        index: old_bc - 1,
                    });
                } else {
                    debug_assert!(
                        false,
                        "chart_aggregate: last-bucket update guard failed for an existing bucket"
                    );
                }
            }

            if new_bc > old_bc {
                for b in old_bc..new_bc {
                    let start = b * bs;
                    let end = (start + bs).min(new_total);
                    if let Some(d) = compute_bucket_datum(
                        &inner.with_point_fn,
                        series,
                        &inner.aggregate_fn,
                        start,
                        end,
                    ) {
                        inner.buckets.entry(series).or_default().push(d);
                    }
                }
                inner.divergence.insert(series, old_bc.saturating_sub(1));
                out.push(ChartChange::PointsInserted {
                    series,
                    range: old_bc..new_bc,
                });
            }
            out
        }
        ChartChange::PointsRemoved { series, .. } => {
            let series = *series;
            rebuild_series(inner, series);
            vec![ChartChange::SeriesDataReplaced { series }]
        }
        ChartChange::PointUpdated { series, index } => {
            let (series, index) = (*series, *index);
            let bs = inner.bucket_size;
            let bucket_index = index / bs;
            let n = (inner.point_count_fn)(series);
            let start = bucket_index * bs;
            let end = (start + bs).min(n);
            let mut wrote = false;
            if let Some(d) = compute_bucket_datum(
                &inner.with_point_fn,
                series,
                &inner.aggregate_fn,
                start,
                end,
            ) && let Some(list) = inner.buckets.get_mut(&series)
                && bucket_index < list.len()
            {
                list[bucket_index] = d;
                wrote = true;
            }
            // Only notify/diverge if the write actually happened — see the
            // matching guard in the `PointsInserted` arm above.
            if wrote {
                inner.divergence.insert(series, bucket_index);
                vec![ChartChange::PointUpdated {
                    series,
                    index: bucket_index,
                }]
            } else {
                debug_assert!(
                    false,
                    "chart_aggregate: point-update guard failed for an existing bucket"
                );
                vec![]
            }
        }
        ChartChange::SeriesDataReplaced { series } => {
            let series = *series;
            rebuild_series(inner, series);
            vec![ChartChange::SeriesDataReplaced { series }]
        }
        ChartChange::Reset => {
            rebuild_all(inner);
            vec![ChartChange::Reset]
        }
    }
}

fn translate_and_notify<T: Clone + 'static>(
    inner: &Rc<RefCell<ChartAggregateInner<T>>>,
    change: &ChartChange,
) {
    let (changes, callbacks) = {
        let mut guard = inner.borrow_mut();
        let changes = translate(&mut guard, change);
        (changes, guard.snapshot_callbacks())
    };
    for c in &changes {
        for cb in &callbacks {
            cb(c);
        }
    }
}

impl<T: Clone + 'static> ChartAggregate<T> {
    /// Wrap `source`, bucketing every series into groups of `bucket_size`
    /// source points reduced via `aggregate_fn`. `bucket_size` is clamped
    /// to a minimum of 1.
    pub fn new(source: ChartModel<T>, bucket_size: usize, aggregate_fn: ChartAggregateFn) -> Self {
        let series_ids_fn: SeriesIdsFn = {
            let m = source.clone();
            Rc::new(move || m.series_ids())
        };
        let point_count_fn: PointCountFn = {
            let m = source.clone();
            Rc::new(move |series| m.point_count(series))
        };
        let with_point_fn: WithPointFn<T> = {
            let m = source.clone();
            Rc::new(move |series, idx, f| {
                m.with_point(series, idx, |d| f(d));
            })
        };
        let with_series_fn: WithSeriesFn = {
            let m = source.clone();
            Rc::new(move |series, f| {
                m.with_series(series, |name, color, visible| f(name, color, visible));
            })
        };
        let observe_fn: ObserveChartFn = {
            let m = source;
            Rc::new(move |callback| m.observe_changes(move |change| callback(change)))
        };

        let inner = Rc::new(RefCell::new(ChartAggregateInner {
            series_ids_fn,
            point_count_fn,
            with_point_fn,
            with_series_fn,
            bucket_size: bucket_size.max(1),
            aggregate_fn,
            buckets: HashMap::new(),
            divergence: HashMap::new(),
            observers: Vec::new(),
            next_observer_id: 1,
            _upstream_handle: None,
        }));

        rebuild_all(&mut inner.borrow_mut());

        let weak = Rc::downgrade(&inner);
        let upstream_handle = (observe_fn)(Box::new(move |change| {
            if let Some(strong) = weak.upgrade() {
                translate_and_notify(&strong, change);
            }
        }));
        inner.borrow_mut()._upstream_handle = Some(upstream_handle);

        Self { inner }
    }

    /// Change the bucket size, rebuilding every series and emitting
    /// `ChartChange::Reset`. Clamped to a minimum of 1.
    pub fn set_bucket_size(&self, bucket_size: usize) {
        let callbacks = {
            let mut guard = self.inner.borrow_mut();
            guard.bucket_size = bucket_size.max(1);
            rebuild_all(&mut guard);
            guard.snapshot_callbacks()
        };
        for cb in &callbacks {
            cb(&ChartChange::Reset);
        }
    }

    /// Change the aggregate reduction, rebuilding every series and emitting
    /// `ChartChange::Reset`.
    pub fn set_aggregate_fn(&self, aggregate_fn: ChartAggregateFn) {
        let callbacks = {
            let mut guard = self.inner.borrow_mut();
            guard.aggregate_fn = aggregate_fn;
            rebuild_all(&mut guard);
            guard.snapshot_callbacks()
        };
        for cb in &callbacks {
            cb(&ChartChange::Reset);
        }
    }
}

impl<T: 'static> ChartAggregate<T> {
    /// The configured bucket size.
    pub fn bucket_size(&self) -> usize {
        self.inner.borrow().bucket_size
    }

    /// Number of series (same set as the source).
    pub fn series_count(&self) -> usize {
        (self.inner.borrow().series_ids_fn)().len()
    }

    /// The series ids, in the source's display order.
    pub fn series_ids(&self) -> Vec<SeriesId> {
        (self.inner.borrow().series_ids_fn)()
    }

    /// Number of buckets currently materialized for `series`.
    pub fn point_count(&self, series: SeriesId) -> usize {
        self.inner
            .borrow()
            .buckets
            .get(&series)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    /// Access a series' metadata (delegates straight through to the
    /// source). Returns `None` if `series` is unknown.
    pub fn with_series<R>(
        &self,
        series: SeriesId,
        f: impl FnOnce(&str, Option<&ColorProp>, bool) -> R,
    ) -> Option<R> {
        let with_series_fn = self.inner.borrow().with_series_fn.clone();
        let f_cell: Cell<Option<_>> = Cell::new(Some(f));
        let slot: Cell<Option<R>> = Cell::new(None);
        (with_series_fn)(series, &|name, color, visible| {
            if let Some(f) = f_cell.take() {
                slot.set(Some(f(name, color, visible)));
            }
        });
        slot.into_inner()
    }

    /// Access the bucket at `index` within `series`. Returns `None` if
    /// `series` or `index` is unknown.
    pub fn with_point<R>(
        &self,
        series: SeriesId,
        index: usize,
        f: impl FnOnce(&ChartDatum<T>) -> R,
    ) -> Option<R> {
        let guard = self.inner.borrow();
        guard.buckets.get(&series).and_then(|v| v.get(index)).map(f)
    }

    /// Register an observer for translated bucket changes. Returns an
    /// `ObserverHandle` — dropping it removes the callback.
    pub fn observe_changes(&self, f: impl Fn(&ChartChange) + 'static) -> ObserverHandle {
        let mut guard = self.inner.borrow_mut();
        let id = guard.next_observer_id;
        guard.next_observer_id += 1;
        guard.observers.push(ObserverEntry {
            id,
            callback: Rc::new(f),
        });
        let inner = self.inner.clone();
        ObserverHandle::new(
            self.inner.clone(),
            id,
            Rc::new(move |observer_id| {
                inner.borrow_mut().observers.retain(|e| e.id != observer_id);
            }),
        )
    }

    /// First bucket index of `series` whose content may differ since the
    /// latest translated change. `None` if `series` is unknown or
    /// unaffected yet.
    pub fn first_changed_index(&self, series: SeriesId) -> Option<usize> {
        self.inner.borrow().divergence.get(&series).copied()
    }
}

impl<T: 'static> Clone for ChartAggregate<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T: 'static> std::fmt::Debug for ChartAggregate<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = self.inner.borrow();
        f.debug_struct("ChartAggregate")
            .field("bucket_size", &guard.bucket_size)
            .field("series_count", &(guard.series_ids_fn)().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_on_empty_slice_is_zero_for_every_built_in_variant() {
        // Regression: `Min`/`Max` used to leak their fold seed (`±INFINITY`)
        // on an empty bucket, inconsistent with `Mean`/`Sum`/`First`/`Last`
        // all returning `0.0`. No internal caller hits this path, but a
        // direct caller shouldn't see an infinity fall out of a chart value.
        for f in [
            ChartAggregateFn::Mean,
            ChartAggregateFn::Sum,
            ChartAggregateFn::Min,
            ChartAggregateFn::Max,
            ChartAggregateFn::First,
            ChartAggregateFn::Last,
        ] {
            assert_eq!(f.apply(&[]), 0.0, "{f:?} on empty slice");
        }
    }

    fn series_with(values: &[f32]) -> (ChartModel<i32>, SeriesId) {
        let model: ChartModel<i32> = ChartModel::new();
        let s = model.add_series("s");
        for (i, &v) in values.iter().enumerate() {
            model.push_point(s, i as i32, v);
        }
        (model, s)
    }

    fn values<T: 'static>(agg: &ChartAggregate<T>, s: SeriesId) -> Vec<f32> {
        (0..agg.point_count(s))
            .map(|i| agg.with_point(s, i, |d| d.value).unwrap())
            .collect()
    }

    fn track(agg: &ChartAggregate<i32>) -> (Rc<RefCell<Vec<ChartChange>>>, ObserverHandle) {
        let log: Rc<RefCell<Vec<ChartChange>>> = Rc::new(RefCell::new(Vec::new()));
        let l = log.clone();
        let handle = agg.observe_changes(move |c| l.borrow_mut().push(c.clone()));
        (log, handle)
    }

    #[test]
    fn bucket_size_one_is_identity() {
        let (model, s) = series_with(&[0.0, 10.0, 20.0, 30.0]);
        let agg = ChartAggregate::new(model, 1, ChartAggregateFn::Mean);
        assert_eq!(agg.point_count(s), 4);
        assert_eq!(values(&agg, s), vec![0.0, 10.0, 20.0, 30.0]);
    }

    #[test]
    fn mean_aggregate() {
        let (model, s) = series_with(&[1.0, 2.0, 3.0, 4.0]);
        let agg = ChartAggregate::new(model, 2, ChartAggregateFn::Mean);
        assert_eq!(agg.point_count(s), 2);
        assert_eq!(values(&agg, s), vec![1.5, 3.5]);
    }

    #[test]
    fn max_aggregate() {
        let (model, s) = series_with(&[1.0, 5.0, 2.0, 9.0]);
        let agg = ChartAggregate::new(model, 2, ChartAggregateFn::Max);
        assert_eq!(values(&agg, s), vec![5.0, 9.0]);
    }

    #[test]
    fn custom_aggregate() {
        let (model, s) = series_with(&[1.0, 2.0, 3.0]);
        let agg = ChartAggregate::new(
            model,
            3,
            ChartAggregateFn::Custom(Rc::new(|vs: &[f32]| vs.iter().product())),
        );
        assert_eq!(values(&agg, s), vec![6.0]);
    }

    #[test]
    fn trailing_partial_bucket_is_included() {
        let (model, s) = series_with(&[1.0, 2.0, 3.0]);
        let agg = ChartAggregate::new(model, 2, ChartAggregateFn::Sum);
        assert_eq!(agg.point_count(s), 2);
        assert_eq!(values(&agg, s), vec![3.0, 3.0]); // [1,2] and trailing [3]
    }

    #[test]
    fn tail_append_worked_trace_bucket_size_3() {
        // 7 points -> 3 buckets: [0,1,2], [3,4,5], [6]
        let (model, s) = series_with(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let agg = ChartAggregate::new(model.clone(), 3, ChartAggregateFn::Sum);
        assert_eq!(agg.point_count(s), 3);
        let (log, _h) = track(&agg);

        // 7 -> 8: same bucket count (last bucket [6,7)).
        model.push_point(s, 7, 7.0);
        {
            let entries = log.borrow();
            assert_eq!(entries.len(), 1);
            assert_eq!(
                entries[0],
                ChartChange::PointUpdated {
                    series: s,
                    index: 2
                }
            );
        }
        assert_eq!(agg.point_count(s), 3);
        assert_eq!(agg.with_point(s, 2, |d| d.value), Some(13.0)); // 6+7
        log.borrow_mut().clear();

        // 8 -> 9: still same bucket count (last bucket [6,9)).
        model.push_point(s, 8, 8.0);
        {
            let entries = log.borrow();
            assert_eq!(entries.len(), 1);
            assert_eq!(
                entries[0],
                ChartChange::PointUpdated {
                    series: s,
                    index: 2
                }
            );
        }
        assert_eq!(agg.with_point(s, 2, |d| d.value), Some(21.0)); // 6+7+8
        log.borrow_mut().clear();

        // 9 -> 10: new bucket 3 = [9].
        model.push_point(s, 9, 9.0);
        {
            let entries = log.borrow();
            assert_eq!(entries.len(), 2);
            assert_eq!(
                entries[0],
                ChartChange::PointUpdated {
                    series: s,
                    index: 2
                }
            );
            assert_eq!(
                entries[1],
                ChartChange::PointsInserted {
                    series: s,
                    range: 3..4
                }
            );
        }
        assert_eq!(agg.point_count(s), 4);
        assert_eq!(agg.with_point(s, 3, |d| d.value), Some(9.0));
    }

    #[test]
    fn mid_series_insert_falls_back_to_replace() {
        let (model, s) = series_with(&[0.0, 1.0, 2.0]);
        let agg = ChartAggregate::new(model.clone(), 2, ChartAggregateFn::Sum);
        let (log, _h) = track(&agg);

        model.insert_point(s, 1, 99, 99.0);

        let entries = log.borrow();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], ChartChange::SeriesDataReplaced { series: s });
    }

    #[test]
    fn any_removal_falls_back_to_replace() {
        let (model, s) = series_with(&[0.0, 1.0, 2.0, 3.0]);
        let agg = ChartAggregate::new(model.clone(), 2, ChartAggregateFn::Sum);
        let (log, _h) = track(&agg);

        model.remove_point(s, 0);

        let entries = log.borrow();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], ChartChange::SeriesDataReplaced { series: s });
        assert_eq!(agg.point_count(s), 2); // 3 remaining points / 2
    }

    #[test]
    fn point_updated_recomputes_its_bucket() {
        let (model, s) = series_with(&[1.0, 2.0, 3.0, 4.0]);
        let agg = ChartAggregate::new(model.clone(), 2, ChartAggregateFn::Sum);
        let (log, _h) = track(&agg);

        model.update_point(s, 2, 2, 30.0); // bucket 1 = [3,4) -> now [30,4)

        let entries = log.borrow();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0],
            ChartChange::PointUpdated {
                series: s,
                index: 1
            }
        );
        drop(entries);
        assert_eq!(agg.with_point(s, 1, |d| d.value), Some(34.0));
    }

    #[test]
    fn reset_rebuilds_and_passes_through() {
        let (model, s) = series_with(&[1.0, 2.0, 3.0]);
        let agg = ChartAggregate::new(model.clone(), 2, ChartAggregateFn::Sum);
        let (log, _h) = track(&agg);

        model.clear();
        assert_eq!(log.borrow().last(), Some(&ChartChange::Reset));
        assert_eq!(agg.point_count(s), 0);
    }

    #[test]
    fn dropping_aggregate_unregisters_upstream_observer() {
        let model: ChartModel<i32> = ChartModel::new();
        let before = model.observer_count();
        let agg = ChartAggregate::new(model.clone(), 3, ChartAggregateFn::Sum);
        assert_eq!(model.observer_count(), before + 1);
        drop(agg);
        assert_eq!(model.observer_count(), before);
    }

    #[test]
    fn set_bucket_size_rebuilds() {
        let (model, s) = series_with(&[1.0, 2.0, 3.0, 4.0]);
        let agg = ChartAggregate::new(model, 2, ChartAggregateFn::Sum);
        assert_eq!(agg.point_count(s), 2);
        agg.set_bucket_size(4);
        assert_eq!(agg.point_count(s), 1);
        assert_eq!(values(&agg, s), vec![10.0]);
    }

    #[test]
    fn set_aggregate_fn_rebuilds() {
        let (model, s) = series_with(&[1.0, 5.0, 2.0]);
        let agg = ChartAggregate::new(model, 3, ChartAggregateFn::Sum);
        assert_eq!(values(&agg, s), vec![8.0]);
        agg.set_aggregate_fn(ChartAggregateFn::Max);
        assert_eq!(values(&agg, s), vec![5.0]);
    }
}
