// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `ChartWindow<T>` — a "last N points per series" streaming projection over
//! a [`crate::ChartModel`].
//!
//! Wraps a [`ChartModel<T>`](crate::ChartModel) and exposes the tail
//! `window_size` points of every series — the live-scrolling-strip-chart
//! pattern (a sensor feed, a log-rate graph, a stock ticker). Unlike
//! [`crate::ChartAggregate`], `ChartWindow` copies **no point data**: it
//! tracks, per series, the source index of the window's first visible point
//! (`starts`) and delegates every read straight through to the source. That
//! means a `ChartWindow<T>` needs no `T: Clone` bound at all.
//!
//! ## Reactivity
//!
//! The upstream [`ChartChange`] stream is translated, not collapsed to a
//! blanket `Reset` (unlike [`crate::SortFilterListModel`], where an
//! arbitrary sort-key move makes fine-grained translation unsafe — a
//! fixed-size tail window has no such hazard): a tail append into a full
//! window becomes a `PointsRemoved` + `PointsInserted` pair (the window
//! slides), a tail append into a still-growing window becomes a plain
//! `PointsInserted`, and anything that isn't a clean tail append (a
//! mid-series insert, any removal) falls back to a per-series rebuild
//! reported as `SeriesDataReplaced`.
//!
//! ```ignore
//! use bastyde_data::{ChartModel, ChartWindow};
//! let model: ChartModel<i32> = ChartModel::new();
//! let s = model.add_series("sensor");
//! for i in 0..100 {
//!     model.push_point(s, i, i as f32);
//! }
//! let window = ChartWindow::new(model.clone(), 10);
//! assert_eq!(window.point_count(s), 10); // last 10 points only
//! ```

use std::cell::Cell;
use std::cell::RefCell;
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

struct ObserverEntry {
    id: u64,
    callback: Rc<dyn Fn(&ChartChange)>,
}

struct ChartWindowInner<T: 'static> {
    series_ids_fn: SeriesIdsFn,
    point_count_fn: PointCountFn,
    with_point_fn: WithPointFn<T>,
    with_series_fn: WithSeriesFn,
    window_size: usize,
    /// Source index of window position 0, per series.
    starts: HashMap<SeriesId, usize>,
    /// First window-local index that may have changed, per series. See
    /// [`ChartWindow::first_changed_index`].
    divergence: HashMap<SeriesId, usize>,
    observers: Vec<ObserverEntry>,
    next_observer_id: u64,
    _upstream_handle: Option<ObserverHandle>,
}

impl<T: 'static> ChartWindowInner<T> {
    fn start_for(&self, series: SeriesId) -> usize {
        let total = (self.point_count_fn)(series);
        total.saturating_sub(self.window_size)
    }

    fn rebuild_series(&mut self, series: SeriesId) {
        let start = self.start_for(series);
        self.starts.insert(series, start);
        self.divergence.insert(series, 0);
    }

    fn rebuild_all(&mut self) {
        self.starts.clear();
        self.divergence.clear();
        for series in (self.series_ids_fn)() {
            self.rebuild_series(series);
        }
    }

    fn snapshot_callbacks(&self) -> Vec<Rc<dyn Fn(&ChartChange)>> {
        self.observers.iter().map(|e| e.callback.clone()).collect()
    }
}

/// A "last N points per series" streaming projection over a [`ChartModel<T>`].
///
/// See the module documentation for semantics.
pub struct ChartWindow<T: 'static> {
    inner: Rc<RefCell<ChartWindowInner<T>>>,
}

impl<T: 'static> ChartWindow<T> {
    /// Wrap `source`, showing only the last `window_size` points of every
    /// series.
    pub fn new(source: ChartModel<T>, window_size: usize) -> Self {
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

        let inner = Rc::new(RefCell::new(ChartWindowInner {
            series_ids_fn,
            point_count_fn,
            with_point_fn,
            with_series_fn,
            window_size,
            starts: HashMap::new(),
            divergence: HashMap::new(),
            observers: Vec::new(),
            next_observer_id: 1,
            _upstream_handle: None,
        }));

        inner.borrow_mut().rebuild_all();

        let weak = Rc::downgrade(&inner);
        let upstream_handle = (observe_fn)(Box::new(move |change| {
            if let Some(strong) = weak.upgrade() {
                translate_and_notify(&strong, change);
            }
        }));
        inner.borrow_mut()._upstream_handle = Some(upstream_handle);

        Self { inner }
    }

    /// The configured window size.
    pub fn window_size(&self) -> usize {
        self.inner.borrow().window_size
    }

    /// Change the window size, rebuilding every series and emitting
    /// `ChartChange::Reset`.
    pub fn set_window_size(&self, window_size: usize) {
        let callbacks = {
            let mut guard = self.inner.borrow_mut();
            guard.window_size = window_size;
            guard.rebuild_all();
            guard.snapshot_callbacks()
        };
        for cb in &callbacks {
            cb(&ChartChange::Reset);
        }
    }

    /// Number of series (same set as the source).
    pub fn series_count(&self) -> usize {
        (self.inner.borrow().series_ids_fn)().len()
    }

    /// The series ids, in the source's display order.
    pub fn series_ids(&self) -> Vec<SeriesId> {
        (self.inner.borrow().series_ids_fn)()
    }

    /// Number of points currently visible in the window for `series`.
    pub fn point_count(&self, series: SeriesId) -> usize {
        let guard = self.inner.borrow();
        let total = (guard.point_count_fn)(series);
        let start = guard
            .starts
            .get(&series)
            .copied()
            .unwrap_or_else(|| total.saturating_sub(guard.window_size));
        total.saturating_sub(start)
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

    /// Access the point at window-local `index` within `series`. Returns
    /// `None` if `series` is unknown or `index` is outside the window.
    pub fn with_point<R>(
        &self,
        series: SeriesId,
        index: usize,
        f: impl FnOnce(&ChartDatum<T>) -> R,
    ) -> Option<R> {
        let (with_point_fn, src_idx, in_bounds) = {
            let guard = self.inner.borrow();
            let total = (guard.point_count_fn)(series);
            let start = guard
                .starts
                .get(&series)
                .copied()
                .unwrap_or_else(|| total.saturating_sub(guard.window_size));
            let visible = total.saturating_sub(start);
            (guard.with_point_fn.clone(), start + index, index < visible)
        };
        if !in_bounds {
            return None;
        }
        let f_cell: Cell<Option<_>> = Cell::new(Some(f));
        let slot: Cell<Option<R>> = Cell::new(None);
        (with_point_fn)(series, src_idx, &|d: &ChartDatum<T>| {
            if let Some(f) = f_cell.take() {
                slot.set(Some(f(d)));
            }
        });
        slot.into_inner()
    }

    /// Register an observer for translated window changes. Returns an
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

    /// First window-local index of `series` whose content may differ since
    /// the latest translated change. Per-series (chart data is 2-level:
    /// series, then points), unlike
    /// [`SortFilterListModel::first_changed_index`](crate::SortFilterListModel::first_changed_index)'s
    /// single flat value. `None` if `series` is unknown or unaffected yet.
    pub fn first_changed_index(&self, series: SeriesId) -> Option<usize> {
        self.inner.borrow().divergence.get(&series).copied()
    }
}

/// Translate one upstream `ChartChange` into zero or more local changes,
/// mutating `starts` / `divergence` as needed. See module docs.
fn translate<T: 'static>(
    inner: &mut ChartWindowInner<T>,
    change: &ChartChange,
) -> Vec<ChartChange> {
    match change {
        ChartChange::SeriesInserted { index, series } => {
            let (index, series) = (*index, *series);
            inner.starts.insert(series, 0);
            inner.divergence.insert(series, 0);
            vec![ChartChange::SeriesInserted { index, series }]
        }
        ChartChange::SeriesRemoved { series } => {
            let series = *series;
            inner.starts.remove(&series);
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
            let window_size = inner.window_size;
            let new_total = (inner.point_count_fn)(series);
            let inserted = range.end - range.start;
            let old_total = new_total - inserted;
            let old_start = inner
                .starts
                .get(&series)
                .copied()
                .unwrap_or_else(|| old_total.saturating_sub(window_size));

            if range.start != old_total {
                // Not a tail append — a mid-series insert. Rebuild.
                inner.rebuild_series(series);
                return vec![ChartChange::SeriesDataReplaced { series }];
            }

            let new_start = new_total.saturating_sub(window_size);
            let shift = new_start.saturating_sub(old_start);
            let out = if shift == 0 {
                // Window not yet full — the new points land directly.
                let local = (range.start - new_start)..(range.end - new_start);
                inner.divergence.insert(series, local.start);
                vec![ChartChange::PointsInserted {
                    series,
                    range: local,
                }]
            } else if shift >= window_size {
                // Pathological bulk append larger than the window itself.
                inner.divergence.insert(series, 0);
                vec![ChartChange::SeriesDataReplaced { series }]
            } else {
                // Window was full (or became full): it slides.
                inner.divergence.insert(series, 0);
                vec![
                    ChartChange::PointsRemoved {
                        series,
                        range: 0..shift,
                    },
                    ChartChange::PointsInserted {
                        series,
                        range: (window_size - shift)..window_size,
                    },
                ]
            };
            inner.starts.insert(series, new_start);
            out
        }
        ChartChange::PointsRemoved { series, .. } => {
            let series = *series;
            inner.rebuild_series(series);
            vec![ChartChange::SeriesDataReplaced { series }]
        }
        ChartChange::PointUpdated { series, index } => {
            let (series, index) = (*series, *index);
            let start = inner.starts.get(&series).copied().unwrap_or(0);
            if index >= start {
                let local = index - start;
                inner.divergence.insert(series, local);
                vec![ChartChange::PointUpdated {
                    series,
                    index: local,
                }]
            } else {
                vec![]
            }
        }
        ChartChange::SeriesDataReplaced { series } => {
            let series = *series;
            inner.rebuild_series(series);
            vec![ChartChange::SeriesDataReplaced { series }]
        }
        ChartChange::Reset => {
            inner.rebuild_all();
            vec![ChartChange::Reset]
        }
    }
}

fn translate_and_notify<T: 'static>(
    inner: &Rc<RefCell<ChartWindowInner<T>>>,
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

impl<T: 'static> Clone for ChartWindow<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T: 'static> std::fmt::Debug for ChartWindow<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = self.inner.borrow();
        f.debug_struct("ChartWindow")
            .field("window_size", &guard.window_size)
            .field("series_count", &(guard.series_ids_fn)().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_series(n: usize) -> (ChartModel<i32>, SeriesId) {
        let model: ChartModel<i32> = ChartModel::new();
        let s = model.add_series("s");
        for i in 0..n {
            model.push_point(s, i as i32, i as f32);
        }
        (model, s)
    }

    fn track(window: &ChartWindow<i32>) -> (Rc<RefCell<Vec<ChartChange>>>, ObserverHandle) {
        let log: Rc<RefCell<Vec<ChartChange>>> = Rc::new(RefCell::new(Vec::new()));
        let l = log.clone();
        let handle = window.observe_changes(move |c| l.borrow_mut().push(c.clone()));
        (log, handle)
    }

    #[test]
    fn initial_window_shows_only_the_tail() {
        let (_model, s) = one_series(5);
        let window = ChartWindow::new(_model, 3);
        assert_eq!(window.point_count(s), 3);
        let vals: Vec<f32> = (0..3)
            .map(|i| window.with_point(s, i, |d| d.value).unwrap())
            .collect();
        assert_eq!(vals, vec![2.0, 3.0, 4.0]);
    }

    #[test]
    fn full_window_shift_emits_removed_then_inserted() {
        let (model, s) = one_series(3);
        let window = ChartWindow::new(model.clone(), 3);
        assert_eq!(window.point_count(s), 3);
        let (log, _h) = track(&window);

        model.push_point(s, 3, 3.0);

        let entries = log.borrow();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            ChartChange::PointsRemoved {
                series: s,
                range: 0..1
            }
        );
        assert_eq!(
            entries[1],
            ChartChange::PointsInserted {
                series: s,
                range: 2..3
            }
        );
        drop(entries);

        assert_eq!(window.point_count(s), 3);
        let vals: Vec<f32> = (0..3)
            .map(|i| window.with_point(s, i, |d| d.value).unwrap())
            .collect();
        assert_eq!(vals, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn partial_window_tail_append_emits_plain_inserted() {
        let (model, s) = one_series(1);
        let window = ChartWindow::new(model.clone(), 5);
        let (log, _h) = track(&window);

        model.push_point(s, 1, 1.0);

        let entries = log.borrow();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0],
            ChartChange::PointsInserted {
                series: s,
                range: 1..2
            }
        );
        drop(entries);
        assert_eq!(window.point_count(s), 2);
    }

    #[test]
    fn mid_series_insert_falls_back_to_replace() {
        let (model, s) = one_series(3);
        let window = ChartWindow::new(model.clone(), 5);
        let (log, _h) = track(&window);

        model.insert_point(s, 1, 99, 99.0);

        let entries = log.borrow();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], ChartChange::SeriesDataReplaced { series: s });
        drop(entries);
        assert_eq!(window.point_count(s), 4);
    }

    #[test]
    fn any_removal_falls_back_to_replace() {
        let (model, s) = one_series(3);
        let window = ChartWindow::new(model.clone(), 5);
        let (log, _h) = track(&window);

        model.remove_point(s, 0);

        let entries = log.borrow();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], ChartChange::SeriesDataReplaced { series: s });
        drop(entries);
        assert_eq!(window.point_count(s), 2);
    }

    #[test]
    fn update_in_and_out_of_window() {
        let (model, s) = one_series(5);
        let window = ChartWindow::new(model.clone(), 2); // shows source indices 3,4
        let (log, _h) = track(&window);

        model.update_point(s, 4, 4, 40.0); // in window -> local index 1
        model.update_point(s, 0, 0, 5.0); // out of window -> no emit

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
        assert_eq!(window.with_point(s, 1, |d| d.value), Some(40.0));
    }

    #[test]
    fn reset_passes_through_and_rebuilds() {
        let (model, s) = one_series(3);
        let window = ChartWindow::new(model.clone(), 2);
        let (log, _h) = track(&window);

        model.clear();

        assert_eq!(log.borrow().last(), Some(&ChartChange::Reset));
        assert_eq!(window.point_count(s), 0);
    }

    #[test]
    fn dropping_window_unregisters_upstream_observer() {
        let model: ChartModel<i32> = ChartModel::new();
        let before = model.observer_count();
        let window = ChartWindow::new(model.clone(), 3);
        assert_eq!(model.observer_count(), before + 1);
        drop(window);
        assert_eq!(model.observer_count(), before);
    }

    #[test]
    fn set_window_size_rebuilds_and_emits_reset() {
        let (model, s) = one_series(5);
        let window = ChartWindow::new(model, 2);
        assert_eq!(window.point_count(s), 2);
        let (log, _h) = track(&window);

        window.set_window_size(4);
        assert_eq!(window.point_count(s), 4);
        assert_eq!(log.borrow().last(), Some(&ChartChange::Reset));
    }

    #[test]
    fn series_inserted_and_removed_pass_through() {
        let model: ChartModel<i32> = ChartModel::new();
        let window = ChartWindow::new(model.clone(), 3);
        let (log, _h) = track(&window);

        let s = model.add_series("new");
        assert_eq!(
            log.borrow().last(),
            Some(&ChartChange::SeriesInserted {
                index: 0,
                series: s
            })
        );

        model.remove_series(s);
        assert_eq!(
            log.borrow().last(),
            Some(&ChartChange::SeriesRemoved { series: s })
        );
        assert_eq!(window.series_count(), 0);
    }
}
