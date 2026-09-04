// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `KeyedSelectionModel<K>` — identity-based selection for collection widgets.
//!
//! [`KeyedSelectionModel<K>`](KeyedSelectionModel) stores selection as a set of
//! source-defined **keys** rather than visible **indices**. This is what
//! [`SelectionModel`](crate::SelectionModel) cannot do: survive lazy
//! window-slides and external reorders, and stay consistent across two views of
//! the same source that scroll/sort/filter independently (selection is a set of
//! identities, not positions). It coexists with the index-based
//! [`SelectionModel`](crate::SelectionModel) — views opt into one or the other.
//!
//! Shift+click range extension is index-ordered by nature, so `extend_to` takes
//! the current visible key order from the caller (the projection) at click
//! time; the anchor is stored as a *key* so it survives scrolling out of the
//! resident window. The selection is exposed as a reactive
//! `Signal<HashSet<K>>` via `selection_signal()`.
//!
//! ## When to use
//!
//! Use [`KeyedSelectionModel`] when rows are identified by a stable domain key
//! (entity id, file path, UUID) that survives reorders, sorts, and lazy-loading
//! evictions. Use [`SelectionModel`](crate::SelectionModel) when rows are
//! identified by their current visible index (simple in-memory lists).
//!
//! ```rust
//! # use teksilo_data::KeyedSelectionModel;
//! # use teksilo_data::SelectionMode;
//! let sel: KeyedSelectionModel<u64> = KeyedSelectionModel::new(SelectionMode::Multi);
//! sel.select(10);
//! sel.toggle(20);
//! sel.toggle(30);
//! assert_eq!(sel.count(), 3);
//! sel.toggle(10); // deselect
//! assert!(!sel.is_selected(&10));
//! sel.clear();
//! assert_eq!(sel.count(), 0);
//! ```

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use teksilo_core::signal::Signal;

use crate::dnd_types::ItemKey;
use crate::selection_model::SelectionMode;

/// Selection state keyed by source-defined identity rather than visible index.
///
/// The selection set is exposed as a `Signal<HashSet<K>>` (via
/// [`selection_signal`](KeyedSelectionModel::selection_signal)) so widgets
/// observe it reactively without polling. Cloning the model shares the same
/// selection and anchor across all handles. The Shift+click anchor is stored as
/// a `K` so it survives lazy-window evictions and visible-order changes.
pub struct KeyedSelectionModel<K: ItemKey> {
    mode: SelectionMode,
    selection: Signal<HashSet<K>>,
    anchor: Rc<RefCell<Option<K>>>,
    /// The selection as it stood when the current Shift gesture began — the
    /// set a range extension is unioned *with*, so reversing the gesture
    /// shrinks the range instead of growing it. Committed by every mutator
    /// that is not itself an extension; see `SelectionModel::base`, which
    /// carries the full rationale.
    base: Rc<RefCell<HashSet<K>>>,
    /// Whether a range extension is already in progress, so
    /// [`KeyedSelectionModel::extend_to_additive`] captures its base once per
    /// gesture rather than on every keystroke.
    extending: Rc<std::cell::Cell<bool>>,
    /// Strong holder for the debug-registry adapter; shared across clones.
    /// Compiled out in release.
    #[cfg(debug_assertions)]
    debug_adapter_holder: Rc<RefCell<Option<Rc<dyn crate::debug_registry::ModelDebug>>>>,
}

impl<K: ItemKey> KeyedSelectionModel<K> {
    /// Create a new keyed selection model with the given mode.
    pub fn new(mode: SelectionMode) -> Self {
        Self {
            mode,
            selection: Signal::new(HashSet::new()),
            anchor: Rc::new(RefCell::new(None)),
            base: Rc::new(RefCell::new(HashSet::new())),
            extending: Rc::new(std::cell::Cell::new(false)),
            #[cfg(debug_assertions)]
            debug_adapter_holder: Rc::new(RefCell::new(None)),
        }
    }

    /// Commit `base` and end any range gesture in progress.
    fn commit_base(&self, base: HashSet<K>) {
        *self.base.borrow_mut() = base;
        self.extending.set(false);
    }

    /// The selection mode.
    pub fn mode(&self) -> SelectionMode {
        self.mode
    }

    /// A clone of the selection signal for reactive binding.
    pub fn selection_signal(&self) -> Signal<HashSet<K>> {
        self.selection.clone()
    }

    /// Whether `key` is currently selected (O(1)).
    pub fn is_selected(&self, key: &K) -> bool {
        self.selection.get().contains(key)
    }

    /// The currently selected keys (unordered snapshot).
    pub fn selected_keys(&self) -> Vec<K> {
        self.selection.get().into_iter().collect()
    }

    /// Number of selected items.
    pub fn count(&self) -> usize {
        self.selection.get().len()
    }

    /// Select a single key, clearing previous selection and setting the anchor.
    pub fn select(&self, key: K) {
        if self.mode == SelectionMode::None {
            return;
        }
        let mut set = HashSet::new();
        set.insert(key.clone());
        self.selection.set(set);
        *self.anchor.borrow_mut() = Some(key);
        self.commit_base(HashSet::new());
    }

    /// Toggle a key (Ctrl+click in Multi mode; acts as `select` in Single).
    pub fn toggle(&self, key: K) {
        match self.mode {
            SelectionMode::None => {}
            SelectionMode::Single => self.select(key),
            SelectionMode::Multi => {
                let mut set = self.selection.get();
                if set.contains(&key) {
                    set.remove(&key);
                } else {
                    set.insert(key.clone());
                }
                self.selection.set(set.clone());
                // The anchor moves to the toggled key in either direction, so
                // a following Shift range runs from the row just picked.
                *self.anchor.borrow_mut() = Some(key);
                self.commit_base(set);
            }
        }
    }

    /// Extend the selection from the anchor to `target` over the current visible
    /// key order (Shift+click). `ordered_keys` is the projection's visible order
    /// at click time. If the anchor isn't currently visible (scrolled out /
    /// evicted), falls back to a single-key select.
    pub fn extend_to(&self, target: K, ordered_keys: &[K]) {
        self.extend_from_base(target, ordered_keys, false);
    }

    /// Extend from the anchor to `target`, keeping whatever was selected when
    /// this gesture began (Ctrl+Shift+navigation).
    ///
    /// The keyed twin of
    /// [`SelectionModel::extend_to_additive`](crate::SelectionModel::extend_to_additive):
    /// the range is unioned with the live selection captured on the gesture's
    /// first keystroke, so a second disjoint range can be built without losing
    /// the first, and the range still shrinks when reversed.
    pub fn extend_to_additive(&self, target: K, ordered_keys: &[K]) {
        self.extend_from_base(target, ordered_keys, true);
    }

    fn extend_from_base(&self, target: K, ordered_keys: &[K], additive: bool) {
        match self.mode {
            SelectionMode::None => {}
            SelectionMode::Single => self.select(target),
            SelectionMode::Multi => {
                let anchor = self.anchor.borrow().clone();
                let Some(anchor) = anchor else {
                    self.select(target);
                    return;
                };
                let a = ordered_keys.iter().position(|k| *k == anchor);
                let t = ordered_keys.iter().position(|k| *k == target);
                match (a, t) {
                    (Some(a), Some(t)) => {
                        if additive && !self.extending.get() {
                            *self.base.borrow_mut() = self.selection.get();
                        }
                        let (lo, hi) = (a.min(t), a.max(t));
                        // Recomputed from the base rather than unioned into the
                        // live selection, so shrinking the range gives back the
                        // rows it took.
                        let mut set = self.base.borrow().clone();
                        for k in &ordered_keys[lo..=hi] {
                            set.insert(k.clone());
                        }
                        self.selection.set(set);
                        self.extending.set(true);
                        // Anchor stays put.
                    }
                    _ => self.select(target),
                }
            }
        }
    }

    /// Replace the selection with `keys` (or, when `additive`, union them in).
    /// Used by rubber-band selection. In `Single` mode an arbitrary one wins.
    pub fn select_keys(&self, keys: impl IntoIterator<Item = K>, additive: bool) {
        if self.mode == SelectionMode::None {
            return;
        }
        let mut set = if additive {
            self.selection.get()
        } else {
            HashSet::new()
        };
        set.extend(keys);
        if self.mode == SelectionMode::Single && set.len() > 1 {
            let keep = set.iter().next().cloned();
            set = keep.into_iter().collect();
        }
        self.selection.set(set.clone());
        self.commit_base(if additive { set } else { HashSet::new() });
    }

    /// Clear the selection and anchor.
    pub fn clear(&self) {
        self.selection.set(HashSet::new());
        *self.anchor.borrow_mut() = None;
        self.commit_base(HashSet::new());
    }

    /// Drop any selected key (and the anchor) for which `exists` returns false.
    /// Call after a removal/reset to prune deleted rows — the index-based
    /// `adjust_for_insert`/`adjust_for_remove` are unnecessary here because keys
    /// are stable across inserts, moves, sorts and filters.
    pub fn prune_missing(&self, exists: impl Fn(&K) -> bool) {
        let old = self.selection.get();
        let new: HashSet<K> = old.iter().filter(|k| exists(k)).cloned().collect();
        if new.len() != old.len() {
            self.selection.set(new);
        }
        let drop_anchor = self.anchor.borrow().as_ref().is_some_and(|a| !exists(a));
        if drop_anchor {
            *self.anchor.borrow_mut() = None;
        }
        // The gesture base is selection state too, so a deleted row has to
        // leave it as well or the next Shift range resurrects the key.
        let mut base = self.base.borrow_mut();
        if !base.is_empty() {
            base.retain(|k| exists(k));
        }
    }
}

impl<K: ItemKey> Clone for KeyedSelectionModel<K> {
    fn clone(&self) -> Self {
        Self {
            mode: self.mode,
            selection: self.selection.clone(),
            anchor: self.anchor.clone(),
            base: self.base.clone(),
            extending: self.extending.clone(),
            #[cfg(debug_assertions)]
            debug_adapter_holder: self.debug_adapter_holder.clone(),
        }
    }
}

impl<K: ItemKey> KeyedSelectionModel<K> {
    /// Register this model with the debug inspector under `name`; no-op in
    /// release builds (`!cfg(debug_assertions)`). Returns `self` for chaining.
    pub fn debug_named(self, _name: impl Into<String>) -> Self {
        #[cfg(debug_assertions)]
        {
            let adapter: Rc<dyn crate::debug_registry::ModelDebug> =
                Rc::new(KeyedSelectionModelDebug {
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
struct KeyedSelectionModelDebug<K: ItemKey> {
    selection: Signal<HashSet<K>>,
    mode: SelectionMode,
}

#[cfg(debug_assertions)]
impl<K: ItemKey> crate::debug_registry::ModelDebug for KeyedSelectionModelDebug<K> {
    fn kind(&self) -> &'static str {
        "KeyedSelectionModel"
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
        for k in sel.iter() {
            let _ = writeln!(out, "{:?}", k);
        }
    }
}

impl<K: ItemKey> std::fmt::Debug for KeyedSelectionModel<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyedSelectionModel")
            .field("mode", &self.mode)
            .field("selected_count", &self.selection.get().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_select_by_key() {
        let m: KeyedSelectionModel<u64> = KeyedSelectionModel::new(SelectionMode::Single);
        m.select(10);
        assert!(m.is_selected(&10));
        m.select(20);
        assert!(!m.is_selected(&10));
        assert!(m.is_selected(&20));
    }

    #[test]
    fn multi_toggle_and_count() {
        let m: KeyedSelectionModel<u64> = KeyedSelectionModel::new(SelectionMode::Multi);
        m.toggle(1);
        m.toggle(3);
        assert_eq!(m.count(), 2);
        m.toggle(1);
        assert!(!m.is_selected(&1));
        assert!(m.is_selected(&3));
    }

    #[test]
    fn reversing_a_keyed_shift_gesture_shrinks_the_range() {
        let m: KeyedSelectionModel<u64> = KeyedSelectionModel::new(SelectionMode::Multi);
        let order = vec![10_u64, 20, 30, 40, 50];
        m.select(20);
        m.extend_to(50, &order);
        let mut got = m.selected_keys();
        got.sort();
        assert_eq!(got, vec![20, 30, 40, 50]);
        m.extend_to(30, &order);
        let mut got = m.selected_keys();
        got.sort();
        assert_eq!(got, vec![20, 30]);
    }

    #[test]
    fn a_keyed_toggle_survives_the_next_shift_range() {
        let m: KeyedSelectionModel<u64> = KeyedSelectionModel::new(SelectionMode::Multi);
        let order = vec![10_u64, 20, 30, 40, 50];
        m.select(10);
        m.toggle(30);
        m.extend_to(50, &order);
        let mut got = m.selected_keys();
        got.sort();
        assert_eq!(got, vec![10, 30, 40, 50]);
    }

    #[test]
    fn a_keyed_additive_extend_keeps_the_earlier_range() {
        let m: KeyedSelectionModel<u64> = KeyedSelectionModel::new(SelectionMode::Multi);
        let order = vec![10_u64, 20, 30, 40, 50, 60];
        m.select(10);
        m.extend_to(20, &order);
        m.toggle(40);
        m.extend_to_additive(60, &order);
        let mut got = m.selected_keys();
        got.sort();
        assert_eq!(got, vec![10, 20, 40, 50, 60]);
        m.extend_to_additive(50, &order);
        let mut got = m.selected_keys();
        got.sort();
        assert_eq!(got, vec![10, 20, 40, 50]);
    }

    #[test]
    fn pruning_a_deleted_key_also_drops_it_from_the_gesture_base() {
        let m: KeyedSelectionModel<u64> = KeyedSelectionModel::new(SelectionMode::Multi);
        let order = vec![10_u64, 20, 30, 40];
        m.select(10);
        m.toggle(20); // base = {10, 20}
        m.prune_missing(|k| *k != 10);
        m.extend_to(40, &order);
        let mut got = m.selected_keys();
        got.sort();
        // Key 10 is gone; the range from the anchor at 20 must not revive it.
        assert_eq!(got, vec![20, 30, 40]);
    }

    #[test]
    fn extend_to_over_visible_order() {
        let m: KeyedSelectionModel<u64> = KeyedSelectionModel::new(SelectionMode::Multi);
        let order = vec![10_u64, 20, 30, 40, 50];
        m.select(20); // anchor at key 20
        m.extend_to(40, &order);
        let mut got = m.selected_keys();
        got.sort();
        assert_eq!(got, vec![20, 30, 40]);
    }

    #[test]
    fn selection_survives_reorder_of_visible_order() {
        // The whole point: selection is by identity, so reordering the
        // projection does not change which keys are selected.
        let m: KeyedSelectionModel<u64> = KeyedSelectionModel::new(SelectionMode::Multi);
        m.toggle(30);
        m.toggle(10);
        // Visible order changes (e.g. a sort) — selection unaffected.
        assert!(m.is_selected(&10));
        assert!(m.is_selected(&30));
        assert!(!m.is_selected(&20));
    }

    #[test]
    fn prune_missing_drops_deleted_keys_and_anchor() {
        let m: KeyedSelectionModel<u64> = KeyedSelectionModel::new(SelectionMode::Multi);
        m.toggle(1);
        m.toggle(2);
        m.toggle(3); // selection {1,2,3}, anchor = 3
        // Keys 2 and 3 no longer exist.
        let live: HashSet<u64> = [1_u64, 4, 5].into_iter().collect();
        m.prune_missing(|k| live.contains(k));
        assert!(m.is_selected(&1));
        assert!(!m.is_selected(&2));
        assert!(!m.is_selected(&3));
        // Anchor (3) was pruned: extend_to now falls back to single-select.
        m.extend_to(5, &[1, 4, 5]);
        assert!(m.is_selected(&5));
    }

    #[test]
    fn anchor_not_visible_falls_back_to_single() {
        let m: KeyedSelectionModel<u64> = KeyedSelectionModel::new(SelectionMode::Multi);
        m.select(99); // anchor 99, not in the visible order below
        m.extend_to(20, &[10, 20, 30]);
        // Anchor wasn't visible → single-select target.
        assert_eq!(m.selected_keys(), vec![20]);
    }

    #[test]
    fn none_mode_ignores() {
        let m: KeyedSelectionModel<u64> = KeyedSelectionModel::new(SelectionMode::None);
        m.select(1);
        m.toggle(2);
        assert_eq!(m.count(), 0);
    }
}
