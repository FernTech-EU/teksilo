// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `KeyedTreeCheckedModel<K>` — per-node checkbox state for a tree **keyed by a
//! stable domain id**, with optional descendant→ancestor tristate aggregation.
//!
//! The keyed counterpart of [`TreeCheckedModel`](crate::TreeCheckedModel) — the
//! checkbox twin of [`KeyedSelectionModel`](crate::KeyedSelectionModel). Where
//! `TreeCheckedModel` is bound to a `TreeModel<T>` and keyed by `NodeId`, this
//! model is keyed by *your* domain key `K` (an entity id, a tagged enum) and
//! takes the tree *shape* as two injected closures (`children` + `parent`), so
//! it composes over a [`TreeDataSlice`](crate::TreeDataSlice) or any
//! [`TreeDataSource`] — the "select scenes to export"
//! tristate over an external outline, without mirroring into a `TreeModel`.
//!
//! Because identity is the domain key (stable across a full re-source), a
//! node's check state survives the tree reloading — a checked scene stays
//! checked after the backend refreshes. Use [`prune_missing`](KeyedTreeCheckedModel::prune_missing)
//! after a reload to drop the state of nodes that no longer exist.
//!
//! Semantics, cascade behaviour, the `Signal<CheckState>` / `Signal<bool>`
//! bridge, and the re-entry guard are identical to `TreeCheckedModel` — see its
//! [module docs](crate::tree_checked_model) for the detail. This model is a
//! share-by-clone handle (`Rc<RefCell<…>>` internally).
//!
//! ## Example
//!
//! ```
//! use bastyde_data::{KeyedTreeCheckedModel, CheckState, TreeDataSlice, TreeRow};
//!
//! // An outline: Binder(1) → { Chapter(2) → Scene(3), Scene(4) }
//! let slice: TreeDataSlice<u64, &str> = TreeDataSlice::from_rows(vec![
//!     TreeRow::new(1, "Binder", 0),
//!     TreeRow::new(2, "Chapter", 1),
//!     TreeRow::new(3, "Scene A", 2),
//!     TreeRow::new(4, "Scene B", 1),
//! ]);
//!
//! let checked = KeyedTreeCheckedModel::from_source(slice.clone());
//! let _ = (checked.signal_for(1), checked.signal_for(2), checked.signal_for(3), checked.signal_for(4));
//!
//! checked.check(3);                                         // one scene under the chapter
//! assert_eq!(checked.check_state(&2), CheckState::Checked); // chapter has only Scene A → Checked
//! assert_eq!(checked.check_state(&1), CheckState::Indeterminate); // Binder: 2 of {chapter, Scene B}
//! ```

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use bastyde_core::signal::{ObserverHandle, Signal};

use crate::check_state::CheckState;
use crate::dnd_types::ItemKey;
use crate::tree_checked_model::AggregateMode;
use crate::tree_data_source::TreeDataSource;

/// A tree-shape query: `key -> children keys` / `key -> parent key`.
type ChildrenFn<K> = Rc<dyn Fn(&K) -> Vec<K>>;
type ParentFn<K> = Rc<dyn Fn(&K) -> Option<K>>;

struct Inner<K: ItemKey> {
    state: HashMap<K, Signal<CheckState>>,
    observers: HashMap<K, ObserverHandle>,
    bool_signals: HashMap<K, Signal<bool>>,
    bridge_guards: HashMap<K, Rc<Cell<bool>>>,
    bridge_observers: HashMap<K, (ObserverHandle, ObserverHandle)>,
    /// True while performing an internal cascade — suppresses cascade observers.
    suppress: bool,
}

/// Per-node checkbox state for a domain-keyed tree, with optional
/// descendant→ancestor tristate aggregation. See the [module docs](self).
pub struct KeyedTreeCheckedModel<K: ItemKey> {
    children: ChildrenFn<K>,
    parent: ParentFn<K>,
    inner: Rc<RefCell<Inner<K>>>,
    mode: Rc<Cell<AggregateMode>>,
}

impl<K: ItemKey> KeyedTreeCheckedModel<K> {
    /// Create a model over a tree whose shape is given by two closures:
    /// `children(key) -> Vec<K>` and `parent(key) -> Option<K>`. Uses the
    /// default [`AggregateMode::DescendantsDriveAncestors`].
    pub fn new(
        children: impl Fn(&K) -> Vec<K> + 'static,
        parent: impl Fn(&K) -> Option<K> + 'static,
    ) -> Self {
        Self {
            children: Rc::new(children),
            parent: Rc::new(parent),
            inner: Rc::new(RefCell::new(Inner {
                state: HashMap::new(),
                observers: HashMap::new(),
                bool_signals: HashMap::new(),
                bridge_guards: HashMap::new(),
                bridge_observers: HashMap::new(),
                suppress: false,
            })),
            mode: Rc::new(Cell::new(AggregateMode::default())),
        }
    }

    /// Create a model whose tree shape is read from a cloneable
    /// [`TreeDataSource`] (e.g. a [`TreeDataSlice`](crate::TreeDataSlice)). The
    /// source is cloned into the shape closures, so the model reflects the live
    /// tree — call [`prune_missing`](Self::prune_missing) after the source
    /// reloads to drop state for removed nodes.
    pub fn from_source<S>(source: S) -> Self
    where
        S: TreeDataSource<Key = K> + Clone + 'static,
    {
        let for_children = source.clone();
        let for_parent = source;
        Self::new(
            move |k| for_children.child_keys(k),
            move |k| for_parent.parent(k),
        )
    }

    /// Set the [`AggregateMode`] at construction.
    pub fn with_mode(self, mode: AggregateMode) -> Self {
        self.mode.set(mode);
        self
    }

    /// The current [`AggregateMode`].
    pub fn aggregate_mode(&self) -> AggregateMode {
        self.mode.get()
    }

    /// Change the cascade behaviour; takes effect on the next write.
    pub fn set_aggregate_mode(&self, mode: AggregateMode) {
        self.mode.set(mode);
    }

    /// Writable `Signal<CheckState>` for `key` (cached). External writes trigger
    /// the configured aggregation pass. The cascade observer is wired
    /// **idempotently** — including for a signal first materialised by a cascade
    /// (`write_state`) before its own `signal_for` was ever called — so binding a
    /// lazily-realised (e.g. virtualized) row still cascades on write.
    pub fn signal_for(&self, key: K) -> Signal<CheckState> {
        // Get or create the signal (a cascade may have created it observer-less).
        let sig = self
            .inner
            .borrow_mut()
            .state
            .entry(key.clone())
            .or_insert_with(|| Signal::new(CheckState::Unchecked))
            .clone();
        // Wire the cascade observer once, if this key doesn't have one yet.
        if !self.inner.borrow().observers.contains_key(&key) {
            let handle = self.make_cascade_observer(&sig, key.clone());
            self.inner.borrow_mut().observers.insert(key, handle);
        }
        sig
    }

    /// Build the cascade observer for `node`'s signal: on any write, cascade
    /// Checked/Unchecked to descendants and recompute ancestors, guarded against
    /// re-entry.
    fn make_cascade_observer(&self, sig: &Signal<CheckState>, node: K) -> ObserverHandle {
        let inner_w = Rc::downgrade(&self.inner);
        let mode_w = Rc::downgrade(&self.mode);
        let children = self.children.clone();
        let parent = self.parent.clone();
        sig.observe(move |new_state| {
            let Some(inner_rc) = inner_w.upgrade() else {
                return;
            };
            let Some(mode_rc) = mode_w.upgrade() else {
                return;
            };
            if inner_rc.borrow().suppress {
                return;
            }
            if mode_rc.get() != AggregateMode::DescendantsDriveAncestors {
                return;
            }
            // RAII: suppress re-entrant observers for the whole cascade and clear
            // the flag on every exit path (even a panic in a shape closure).
            let _guard = SuppressGuard::new(&inner_rc);
            // Cascade Checked / Unchecked down; Indeterminate is parent-only.
            if *new_state != CheckState::Indeterminate {
                cascade_descendants(&children, &inner_rc, &node, *new_state);
            }
            // Recompute ancestors.
            let mut cur = parent(&node);
            while let Some(p) = cur {
                recompute_from_children(&children, &inner_rc, &p);
                cur = parent(&p);
            }
        })
    }

    /// Two-state `Signal<bool>` projection of [`signal_for`](Self::signal_for)
    /// (cached, writable). `Checked → true`; anything else → `false`. See
    /// [`crate::TreeCheckedModel::bool_signal_for`].
    pub fn bool_signal_for(&self, key: K) -> Signal<bool> {
        if let Some(b) = self.inner.borrow().bool_signals.get(&key) {
            return b.clone();
        }
        let tristate = self.signal_for(key.clone());
        let bool_sig = Signal::new(tristate.get() == CheckState::Checked);
        let guard = Rc::new(Cell::new(false));

        // tristate → bool
        let bool_for_tri = bool_sig.clone();
        let guard_for_tri = guard.clone();
        let tri_to_bool = tristate.observe(move |state| {
            if guard_for_tri.get() {
                return;
            }
            let want = matches!(state, CheckState::Checked);
            if bool_for_tri.get() != want {
                guard_for_tri.set(true);
                bool_for_tri.set(want);
                guard_for_tri.set(false);
            }
        });

        // bool → tristate (the tristate cascade observer takes it from there)
        let tri_for_bool = tristate.clone();
        let guard_for_bool = guard.clone();
        let bool_to_tri = bool_sig.observe(move |checked| {
            if guard_for_bool.get() {
                return;
            }
            let want = if *checked {
                CheckState::Checked
            } else {
                CheckState::Unchecked
            };
            if tri_for_bool.get() != want {
                guard_for_bool.set(true);
                tri_for_bool.set(want);
                guard_for_bool.set(false);
            }
        });

        let mut inner = self.inner.borrow_mut();
        inner.bool_signals.insert(key.clone(), bool_sig.clone());
        inner.bridge_guards.insert(key.clone(), guard);
        inner
            .bridge_observers
            .insert(key, (tri_to_bool, bool_to_tri));
        bool_sig
    }

    /// The current [`CheckState`] for `key` (`Unchecked` if never touched).
    pub fn check_state(&self, key: &K) -> CheckState {
        self.inner
            .borrow()
            .state
            .get(key)
            .map(|s| s.get())
            .unwrap_or(CheckState::Unchecked)
    }

    /// Set `key` to [`CheckState::Checked`] (triggers cascade + ancestor recompute).
    pub fn check(&self, key: K) {
        self.signal_for(key).set(CheckState::Checked);
    }

    /// Set `key` to [`CheckState::Unchecked`] (triggers cascade + ancestor recompute).
    pub fn uncheck(&self, key: K) {
        self.signal_for(key).set(CheckState::Unchecked);
    }

    /// Toggle `key`: a leaf under `DescendantsDriveAncestors` cycles two-state;
    /// a branch or `AggregateMode::None` cycles the full tristate sequence.
    pub fn toggle(&self, key: K) {
        let current = self.check_state(&key);
        let next = match (self.mode.get(), self.is_leaf(&key), current) {
            (AggregateMode::DescendantsDriveAncestors, true, CheckState::Unchecked) => {
                CheckState::Checked
            }
            (AggregateMode::DescendantsDriveAncestors, true, _) => CheckState::Unchecked,
            (_, _, _) => current.next_tristate(),
        };
        self.signal_for(key).set(next);
    }

    /// All keys whose current state is exactly [`CheckState::Checked`]. May
    /// include stale keys after a tree mutation — call [`prune_missing`](Self::prune_missing)
    /// or filter against the current tree yourself.
    pub fn checked_keys(&self) -> Vec<K> {
        self.inner
            .borrow()
            .state
            .iter()
            .filter(|(_, sig)| sig.get() == CheckState::Checked)
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Reset all known nodes to [`CheckState::Unchecked`].
    pub fn clear(&self) {
        let keys: Vec<K> = self.inner.borrow().state.keys().cloned().collect();
        for k in keys {
            let sig = self.signal_for(k);
            if sig.get() != CheckState::Unchecked {
                sig.set(CheckState::Unchecked);
            }
        }
    }

    /// Drop cached check state (and its signals/observers) for every key for
    /// which `exists(&key)` returns `false`, then [`reaggregate`](Self::reaggregate)
    /// surviving parents against the current tree. Call after a reload so a
    /// deleted node's state doesn't linger in `checked_keys()` **and** the
    /// ancestors it used to affect show the correct tristate. Mirrors
    /// [`crate::KeyedSelectionModel::prune_missing`].
    pub fn prune_missing(&self, exists: impl Fn(&K) -> bool) {
        // Snapshot keys first, then run the caller's `exists` with no borrow held
        // (it may query the model / source without a double-borrow panic).
        let all: Vec<K> = self.inner.borrow().state.keys().cloned().collect();
        let stale: Vec<K> = all.into_iter().filter(|k| !exists(k)).collect();
        if stale.is_empty() {
            return;
        }
        {
            let mut inner = self.inner.borrow_mut();
            for k in &stale {
                inner.state.remove(k);
                inner.observers.remove(k);
                inner.bool_signals.remove(k);
                inner.bridge_guards.remove(k);
                inner.bridge_observers.remove(k);
            }
        }
        self.reaggregate();
    }

    /// Recompute every surviving parent's aggregate from the **current** tree
    /// shape + leaf states, deepest first. Call after the backing tree's
    /// structure changed (a reload that added/removed/moved nodes) so parent
    /// tristates reflect the new children; [`prune_missing`](Self::prune_missing)
    /// does this for you. A no-op under [`AggregateMode::None`].
    pub fn reaggregate(&self) {
        if self.mode.get() != AggregateMode::DescendantsDriveAncestors {
            return;
        }
        let mut keys: Vec<K> = self.inner.borrow().state.keys().cloned().collect();
        // Deepest first, so a parent recomputes after its children are finalised.
        keys.sort_by_key(|k| std::cmp::Reverse(self.depth_of(k)));
        let _guard = SuppressGuard::new(&self.inner);
        for k in keys {
            recompute_from_children(&self.children, &self.inner, &k);
        }
    }

    /// Depth of `key` in the current tree (root = 0), via the parent closure.
    fn depth_of(&self, key: &K) -> usize {
        let mut depth = 0usize;
        let mut cur = (self.parent)(key);
        // Bound the walk against a malformed (cyclic) parent closure.
        while let Some(p) = cur {
            depth += 1;
            if depth > 1_000_000 {
                break;
            }
            cur = (self.parent)(&p);
        }
        depth
    }

    fn is_leaf(&self, key: &K) -> bool {
        (self.children)(key).is_empty()
    }
}

/// RAII guard: sets `suppress = true` on creation, clears it on drop — so a
/// panic in a shape closure mid-cascade can't leave the model permanently
/// unable to cascade.
struct SuppressGuard<K: ItemKey> {
    inner: Rc<RefCell<Inner<K>>>,
}

impl<K: ItemKey> SuppressGuard<K> {
    fn new(inner: &Rc<RefCell<Inner<K>>>) -> Self {
        inner.borrow_mut().suppress = true;
        Self {
            inner: inner.clone(),
        }
    }
}

impl<K: ItemKey> Drop for SuppressGuard<K> {
    fn drop(&mut self) {
        // A borrow may still be held during a panic unwind; best-effort clear.
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.suppress = false;
        }
    }
}

// Free functions — the cascade observer holds only closures + a `Weak<Inner>`.

fn cascade_descendants<K: ItemKey>(
    children: &ChildrenFn<K>,
    inner: &Rc<RefCell<Inner<K>>>,
    root: &K,
    target: CheckState,
) {
    for child in children(root) {
        write_state(inner, &child, target);
        cascade_descendants(children, inner, &child, target);
    }
}

fn recompute_from_children<K: ItemKey>(
    children: &ChildrenFn<K>,
    inner: &Rc<RefCell<Inner<K>>>,
    node: &K,
) {
    let kids = children(node);
    if kids.is_empty() {
        return;
    }
    let mut all_checked = true;
    let mut all_unchecked = true;
    for child in &kids {
        match read_state(inner, child) {
            CheckState::Checked => all_unchecked = false,
            CheckState::Unchecked => all_checked = false,
            CheckState::Indeterminate => {
                all_checked = false;
                all_unchecked = false;
            }
        }
    }
    let new_state = if all_checked {
        CheckState::Checked
    } else if all_unchecked {
        CheckState::Unchecked
    } else {
        CheckState::Indeterminate
    };
    write_state(inner, node, new_state);
}

fn read_state<K: ItemKey>(inner: &Rc<RefCell<Inner<K>>>, node: &K) -> CheckState {
    inner
        .borrow()
        .state
        .get(node)
        .map(|s| s.get())
        .unwrap_or(CheckState::Unchecked)
}

fn write_state<K: ItemKey>(inner: &Rc<RefCell<Inner<K>>>, node: &K, state: CheckState) {
    let sig = {
        let mut map = inner.borrow_mut();
        map.state
            .entry(node.clone())
            .or_insert_with(|| Signal::new(CheckState::Unchecked))
            .clone()
    };
    if sig.get() != state {
        sig.set(state);
    }
}

impl<K: ItemKey> Clone for KeyedTreeCheckedModel<K> {
    fn clone(&self) -> Self {
        Self {
            children: self.children.clone(),
            parent: self.parent.clone(),
            inner: self.inner.clone(),
            mode: self.mode.clone(),
        }
    }
}

impl<K: ItemKey> std::fmt::Debug for KeyedTreeCheckedModel<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyedTreeCheckedModel")
            .field("mode", &self.mode.get())
            .field("tracked_nodes", &self.inner.borrow().state.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TreeDataSlice, TreeRow};

    // Binder(1) → { Chapter(2) → { Scene(3), Scene(5) }, Scene(4) }
    fn slice() -> TreeDataSlice<u64, &'static str> {
        TreeDataSlice::from_rows(vec![
            TreeRow::new(1, "Binder", 0),
            TreeRow::new(2, "Chapter", 1),
            TreeRow::new(3, "Scene A", 2),
            TreeRow::new(5, "Scene C", 2),
            TreeRow::new(4, "Scene B", 1),
        ])
    }

    fn model() -> KeyedTreeCheckedModel<u64> {
        let m = KeyedTreeCheckedModel::from_source(slice());
        // Pre-register the observer chain before mutating.
        let _ = (
            m.signal_for(1),
            m.signal_for(2),
            m.signal_for(3),
            m.signal_for(4),
            m.signal_for(5),
        );
        m
    }

    #[test]
    fn descendants_drive_ancestors() {
        let m = model();
        m.check(3);
        assert_eq!(m.check_state(&3), CheckState::Checked);
        // Chapter(2) has children {3, 5}; only 3 checked → Indeterminate.
        assert_eq!(m.check_state(&2), CheckState::Indeterminate);
        // Binder(1) has children {2, 4}; 2 indeterminate → Indeterminate.
        assert_eq!(m.check_state(&1), CheckState::Indeterminate);

        m.check(5);
        // Chapter now fully checked.
        assert_eq!(m.check_state(&2), CheckState::Checked);
        assert_eq!(m.check_state(&1), CheckState::Indeterminate); // Scene B still unchecked
        m.check(4);
        assert_eq!(m.check_state(&1), CheckState::Checked);
    }

    #[test]
    fn parent_cascades_to_descendants() {
        let m = model();
        m.check(2); // Chapter → both scenes
        assert_eq!(m.check_state(&3), CheckState::Checked);
        assert_eq!(m.check_state(&5), CheckState::Checked);
        m.uncheck(2);
        assert_eq!(m.check_state(&3), CheckState::Unchecked);
        assert_eq!(m.check_state(&5), CheckState::Unchecked);
    }

    #[test]
    fn check_root_cascades_whole_tree() {
        let m = model();
        m.check(1);
        for k in [2u64, 3, 4, 5] {
            assert_eq!(m.check_state(&k), CheckState::Checked);
        }
    }

    #[test]
    fn aggregate_mode_none_is_independent() {
        let m = KeyedTreeCheckedModel::from_source(slice()).with_mode(AggregateMode::None);
        let _ = (m.signal_for(1), m.signal_for(3));
        m.check(3);
        assert_eq!(m.check_state(&3), CheckState::Checked);
        assert_eq!(m.check_state(&2), CheckState::Unchecked);
        assert_eq!(m.check_state(&1), CheckState::Unchecked);
    }

    #[test]
    fn external_signal_write_cascades() {
        let m = model();
        m.signal_for(2).set(CheckState::Checked); // as if a Checkbox wrote it
        assert_eq!(m.check_state(&3), CheckState::Checked);
        assert_eq!(m.check_state(&5), CheckState::Checked);
    }

    #[test]
    fn bool_signal_bridge() {
        let m = model();
        let b = m.bool_signal_for(3);
        assert!(!b.get());
        b.set(true);
        assert_eq!(m.check_state(&3), CheckState::Checked);
        m.uncheck(3);
        assert!(!b.get());
    }

    #[test]
    fn bool_signal_indeterminate_reads_false() {
        let m = model();
        let binder_bool = m.bool_signal_for(1);
        m.check(3); // Binder → Indeterminate
        assert_eq!(m.check_state(&1), CheckState::Indeterminate);
        assert!(!binder_bool.get());
    }

    #[test]
    fn checked_keys_excludes_indeterminate() {
        let m = model();
        m.check(3);
        let keys = m.checked_keys();
        assert!(keys.contains(&3));
        assert!(!keys.contains(&2)); // Indeterminate
        assert!(!keys.contains(&1));
    }

    #[test]
    fn signal_is_stable_across_calls() {
        let m = model();
        let s1 = m.signal_for(3);
        let s2 = m.signal_for(3);
        m.check(3);
        assert_eq!(s1.get(), CheckState::Checked);
        assert_eq!(s2.get(), CheckState::Checked);
    }

    #[test]
    fn prune_missing_drops_stale_state() {
        let m = model();
        m.check(3);
        assert!(m.checked_keys().contains(&3));
        // Simulate a reload where scene 3 was deleted: only {1,2,4,5} survive.
        m.prune_missing(|k| *k != 3);
        assert!(!m.checked_keys().contains(&3));
        assert_eq!(m.check_state(&3), CheckState::Unchecked); // forgotten
    }

    #[test]
    fn clear_resets_all() {
        let m = model();
        m.check(1);
        m.clear();
        assert_eq!(m.checked_keys(), Vec::<u64>::new());
    }

    #[test]
    fn lazy_signal_still_cascades() {
        // Regression: a node's signal first materialised by a cascade (write_state)
        // must still cascade when its own signal_for is called later (virtualized
        // row realizing after a parent was checked).
        let m = KeyedTreeCheckedModel::from_source(slice());
        let _ = m.signal_for(1); // only the root is realized
        m.check(1); // cascades Checked to 2,3,4,5 via observer-less signals
        assert_eq!(m.check_state(&3), CheckState::Checked);

        let scene3 = m.signal_for(3); // scene 3's row finally realizes + binds
        scene3.set(CheckState::Unchecked); // user unchecks it
        // Chapter(2) must recompute (5 still Checked, 3 now Unchecked → mixed).
        assert_eq!(m.check_state(&2), CheckState::Indeterminate);
        assert_eq!(m.check_state(&1), CheckState::Indeterminate);
    }

    #[test]
    fn prune_missing_reaggregates_ancestors() {
        // Regression: after a reload that removes a checked node, surviving
        // ancestors must show the recomputed tristate, not the stale one.
        let s = slice(); // Binder(1)→Chapter(2)→{A(3),C(5)}, B(4) under Binder
        let m = KeyedTreeCheckedModel::from_source(s.clone());
        let _ = (
            m.signal_for(1),
            m.signal_for(2),
            m.signal_for(3),
            m.signal_for(5),
        );
        m.check(3);
        assert_eq!(m.check_state(&2), CheckState::Indeterminate);
        assert_eq!(m.check_state(&1), CheckState::Indeterminate);

        // Reload with Scene A(3) removed → Chapter(2) now only has C(5), unchecked.
        s.set_rows(vec![
            TreeRow::new(1, "Binder", 0),
            TreeRow::new(2, "Chapter", 1),
            TreeRow::new(5, "Scene C", 2),
            TreeRow::new(4, "Scene B", 1),
        ]);
        assert_eq!(s.child_keys_of(&2), vec![5]);

        m.prune_missing(|k| s.contains_key(k));
        assert!(!m.checked_keys().contains(&3));
        assert_eq!(m.check_state(&2), CheckState::Unchecked);
        assert_eq!(m.check_state(&1), CheckState::Unchecked);
    }

    #[test]
    fn reaggregate_after_added_child() {
        let s = slice();
        let m = KeyedTreeCheckedModel::from_source(s.clone());
        let _ = (m.signal_for(2), m.signal_for(3), m.signal_for(5));
        m.check(2); // Chapter fully checked (A + C)
        assert_eq!(m.check_state(&2), CheckState::Checked);

        // Reload: add a new unchecked scene D(6) under Chapter(2).
        s.set_rows(vec![
            TreeRow::new(1, "Binder", 0),
            TreeRow::new(2, "Chapter", 1),
            TreeRow::new(3, "Scene A", 2),
            TreeRow::new(5, "Scene C", 2),
            TreeRow::new(6, "Scene D", 2),
            TreeRow::new(4, "Scene B", 1),
        ]);
        m.reaggregate();
        // Chapter gained an unchecked child → Indeterminate.
        assert_eq!(m.check_state(&2), CheckState::Indeterminate);
    }

    #[test]
    fn new_with_explicit_closures() {
        // No TreeDataSource — pure closures. A -> [B, C].
        let parents: HashMap<&str, &str> = [("B", "A"), ("C", "A")].into_iter().collect();
        let m = KeyedTreeCheckedModel::<&str>::new(
            |k| match *k {
                "A" => vec!["B", "C"],
                _ => vec![],
            },
            move |k| parents.get(k).copied(),
        );
        let _ = (m.signal_for("A"), m.signal_for("B"), m.signal_for("C"));
        m.check("B");
        assert_eq!(m.check_state(&"A"), CheckState::Indeterminate);
        m.check("C");
        assert_eq!(m.check_state(&"A"), CheckState::Checked);
    }
}
