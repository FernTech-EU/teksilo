// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `ChartModel<T>` — concrete reactive multi-series chart data model.
//!
//! `ChartModel<T>` owns an ordered collection of named series, each holding a
//! `Vec<ChartDatum<T>>` (a `category: T` paired with a numeric `value: f32`),
//! in a flat SlotMap arena — the same shape as [`crate::TreeModel`]. Every
//! mutation (series add/remove/move/rename/recolor/show-hide, point
//! push/insert/remove/update/replace) emits a [`ChartChange`] to all
//! registered observers *and* bumps one of two reactive version signals:
//! [`ChartModel::style_version`] (color changes only — a paint-only signal a
//! chart can bind at `BindingLevel::RepaintOnly`) or
//! [`ChartModel::structure_version`] (everything else — series/point shape,
//! bound at `BindingLevel::Relayout`/`Rebuild`). Series identity is a stable,
//! versioned [`SeriesId`] (a SlotMap key) that is never reused after removal.
//!
//! Cloning produces a second handle to the **same** data — all handles share
//! series/points and receive the same change notifications. Register
//! observers via [`observe_changes`](ChartModel::observe_changes); the
//! returned `ObserverHandle` is RAII — dropping it unregisters the callback.
//!
//! For a bounded "last N points" streaming view use
//! [`ChartWindow`](crate::ChartWindow). For bucketed/rolled-up display use
//! [`ChartAggregate`](crate::ChartAggregate). For point-level selection use
//! [`ChartSelection`](crate::ChartSelection).
//!
//! ```rust
//! # use bastyde_data::{ChartModel, ChartSeries, ChartDatum};
//! let model = ChartModel::from_series_vec(vec![
//!     ChartSeries::new("Revenue").data(vec![
//!         ChartDatum::new("Q1".to_string(), 10.0),
//!         ChartDatum::new("Q2".to_string(), 20.0),
//!     ]),
//! ]);
//! assert_eq!(model.series_count(), 1);
//! let s = model.series_id_at(0).unwrap();
//! assert_eq!(model.point_count(s), 2);
//!
//! model.push_point(s, "Q3".to_string(), 30.0);
//! assert_eq!(model.point_count(s), 3);
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use slotmap::SlotMap;

use bastyde_core::ObserverHandle;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::signal::Signal;

use crate::chart_change::{ChartChange, SeriesId};

/// One numeric data point at a category/x-axis position, with an optional
/// per-point color that overrides the series color (bar charts only).
#[derive(Debug, Clone)]
pub struct ChartDatum<T> {
    pub category: T,
    pub value: f32,
    pub color: Option<ColorProp>,
}

impl<T> ChartDatum<T> {
    pub fn new(category: T, value: f32) -> Self {
        Self { category, value, color: None }
    }

    /// Override this point's color (a bar's fill). Ignored by line/pie charts,
    /// which color by series.
    pub fn with_color(mut self, color: impl Into<ColorProp>) -> Self {
        self.color = Some(color.into());
        self
    }
}

/// A named series of data points with an optional explicit color and a
/// visibility flag, used to construct a [`ChartModel`] (via
/// [`ChartModel::from_series_vec`]) or to describe one series' desired
/// shape. Unlike the model, `visible` here is a plain `bool` — reactivity
/// lives in the model's [`ChartModel::structure_version`] /
/// [`ChartModel::style_version`] signals, not in this construction DTO.
pub struct ChartSeries<T> {
    pub name: String,
    pub color: Option<ColorProp>,
    pub visible: bool,
    pub points: Vec<ChartDatum<T>>,
}

impl<T> std::fmt::Debug for ChartSeries<T>
where
    T: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChartSeries")
            .field("name", &self.name)
            .field("visible", &self.visible)
            .field("len", &self.points.len())
            .finish()
    }
}

impl<T> Clone for ChartSeries<T>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            color: self.color.clone(),
            visible: self.visible,
            points: self.points.clone(),
        }
    }
}

impl<T> ChartSeries<T> {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            color: None,
            visible: true,
            points: Vec::new(),
        }
    }

    pub fn color(mut self, color: impl Into<ColorProp>) -> Self {
        self.color = Some(color.into());
        self
    }

    pub fn visibility(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    pub fn push(&mut self, category: T, value: f32) {
        self.points.push(ChartDatum::new(category, value));
    }

    pub fn data(mut self, points: Vec<ChartDatum<T>>) -> Self {
        self.points = points;
        self
    }
}

/// A read-only, borrowed view over one series — returned by
/// [`ChartModel::with_series_view`] / [`ChartModel::with_all_series`].
pub struct SeriesView<'a, T> {
    pub id: SeriesId,
    pub name: &'a str,
    pub color: Option<&'a ColorProp>,
    pub visible: bool,
    pub points: &'a [ChartDatum<T>],
}

struct SeriesEntry<T> {
    name: String,
    color: Option<ColorProp>,
    visible: bool,
    points: Vec<ChartDatum<T>>,
}

struct ObserverEntry {
    id: u64,
    callback: Rc<dyn Fn(&ChartChange)>,
}

struct ChartModelInner<T> {
    arena: SlotMap<slotmap::DefaultKey, SeriesEntry<T>>,
    order: Vec<SeriesId>,
    observers: Vec<ObserverEntry>,
    next_observer_id: u64,
    /// Structural version — bumped by every mutation except a color change.
    structure_version: Signal<u64>,
    /// Style version — bumped only by `SeriesColorChanged`.
    style_version: Signal<u64>,
    /// Strong handle to the debug-registry adapter for this model. Owned
    /// here so that the registration drops automatically when the inner is
    /// freed (the adapter holds only a `Weak` to inner, breaking the
    /// cycle). `None` until `.debug_named()` is called. Compiled out in
    /// release.
    #[cfg(debug_assertions)]
    debug_adapter: Option<Rc<dyn crate::debug_registry::ModelDebug>>,
}

/// A concrete reactive multi-series chart data model.
///
/// `ChartModel<T>` is `Clone` — cloning produces a second handle to the same
/// data. Multiple charts can hold clones and all see the same series and
/// points, and receive the same [`ChartChange`] notifications.
pub struct ChartModel<T: 'static> {
    inner: Rc<RefCell<ChartModelInner<T>>>,
}

impl<T: 'static> ChartModel<T> {
    /// Create an empty chart model with no series.
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(ChartModelInner {
                arena: SlotMap::new(),
                order: Vec::new(),
                observers: Vec::new(),
                next_observer_id: 1,
                structure_version: Signal::new(0),
                style_version: Signal::new(0),
                #[cfg(debug_assertions)]
                debug_adapter: None,
            })),
        }
    }

    /// Build a model directly from a vector of [`ChartSeries`] DTOs — the
    /// primary constructor. Populates the arena in one pass with no
    /// per-item notification (mirrors [`crate::ListModel::from_vec`]).
    pub fn from_series_vec(series: Vec<ChartSeries<T>>) -> Self {
        let model = Self::new();
        {
            let mut guard = model.inner.borrow_mut();
            for s in series {
                let key = guard.arena.insert(SeriesEntry {
                    name: s.name,
                    color: s.color,
                    visible: s.visible,
                    points: s.points,
                });
                let id = SeriesId::from_key(key);
                guard.order.push(id);
            }
        }
        model
    }

    /// Build a model with a single anonymous, visible series holding
    /// `points` — the flat/pie-chart path where series structure doesn't
    /// matter.
    pub fn from_points(points: Vec<ChartDatum<T>>) -> Self {
        Self::from_series_vec(vec![ChartSeries::new(String::new()).data(points)])
    }

    /// The model's sole series id, iff it has exactly one series.
    pub fn only_series(&self) -> Option<SeriesId> {
        let guard = self.inner.borrow();
        if guard.order.len() == 1 {
            Some(guard.order[0])
        } else {
            None
        }
    }

    // --- Series mutations ---

    /// Append a new, empty, visible series named `name`.
    pub fn add_series(&self, name: impl Into<String>) -> SeriesId {
        let (id, index) = {
            let mut guard = self.inner.borrow_mut();
            let key = guard.arena.insert(SeriesEntry {
                name: name.into(),
                color: None,
                visible: true,
                points: Vec::new(),
            });
            let id = SeriesId::from_key(key);
            let index = guard.order.len();
            guard.order.push(id);
            (id, index)
        };
        self.notify(ChartChange::SeriesInserted { index, series: id });
        self.bump_structure();
        id
    }

    /// Insert a new, empty, visible series named `name` at `index`.
    ///
    /// # Panics
    /// Panics if `index > series_count()`.
    pub fn insert_series(&self, index: usize, name: impl Into<String>) -> SeriesId {
        let id = {
            let mut guard = self.inner.borrow_mut();
            let key = guard.arena.insert(SeriesEntry {
                name: name.into(),
                color: None,
                visible: true,
                points: Vec::new(),
            });
            let id = SeriesId::from_key(key);
            guard.order.insert(index, id);
            id
        };
        self.notify(ChartChange::SeriesInserted { index, series: id });
        self.bump_structure();
        id
    }

    /// Remove a series and all of its points.
    ///
    /// # Panics
    /// Panics if `series` is unknown.
    pub fn remove_series(&self, series: SeriesId) {
        {
            let mut guard = self.inner.borrow_mut();
            guard.arena.remove(series.key()).expect("unknown SeriesId");
            guard.order.retain(|&id| id != series);
        }
        self.notify(ChartChange::SeriesRemoved { series });
        self.bump_structure();
    }

    /// Rename a series.
    ///
    /// # Panics
    /// Panics if `series` is unknown.
    pub fn rename_series(&self, series: SeriesId, name: impl Into<String>) {
        {
            let mut guard = self.inner.borrow_mut();
            guard.arena[series.key()].name = name.into();
        }
        self.notify(ChartChange::SeriesRenamed { series });
        self.bump_structure();
    }

    /// Set a series' explicit color. Bumps [`Self::style_version`] (not
    /// [`Self::structure_version`]) — this is a paint-only change.
    ///
    /// # Panics
    /// Panics if `series` is unknown.
    pub fn set_series_color(&self, series: SeriesId, color: impl Into<ColorProp>) {
        {
            let mut guard = self.inner.borrow_mut();
            guard.arena[series.key()].color = Some(color.into());
        }
        self.notify(ChartChange::SeriesColorChanged { series });
        self.bump_style();
    }

    /// Clear a series' explicit color (falls back to the chart's palette).
    /// Bumps [`Self::style_version`].
    ///
    /// # Panics
    /// Panics if `series` is unknown.
    pub fn clear_series_color(&self, series: SeriesId) {
        {
            let mut guard = self.inner.borrow_mut();
            guard.arena[series.key()].color = None;
        }
        self.notify(ChartChange::SeriesColorChanged { series });
        self.bump_style();
    }

    /// Show or hide a series. A no-op (no notify, no version bump) if
    /// `visible` already matches the current value.
    ///
    /// # Panics
    /// Panics if `series` is unknown.
    pub fn set_series_visible(&self, series: SeriesId, visible: bool) {
        let changed = {
            let mut guard = self.inner.borrow_mut();
            let entry = &mut guard.arena[series.key()];
            if entry.visible == visible {
                false
            } else {
                entry.visible = visible;
                true
            }
        };
        if !changed {
            return;
        }
        self.notify(ChartChange::SeriesVisibilityChanged { series });
        self.bump_structure();
    }

    /// Move a series to a new position among its siblings. A no-op (no
    /// notify, no version bump) if `to` is already the series' position.
    ///
    /// # Panics
    /// Panics if `series` is unknown or `to` is out of bounds.
    pub fn move_series(&self, series: SeriesId, to: usize) {
        let from = {
            let guard = self.inner.borrow();
            guard
                .order
                .iter()
                .position(|&id| id == series)
                .expect("unknown SeriesId")
        };
        if from == to {
            return;
        }
        {
            let mut guard = self.inner.borrow_mut();
            let id = guard.order.remove(from);
            guard.order.insert(to, id);
        }
        self.notify(ChartChange::SeriesMoved { series, from, to });
        self.bump_structure();
    }

    /// Remove every series.
    pub fn clear(&self) {
        {
            let mut guard = self.inner.borrow_mut();
            guard.arena.clear();
            guard.order.clear();
        }
        self.notify(ChartChange::Reset);
        self.bump_structure();
    }

    // --- Point mutations ---

    /// Append a point to the end of `series`.
    ///
    /// # Panics
    /// Panics if `series` is unknown.
    pub fn push_point(&self, series: SeriesId, category: T, value: f32) {
        let index = {
            let mut guard = self.inner.borrow_mut();
            let entry = &mut guard.arena[series.key()];
            let index = entry.points.len();
            entry.points.push(ChartDatum::new(category, value));
            index
        };
        self.notify(ChartChange::PointsInserted {
            series,
            range: index..index + 1,
        });
        self.bump_structure();
    }

    /// Insert a point at `index` within `series`.
    ///
    /// # Panics
    /// Panics if `series` is unknown or `index > point_count(series)`.
    pub fn insert_point(&self, series: SeriesId, index: usize, category: T, value: f32) {
        {
            let mut guard = self.inner.borrow_mut();
            guard.arena[series.key()]
                .points
                .insert(index, ChartDatum::new(category, value));
        }
        self.notify(ChartChange::PointsInserted {
            series,
            range: index..index + 1,
        });
        self.bump_structure();
    }

    /// Remove and return the point at `index` within `series`.
    ///
    /// # Panics
    /// Panics if `series` is unknown or `index >= point_count(series)`.
    pub fn remove_point(&self, series: SeriesId, index: usize) -> ChartDatum<T> {
        let datum = {
            let mut guard = self.inner.borrow_mut();
            guard.arena[series.key()].points.remove(index)
        };
        self.notify(ChartChange::PointsRemoved {
            series,
            range: index..index + 1,
        });
        self.bump_structure();
        datum
    }

    /// Replace the point at `index` within `series`.
    ///
    /// # Panics
    /// Panics if `series` is unknown or `index >= point_count(series)`.
    pub fn update_point(&self, series: SeriesId, index: usize, category: T, value: f32) {
        {
            let mut guard = self.inner.borrow_mut();
            guard.arena[series.key()].points[index] = ChartDatum::new(category, value);
        }
        self.notify(ChartChange::PointUpdated { series, index });
        self.bump_structure();
    }

    /// Replace `series`' entire point list.
    ///
    /// # Panics
    /// Panics if `series` is unknown.
    pub fn replace_series_data(&self, series: SeriesId, points: Vec<ChartDatum<T>>) {
        {
            let mut guard = self.inner.borrow_mut();
            guard.arena[series.key()].points = points;
        }
        self.notify(ChartChange::SeriesDataReplaced { series });
        self.bump_structure();
    }

    // --- Queries ---

    /// Number of series.
    pub fn series_count(&self) -> usize {
        self.inner.borrow().order.len()
    }

    /// The series ids, in display order.
    pub fn series_ids(&self) -> Vec<SeriesId> {
        self.inner.borrow().order.clone()
    }

    /// The series id at `index`, if any.
    pub fn series_id_at(&self, index: usize) -> Option<SeriesId> {
        self.inner.borrow().order.get(index).copied()
    }

    /// The display index of `series`, if it exists.
    pub fn series_index_of(&self, series: SeriesId) -> Option<usize> {
        self.inner
            .borrow()
            .order
            .iter()
            .position(|&id| id == series)
    }

    /// Number of points in `series` (0 if unknown).
    pub fn point_count(&self, series: SeriesId) -> usize {
        self.inner
            .borrow()
            .arena
            .get(series.key())
            .map(|e| e.points.len())
            .unwrap_or(0)
    }

    /// Access a series' metadata (name, color, visibility) via a callback.
    /// Returns `None` if `series` is unknown.
    pub fn with_series<R>(
        &self,
        series: SeriesId,
        f: impl FnOnce(&str, Option<&ColorProp>, bool) -> R,
    ) -> Option<R> {
        let guard = self.inner.borrow();
        guard
            .arena
            .get(series.key())
            .map(|e| f(&e.name, e.color.as_ref(), e.visible))
    }

    /// Access a point within `series` via a callback. Returns `None` if the
    /// series or index is unknown.
    pub fn with_point<R>(
        &self,
        series: SeriesId,
        index: usize,
        f: impl FnOnce(&ChartDatum<T>) -> R,
    ) -> Option<R> {
        let guard = self.inner.borrow();
        guard
            .arena
            .get(series.key())
            .and_then(|e| e.points.get(index))
            .map(f)
    }

    /// Access a whole-series view (metadata + points slice) via a callback.
    /// Returns `None` if `series` is unknown.
    pub fn with_series_view<R>(
        &self,
        series: SeriesId,
        f: impl FnOnce(SeriesView<'_, T>) -> R,
    ) -> Option<R> {
        let guard = self.inner.borrow();
        guard.arena.get(series.key()).map(|e| {
            f(SeriesView {
                id: series,
                name: &e.name,
                color: e.color.as_ref(),
                visible: e.visible,
                points: &e.points,
            })
        })
    }

    /// Access every series as an ordered slice of views via a callback.
    pub fn with_all_series<R>(&self, f: impl FnOnce(&[SeriesView<'_, T>]) -> R) -> R {
        let guard = self.inner.borrow();
        let views: Vec<SeriesView<'_, T>> = guard
            .order
            .iter()
            .filter_map(|&id| {
                guard.arena.get(id.key()).map(|e| SeriesView {
                    id,
                    name: &e.name,
                    color: e.color.as_ref(),
                    visible: e.visible,
                    points: &e.points,
                })
            })
            .collect();
        f(&views)
    }

    // --- Reactivity ---

    /// Structural version signal — bumped by every mutation except a color
    /// change (series add/remove/move/rename/show-hide, all point ops).
    /// Bind at `BindingLevel::Relayout` or `Rebuild`.
    pub fn structure_version(&self) -> Signal<u64> {
        self.inner.borrow().structure_version.clone()
    }

    /// Style version signal — bumped only by a series color change. Bind at
    /// `BindingLevel::RepaintOnly`.
    pub fn style_version(&self) -> Signal<u64> {
        self.inner.borrow().style_version.clone()
    }

    /// Register an observer that is called on every mutation.
    /// Returns an `ObserverHandle` — dropping it removes the callback.
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

    fn notify(&self, change: ChartChange) {
        let callbacks: Vec<Rc<dyn Fn(&ChartChange)>> = self
            .inner
            .borrow()
            .observers
            .iter()
            .map(|e| e.callback.clone())
            .collect();
        for cb in &callbacks {
            cb(&change);
        }
    }

    /// Bump `structure_version`. Clones the signal handle under a short
    /// borrow and drops it before calling `.set(...)` — an observer bound
    /// to the signal may read back into this model, and holding the
    /// `RefCell` borrow across that call would panic (re-entrant borrow).
    fn bump_structure(&self) {
        let sig = self.inner.borrow().structure_version.clone();
        sig.set(sig.get().wrapping_add(1));
    }

    /// Bump `style_version`. Same drop-borrow-before-set discipline as
    /// [`Self::bump_structure`].
    fn bump_style(&self) {
        let sig = self.inner.borrow().style_version.clone();
        sig.set(sig.get().wrapping_add(1));
    }
}

impl<T: std::fmt::Debug + 'static> ChartModel<T> {
    /// Register this model with the debug inspector under `name`. In
    /// release builds (`!cfg(debug_assertions)`) this is a no-op
    /// pass-through so call sites stay free of `#[cfg]` lines.
    ///
    /// Idempotent on repeated calls — the latest registration wins.
    /// The registration drops automatically when the last `ChartModel`
    /// handle is freed (the adapter the registry holds is `Weak`).
    pub fn debug_named(self, _name: impl Into<String>) -> Self {
        #[cfg(debug_assertions)]
        {
            let weak = Rc::downgrade(&self.inner);
            let adapter: Rc<dyn crate::debug_registry::ModelDebug> =
                Rc::new(ChartModelDebug::<T> { weak });
            let name = _name.into();
            crate::debug_registry::register(name, Rc::downgrade(&adapter));
            self.inner.borrow_mut().debug_adapter = Some(adapter);
        }
        self
    }
}

#[cfg(debug_assertions)]
struct ChartModelDebug<T> {
    weak: std::rc::Weak<RefCell<ChartModelInner<T>>>,
}

#[cfg(debug_assertions)]
impl<T: std::fmt::Debug + 'static> crate::debug_registry::ModelDebug for ChartModelDebug<T> {
    fn kind(&self) -> &'static str {
        "ChartModel"
    }
    fn len(&self) -> usize {
        self.weak
            .upgrade()
            .map(|inner| inner.borrow().arena.values().map(|e| e.points.len()).sum())
            .unwrap_or(0)
    }
    fn debug_dump(&self, out: &mut dyn std::fmt::Write) {
        let Some(inner) = self.weak.upgrade() else {
            return;
        };
        let guard = inner.borrow();
        for (i, &id) in guard.order.iter().enumerate() {
            if let Some(e) = guard.arena.get(id.key()) {
                let _ = writeln!(
                    out,
                    "[{}] {:?} ({} pts, visible={})",
                    i,
                    e.name,
                    e.points.len(),
                    e.visible
                );
            }
        }
    }
}

impl<T: 'static> Default for ChartModel<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: 'static> Clone for ChartModel<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T: std::fmt::Debug + 'static> std::fmt::Debug for ChartModel<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = self.inner.borrow();
        f.debug_struct("ChartModel")
            .field("series_count", &guard.order.len())
            .finish()
    }
}

#[cfg(test)]
impl<T: 'static> ChartModel<T> {
    /// Test-only introspection: number of live observers. Not part of the
    /// public API surface.
    pub(crate) fn observer_count(&self) -> usize {
        self.inner.borrow().observers.len()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn datum_with_color_sets_a_per_point_override() {
        use bastyde_core::color_prop::ColorProp;
        use bastyde_tokens::SurfaceRole;
        let plain = ChartDatum::new("Q1".to_string(), 5.0);
        assert!(plain.color.is_none(), "a plain datum has no color override");
        let colored = ChartDatum::new("Q1".to_string(), 5.0).with_color(SurfaceRole::StatusError);
        assert!(matches!(
            colored.color,
            Some(ColorProp::SurfaceRole(SurfaceRole::StatusError))
        ));
        // The 2-arg constructor is unchanged, so existing call sites still compile.
        let _ = ChartDatum::new("Q2".to_string(), 1.0);
    }

    fn sample() -> (ChartModel<String>, SeriesId, SeriesId) {
        let model = ChartModel::from_series_vec(vec![
            ChartSeries::new("Revenue").data(vec![
                ChartDatum::new("Q1".to_string(), 10.0),
                ChartDatum::new("Q2".to_string(), 20.0),
            ]),
            ChartSeries::new("Costs").data(vec![ChartDatum::new("Q1".to_string(), 5.0)]),
        ]);
        let a = model.series_id_at(0).unwrap();
        let b = model.series_id_at(1).unwrap();
        (model, a, b)
    }

    #[test]
    fn from_series_vec_builds_correctly() {
        let (model, a, b) = sample();
        assert_eq!(model.series_count(), 2);
        assert_eq!(model.point_count(a), 2);
        assert_eq!(model.point_count(b), 1);
        assert_eq!(
            model.with_series(a, |name, _, visible| (name.to_string(), visible)),
            Some(("Revenue".to_string(), true))
        );
    }

    #[test]
    fn new_is_empty() {
        let model: ChartModel<String> = ChartModel::new();
        assert_eq!(model.series_count(), 0);
        assert_eq!(model.only_series(), None);
    }

    #[test]
    fn only_series_some_iff_exactly_one() {
        let model: ChartModel<String> = ChartModel::new();
        assert_eq!(model.only_series(), None);
        let a = model.add_series("A");
        assert_eq!(model.only_series(), Some(a));
        model.add_series("B");
        assert_eq!(model.only_series(), None);
    }

    #[test]
    fn from_points_builds_single_anonymous_series() {
        let model = ChartModel::from_points(vec![
            ChartDatum::new("a".to_string(), 1.0),
            ChartDatum::new("b".to_string(), 2.0),
        ]);
        assert_eq!(model.series_count(), 1);
        let s = model.only_series().unwrap();
        assert_eq!(model.point_count(s), 2);
        assert_eq!(model.with_series(s, |_, _, visible| visible), Some(true));
    }

    fn track_changes(
        model: &ChartModel<String>,
    ) -> (Rc<RefCell<Vec<ChartChange>>>, ObserverHandle) {
        let log: Rc<RefCell<Vec<ChartChange>>> = Rc::new(RefCell::new(Vec::new()));
        let l = log.clone();
        let handle = model.observe_changes(move |c| l.borrow_mut().push(c.clone()));
        (log, handle)
    }

    #[test]
    fn add_series_emits_inserted_and_bumps_structure() {
        let model: ChartModel<String> = ChartModel::new();
        let structure = model.structure_version();
        let style = model.style_version();
        let (log, _handle) = track_changes(&model);

        let s = model.add_series("A");
        assert_eq!(log.borrow().len(), 1);
        assert_eq!(
            log.borrow()[0],
            ChartChange::SeriesInserted {
                index: 0,
                series: s
            }
        );
        assert_eq!(structure.get(), 1);
        assert_eq!(style.get(), 0);
    }

    #[test]
    fn insert_series_at_index() {
        let (model, a, b) = sample();
        let c = model.insert_series(1, "Middle");
        assert_eq!(model.series_ids(), vec![a, c, b]);
    }

    #[test]
    fn remove_series_emits_removed_and_bumps_structure() {
        let (model, a, _b) = sample();
        let structure_before = model.structure_version().get();
        let (log, _handle) = track_changes(&model);

        model.remove_series(a);
        assert_eq!(log.borrow().len(), 1);
        assert_eq!(log.borrow()[0], ChartChange::SeriesRemoved { series: a });
        assert_eq!(model.series_count(), 1);
        assert!(model.structure_version().get() > structure_before);
    }

    #[test]
    fn rename_series_emits_renamed_and_bumps_structure() {
        let (model, a, _b) = sample();
        let style_before = model.style_version().get();
        let (log, _handle) = track_changes(&model);

        model.rename_series(a, "New Name");
        assert_eq!(log.borrow().len(), 1);
        assert_eq!(log.borrow()[0], ChartChange::SeriesRenamed { series: a });
        assert_eq!(
            model.with_series(a, |name, _, _| name.to_string()),
            Some("New Name".to_string())
        );
        assert_eq!(
            model.style_version().get(),
            style_before,
            "renaming is not a style change"
        );
    }

    #[test]
    fn set_series_color_bumps_style_not_structure() {
        let (model, a, _b) = sample();
        let structure_before = model.structure_version().get();
        let style_before = model.style_version().get();
        let (log, _handle) = track_changes(&model);

        model.set_series_color(a, test_color());
        assert_eq!(log.borrow().len(), 1);
        assert_eq!(
            log.borrow()[0],
            ChartChange::SeriesColorChanged { series: a }
        );
        assert_eq!(
            model.structure_version().get(),
            structure_before,
            "color change must not bump structure_version"
        );
        assert!(model.style_version().get() > style_before);
        assert!(model.with_series(a, |_, color, _| color.is_some()).unwrap());
    }

    #[test]
    fn clear_series_color_bumps_style_and_clears() {
        let (model, a, _b) = sample();
        model.set_series_color(a, test_color());
        let style_before = model.style_version().get();
        let (log, _handle) = track_changes(&model);

        model.clear_series_color(a);
        assert_eq!(log.borrow().len(), 1);
        assert_eq!(
            log.borrow()[0],
            ChartChange::SeriesColorChanged { series: a }
        );
        assert!(model.style_version().get() > style_before);
        assert!(!model.with_series(a, |_, color, _| color.is_some()).unwrap());
    }

    #[test]
    fn set_series_visible_bumps_structure() {
        let (model, a, _b) = sample();
        let structure_before = model.structure_version().get();
        let (log, _handle) = track_changes(&model);

        model.set_series_visible(a, false);
        assert_eq!(log.borrow().len(), 1);
        assert_eq!(
            log.borrow()[0],
            ChartChange::SeriesVisibilityChanged { series: a }
        );
        assert!(model.structure_version().get() > structure_before);
        assert_eq!(model.with_series(a, |_, _, visible| visible), Some(false));
    }

    #[test]
    fn set_series_visible_noop_does_not_notify() {
        let (model, a, _b) = sample();
        let structure_before = model.structure_version().get();
        let (log, _handle) = track_changes(&model);

        model.set_series_visible(a, true); // already true
        assert_eq!(log.borrow().len(), 0);
        assert_eq!(model.structure_version().get(), structure_before);
    }

    #[test]
    fn move_series_emits_moved_and_bumps_structure() {
        let (model, a, b) = sample();
        let structure_before = model.structure_version().get();
        let (log, _handle) = track_changes(&model);

        model.move_series(a, 1);
        assert_eq!(model.series_ids(), vec![b, a]);
        assert_eq!(log.borrow().len(), 1);
        assert_eq!(
            log.borrow()[0],
            ChartChange::SeriesMoved {
                series: a,
                from: 0,
                to: 1
            }
        );
        assert!(model.structure_version().get() > structure_before);
    }

    #[test]
    fn move_series_noop_does_not_notify() {
        let (model, a, _b) = sample();
        let structure_before = model.structure_version().get();
        let (log, _handle) = track_changes(&model);

        model.move_series(a, 0); // already at 0
        assert_eq!(log.borrow().len(), 0);
        assert_eq!(model.structure_version().get(), structure_before);
    }

    #[test]
    fn push_point_emits_inserted_and_bumps_structure() {
        let (model, a, _b) = sample();
        let structure_before = model.structure_version().get();
        let (log, _handle) = track_changes(&model);

        model.push_point(a, "Q3".to_string(), 30.0);
        assert_eq!(model.point_count(a), 3);
        assert_eq!(log.borrow().len(), 1);
        assert_eq!(
            log.borrow()[0],
            ChartChange::PointsInserted {
                series: a,
                range: 2..3
            }
        );
        assert!(model.structure_version().get() > structure_before);
    }

    #[test]
    fn insert_point_at_index() {
        let (model, a, _b) = sample();
        model.insert_point(a, 1, "Q1.5".to_string(), 15.0);
        assert_eq!(model.point_count(a), 3);
        assert_eq!(
            model.with_point(a, 1, |d| d.category.clone()),
            Some("Q1.5".to_string())
        );
    }

    #[test]
    fn remove_point_emits_removed_and_returns_datum() {
        let (model, a, _b) = sample();
        let (log, _handle) = track_changes(&model);

        let removed = model.remove_point(a, 0);
        assert_eq!(removed.category, "Q1");
        assert_eq!(model.point_count(a), 1);
        assert_eq!(log.borrow().len(), 1);
        assert_eq!(
            log.borrow()[0],
            ChartChange::PointsRemoved {
                series: a,
                range: 0..1
            }
        );
    }

    #[test]
    fn update_point_bumps_structure_not_style() {
        let (model, a, _b) = sample();
        let structure_before = model.structure_version().get();
        let style_before = model.style_version().get();
        let (log, _handle) = track_changes(&model);

        model.update_point(a, 0, "Q1-revised".to_string(), 99.0);
        assert_eq!(log.borrow().len(), 1);
        assert_eq!(
            log.borrow()[0],
            ChartChange::PointUpdated {
                series: a,
                index: 0
            }
        );
        assert!(model.structure_version().get() > structure_before);
        assert_eq!(model.style_version().get(), style_before);
        assert_eq!(model.with_point(a, 0, |d| d.value), Some(99.0));
    }

    #[test]
    fn replace_series_data_emits_replaced() {
        let (model, a, _b) = sample();
        let (log, _handle) = track_changes(&model);

        model.replace_series_data(a, vec![ChartDatum::new("X".to_string(), 1.0)]);
        assert_eq!(model.point_count(a), 1);
        assert_eq!(log.borrow().len(), 1);
        assert_eq!(
            log.borrow()[0],
            ChartChange::SeriesDataReplaced { series: a }
        );
    }

    #[test]
    fn clear_emits_reset() {
        let (model, _a, _b) = sample();
        let (log, _handle) = track_changes(&model);

        model.clear();
        assert_eq!(model.series_count(), 0);
        assert_eq!(log.borrow().len(), 1);
        assert_eq!(log.borrow()[0], ChartChange::Reset);
    }

    #[test]
    fn observer_removed_on_handle_drop() {
        let model: ChartModel<String> = ChartModel::new();
        let count = Rc::new(Cell::new(0));
        let c = count.clone();
        let handle = model.observe_changes(move |_| c.set(c.get() + 1));

        model.add_series("A");
        assert_eq!(count.get(), 1);

        drop(handle);
        model.add_series("B");
        assert_eq!(count.get(), 1);
    }

    #[test]
    fn multiple_observers() {
        let model: ChartModel<String> = ChartModel::new();
        let count = Rc::new(Cell::new(0));
        let c1 = count.clone();
        let c2 = count.clone();
        let _h1 = model.observe_changes(move |_| c1.set(c1.get() + 1));
        let _h2 = model.observe_changes(move |_| c2.set(c2.get() + 1));

        model.add_series("A");
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn clone_shares_data_and_observers() {
        let (model, _a, _b) = sample();
        let clone = model.clone();
        let count = Rc::new(Cell::new(0));
        let c = count.clone();
        let _handle = model.observe_changes(move |_| c.set(c.get() + 1));

        clone.add_series("New");
        assert_eq!(model.series_count(), 3);
        assert_eq!(count.get(), 1);
    }

    #[test]
    fn with_all_series_returns_views_in_order() {
        let (model, a, b) = sample();
        let ids: Vec<SeriesId> =
            model.with_all_series(|views| views.iter().map(|v| v.id).collect());
        assert_eq!(ids, vec![a, b]);
        let names: Vec<String> =
            model.with_all_series(|views| views.iter().map(|v| v.name.to_string()).collect());
        assert_eq!(names, vec!["Revenue".to_string(), "Costs".to_string()]);
    }

    #[test]
    fn with_point_out_of_bounds_returns_none() {
        let (model, a, _b) = sample();
        assert_eq!(model.with_point(a, 99, |d| d.value), None);
    }

    /// A `SeriesId` that is guaranteed stale **within `model`'s own arena**.
    /// A `SeriesId` minted by a *different* `ChartModel` is not a safe
    /// stand-in — `SlotMap` keys are per-arena, so two freshly-created
    /// arenas hand out colliding keys (both start at index 0, generation
    /// 1). Removing a series from `model` itself bumps that slot's
    /// generation, so its old id is reliably rejected by this same arena.
    fn stale_id(model: &ChartModel<String>) -> SeriesId {
        let ghost = model.add_series("Ghost");
        model.remove_series(ghost);
        ghost
    }

    #[test]
    fn with_series_unknown_id_returns_none() {
        let (model, _a, _b) = sample();
        let ghost = stale_id(&model);
        assert_eq!(model.with_series(ghost, |_, _, _| ()), None);
        assert_eq!(model.with_point(ghost, 0, |d| d.value), None);
    }

    #[test]
    #[should_panic(expected = "unknown SeriesId")]
    fn remove_series_unknown_id_panics() {
        let (model, _a, _b) = sample();
        let ghost = stale_id(&model);
        model.remove_series(ghost);
    }

    #[test]
    #[should_panic]
    fn rename_series_unknown_id_panics() {
        let (model, _a, _b) = sample();
        let ghost = stale_id(&model);
        model.rename_series(ghost, "X");
    }

    #[test]
    #[should_panic]
    fn push_point_unknown_series_panics() {
        let (model, _a, _b) = sample();
        let ghost = stale_id(&model);
        model.push_point(ghost, "X".to_string(), 1.0);
    }

    #[test]
    #[should_panic(expected = "unknown SeriesId")]
    fn move_series_unknown_id_panics() {
        let (model, _a, _b) = sample();
        let ghost = stale_id(&model);
        model.move_series(ghost, 0);
    }

    // A minimal ColorProp value, built without a direct bastyde-tokens
    // *production* dependency — `bastyde-tokens` is pulled in as a
    // dev-dependency solely so tests can construct a concrete `Color`.
    fn test_color() -> bastyde_tokens::Color {
        bastyde_tokens::Color::from_hex("#FF0000")
    }
}
