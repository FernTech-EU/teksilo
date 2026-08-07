// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TreeSlice` — per-view flattened projection of a [`TreeModel`].
//!
//! `TreeSlice<T>` wraps a `TreeModel<T>` and maintains an independent
//! expand/collapse set so two `TreeView` widgets sharing the same model have
//! independent visible rows — dual-pane file managers, overview/detail splits,
//! and search results panels are each one `TreeSlice::new(model.clone())`. The
//! slice re-flattens automatically whenever the underlying model emits a
//! [`TreeChange`], and bumps a [`version_signal`](TreeSlice::version_signal)
//! `Signal<u64>` that views bind at `BindingLevel::Rebuild`.
//!
//! A lightweight [`TreeSliceHandle`] (created via [`TreeSlice::handle`]) shares
//! all `Rc`-based internals and is usable in closures without keeping the
//! tree-change observer alive.
//!
//! `TreeSlice` implements [`TreeDataSource`] and is the
//! built-in source for `TreeView` / `TreeTableView`.
//!
//! ## Example
//!
//! ```rust
//! # use teksilo_data::{TreeModel, TreeSlice};
//! let tree = TreeModel::new();
//! let root = tree.insert_root(0, "root");
//! let child = tree.insert_child(root, 0, "child");
//!
//! let slice1 = TreeSlice::new(tree.clone());
//! let slice2 = TreeSlice::new(tree.clone());
//!
//! slice1.expand(root);
//! assert_eq!(slice1.visible_count(), 2); // root + child visible
//! assert_eq!(slice2.visible_count(), 1); // still collapsed in slice2
//!
//! // Inserting into the model notifies both slices.
//! tree.insert_child(root, 1, "child2");
//! assert_eq!(slice1.visible_count(), 3); // child2 also visible in the expanded slice
//! ```

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use teksilo_core::ObserverHandle;
use teksilo_core::signal::Signal;

use crate::TreeModel;
use crate::dnd_types::{DragEligibility, DragSource, DropCommit, DropQuery, DropResponse};
use crate::tree_change::{NodeId, TreeChange};
use crate::tree_data_source::{
    FlatEntry, TreeDataSource, tree_apply_reorder, tree_is_desc_or_self,
};

/// Per-view flattened projection of a [`TreeModel<T>`](crate::TreeModel).
///
/// Owns an independent expand/collapse set and re-flattens automatically on
/// every [`TreeChange`] from the underlying model. Two slices
/// over the same model have completely independent expand state. See the
/// [module documentation](self) for the full picture.
pub struct TreeSlice<T: 'static> {
    tree: TreeModel<T>,
    expanded: Rc<RefCell<HashSet<NodeId>>>,
    flattened: Rc<RefCell<Vec<FlatEntry>>>,
    /// `NodeId` → flat index, rebuilt alongside `flattened` on every
    /// reflatten. Keeps `flat_index_of` O(1) (mirrors `TreeDataSlice`'s
    /// `vis_pos`) instead of a linear scan over `flattened`.
    positions: Rc<RefCell<HashMap<NodeId, usize>>>,
    version: Signal<u64>,
    version_counter: Rc<std::cell::Cell<u64>>,
    /// First flat index whose content may differ after the latest
    /// reflatten. See [`first_changed_index`](Self::first_changed_index).
    divergence: Rc<std::cell::Cell<Option<usize>>>,
    _tree_observer: ObserverHandle,
}

impl<T: 'static> TreeSlice<T> {
    /// Create a new `TreeSlice` for the given `TreeModel`.
    /// All nodes start collapsed (only roots are visible).
    pub fn new(tree: TreeModel<T>) -> Self {
        let expanded: Rc<RefCell<HashSet<NodeId>>> = Rc::new(RefCell::new(HashSet::new()));
        let flattened: Rc<RefCell<Vec<FlatEntry>>> = Rc::new(RefCell::new(Vec::new()));
        let positions: Rc<RefCell<HashMap<NodeId, usize>>> = Rc::new(RefCell::new(HashMap::new()));
        let version = Signal::new(0_u64);
        let version_counter = Rc::new(std::cell::Cell::new(0_u64));
        let divergence = Rc::new(std::cell::Cell::new(None));

        // Initial flatten
        Self::rebuild_flat_list(
            &tree,
            &expanded.borrow(),
            &mut flattened.borrow_mut(),
            &mut positions.borrow_mut(),
        );

        // Observe tree changes
        let exp = expanded.clone();
        let flat = flattened.clone();
        let pos = positions.clone();
        let tree_for_obs = tree.clone();
        let ver = version.clone();
        let vc = version_counter.clone();
        let div = divergence.clone();
        let observer = tree.observe_changes(move |change| {
            let mut d = Self::rebuild_flat_list(
                &tree_for_obs,
                &exp.borrow(),
                &mut flat.borrow_mut(),
                &mut pos.borrow_mut(),
            );
            // A NodeUpdated leaves the flat structure identical, but the
            // updated node's content (and thus any per-row derived state
            // such as a measured height) changed — fold its flat position
            // into the divergence.
            if let TreeChange::NodeUpdated { node } = change
                && let Some(&p) = pos.borrow().get(node)
            {
                d = d.min(p);
            }
            div.set(Some(d));
            let next = vc.get() + 1;
            vc.set(next);
            ver.set(next);
        });

        Self {
            tree,
            expanded,
            flattened,
            positions,
            version,
            version_counter,
            divergence,
            _tree_observer: observer,
        }
    }

    /// Number of currently visible (flattened) rows.
    pub fn visible_count(&self) -> usize {
        self.flattened.borrow().len()
    }

    /// Access a flat entry by index via callback.
    /// The callback receives `(&T, &FlatEntry)`.
    pub fn with_entry<R>(
        &self,
        flat_index: usize,
        f: impl FnOnce(&T, &FlatEntry) -> R,
    ) -> Option<R> {
        let flat = self.flattened.borrow();
        let entry = flat.get(flat_index)?;
        let node_id = entry.node_id;
        // We need to access tree data while holding the flat borrow.
        // Since tree.with_item borrows the tree's inner RefCell (separate from ours), this is safe.
        self.tree.with_item(node_id, |item| f(item, entry))
    }

    /// Get the `NodeId` at the given flat index.
    pub fn visible_node_id(&self, flat_index: usize) -> Option<NodeId> {
        self.flattened.borrow().get(flat_index).map(|e| e.node_id)
    }

    /// Get the `FlatEntry` at the given flat index (cloned).
    pub fn entry_at(&self, flat_index: usize) -> Option<FlatEntry> {
        self.flattened.borrow().get(flat_index).cloned()
    }

    /// Get the depth at the given flat index.
    pub fn depth_at(&self, flat_index: usize) -> usize {
        self.flattened
            .borrow()
            .get(flat_index)
            .map(|e| e.depth)
            .unwrap_or(0)
    }

    /// Find the flat index for a given `NodeId`, or `None` if not visible.
    /// O(1) — backed by a position map rebuilt on every reflatten.
    pub fn flat_index_of(&self, node: NodeId) -> Option<usize> {
        self.positions.borrow().get(&node).copied()
    }

    // --- Expand / Collapse ---

    /// Whether the given node is expanded.
    pub fn is_expanded(&self, node: NodeId) -> bool {
        self.expanded.borrow().contains(&node)
    }

    /// Expand a node (make its children visible).
    pub fn expand(&self, node: NodeId) {
        {
            let mut exp = self.expanded.borrow_mut();
            if !exp.insert(node) {
                return; // Already expanded
            }
        }
        self.reflatten_and_notify();
    }

    /// Collapse a node (hide its children).
    pub fn collapse(&self, node: NodeId) {
        {
            let mut exp = self.expanded.borrow_mut();
            if !exp.remove(&node) {
                return; // Already collapsed
            }
        }
        self.reflatten_and_notify();
    }

    /// Toggle expand/collapse state of a node.
    pub fn toggle(&self, node: NodeId) {
        {
            let mut exp = self.expanded.borrow_mut();
            if exp.contains(&node) {
                exp.remove(&node);
            } else {
                exp.insert(node);
            }
        }
        self.reflatten_and_notify();
    }

    /// Expand all nodes in the tree.
    pub fn expand_all(&self) {
        {
            let mut exp = self.expanded.borrow_mut();
            self.expand_all_recursive(&mut exp);
        }
        self.reflatten_and_notify();
    }

    /// Collapse all nodes in the tree.
    pub fn collapse_all(&self) {
        {
            let mut exp = self.expanded.borrow_mut();
            exp.clear();
        }
        self.reflatten_and_notify();
    }

    /// Get all expanded node IDs (for persistence).
    pub fn expanded_nodes(&self) -> Vec<NodeId> {
        self.expanded.borrow().iter().copied().collect()
    }

    /// Restore expanded state (for persistence).
    pub fn set_expanded_nodes(&self, nodes: &[NodeId]) {
        {
            let mut exp = self.expanded.borrow_mut();
            exp.clear();
            for &node in nodes {
                exp.insert(node);
            }
        }
        self.reflatten_and_notify();
    }

    /// Get the version signal for binding to `BindingLevel::Rebuild`.
    pub fn version_signal(&self) -> Signal<u64> {
        self.version.clone()
    }

    /// First flat index whose content may differ from before the latest
    /// reflatten — the rows `0..index` are the same nodes, at the same
    /// depths, with the same expand state as before, so any per-row
    /// derived state (e.g. a measured row height) remains valid for them.
    /// Equal to `visible_count()` when the visible list is unchanged.
    ///
    /// `None` means unknown (no reflatten observed yet) — treat as a full
    /// change. The value describes the **latest** reflatten only; read it
    /// synchronously from a `version_signal()` observer (observers fire
    /// inline on every bump, so per-change reads cannot miss a value).
    pub fn first_changed_index(&self) -> Option<usize> {
        self.divergence.get()
    }

    /// Access the underlying `TreeModel`.
    pub fn tree(&self) -> &TreeModel<T> {
        &self.tree
    }

    /// Create a lightweight handle for use in closures.
    /// Shares all Rc-based internals but does not keep the observer alive.
    pub fn handle(&self) -> TreeSliceHandle<T> {
        TreeSliceHandle {
            tree: self.tree.clone(),
            expanded: self.expanded.clone(),
            flattened: self.flattened.clone(),
            positions: self.positions.clone(),
            version: self.version.clone(),
            version_counter: self.version_counter.clone(),
            divergence: self.divergence.clone(),
        }
    }

    // --- Internal ---

    fn reflatten_and_notify(&self) {
        let d = Self::rebuild_flat_list(
            &self.tree,
            &self.expanded.borrow(),
            &mut self.flattened.borrow_mut(),
            &mut self.positions.borrow_mut(),
        );
        self.divergence.set(Some(d));
        let next = self.version_counter.get() + 1;
        self.version_counter.set(next);
        self.version.set(next);
    }

    fn expand_all_recursive(&self, expanded: &mut HashSet<NodeId>) {
        // Walk the full tree to find all nodes with children
        let root_count = self.tree.root_count();
        for i in 0..root_count {
            let root = self.tree.root(i);
            Self::expand_subtree_recursive(&self.tree, root, expanded);
        }
    }

    /// Explicit-stack walk — `expand_all` is the natural companion of a deep
    /// `flatten_node` walk, so it needs the same depth-bounded traversal.
    fn expand_subtree_recursive(tree: &TreeModel<T>, root: NodeId, expanded: &mut HashSet<NodeId>) {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if tree.has_children(node) {
                expanded.insert(node);
                for child in tree.children(node) {
                    stack.push(child);
                }
            }
        }
    }

    /// Rebuild `out` (and the `pos` position map alongside it) from scratch
    /// and return the length of the common prefix with the previous flat
    /// list — the first flat index at which the projection diverges
    /// (`out.len()` when nothing visible changed). `NodeId`s are stable
    /// slotmap keys, so equal entries denote the same node at the same
    /// depth/expand state.
    fn rebuild_flat_list(
        tree: &TreeModel<T>,
        expanded: &HashSet<NodeId>,
        out: &mut Vec<FlatEntry>,
        pos: &mut HashMap<NodeId, usize>,
    ) -> usize {
        let old = std::mem::take(out);
        out.reserve(old.len());
        let root_count = tree.root_count();
        for i in 0..root_count {
            let root = tree.root(i);
            Self::flatten_node(tree, root, 0, expanded, out);
        }
        pos.clear();
        pos.extend(out.iter().enumerate().map(|(i, e)| (e.node_id, i)));
        old.iter()
            .zip(out.iter())
            .take_while(|(a, b)| a == b)
            .count()
    }

    /// Explicit-stack pre-order walk (children pushed in reverse so `pop()`
    /// yields them in source order) — depth-bounded by tree size, not the
    /// call stack.
    fn flatten_node(
        tree: &TreeModel<T>,
        root: NodeId,
        depth: usize,
        expanded: &HashSet<NodeId>,
        out: &mut Vec<FlatEntry>,
    ) {
        let mut stack = vec![(root, depth)];
        while let Some((node, depth)) = stack.pop() {
            let has_children = tree.has_children(node);
            let is_expanded = expanded.contains(&node);

            out.push(FlatEntry {
                node_id: node,
                depth,
                has_children,
                is_expanded,
            });

            if is_expanded && has_children {
                for child in tree.children(node).into_iter().rev() {
                    stack.push((child, depth + 1));
                }
            }
        }
    }
}

impl<T: 'static> std::fmt::Debug for TreeSlice<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreeSlice")
            .field("visible_count", &self.visible_count())
            .field("expanded_count", &self.expanded.borrow().len())
            .finish()
    }
}

/// Lightweight handle to a [`TreeSlice`]'s shared state, usable in closures.
///
/// Created via [`TreeSlice::handle`]. Shares all `Rc`-based internals with its
/// parent `TreeSlice` but does **not** keep the tree-change observer alive —
/// the `TreeSlice` that owns the observer must outlive all handles that rely on
/// automatic re-flattening on model changes.
pub struct TreeSliceHandle<T: 'static> {
    tree: TreeModel<T>,
    expanded: Rc<RefCell<HashSet<NodeId>>>,
    flattened: Rc<RefCell<Vec<FlatEntry>>>,
    positions: Rc<RefCell<HashMap<NodeId, usize>>>,
    version: Signal<u64>,
    version_counter: Rc<std::cell::Cell<u64>>,
    divergence: Rc<std::cell::Cell<Option<usize>>>,
}

impl<T: 'static> TreeSliceHandle<T> {
    /// Number of currently-visible (flattened) rows.
    pub fn visible_count(&self) -> usize {
        self.flattened.borrow().len()
    }

    /// Get the [`FlatEntry`] at `flat_index` (cloned), or `None` if out of bounds.
    pub fn entry_at(&self, flat_index: usize) -> Option<FlatEntry> {
        self.flattened.borrow().get(flat_index).cloned()
    }

    /// Get the [`NodeId`] at `flat_index`, or `None` if out of bounds.
    pub fn visible_node_id(&self, flat_index: usize) -> Option<NodeId> {
        self.flattened.borrow().get(flat_index).map(|e| e.node_id)
    }

    /// Expand `node` (make its children visible) and bump the version signal.
    /// No-op if already expanded.
    pub fn expand(&self, node: NodeId) {
        let inserted = self.expanded.borrow_mut().insert(node);
        if inserted {
            self.reflatten_and_notify();
        }
    }

    /// Collapse `node` (hide its children) and bump the version signal.
    /// No-op if already collapsed.
    pub fn collapse(&self, node: NodeId) {
        let removed = self.expanded.borrow_mut().remove(&node);
        if removed {
            self.reflatten_and_notify();
        }
    }

    /// Returns `true` if `node` is currently expanded.
    pub fn is_expanded(&self, node: NodeId) -> bool {
        self.expanded.borrow().contains(&node)
    }

    /// Toggle `node`'s expand/collapse state and bump the version signal.
    pub fn toggle_expand(&self, node: NodeId) {
        {
            let mut exp = self.expanded.borrow_mut();
            if exp.contains(&node) {
                exp.remove(&node);
            } else {
                exp.insert(node);
            }
        }
        self.reflatten_and_notify();
    }

    /// Access the underlying [`TreeModel`].
    pub fn tree(&self) -> &TreeModel<T> {
        &self.tree
    }

    /// Expand every node with children — see [`TreeSlice::expand_all`]. Useful
    /// after a model rebuild reassigns `NodeId`s (the old expand set no longer
    /// matches), to keep the view fully expanded.
    pub fn expand_all(&self) {
        {
            let mut exp = self.expanded.borrow_mut();
            let root_count = self.tree.root_count();
            for i in 0..root_count {
                let root = self.tree.root(i);
                TreeSlice::<T>::expand_subtree_recursive(&self.tree, root, &mut exp);
            }
        }
        self.reflatten_and_notify();
    }

    /// See [`TreeSlice::first_changed_index`].
    pub fn first_changed_index(&self) -> Option<usize> {
        self.divergence.get()
    }

    fn reflatten_and_notify(&self) {
        let d = TreeSlice::<T>::rebuild_flat_list(
            &self.tree,
            &self.expanded.borrow(),
            &mut self.flattened.borrow_mut(),
            &mut self.positions.borrow_mut(),
        );
        self.divergence.set(Some(d));
        let next = self.version_counter.get() + 1;
        self.version_counter.set(next);
        self.version.set(next);
    }
}

impl<T: 'static> Clone for TreeSliceHandle<T> {
    fn clone(&self) -> Self {
        Self {
            tree: self.tree.clone(),
            expanded: self.expanded.clone(),
            flattened: self.flattened.clone(),
            positions: self.positions.clone(),
            version: self.version.clone(),
            version_counter: self.version_counter.clone(),
            divergence: self.divergence.clone(),
        }
    }
}

/// `TreeSlice` is the built-in per-view `TreeDataSource` over an in-memory
/// `TreeModel`. Identity is `NodeId`; a `SameView` drop reorders via
/// `move_node`/`move_to_root` (with the cycle guard). `Foreign` drops are
/// rejected — a bare slice knows no foreign payloads.
impl<T: 'static> TreeDataSource for TreeSlice<T> {
    type Item = T;
    type Key = NodeId;

    fn visible_count(&self) -> usize {
        TreeSlice::visible_count(self)
    }

    fn with_entry<R>(
        &self,
        flat_index: usize,
        f: impl FnOnce(&Self::Item, &FlatEntry<Self::Key>) -> R,
    ) -> Option<R> {
        TreeSlice::with_entry(self, flat_index, f)
    }

    fn key_at(&self, flat_index: usize) -> Option<NodeId> {
        self.visible_node_id(flat_index)
    }

    fn flat_index_of(&self, key: &NodeId) -> Option<usize> {
        TreeSlice::flat_index_of(self, *key)
    }

    fn parent(&self, key: &NodeId) -> Option<NodeId> {
        self.tree().parent(*key)
    }

    fn child_keys(&self, key: &NodeId) -> Vec<NodeId> {
        self.tree().children(*key)
    }

    fn version_signal(&self) -> Signal<u64> {
        TreeSlice::version_signal(self)
    }

    fn first_changed_index(&self) -> Option<usize> {
        TreeSlice::first_changed_index(self)
    }

    fn contains_key(&self, key: &NodeId) -> bool {
        // Existence against the backing tree, not the visible projection, so a
        // node hidden under a collapsed ancestor keeps its keyed selection.
        self.tree().with_item(*key, |_| ()).is_some()
    }

    fn is_expanded(&self, key: &NodeId) -> bool {
        TreeSlice::is_expanded(self, *key)
    }

    fn set_expanded(&self, key: &NodeId, expanded: bool) {
        if expanded {
            self.expand(*key);
        } else {
            self.collapse(*key);
        }
    }

    fn drag(&self, _key: &NodeId) -> DragEligibility {
        DragEligibility::CanDrag
    }

    fn can_accept(&self, query: &DropQuery<'_, NodeId>) -> DropResponse {
        match &query.source {
            DragSource::SameView { key: source } => {
                if *source == query.target
                    || tree_is_desc_or_self(self.tree(), query.target, *source)
                {
                    DropResponse::Reject
                } else {
                    DropResponse::Accept
                }
            }
            DragSource::Foreign { .. } => DropResponse::Reject,
        }
    }

    fn accept_drop(&self, commit: DropCommit<'_, NodeId>) -> bool {
        match commit.source {
            DragSource::SameView { key: source } => {
                tree_apply_reorder(self.tree(), source, commit.target, commit.position)
            }
            DragSource::Foreign { .. } => false,
        }
    }

    fn on_drag_out(&self, key: &NodeId) {
        // Source-side completion for a foreign move: drop the node (and its
        // subtree) that was accepted elsewhere. Re-check existence first — a
        // reactive observer reacting to an earlier removal in the same batch
        // (or any unrelated mutation) could have already freed this node, and
        // `TreeModel::remove` panics on a stale key.
        if self.tree().with_item(*key, |_| ()).is_some() {
            self.tree().remove(*key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dnd_types::DropPosition;

    /// Build a sample tree:
    /// A
    ///   A1
    ///     A1a
    ///   A2
    /// B
    ///   B1
    /// C
    fn sample_tree() -> TreeModel<&'static str> {
        let tree = TreeModel::new();
        let a = tree.insert_root(0, "A");
        let a1 = tree.insert_child(a, 0, "A1");
        tree.insert_child(a1, 0, "A1a");
        tree.insert_child(a, 1, "A2");
        let b = tree.insert_root(1, "B");
        tree.insert_child(b, 0, "B1");
        tree.insert_root(2, "C");
        tree
    }

    #[test]
    fn initial_state_shows_only_roots() {
        let tree = sample_tree();
        let slice = TreeSlice::new(tree);

        assert_eq!(slice.visible_count(), 3); // A, B, C
        assert_eq!(
            slice.with_entry(0, |item, entry| {
                assert_eq!(*item, "A");
                assert_eq!(entry.depth, 0);
                assert!(entry.has_children);
                assert!(!entry.is_expanded);
            }),
            Some(())
        );
        assert_eq!(slice.with_entry(1, |item, _| *item), Some("B"));
        assert_eq!(slice.with_entry(2, |item, _| *item), Some("C"));
    }

    #[test]
    fn expand_shows_children() {
        let tree = sample_tree();
        let a = tree.root(0);
        let slice = TreeSlice::new(tree);

        assert_eq!(slice.visible_count(), 3);

        slice.expand(a);
        assert_eq!(slice.visible_count(), 5); // A, A1, A2, B, C

        assert_eq!(slice.with_entry(0, |item, _| *item), Some("A"));
        assert_eq!(
            slice.with_entry(1, |item, entry| {
                assert_eq!(*item, "A1");
                assert_eq!(entry.depth, 1);
                assert!(entry.has_children); // A1 has A1a
            }),
            Some(())
        );
        assert_eq!(slice.with_entry(2, |item, _| *item), Some("A2"));
        assert_eq!(slice.with_entry(3, |item, _| *item), Some("B"));
    }

    #[test]
    fn collapse_hides_children() {
        let tree = sample_tree();
        let a = tree.root(0);
        let slice = TreeSlice::new(tree);

        slice.expand(a);
        assert_eq!(slice.visible_count(), 5);

        slice.collapse(a);
        assert_eq!(slice.visible_count(), 3);
    }

    #[test]
    fn deep_expand() {
        let tree = sample_tree();
        let a = tree.root(0);
        let slice = TreeSlice::new(tree.clone());

        slice.expand(a);
        let a1 = slice.visible_node_id(1).unwrap();
        slice.expand(a1);

        // A, A1, A1a, A2, B, C
        assert_eq!(slice.visible_count(), 6);
        assert_eq!(
            slice.with_entry(2, |item, entry| {
                assert_eq!(*item, "A1a");
                assert_eq!(entry.depth, 2);
            }),
            Some(())
        );
    }

    #[test]
    fn toggle() {
        let tree = sample_tree();
        let a = tree.root(0);
        let slice = TreeSlice::new(tree);

        slice.toggle(a);
        assert_eq!(slice.visible_count(), 5); // expanded
        assert!(slice.is_expanded(a));

        slice.toggle(a);
        assert_eq!(slice.visible_count(), 3); // collapsed
        assert!(!slice.is_expanded(a));
    }

    #[test]
    fn expand_all() {
        let tree = sample_tree();
        let slice = TreeSlice::new(tree);

        slice.expand_all();
        // A, A1, A1a, A2, B, B1, C
        assert_eq!(slice.visible_count(), 7);
    }

    #[test]
    fn collapse_all() {
        let tree = sample_tree();
        let slice = TreeSlice::new(tree);

        slice.expand_all();
        assert_eq!(slice.visible_count(), 7);

        slice.collapse_all();
        assert_eq!(slice.visible_count(), 3);
    }

    #[test]
    fn handle_expand_all_matches_slice() {
        let tree = sample_tree();
        let slice = TreeSlice::new(tree);
        let handle = slice.handle();

        assert_eq!(slice.visible_count(), 3); // roots collapsed
        handle.expand_all();
        // A, A1, A1a, A2, B, B1, C — visible through the shared slice.
        assert_eq!(slice.visible_count(), 7);
    }

    #[test]
    fn two_slices_independent_expand() {
        let tree = sample_tree();
        let a = tree.root(0);
        let b = tree.root(1);

        let slice1 = TreeSlice::new(tree.clone());
        let slice2 = TreeSlice::new(tree);

        slice1.expand(a);
        slice2.expand(b);

        // Slice 1: A expanded, B collapsed
        assert_eq!(slice1.visible_count(), 5); // A, A1, A2, B, C
        assert!(slice1.is_expanded(a));
        assert!(!slice1.is_expanded(b));

        // Slice 2: A collapsed, B expanded
        assert_eq!(slice2.visible_count(), 4); // A, B, B1, C
        assert!(!slice2.is_expanded(a));
        assert!(slice2.is_expanded(b));
    }

    #[test]
    fn tree_mutation_updates_slice() {
        let tree = sample_tree();
        let a = tree.root(0);
        let slice = TreeSlice::new(tree.clone());

        slice.expand(a);
        assert_eq!(slice.visible_count(), 5); // A, A1, A2, B, C

        // Insert a new child under A
        tree.insert_child(a, 2, "A3");
        assert_eq!(slice.visible_count(), 6); // A, A1, A2, A3, B, C
    }

    #[test]
    fn tree_remove_updates_slice() {
        let tree = sample_tree();
        let a = tree.root(0);
        let slice = TreeSlice::new(tree.clone());

        slice.expand(a);
        let a1 = slice.visible_node_id(1).unwrap();
        tree.remove(a1);

        assert_eq!(slice.visible_count(), 4); // A, A2, B, C
    }

    #[test]
    fn version_signal_increments() {
        let tree = sample_tree();
        let a = tree.root(0);
        let slice = TreeSlice::new(tree.clone());

        let v0 = slice.version_signal().get();

        slice.expand(a);
        let v1 = slice.version_signal().get();
        assert!(v1 > v0, "version should increment on expand");

        tree.insert_root(3, "D");
        let v2 = slice.version_signal().get();
        assert!(v2 > v1, "version should increment on tree mutation");
    }

    #[test]
    fn flat_index_of() {
        let tree = sample_tree();
        let a = tree.root(0);
        let b = tree.root(1);
        let slice = TreeSlice::new(tree);

        assert_eq!(slice.flat_index_of(a), Some(0));
        assert_eq!(slice.flat_index_of(b), Some(1));
    }

    /// The position map underlying `flat_index_of` must agree with iteration
    /// order (`visible_node_id`) after every kind of reflatten-triggering
    /// mutation — expand, collapse, and an upstream model change (a filter
    /// pass on `TreeSlice` would be the model-level equivalent).
    fn assert_positions_match_iteration_order<T>(slice: &TreeSlice<T>) {
        for i in 0..slice.visible_count() {
            let node = slice.visible_node_id(i).unwrap();
            assert_eq!(
                slice.flat_index_of(node),
                Some(i),
                "flat_index_of({node:?}) should be the iteration position {i}"
            );
        }
    }

    #[test]
    fn flat_index_of_matches_iteration_order_across_mutations() {
        let tree = sample_tree();
        let a = tree.root(0);
        let b = tree.root(1);
        let slice = TreeSlice::new(tree.clone());

        assert_positions_match_iteration_order(&slice);

        slice.expand(a);
        assert_positions_match_iteration_order(&slice);

        slice.expand(b);
        assert_positions_match_iteration_order(&slice);

        slice.collapse(a);
        assert_positions_match_iteration_order(&slice);

        tree.insert_root(3, "D");
        assert_positions_match_iteration_order(&slice);

        tree.remove(b);
        assert_positions_match_iteration_order(&slice);
    }

    #[test]
    fn persistence_save_restore() {
        let tree = sample_tree();
        let a = tree.root(0);
        let slice = TreeSlice::new(tree.clone());

        slice.expand(a);
        let saved = slice.expanded_nodes();
        assert_eq!(saved.len(), 1);

        slice.collapse_all();
        assert_eq!(slice.visible_count(), 3);

        slice.set_expanded_nodes(&saved);
        assert_eq!(slice.visible_count(), 5); // A expanded again
    }

    #[test]
    fn out_of_bounds_returns_none() {
        let tree = sample_tree();
        let slice = TreeSlice::new(tree);
        assert_eq!(slice.with_entry(99, |_, _| ()), None);
        assert_eq!(slice.visible_node_id(99), None);
    }

    // ── first_changed_index (divergence) ────────────────────────────────

    #[test]
    fn divergence_unknown_before_first_reflatten() {
        let tree = sample_tree();
        let slice = TreeSlice::new(tree);
        assert_eq!(slice.first_changed_index(), None);
    }

    #[test]
    fn divergence_on_expand_is_the_toggled_row() {
        let tree = sample_tree();
        let b = tree.root(1);
        let slice = TreeSlice::new(tree);

        // Expanding B (flat index 1) changes B's own entry (is_expanded)
        // and inserts B1 after it — A (flat 0) is untouched.
        slice.expand(b);
        assert_eq!(slice.first_changed_index(), Some(1));

        slice.collapse(b);
        assert_eq!(slice.first_changed_index(), Some(1));
    }

    #[test]
    fn divergence_on_append_is_old_len() {
        let tree = sample_tree();
        let slice = TreeSlice::new(tree.clone());

        tree.insert_root(3, "D"); // old visible: A, B, C
        assert_eq!(slice.first_changed_index(), Some(3));
    }

    #[test]
    fn divergence_on_remove_is_removed_position() {
        let tree = sample_tree();
        let b = tree.root(1);
        let slice = TreeSlice::new(tree.clone());

        tree.remove(b); // old: A, B, C → new: A, C
        assert_eq!(slice.first_changed_index(), Some(1));
    }

    #[test]
    fn divergence_on_node_update_is_its_flat_index() {
        let tree = sample_tree();
        let c = tree.root(2);
        let slice = TreeSlice::new(tree.clone());

        // Structure unchanged, but C's content (flat index 2) changed.
        tree.update(c, "C-updated");
        assert_eq!(slice.first_changed_index(), Some(2));
    }

    #[test]
    fn divergence_on_invisible_update_is_visible_count() {
        let tree = sample_tree();
        let a = tree.root(0);
        let a1 = tree.children(a)[0];
        let slice = TreeSlice::new(tree.clone());

        // A1 is hidden (A collapsed) — nothing visible changed.
        tree.update(a1, "A1-updated");
        assert_eq!(slice.first_changed_index(), Some(slice.visible_count()));
    }

    #[test]
    fn divergence_via_handle_toggle() {
        let tree = sample_tree();
        let b = tree.root(1);
        let slice = TreeSlice::new(tree);
        let handle = slice.handle();

        handle.toggle_expand(b);
        assert_eq!(handle.first_changed_index(), Some(1));
        assert_eq!(slice.first_changed_index(), Some(1));
    }

    // ── TreeDataSource capability protocol ──────────────────────────────

    #[test]
    fn tree_source_accept_drop_reparents_into() {
        // Move B (root 1) Into A (root 0). Roots become A, C; B's parent is A.
        let tree = sample_tree();
        let a = tree.root(0);
        let b = tree.root(1);
        let slice = TreeSlice::new(tree.clone());
        assert!(slice.accept_drop(DropCommit {
            source: DragSource::SameView { key: b },
            target: a,
            position: DropPosition::Into,
        }));
        assert_eq!(tree.root_count(), 2);
        assert_eq!(tree.parent(b), Some(a));
    }

    #[test]
    fn tree_source_can_accept_rejects_cycle_and_refuses_drop() {
        // Cannot drop A into its own descendant A1.
        let tree = sample_tree();
        let a = tree.root(0);
        let slice = TreeSlice::new(tree.clone());
        slice.expand(a);
        let a1 = slice.visible_node_id(1).unwrap();
        assert_eq!(
            slice.can_accept(&DropQuery {
                source: DragSource::SameView { key: a },
                target: a1,
                position: DropPosition::Into,
            }),
            DropResponse::Reject
        );
        // accept_drop refuses rather than panicking in TreeModel::move_node.
        assert!(!slice.accept_drop(DropCommit {
            source: DragSource::SameView { key: a },
            target: a1,
            position: DropPosition::Into,
        }));
    }

    #[test]
    fn tree_source_reorders_root_siblings() {
        // Move C (root 2) Before A (root 0) → C, A, B at the root level.
        let tree = sample_tree();
        let a = tree.root(0);
        let c = tree.root(2);
        let slice = TreeSlice::new(tree.clone());
        assert!(slice.accept_drop(DropCommit {
            source: DragSource::SameView { key: c },
            target: a,
            position: DropPosition::Before,
        }));
        assert_eq!(slice.with_entry(0, |v, _| *v), Some("C"));
        assert_eq!(slice.with_entry(1, |v, _| *v), Some("A"));
        assert_eq!(slice.with_entry(2, |v, _| *v), Some("B"));
    }

    #[test]
    fn tree_reorder_within_filters_descendants_of_selected() {
        // Dragging {A, A1} (A1 is A's child) after C must move only A — A1
        // rides along inside A's subtree, it is not relocated independently.
        let tree = sample_tree();
        let a = tree.root(0);
        let c = tree.root(2);
        let slice = TreeSlice::new(tree.clone());
        let a1 = slice.child_keys(&a)[0];
        assert!(slice.reorder_within(&[a, a1], &c, DropPosition::After));
        assert_eq!(slice.with_entry(0, |v, _| *v), Some("B"));
        assert_eq!(slice.with_entry(1, |v, _| *v), Some("C"));
        assert_eq!(slice.with_entry(2, |v, _| *v), Some("A"));
        // A still owns both its children (A1 stayed put under A).
        assert_eq!(slice.child_keys(&a).len(), 2);
    }

    #[test]
    fn tree_on_drag_out_removes_node_and_subtree() {
        let tree = sample_tree();
        let b = tree.root(1);
        let slice = TreeSlice::new(tree.clone());
        slice.on_drag_out(&b);
        // B (and B1) gone → roots A, C remain.
        assert_eq!(slice.visible_count(), 2);
        assert_eq!(slice.with_entry(0, |v, _| *v), Some("A"));
        assert_eq!(slice.with_entry(1, |v, _| *v), Some("C"));
    }

    /// `flatten_node` and `expand_subtree_recursive` are both explicit-stack
    /// walks; a 50,000-deep single-child chain must flatten (and fully
    /// expand) without overflowing the call stack.
    #[test]
    fn deep_chain_flattens_and_expands_without_overflow() {
        const DEPTH: usize = 50_000;
        let tree = TreeModel::new();
        let root = tree.insert_root(0, 0usize);
        let mut leaf = root;
        for i in 1..DEPTH {
            leaf = tree.insert_child(leaf, 0, i);
        }
        let slice = TreeSlice::new(tree);

        // expand_all walks the whole tree (expand_subtree_recursive) then
        // reflattens once (flatten_node walks the full DEPTH) — both
        // explicit-stack, so this exercises both in one shot instead of
        // one `expand()` reflatten per node (which would be O(n^2)).
        slice.expand_all();

        assert_eq!(slice.visible_count(), DEPTH);
        assert_eq!(slice.flat_index_of(leaf), Some(DEPTH - 1));
        assert_eq!(slice.depth_at(DEPTH - 1), DEPTH - 1);
    }
}
