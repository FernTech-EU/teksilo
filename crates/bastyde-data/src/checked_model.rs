// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `CheckedModel` — per-row checkbox state for flat collection widgets.
//!
//! Tracks which rows in a list view are marked (checked), independently of
//! which row is selected. Selection (cursor position) and checked-ness
//! (persistent marks) are orthogonal axes — the Outlook / Files-app
//! convention where you can check many items and then act on them all.
//!
//! The model issues one writable `Signal<bool>` per row index via
//! [`CheckedModel::signal_for`]; repeated calls for the same index return the
//! same cached handle. A `Checkbox` widget writes to that signal on click;
//! the model observes every per-index signal and keeps a central
//! `Signal<BTreeSet<usize>>` in sync so consumers can react to the complete
//! checked set without subscribing to each row individually.
//!
//! `CheckedModel` is a share-by-clone handle (`Rc<RefCell<…>>` internally);
//! cloning produces a second handle to the same state. When rows are inserted,
//! removed, or reordered, call the corresponding `adjust_for_*` method so that
//! checked state follows the moved items rather than sticking to stale indices.
//!
//! For hierarchical lists with descendant→ancestor tristate aggregation, see
//! [`crate::TreeCheckedModel`] instead.
//!
//! ```rust
//! # use bastyde_data::CheckedModel;
//! let model = CheckedModel::new();
//! model.check(1);
//! model.check(3);
//! assert!(model.is_checked(1));
//! assert_eq!(model.checked_count(), 2);
//! model.toggle(1);
//! assert!(!model.is_checked(1));
//! ```

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use bastyde_core::signal::{ObserverHandle, Signal};

struct Inner {
    per_index: HashMap<usize, Signal<bool>>,
    /// Observer handles keep the per-index → central propagation
    /// alive for the lifetime of the model.
    observers: HashMap<usize, ObserverHandle>,
}

/// Per-row checkbox state for a flat list, with a reactive aggregate checked-set.
pub struct CheckedModel {
    /// Aggregate set, derived from per-index signals via observers.
    /// Read-only externally; the model updates it whenever a per-index
    /// signal flips.
    checked: Signal<BTreeSet<usize>>,
    inner: Rc<RefCell<Inner>>,
}

impl CheckedModel {
    /// Creates a new, empty `CheckedModel` with no rows checked.
    pub fn new() -> Self {
        Self {
            checked: Signal::new(BTreeSet::new()),
            inner: Rc::new(RefCell::new(Inner {
                per_index: HashMap::new(),
                observers: HashMap::new(),
            })),
        }
    }

    /// Reactive view of the full checked-set.
    pub fn checked_signal(&self) -> Signal<BTreeSet<usize>> {
        self.checked.clone()
    }

    /// Writable per-index signal. Repeat calls cache the same handle —
    /// any consumer (the model itself, the Checkbox widget, an external
    /// observer) writing through it propagates to the central
    /// `checked_signal()`.
    pub fn signal_for(&self, index: usize) -> Signal<bool> {
        // Fast path — already cached.
        if let Some(sig) = self.inner.borrow().per_index.get(&index) {
            return sig.clone();
        }
        // Slow path — create + observe.
        let sig = Signal::new(false);
        let mut inner = self.inner.borrow_mut();
        Self::install_signal(&mut inner, &self.checked, index, sig.clone());
        sig
    }

    /// Register `sig` in `per_index` at `index`, wiring an observer that keeps
    /// the central `checked` set in sync. The observer captures `index` by
    /// value, so re-keying after an insert/remove/move must re-install the
    /// signal at its new index (replacing the stale-index observer) — see the
    /// `adjust_for_*` methods.
    fn install_signal(
        inner: &mut Inner,
        central: &Signal<BTreeSet<usize>>,
        index: usize,
        sig: Signal<bool>,
    ) {
        let central = central.clone();
        let handle = sig.observe(move |checked| {
            let mut set = central.get();
            let changed = if *checked {
                set.insert(index)
            } else {
                set.remove(&index)
            };
            if changed {
                central.set(set);
            }
        });
        inner.per_index.insert(index, sig);
        inner.observers.insert(index, handle);
    }

    /// Re-key every per-index signal through `map`, dropping any whose `map`
    /// returns `None` (removed rows). Observers are rebuilt so each captures
    /// its new index. The central set is recomputed to match. Shared spine of
    /// `adjust_for_insert` / `adjust_for_remove` / `adjust_for_move`.
    fn rekey(&self, map: impl Fn(usize) -> Option<usize>) {
        let entries: Vec<(usize, Signal<bool>)> = {
            let inner = self.inner.borrow();
            inner
                .per_index
                .iter()
                .map(|(&i, s)| (i, s.clone()))
                .collect()
        };
        {
            let mut inner = self.inner.borrow_mut();
            // Dropping the old observers here detaches their stale-index
            // callbacks before the rebuilt ones are installed.
            inner.per_index.clear();
            inner.observers.clear();
            for (idx, sig) in entries {
                if let Some(new_idx) = map(idx) {
                    Self::install_signal(&mut inner, &self.checked, new_idx, sig);
                }
            }
        }
        let old = self.checked.get();
        let new: BTreeSet<usize> = old.iter().filter_map(|&i| map(i)).collect();
        if new != old {
            self.checked.set(new);
        }
    }

    /// Shift checked-state after `count` rows are inserted at `start`.
    /// Indices `>= start` move up by `count`.
    pub fn adjust_for_insert(&self, start: usize, count: usize) {
        if count == 0 {
            return;
        }
        self.rekey(|i| Some(if i >= start { i + count } else { i }));
    }

    /// Shift checked-state after `count` rows starting at `start` are removed.
    /// Checked rows in `start..start+count` are dropped; later rows shift down.
    pub fn adjust_for_remove(&self, start: usize, count: usize) {
        if count == 0 {
            return;
        }
        let end = start + count;
        self.rekey(|i| {
            if i < start {
                Some(i)
            } else if i >= end {
                Some(i - count)
            } else {
                None
            }
        });
    }

    /// Shift checked-state after a block of `count` rows moved from `from` to
    /// `to` (a post-removal index, matching `ListModel::move_item`). Checked
    /// rows follow their items.
    pub fn adjust_for_move(&self, from: usize, to: usize, count: usize) {
        if from == to || count == 0 {
            return;
        }
        self.rekey(|i| Some(crate::map_index_after_move(i, from, to, count)));
    }

    /// Returns `true` if the row at `index` is currently checked.
    pub fn is_checked(&self, index: usize) -> bool {
        self.inner
            .borrow()
            .per_index
            .get(&index)
            .map(|s| s.get())
            .unwrap_or(false)
    }

    /// Returns a sorted `Vec` of every currently checked row index.
    pub fn checked_indices(&self) -> Vec<usize> {
        self.checked.get().into_iter().collect()
    }

    /// Returns the number of currently checked rows.
    pub fn checked_count(&self) -> usize {
        self.checked.get().len()
    }

    /// Marks the row at `index` as checked; notifies observers if the state changed.
    pub fn check(&self, index: usize) {
        let sig = self.signal_for(index);
        if !sig.get() {
            sig.set(true);
        }
    }

    /// Marks the row at `index` as unchecked; notifies observers if the state changed.
    pub fn uncheck(&self, index: usize) {
        let sig = self.signal_for(index);
        if sig.get() {
            sig.set(false);
        }
    }

    /// Flips the checked state of the row at `index`; notifies observers.
    pub fn toggle(&self, index: usize) {
        let sig = self.signal_for(index);
        sig.set(!sig.get());
    }

    /// Checks every row in `0..count`; notifies observers for each row that was unchecked.
    pub fn check_all(&self, count: usize) {
        for i in 0..count {
            self.check(i);
        }
    }

    /// Unchecks every currently checked row; notifies observers for each change.
    pub fn clear(&self) {
        // Snapshot keys to avoid borrow-during-iteration when set()
        // recurses into the observer.
        let keys: Vec<usize> = self.inner.borrow().per_index.keys().copied().collect();
        for i in keys {
            self.uncheck(i);
        }
    }
}

impl Default for CheckedModel {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for CheckedModel {
    fn clone(&self) -> Self {
        Self {
            checked: self.checked.clone(),
            inner: self.inner.clone(),
        }
    }
}

impl std::fmt::Debug for CheckedModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CheckedModel")
            .field("checked_count", &self.checked.get().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_for_returns_same_root_per_index() {
        let m = CheckedModel::new();
        let a = m.signal_for(2);
        let b = m.signal_for(2);
        assert_eq!(a.get(), b.get());
        a.set(true);
        assert!(b.get(), "cached signal handles share the same root");
    }

    #[test]
    fn writing_per_index_signal_updates_central() {
        let m = CheckedModel::new();
        let s = m.signal_for(7);
        s.set(true);
        assert_eq!(m.checked_indices(), vec![7]);
        s.set(false);
        assert_eq!(m.checked_indices(), Vec::<usize>::new());
    }

    #[test]
    fn check_uncheck_toggle_round_trip() {
        let m = CheckedModel::new();
        assert!(!m.is_checked(3));
        m.check(3);
        assert!(m.is_checked(3));
        m.toggle(3);
        assert!(!m.is_checked(3));
        m.toggle(3);
        assert!(m.is_checked(3));
        m.uncheck(3);
        assert!(!m.is_checked(3));
    }

    #[test]
    fn check_all_then_clear() {
        let m = CheckedModel::new();
        m.check_all(5);
        assert_eq!(m.checked_indices(), vec![0, 1, 2, 3, 4]);
        m.clear();
        assert_eq!(m.checked_count(), 0);
    }

    #[test]
    fn signal_updates_propagate() {
        let m = CheckedModel::new();
        let s = m.signal_for(7);
        assert!(!s.get());
        m.check(7);
        assert!(s.get());
        m.uncheck(7);
        assert!(!s.get());
    }

    #[test]
    fn unrelated_index_does_not_flip_signal() {
        let m = CheckedModel::new();
        let s = m.signal_for(1);
        m.check(2);
        assert!(!s.get());
    }

    #[test]
    fn adjust_for_insert_shifts_checked_rows() {
        let m = CheckedModel::new();
        m.check(2);
        m.check(4);
        m.adjust_for_insert(3, 2);
        // 2 stays, 4 -> 6.
        assert_eq!(m.checked_indices(), vec![2, 6]);
        assert!(m.is_checked(2));
        assert!(m.is_checked(6));
        assert!(!m.is_checked(4));
    }

    #[test]
    fn adjust_for_remove_drops_in_range_and_shifts() {
        let m = CheckedModel::new();
        m.check(1);
        m.check(3);
        m.check(5);
        m.adjust_for_remove(2, 2); // remove rows 2,3
        // 1 stays, 3 dropped, 5 -> 3.
        assert_eq!(m.checked_indices(), vec![1, 3]);
        assert!(m.is_checked(3), "row that shifted in is checked");
    }

    #[test]
    fn adjust_for_move_follows_checked_row() {
        let m = CheckedModel::new();
        m.check(0); // row A checked
        m.adjust_for_move(0, 2, 1); // [B,C,A] — A now at 2
        assert_eq!(m.checked_indices(), vec![2]);
        assert!(m.is_checked(2));
    }

    #[test]
    fn rekey_rewires_observer_so_later_clicks_target_the_new_index() {
        // After a shift, the per-index signal handle must drive the *new*
        // central index when toggled, not the stale captured one.
        let m = CheckedModel::new();
        let s = m.signal_for(2);
        s.set(true);
        assert_eq!(m.checked_indices(), vec![2]);
        m.adjust_for_insert(0, 1); // row 2 -> 3, same Signal handle
        assert_eq!(m.checked_indices(), vec![3]);
        // The handle the widget kept now flips index 3 in the central set.
        s.set(false);
        assert_eq!(m.checked_indices(), Vec::<usize>::new());
        s.set(true);
        assert_eq!(m.checked_indices(), vec![3], "observer re-keyed to 3");
    }

    #[test]
    fn adjust_is_noop_on_empty_and_zero_count() {
        let m = CheckedModel::new();
        m.check(1);
        m.adjust_for_insert(0, 0);
        m.adjust_for_remove(5, 0);
        m.adjust_for_move(2, 2, 1);
        assert_eq!(m.checked_indices(), vec![1]);
    }
}
