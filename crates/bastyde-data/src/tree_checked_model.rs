// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TreeCheckedModel` — per-node checkbox state for a tree, with optional
//! descendant→ancestor tristate aggregation.
//!
//! Companion to [`crate::CheckedModel`] for trees. Defaults to the standard
//! "Outlook folder selection" semantic: a parent's state is `Checked` if all
//! descendants are checked, `Unchecked` if none, `Indeterminate` otherwise;
//! toggling a parent cascades `Checked`/`Unchecked` down to all descendants.
//! Set the mode to [`AggregateMode::None`] to give every node independent
//! state instead. The model is a share-by-clone handle (`Rc<RefCell<…>>`
//! internally) — cloning produces a second view onto the same checkbox state.
//!
//! External writes (e.g. a `Checkbox` widget bound to
//! `signal_for(node)` setting it directly) trigger the same
//! cascade-and-recompute pass as the model's own
//! `check`/`uncheck`/`toggle` methods, via per-node observers. A
//! re-entry guard prevents the cascade pass from re-firing
//! observers it triggers itself.
//!
//! ## Example
//!
//! ```rust
//! # use bastyde_data::{TreeModel, TreeCheckedModel, CheckState};
//! let tree = TreeModel::new();
//! let root = tree.insert_root(0, "root");
//! let child_a = tree.insert_child(root, 0, "a");
//! let child_b = tree.insert_child(root, 1, "b");
//!
//! let model = TreeCheckedModel::new(tree);
//! // Pre-register signal chains before mutating.
//! let _ = (model.signal_for(root), model.signal_for(child_a), model.signal_for(child_b));
//!
//! model.check(child_a);
//! assert_eq!(model.check_state(root), CheckState::Indeterminate);
//! model.check(child_b);
//! assert_eq!(model.check_state(root), CheckState::Checked);
//! ```
//!
//! ## Limitation: tree-mutation desync
//!
//! `signal_for(node)` and `bool_signal_for(node)` cache signals keyed
//! by `NodeId`. The cache is never invalidated. If the underlying
//! `TreeModel<T>` mutates (`remove`, `move_node`, etc.) the cached
//! entry for a removed `NodeId` lingers indefinitely:
//!
//! - `checked_nodes()` may include a stale `NodeId` whose underlying
//!   tree node no longer exists. Callers that consume this list
//!   should validate each id against the current tree state before
//!   acting on it.
//! - `bool_signal_for` / `signal_for` for a removed node still return
//!   their cached signal handle. Setting it has no observable effect
//!   on the tree (the cascade walks `tree.children(node)` which is
//!   empty for a freed node).
//!
//! This is an acceptable trade-off because `NodeId`s are not reused
//! by `TreeModel` (slotmap keys are versioned), so a stale id can
//! never alias a fresh node. If a future use case needs strict
//! invalidation on removal, subscribe to `TreeModel`'s change events
//! and clear the relevant entries. Tracked as out-of-scope for V1.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use bastyde_core::signal::{ObserverHandle, Signal};

use crate::check_state::CheckState;
use crate::tree_change::NodeId;
use crate::tree_model::TreeModel;

/// How a parent's [`CheckState`] relates to its descendants.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AggregateMode {
    /// Each node owns its state independently; parent states do not reflect
    /// their descendants and cascades do not occur.
    None,
    /// All-checked → `Checked`; all-unchecked → `Unchecked`; mixed →
    /// `Indeterminate`. Toggling a parent cascades `Checked`/`Unchecked` to
    /// all descendants and recomputes every ancestor. This is the default and
    /// corresponds to the "Outlook folder selection" tristate pattern.
    #[default]
    DescendantsDriveAncestors,
}

struct Inner {
    state: HashMap<NodeId, Signal<CheckState>>,
    /// Keep cascade-observer handles alive for the model's lifetime.
    observers: HashMap<NodeId, ObserverHandle>,
    /// Cached `Signal<bool>` views for nodes whose callers asked for
    /// the two-state projection (`bool_signal_for`).
    bool_signals: HashMap<NodeId, Signal<bool>>,
    /// Per-node bidirectional bridge guards (tristate ↔ bool). Each
    /// is a small `Cell<bool>` flipped while one side propagates to
    /// the other so the back-channel observer no-ops.
    bridge_guards: HashMap<NodeId, Rc<Cell<bool>>>,
    /// Bridge observer handles (tristate→bool and bool→tristate),
    /// kept alive for the model's lifetime.
    bridge_observers: HashMap<NodeId, (ObserverHandle, ObserverHandle)>,
    /// True while the model is performing an internal cascade —
    /// suppresses cascade observers to prevent re-entry.
    suppress: bool,
}

/// Per-node checkbox state for a [`TreeModel<T>`](crate::TreeModel), with optional
/// descendant→ancestor tristate aggregation.
///
/// See the [module documentation](self) for the full semantics and limitations.
/// Clone to share the same checkbox state between multiple call sites.
pub struct TreeCheckedModel<T: 'static> {
    tree: TreeModel<T>,
    inner: Rc<RefCell<Inner>>,
    mode: Rc<Cell<AggregateMode>>,
}

impl<T: 'static> TreeCheckedModel<T> {
    /// Create a new model wrapping `tree` with the default
    /// [`AggregateMode::DescendantsDriveAncestors`] cascade behaviour.
    pub fn new(tree: TreeModel<T>) -> Self {
        Self {
            tree,
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

    /// Create a new model wrapping `tree` with an explicit [`AggregateMode`].
    pub fn with_mode(tree: TreeModel<T>, mode: AggregateMode) -> Self {
        let m = Self::new(tree);
        m.mode.set(mode);
        m
    }

    /// Returns the current [`AggregateMode`] controlling cascade behaviour.
    pub fn aggregate_mode(&self) -> AggregateMode {
        self.mode.get()
    }

    /// Change the cascade behaviour; takes effect on the next write to any node's signal.
    pub fn set_aggregate_mode(&self, mode: AggregateMode) {
        self.mode.set(mode);
    }

    /// Writable `Signal<CheckState>` for `node`. Cached: repeat calls
    /// return the same root. External writes (e.g. from a `Checkbox`)
    /// trigger the configured aggregation pass. The cascade observer is
    /// wired **idempotently** — including for a signal first materialised by
    /// a cascade (`write_state`) before its own `signal_for` was ever called
    /// (a lazily/virtualized-realised row) — so a later external write to it
    /// still cascades.
    pub fn signal_for(&self, node: NodeId) -> Signal<CheckState> {
        // Get or create the signal (a cascade may have created it observer-less).
        let sig = self
            .inner
            .borrow_mut()
            .state
            .entry(node)
            .or_insert_with(|| Signal::new(CheckState::Unchecked))
            .clone();
        // Wire the cascade observer once, if this node doesn't have one yet.
        if !self.inner.borrow().observers.contains_key(&node) {
            let handle = self.make_cascade_observer(&sig, node);
            self.inner.borrow_mut().observers.insert(node, handle);
        }
        sig
    }

    /// Build the cascade observer for `node`'s signal: on any write, cascade
    /// Checked/Unchecked to descendants and recompute ancestors, guarded
    /// against re-entry. It's a no-op while the model is performing its own
    /// cascade pass (suppress = true).
    fn make_cascade_observer(&self, sig: &Signal<CheckState>, node: NodeId) -> ObserverHandle {
        let inner_w = Rc::downgrade(&self.inner);
        let mode_w = Rc::downgrade(&self.mode);
        let tree = self.tree.clone();
        sig.observe(move |new_state| {
            let inner_rc = match inner_w.upgrade() {
                Some(rc) => rc,
                None => return,
            };
            let mode_rc = match mode_w.upgrade() {
                Some(rc) => rc,
                None => return,
            };
            // Re-entry guard.
            if inner_rc.borrow().suppress {
                return;
            }
            if mode_rc.get() != AggregateMode::DescendantsDriveAncestors {
                return;
            }
            // RAII: suppress re-entrant observers for the whole cascade and
            // clear the flag on every exit path (even a panic mid-cascade).
            let _guard = SuppressGuard::new(&inner_rc);
            // Cascade Checked / Unchecked to all descendants;
            // Indeterminate is a parent-only state and doesn't propagate.
            if *new_state != CheckState::Indeterminate {
                cascade_descendants(&tree, &inner_rc, node, *new_state);
            }
            // Recompute ancestors.
            let mut cur = tree.parent(node);
            while let Some(p) = cur {
                recompute_from_children(&tree, &inner_rc, p);
                cur = tree.parent(p);
            }
        })
    }

    /// Two-state projection of `signal_for` for callers that want
    /// to bind a leaf's check state to a `Signal<bool>`-shaped widget
    /// (e.g. a non-tristate `Checkbox`). The returned signal is
    /// **writable**: setting it to `true` calls `check(node)` (which
    /// runs the configured cascade), `false` calls `uncheck(node)`.
    /// Writes from the model side propagate back into the bool signal
    /// (`Checked → true`, anything else → `false`). Cached: repeat
    /// calls return the same handle.
    ///
    /// For leaves under `AggregateMode::DescendantsDriveAncestors`
    /// this is the right pairing — a leaf's state is two-state by
    /// nature, and the model's ancestor recompute still runs. For
    /// branches you typically want the tristate `signal_for` so
    /// `Indeterminate` is visible.
    pub fn bool_signal_for(&self, node: NodeId) -> Signal<bool> {
        if let Some(b) = self.inner.borrow().bool_signals.get(&node) {
            return b.clone();
        }
        // Make sure the tristate signal exists (with its cascade
        // observer attached) before we wire the bridge.
        let tristate = self.signal_for(node);
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

        // bool → tristate (the existing tristate cascade observer
        // takes it from there, including ancestor recompute).
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
        inner.bool_signals.insert(node, bool_sig.clone());
        inner.bridge_guards.insert(node, guard);
        inner
            .bridge_observers
            .insert(node, (tri_to_bool, bool_to_tri));
        bool_sig
    }

    /// Returns the current [`CheckState`] for `node` (defaults to `Unchecked`
    /// if the node's signal has never been written or read).
    pub fn check_state(&self, node: NodeId) -> CheckState {
        self.inner
            .borrow()
            .state
            .get(&node)
            .map(|s| s.get())
            .unwrap_or(CheckState::Unchecked)
    }

    /// Set `node` to [`CheckState::Checked`], triggering the configured cascade and
    /// ancestor recompute; notifies observers of every affected node's signal.
    pub fn check(&self, node: NodeId) {
        // Setting via signal_for runs through the observer, which
        // performs the cascade. No need to duplicate logic here.
        self.signal_for(node).set(CheckState::Checked);
    }

    /// Set `node` to [`CheckState::Unchecked`], triggering the configured cascade and
    /// ancestor recompute; notifies observers of every affected node's signal.
    pub fn uncheck(&self, node: NodeId) {
        self.signal_for(node).set(CheckState::Unchecked);
    }

    /// Toggle `node`'s check state: under `DescendantsDriveAncestors` a leaf
    /// cycles two-state (`Unchecked` ↔ `Checked`); a branch or `AggregateMode::None`
    /// cycles the full tristate sequence via [`CheckState::next_tristate`].
    pub fn toggle(&self, node: NodeId) {
        let current = self.check_state(node);
        let next = match (self.mode.get(), self.is_leaf(node), current) {
            (AggregateMode::DescendantsDriveAncestors, true, CheckState::Unchecked) => {
                CheckState::Checked
            }
            (AggregateMode::DescendantsDriveAncestors, true, _) => CheckState::Unchecked,
            (_, _, _) => current.next_tristate(),
        };
        self.signal_for(node).set(next);
    }

    /// Returns all `NodeId`s whose current state is exactly [`CheckState::Checked`].
    ///
    /// Note: may include stale ids if the underlying tree has been mutated since
    /// the signals were first registered — see the module-level limitation note.
    pub fn checked_nodes(&self) -> Vec<NodeId> {
        self.inner
            .borrow()
            .state
            .iter()
            .filter_map(|(id, sig)| (sig.get() == CheckState::Checked).then_some(*id))
            .collect()
    }

    /// Reset all known nodes to [`CheckState::Unchecked`] and notify observers.
    pub fn clear(&self) {
        // Snapshot keys to avoid borrow-during-iteration.
        let keys: Vec<NodeId> = self.inner.borrow().state.keys().copied().collect();
        for k in keys {
            let sig = self.signal_for(k);
            if sig.get() != CheckState::Unchecked {
                sig.set(CheckState::Unchecked);
            }
        }
    }

    fn is_leaf(&self, node: NodeId) -> bool {
        self.tree.children(node).is_empty()
    }
}

/// RAII guard: sets `suppress = true` on creation, clears it on drop — so a
/// panic during a cascade can't leave the model permanently unable to cascade.
struct SuppressGuard {
    inner: Rc<RefCell<Inner>>,
}

impl SuppressGuard {
    fn new(inner: &Rc<RefCell<Inner>>) -> Self {
        inner.borrow_mut().suppress = true;
        Self {
            inner: inner.clone(),
        }
    }
}

impl Drop for SuppressGuard {
    fn drop(&mut self) {
        // A borrow may still be held during a panic unwind; best-effort clear.
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.suppress = false;
        }
    }
}

// Free functions so the observer closure can call them without
// holding `&self` (the model isn't `Clone` cheaply, and the closure
// only has a `Weak<Inner>`).

fn cascade_descendants<T: 'static>(
    tree: &TreeModel<T>,
    inner: &Rc<RefCell<Inner>>,
    root: NodeId,
    target: CheckState,
) {
    for child in tree.children(root) {
        write_state(inner, child, target);
        cascade_descendants(tree, inner, child, target);
    }
}

fn recompute_from_children<T: 'static>(
    tree: &TreeModel<T>,
    inner: &Rc<RefCell<Inner>>,
    node: NodeId,
) {
    let kids = tree.children(node);
    if kids.is_empty() {
        return;
    }
    let mut all_checked = true;
    let mut all_unchecked = true;
    for child in &kids {
        let st = read_state(inner, *child);
        match st {
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

fn read_state(inner: &Rc<RefCell<Inner>>, node: NodeId) -> CheckState {
    inner
        .borrow()
        .state
        .get(&node)
        .map(|s| s.get())
        .unwrap_or(CheckState::Unchecked)
}

fn write_state(inner: &Rc<RefCell<Inner>>, node: NodeId, state: CheckState) {
    let sig = {
        let mut map = inner.borrow_mut();
        map.state
            .entry(node)
            .or_insert_with(|| Signal::new(CheckState::Unchecked))
            .clone()
    };
    if sig.get() != state {
        sig.set(state);
    }
}

impl<T: 'static> Clone for TreeCheckedModel<T> {
    fn clone(&self) -> Self {
        Self {
            tree: self.tree.clone(),
            inner: self.inner.clone(),
            mode: self.mode.clone(),
        }
    }
}

impl<T: 'static> std::fmt::Debug for TreeCheckedModel<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreeCheckedModel")
            .field("mode", &self.mode.get())
            .field("tracked_nodes", &self.inner.borrow().state.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tree() -> (
        TreeModel<&'static str>,
        NodeId,
        NodeId,
        NodeId,
        NodeId,
        NodeId,
    ) {
        // root1 (parent)
        //   ├─ a (leaf)
        //   └─ b (leaf)
        // root2 (parent)
        //   └─ c (leaf)
        let t = TreeModel::new();
        let root1 = t.insert_root(0, "root1");
        let a = t.insert_child(root1, 0, "a");
        let b = t.insert_child(root1, 1, "b");
        let root2 = t.insert_root(1, "root2");
        let c = t.insert_child(root2, 0, "c");
        (t, root1, a, b, root2, c)
    }

    #[test]
    fn descendants_drive_ancestors_default() {
        let (t, root1, a, b, _root2, _c) = sample_tree();
        let m = TreeCheckedModel::new(t);
        // Pre-register every node's signal so the observer chain is
        // wired before we mutate.
        let _ = (m.signal_for(root1), m.signal_for(a), m.signal_for(b));

        m.check(a);
        assert_eq!(m.check_state(a), CheckState::Checked);
        assert_eq!(m.check_state(root1), CheckState::Indeterminate);

        m.check(b);
        assert_eq!(m.check_state(root1), CheckState::Checked);
    }

    #[test]
    fn set_parent_cascades_to_descendants() {
        let (t, root1, a, b, _r2, _c) = sample_tree();
        let m = TreeCheckedModel::new(t);
        let _ = (m.signal_for(root1), m.signal_for(a), m.signal_for(b));

        m.check(root1);
        assert_eq!(m.check_state(a), CheckState::Checked);
        assert_eq!(m.check_state(b), CheckState::Checked);
        assert_eq!(m.check_state(root1), CheckState::Checked);

        m.uncheck(root1);
        assert_eq!(m.check_state(a), CheckState::Unchecked);
        assert_eq!(m.check_state(b), CheckState::Unchecked);
    }

    #[test]
    fn external_signal_write_triggers_cascade() {
        // Simulate a Checkbox widget writing directly to the per-node
        // signal — the observer should still cascade.
        let (t, root1, a, b, _r2, _c) = sample_tree();
        let m = TreeCheckedModel::new(t);
        let parent_sig = m.signal_for(root1);
        let _ = (m.signal_for(a), m.signal_for(b));

        parent_sig.set(CheckState::Checked);
        assert_eq!(m.check_state(a), CheckState::Checked);
        assert_eq!(m.check_state(b), CheckState::Checked);
    }

    #[test]
    fn lazy_signal_still_cascades() {
        // Regression: a signal first materialised by a cascade (write_state)
        // must still cascade when its own signal_for is called later (a
        // virtualized row realizing after its parent was checked).
        let (t, root1, a, _b, _r2, _c) = sample_tree();
        let m = TreeCheckedModel::new(t);
        let _ = m.signal_for(root1); // only the parent is realized
        m.check(root1); // cascades Checked to a, b via observer-less signals
        assert_eq!(m.check_state(a), CheckState::Checked);

        let a_sig = m.signal_for(a); // leaf a's row finally realizes + binds
        a_sig.set(CheckState::Unchecked); // user unchecks it
        // root1 must recompute (b still Checked, a now Unchecked → mixed).
        assert_eq!(m.check_state(root1), CheckState::Indeterminate);
    }

    #[test]
    fn aggregate_mode_none_disables_propagation() {
        let (t, root1, a, _b, _r2, _c) = sample_tree();
        let m = TreeCheckedModel::with_mode(t, AggregateMode::None);
        let _ = (m.signal_for(root1), m.signal_for(a));

        m.check(a);
        assert_eq!(m.check_state(a), CheckState::Checked);
        assert_eq!(m.check_state(root1), CheckState::Unchecked);
    }

    #[test]
    fn signal_for_is_stable_across_calls() {
        let (t, _root1, a, _b, _r2, _c) = sample_tree();
        let m = TreeCheckedModel::new(t);

        let s1 = m.signal_for(a);
        let s2 = m.signal_for(a);
        m.check(a);
        assert_eq!(s1.get(), CheckState::Checked);
        assert_eq!(s2.get(), CheckState::Checked);
    }

    #[test]
    fn checked_nodes_excludes_indeterminate() {
        let (t, root1, a, _b, _r2, _c) = sample_tree();
        let m = TreeCheckedModel::new(t);
        let _ = (m.signal_for(root1), m.signal_for(a));
        m.check(a);
        let nodes = m.checked_nodes();
        assert!(nodes.contains(&a));
        assert!(!nodes.contains(&root1));
    }

    #[test]
    fn toggle_leaf_two_state_in_aggregate_mode() {
        let (t, _root1, a, _b, _r2, _c) = sample_tree();
        let m = TreeCheckedModel::new(t);

        m.toggle(a);
        assert_eq!(m.check_state(a), CheckState::Checked);
        m.toggle(a);
        assert_eq!(m.check_state(a), CheckState::Unchecked);
    }

    #[test]
    fn bool_signal_writes_propagate_to_tristate() {
        let (t, _root1, a, _b, _r2, _c) = sample_tree();
        let m = TreeCheckedModel::new(t);
        let bool_sig = m.bool_signal_for(a);
        assert!(!bool_sig.get());

        bool_sig.set(true);
        assert_eq!(m.check_state(a), CheckState::Checked);

        bool_sig.set(false);
        assert_eq!(m.check_state(a), CheckState::Unchecked);
    }

    #[test]
    fn bool_signal_reflects_tristate_writes() {
        let (t, _root1, a, _b, _r2, _c) = sample_tree();
        let m = TreeCheckedModel::new(t);
        let bool_sig = m.bool_signal_for(a);

        m.check(a);
        assert!(bool_sig.get());
        m.uncheck(a);
        assert!(!bool_sig.get());
    }

    #[test]
    fn bool_signal_indeterminate_reads_as_false() {
        let (t, root1, a, b, _r2, _c) = sample_tree();
        let m = TreeCheckedModel::new(t);
        let parent_bool = m.bool_signal_for(root1);
        let _ = (m.signal_for(a), m.signal_for(b));

        m.check(a); // → root1 becomes Indeterminate
        assert_eq!(m.check_state(root1), CheckState::Indeterminate);
        assert!(!parent_bool.get(), "Indeterminate must not read as true");
    }

    #[test]
    fn bool_signal_writes_through_leaves_recompute_ancestors() {
        let (t, root1, a, b, _r2, _c) = sample_tree();
        let m = TreeCheckedModel::new(t);
        let a_bool = m.bool_signal_for(a);
        let b_bool = m.bool_signal_for(b);

        a_bool.set(true);
        assert_eq!(m.check_state(root1), CheckState::Indeterminate);
        b_bool.set(true);
        assert_eq!(m.check_state(root1), CheckState::Checked);
    }

    #[test]
    fn bool_signal_for_is_stable_across_calls() {
        let (t, _root1, a, _b, _r2, _c) = sample_tree();
        let m = TreeCheckedModel::new(t);
        let s1 = m.bool_signal_for(a);
        let s2 = m.bool_signal_for(a);
        s1.set(true);
        assert!(s2.get());
    }

    #[test]
    fn clear_resets_all() {
        let (t, root1, _a, _b, _r2, _c) = sample_tree();
        let m = TreeCheckedModel::new(t);
        let _ = (m.signal_for(root1),);
        m.check(root1);
        m.clear();
        assert_eq!(m.checked_nodes(), Vec::<NodeId>::new());
    }
}
