// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Identity-based selection for collection widgets.
//!
//! [`KeyedSelectionModel<K>`] stores selection as a set of source-defined
//! **keys** rather than visible **indices**. This is what
//! [`SelectionModel`](crate::SelectionModel) cannot do: survive lazy
//! window-slides and external reorders, and stay consistent across two views of
//! the same source that scroll/sort/filter independently (selection is a set of
//! identities, not positions). It coexists with the index-based
//! `SelectionModel` — views opt into one or the other.
//!
//! Shift+click range extension is index-ordered by nature, so `extend_to` takes
//! the current visible key order from the caller (the projection) at click
//! time; the anchor is stored as a *key* so it survives scrolling out of the
//! resident window.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use bastyde_core::signal::Signal;

use crate::dnd_types::ItemKey;
use crate::selection_model::SelectionMode;

/// Manages selection state keyed by source identity.
///
/// The selection is exposed as a `Signal<HashSet<K>>` so widgets observe it
/// reactively. The anchor for Shift+click is a `K` (shared across clones).
pub struct KeyedSelectionModel<K: ItemKey> {
    mode: SelectionMode,
    selection: Signal<HashSet<K>>,
    anchor: Rc<RefCell<Option<K>>>,
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
            #[cfg(debug_assertions)]
            debug_adapter_holder: Rc::new(RefCell::new(None)),
        }
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
                self.selection.set(set);
                *self.anchor.borrow_mut() = Some(key);
            }
        }
    }

    /// Extend the selection from the anchor to `target` over the current visible
    /// key order (Shift+click). `ordered_keys` is the projection's visible order
    /// at click time. If the anchor isn't currently visible (scrolled out /
    /// evicted), falls back to a single-key select.
    pub fn extend_to(&self, target: K, ordered_keys: &[K]) {
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
                        let (lo, hi) = (a.min(t), a.max(t));
                        let mut set = self.selection.get();
                        for k in &ordered_keys[lo..=hi] {
                            set.insert(k.clone());
                        }
                        self.selection.set(set);
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
        self.selection.set(set);
    }

    /// Clear the selection and anchor.
    pub fn clear(&self) {
        self.selection.set(HashSet::new());
        *self.anchor.borrow_mut() = None;
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
        let drop_anchor = self
            .anchor
            .borrow()
            .as_ref()
            .is_some_and(|a| !exists(a));
        if drop_anchor {
            *self.anchor.borrow_mut() = None;
        }
    }
}

impl<K: ItemKey> Clone for KeyedSelectionModel<K> {
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

impl<K: ItemKey> KeyedSelectionModel<K> {
    /// Register this model with the debug inspector under `name`. No-op in
    /// release builds.
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
