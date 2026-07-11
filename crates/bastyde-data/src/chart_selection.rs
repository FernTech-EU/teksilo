// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `ChartSelection` — point-level selection state for chart widgets.
//!
//! [`ChartSelection`] manages which `(series, point index)` pairs are
//! selected across a [`crate::ChartModel`] — the chart counterpart of
//! [`crate::SelectionModel`] (flat lists) and
//! [`crate::KeyedSelectionModel`] (keyed collections). It is a
//! share-by-clone handle: pass a clone to each chart that should share
//! selection state. The current selection is exposed as a reactive
//! `Signal<HashSet<(SeriesId, usize)>>` so widgets can bind to it without
//! polling.
//!
//! `HashSet` (not `BTreeSet`) is used because [`SeriesId`] is intentionally
//! **not** `Ord` (it's an opaque SlotMap key, mirroring [`crate::NodeId`]) —
//! there is no natural ordering across series, only within one series'
//! point indices. This is the same rationale as
//! [`crate::KeyedSelectionModel`], which uses `HashSet<K>` for the same
//! reason.
//!
//! Three selection behaviours are available via
//! [`SelectionMode`]: `None`, `Single`, and `Multi`
//! (toggle + anchor-based range extension). [`ChartSelection::extend_to`]
//! only extends within the anchor's own series — a cross-series "range" has
//! no natural order, so it falls back to a single-point select.
//! [`ChartSelection::adjust`] keeps selected points consistent as the
//! source model mutates (series removed, points inserted/removed).
//!
//! ```rust
//! # use bastyde_data::{ChartModel, ChartSelection, SelectionMode};
//! let model: ChartModel<i32> = ChartModel::new();
//! let s = model.add_series("s");
//! for i in 0..5 {
//!     model.push_point(s, i, i as f32);
//! }
//!
//! let sel = ChartSelection::new(SelectionMode::Multi);
//! sel.select_point(s, 1);
//! sel.extend_to(s, 3);
//! assert_eq!(sel.count(), 3); // (s,1), (s,2), (s,3)
//! sel.clear();
//! assert_eq!(sel.count(), 0);
//! ```

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use bastyde_core::signal::Signal;

use crate::chart_change::{ChartChange, SeriesId};
use crate::selection_model::SelectionMode;

/// Point-level selection state for a chart, keyed by `(series, point
/// index)`. See module documentation for semantics.
pub struct ChartSelection {
    mode: SelectionMode,
    selection: Signal<HashSet<(SeriesId, usize)>>,
    /// Anchor point for range extension. Shared via `Rc` so clones see the
    /// same anchor state.
    anchor: Rc<RefCell<Option<(SeriesId, usize)>>>,
    /// Strong holder for the debug-registry adapter. Shared across clones;
    /// once all `ChartSelection` handles drop, the holder `Rc` reaches
    /// zero and the adapter is freed, marking the registry entry dead.
    /// `None` until `.debug_named()` is called. Compiled out in release.
    #[cfg(debug_assertions)]
    debug_adapter_holder: Rc<RefCell<Option<Rc<dyn crate::debug_registry::ModelDebug>>>>,
}

impl ChartSelection {
    /// Create a new chart selection with the given mode.
    pub fn new(mode: SelectionMode) -> Self {
        Self {
            mode,
            selection: Signal::new(HashSet::new()),
            anchor: Rc::new(RefCell::new(None)),
            #[cfg(debug_assertions)]
            debug_adapter_holder: Rc::new(RefCell::new(None)),
        }
    }

    /// The selection mode.
    pub fn mode(&self) -> SelectionMode {
        self.mode
    }

    /// A clone of the selection signal for reactive binding.
    pub fn selection_signal(&self) -> Signal<HashSet<(SeriesId, usize)>> {
        self.selection.clone()
    }

    /// Whether `(series, index)` is currently selected.
    pub fn is_selected(&self, series: SeriesId, index: usize) -> bool {
        self.selection.get().contains(&(series, index))
    }

    /// The currently selected points (unordered snapshot).
    pub fn selected_points(&self) -> Vec<(SeriesId, usize)> {
        self.selection.get().into_iter().collect()
    }

    /// Number of selected points.
    pub fn count(&self) -> usize {
        self.selection.get().len()
    }

    /// Select a single point, clearing the previous selection and setting
    /// the anchor.
    pub fn select_point(&self, series: SeriesId, index: usize) {
        if self.mode == SelectionMode::None {
            return;
        }
        let mut set = HashSet::new();
        set.insert((series, index));
        self.selection.set(set);
        *self.anchor.borrow_mut() = Some((series, index));
    }

    /// Toggle a point (Ctrl+click in Multi mode; acts as `select_point` in
    /// Single mode).
    pub fn toggle_point(&self, series: SeriesId, index: usize) {
        match self.mode {
            SelectionMode::None => {}
            SelectionMode::Single => self.select_point(series, index),
            SelectionMode::Multi => {
                let key = (series, index);
                let mut set = self.selection.get();
                if set.contains(&key) {
                    set.remove(&key);
                } else {
                    set.insert(key);
                }
                self.selection.set(set);
                *self.anchor.borrow_mut() = Some(key);
            }
        }
    }

    /// Extend the selection from the anchor to `(series, target)` (for
    /// Shift+click). Only extends **within the anchor's own series** — if
    /// the anchor is unset or belongs to a different series, falls back to
    /// a single-point select of `(series, target)`.
    pub fn extend_to(&self, series: SeriesId, target: usize) {
        match self.mode {
            SelectionMode::None => {}
            SelectionMode::Single => self.select_point(series, target),
            SelectionMode::Multi => {
                let anchor = *self.anchor.borrow();
                let Some((a_series, a_index)) = anchor else {
                    self.select_point(series, target);
                    return;
                };
                if a_series != series {
                    self.select_point(series, target);
                    return;
                }
                let start = a_index.min(target);
                let end = a_index.max(target);
                let mut set = self.selection.get();
                for i in start..=end {
                    set.insert((series, i));
                }
                self.selection.set(set);
                // Anchor stays put.
            }
        }
    }

    /// Replace the selection with `points` (or, when `additive`, union
    /// them into the current selection). Used by rubber-band / marquee
    /// selection. In `Single` mode an arbitrary one wins; `None` mode is a
    /// no-op.
    pub fn select_points(
        &self,
        points: impl IntoIterator<Item = (SeriesId, usize)>,
        additive: bool,
    ) {
        if self.mode == SelectionMode::None {
            return;
        }
        let mut set = if additive {
            self.selection.get()
        } else {
            HashSet::new()
        };
        set.extend(points);
        if self.mode == SelectionMode::Single && set.len() > 1 {
            let keep = set.iter().next().copied();
            set = keep.into_iter().collect();
        }
        self.selection.set(set);
    }

    /// Clear the selection and anchor.
    pub fn clear(&self) {
        self.selection.set(HashSet::new());
        *self.anchor.borrow_mut() = None;
    }

    /// React to an upstream [`ChartChange`], keeping selection consistent
    /// with the model: a removed or wholesale-replaced series drops its
    /// selected points (and the anchor, if it pointed there); point
    /// insertions/removals shift or drop indices within their series.
    /// Series metadata changes (rename/recolor/visibility/move/insert) and
    /// in-place point updates never affect which points are selected.
    pub fn adjust(&self, change: &ChartChange) {
        match change {
            ChartChange::SeriesRemoved { series } | ChartChange::SeriesDataReplaced { series } => {
                let series = *series;
                let old = self.selection.get();
                let new: HashSet<(SeriesId, usize)> =
                    old.iter().filter(|(s, _)| *s != series).copied().collect();
                if new.len() != old.len() {
                    self.selection.set(new);
                }
                let drop_anchor = self
                    .anchor
                    .borrow()
                    .as_ref()
                    .is_some_and(|(s, _)| *s == series);
                if drop_anchor {
                    *self.anchor.borrow_mut() = None;
                }
            }
            ChartChange::PointsInserted { series, range } => {
                let series = *series;
                let start = range.start;
                let count = range.end - range.start;
                let old = self.selection.get();
                let new: HashSet<(SeriesId, usize)> = old
                    .iter()
                    .map(|&(s, i)| {
                        if s == series && i >= start {
                            (s, i + count)
                        } else {
                            (s, i)
                        }
                    })
                    .collect();
                if new != old {
                    self.selection.set(new);
                }
                let mut anchor = self.anchor.borrow_mut();
                if let Some((s, i)) = *anchor
                    && s == series
                    && i >= start
                {
                    *anchor = Some((s, i + count));
                }
            }
            ChartChange::PointsRemoved { series, range } => {
                let series = *series;
                let start = range.start;
                let end = range.end;
                let count = range.end - range.start;
                let old = self.selection.get();
                let new: HashSet<(SeriesId, usize)> = old
                    .iter()
                    .filter_map(|&(s, i)| {
                        if s != series {
                            return Some((s, i));
                        }
                        if i < start {
                            Some((s, i))
                        } else if i >= end {
                            Some((s, i - count))
                        } else {
                            None
                        }
                    })
                    .collect();
                if new != old {
                    self.selection.set(new);
                }
                let mut anchor = self.anchor.borrow_mut();
                if let Some((s, i)) = *anchor
                    && s == series
                {
                    if i >= end {
                        *anchor = Some((s, i - count));
                    } else if i >= start {
                        *anchor = None;
                    }
                }
            }
            ChartChange::Reset => self.clear(),
            // Series metadata and in-place point updates don't change
            // which points are selected.
            ChartChange::SeriesInserted { .. }
            | ChartChange::SeriesMoved { .. }
            | ChartChange::SeriesRenamed { .. }
            | ChartChange::SeriesColorChanged { .. }
            | ChartChange::SeriesVisibilityChanged { .. }
            | ChartChange::PointUpdated { .. } => {}
        }
    }

    /// Drop any selected point for which `exists` returns false.
    pub fn prune(&self, exists: impl Fn(SeriesId, usize) -> bool) {
        let old = self.selection.get();
        let new: HashSet<(SeriesId, usize)> = old
            .iter()
            .filter(|(s, i)| exists(*s, *i))
            .copied()
            .collect();
        if new.len() != old.len() {
            self.selection.set(new);
        }
        let drop_anchor = self
            .anchor
            .borrow()
            .as_ref()
            .is_some_and(|(s, i)| !exists(*s, *i));
        if drop_anchor {
            *self.anchor.borrow_mut() = None;
        }
    }
}

impl Clone for ChartSelection {
    fn clone(&self) -> Self {
        Self {
            mode: self.mode,
            selection: self.selection.clone(),
            anchor: self.anchor.clone(),
            #[cfg(debug_assertions)]
            debug_adapter_holder: self.debug_adapter_holder.clone(),
        }
    }
}

impl ChartSelection {
    /// Register this selection with the debug inspector under `name`. In
    /// release builds (`!cfg(debug_assertions)`) this is a no-op
    /// pass-through so call sites stay free of `#[cfg]` lines.
    ///
    /// Idempotent on repeated calls — the latest registration wins. The
    /// registration drops automatically when the last `ChartSelection`
    /// handle is freed (the strong adapter `Rc` lives inside a shared
    /// holder; the registry holds only a `Weak`).
    pub fn debug_named(self, _name: impl Into<String>) -> Self {
        #[cfg(debug_assertions)]
        {
            let adapter: Rc<dyn crate::debug_registry::ModelDebug> = Rc::new(ChartSelectionDebug {
                selection: self.selection.clone(),
                mode: self.mode,
            });
            crate::debug_registry::register(_name.into(), Rc::downgrade(&adapter));
            *self.debug_adapter_holder.borrow_mut() = Some(adapter);
        }
        self
    }
}

#[cfg(debug_assertions)]
struct ChartSelectionDebug {
    selection: Signal<HashSet<(SeriesId, usize)>>,
    mode: SelectionMode,
}

#[cfg(debug_assertions)]
impl crate::debug_registry::ModelDebug for ChartSelectionDebug {
    fn kind(&self) -> &'static str {
        "ChartSelection"
    }
    fn len(&self) -> usize {
        self.selection.get().len()
    }
    fn debug_dump(&self, out: &mut dyn std::fmt::Write) {
        let _ = writeln!(out, "mode = {:?}", self.mode);
        let sel = self.selection.get();
        if sel.is_empty() {
            let _ = writeln!(out, "(empty)");
            return;
        }
        for (s, i) in sel.iter() {
            let _ = writeln!(out, "{:?}[{}]", s, i);
        }
    }
}

impl std::fmt::Debug for ChartSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChartSelection")
            .field("mode", &self.mode)
            .field("selected_count", &self.selection.get().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart_model::ChartModel;

    fn two_series() -> (SeriesId, SeriesId) {
        let model: ChartModel<i32> = ChartModel::new();
        let a = model.add_series("a");
        let b = model.add_series("b");
        (a, b)
    }

    fn set(points: impl IntoIterator<Item = (SeriesId, usize)>) -> HashSet<(SeriesId, usize)> {
        points.into_iter().collect()
    }

    #[test]
    fn select_toggle_extend_within_series() {
        let (a, _b) = two_series();
        let sel = ChartSelection::new(SelectionMode::Multi);
        sel.select_point(a, 2);
        sel.extend_to(a, 5);
        assert_eq!(
            sel.selection_signal().get(),
            set([(a, 2), (a, 3), (a, 4), (a, 5)])
        );
    }

    #[test]
    fn toggle_deselects() {
        let (a, _b) = two_series();
        let sel = ChartSelection::new(SelectionMode::Multi);
        sel.toggle_point(a, 1);
        assert!(sel.is_selected(a, 1));
        sel.toggle_point(a, 1);
        assert!(!sel.is_selected(a, 1));
    }

    #[test]
    fn extend_backwards_within_series() {
        let (a, _b) = two_series();
        let sel = ChartSelection::new(SelectionMode::Multi);
        sel.select_point(a, 5);
        sel.extend_to(a, 2);
        assert_eq!(
            sel.selection_signal().get(),
            set([(a, 2), (a, 3), (a, 4), (a, 5)])
        );
    }

    #[test]
    fn cross_series_extend_falls_back_to_single_select() {
        let (a, b) = two_series();
        let sel = ChartSelection::new(SelectionMode::Multi);
        sel.select_point(a, 2); // anchor (a, 2)
        sel.extend_to(b, 3);
        assert_eq!(sel.selection_signal().get(), set([(b, 3)]));
    }

    #[test]
    fn select_points_additive_and_replace() {
        let (a, b) = two_series();
        let sel = ChartSelection::new(SelectionMode::Multi);
        sel.select_points([(a, 0), (a, 1)], false);
        assert_eq!(sel.count(), 2);
        sel.select_points([(b, 0)], true);
        assert_eq!(sel.selection_signal().get(), set([(a, 0), (a, 1), (b, 0)]));
        sel.select_points([(b, 1)], false);
        assert_eq!(sel.selection_signal().get(), set([(b, 1)]));
    }

    #[test]
    fn none_mode_ignores_all() {
        let (a, _b) = two_series();
        let sel = ChartSelection::new(SelectionMode::None);
        sel.select_point(a, 0);
        sel.toggle_point(a, 1);
        sel.select_points([(a, 2)], false);
        assert_eq!(sel.count(), 0);
    }

    #[test]
    fn single_mode_extend_acts_as_select() {
        let (a, b) = two_series();
        let sel = ChartSelection::new(SelectionMode::Single);
        sel.select_point(a, 1);
        sel.extend_to(b, 5);
        assert_eq!(sel.selection_signal().get(), set([(b, 5)]));
    }

    #[test]
    fn adjust_drops_on_series_removed() {
        let (a, b) = two_series();
        let sel = ChartSelection::new(SelectionMode::Multi);
        sel.select_points([(a, 0), (a, 1), (b, 0)], false);
        sel.adjust(&ChartChange::SeriesRemoved { series: a });
        assert_eq!(sel.selection_signal().get(), set([(b, 0)]));
    }

    #[test]
    fn adjust_drops_anchor_on_series_removed() {
        let (a, b) = two_series();
        let sel = ChartSelection::new(SelectionMode::Multi);
        sel.select_point(a, 0); // anchor = (a, 0)
        sel.adjust(&ChartChange::SeriesRemoved { series: a });
        // Anchor was dropped; extend_to now falls back to a single select.
        sel.extend_to(b, 3);
        assert_eq!(sel.selection_signal().get(), set([(b, 3)]));
    }

    #[test]
    fn adjust_drops_on_series_data_replaced() {
        let (a, b) = two_series();
        let sel = ChartSelection::new(SelectionMode::Multi);
        sel.select_points([(a, 0), (b, 0)], false);
        sel.adjust(&ChartChange::SeriesDataReplaced { series: a });
        assert_eq!(sel.selection_signal().get(), set([(b, 0)]));
    }

    #[test]
    fn adjust_clears_on_reset() {
        let (a, _b) = two_series();
        let sel = ChartSelection::new(SelectionMode::Multi);
        sel.select_point(a, 0);
        sel.adjust(&ChartChange::Reset);
        assert_eq!(sel.count(), 0);
    }

    #[test]
    fn adjust_shifts_on_points_inserted() {
        let (a, b) = two_series();
        let sel = ChartSelection::new(SelectionMode::Multi);
        sel.select_points([(a, 1), (a, 3), (b, 1)], false);
        sel.adjust(&ChartChange::PointsInserted {
            series: a,
            range: 2..4,
        });
        assert_eq!(sel.selection_signal().get(), set([(a, 1), (a, 5), (b, 1)]));
    }

    #[test]
    fn adjust_shifts_and_drops_on_points_removed() {
        let (a, _b) = two_series();
        let sel = ChartSelection::new(SelectionMode::Multi);
        sel.select_points([(a, 1), (a, 3), (a, 5)], false);
        sel.adjust(&ChartChange::PointsRemoved {
            series: a,
            range: 2..4,
        });
        // index 1 stays; index 3 dropped (inside the removed range); index 5 shifts to 3.
        assert_eq!(sel.selection_signal().get(), set([(a, 1), (a, 3)]));
    }

    #[test]
    fn adjust_ignores_metadata_and_point_updated() {
        let (a, _b) = two_series();
        let sel = ChartSelection::new(SelectionMode::Multi);
        sel.select_point(a, 2);
        sel.adjust(&ChartChange::SeriesInserted {
            index: 0,
            series: a,
        });
        sel.adjust(&ChartChange::SeriesMoved {
            series: a,
            from: 0,
            to: 1,
        });
        sel.adjust(&ChartChange::SeriesRenamed { series: a });
        sel.adjust(&ChartChange::SeriesColorChanged { series: a });
        sel.adjust(&ChartChange::SeriesVisibilityChanged { series: a });
        sel.adjust(&ChartChange::PointUpdated {
            series: a,
            index: 2,
        });
        assert_eq!(sel.selection_signal().get(), set([(a, 2)]));
    }

    #[test]
    fn prune_drops_missing_points() {
        let (a, b) = two_series();
        let sel = ChartSelection::new(SelectionMode::Multi);
        sel.select_points([(a, 0), (a, 1), (b, 0)], false);
        sel.prune(|s, i| !(s == a && i == 1));
        assert_eq!(sel.selection_signal().get(), set([(a, 0), (b, 0)]));
    }

    #[test]
    fn prune_drops_anchor_when_missing() {
        let (a, _b) = two_series();
        let sel = ChartSelection::new(SelectionMode::Multi);
        sel.select_point(a, 3); // anchor = (a, 3)
        sel.prune(|_, _| false);
        sel.extend_to(a, 9);
        // Anchor was pruned -> extend_to falls back to single-select.
        assert_eq!(sel.selection_signal().get(), set([(a, 9)]));
    }

    #[test]
    fn signal_reactivity() {
        use std::cell::Cell;
        let (a, _b) = two_series();
        let sel = ChartSelection::new(SelectionMode::Single);
        let signal = sel.selection_signal();
        let changed = Rc::new(Cell::new(false));
        let c = changed.clone();
        let _handle = signal.observe(move |_| c.set(true));
        sel.select_point(a, 0);
        assert!(changed.get());
    }

    #[test]
    fn clone_shares_selection_and_anchor() {
        let (a, b) = two_series();
        let sel = ChartSelection::new(SelectionMode::Multi);
        let clone = sel.clone();
        sel.select_point(a, 1);
        assert!(clone.is_selected(a, 1));
        clone.extend_to(a, 3); // uses the shared anchor from `sel`
        assert_eq!(sel.selection_signal().get(), set([(a, 1), (a, 2), (a, 3)]));
        let _ = b;
    }
}
