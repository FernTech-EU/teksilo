//! Per-view flattened projection of a `TreeModel`.
//!
//! `TreeSlice` owns expand/collapse state independently — two `TreeView`
//! widgets sharing the same `TreeModel` get independent expand states.
//! It maintains a flat list of currently-visible nodes with depth information,
//! and exposes a version `Signal<u64>` for consumers to bind to.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use fern_core::ObserverHandle;
use fern_core::signal::Signal;

use crate::TreeModel;
use crate::tree_change::NodeId;

/// A single entry in the flattened visible-node list.
#[derive(Debug, Clone)]
pub struct FlatEntry {
    /// The node's ID in the underlying `TreeModel`.
    pub node_id: NodeId,
    /// Depth in the tree (0 for roots).
    pub depth: usize,
    /// Whether this node has children in the `TreeModel`.
    pub has_children: bool,
    /// Whether this node is currently expanded (children visible).
    pub is_expanded: bool,
}

/// Per-view flattened projection of a `TreeModel<T>`.
///
/// Owns expand/collapse state and maintains a flat list of currently-visible
/// nodes. Observes `TreeChange` from the underlying model and re-flattens
/// as needed.
///
/// Created via `TreeModel::create_slice()` or `TreeSlice::new()`.
pub struct TreeSlice<T: 'static> {
    tree: TreeModel<T>,
    expanded: Rc<RefCell<HashSet<NodeId>>>,
    flattened: Rc<RefCell<Vec<FlatEntry>>>,
    version: Signal<u64>,
    version_counter: Rc<std::cell::Cell<u64>>,
    _tree_observer: ObserverHandle,
}

impl<T: 'static> TreeSlice<T> {
    /// Create a new `TreeSlice` for the given `TreeModel`.
    /// All nodes start collapsed (only roots are visible).
    pub fn new(tree: TreeModel<T>) -> Self {
        let expanded: Rc<RefCell<HashSet<NodeId>>> = Rc::new(RefCell::new(HashSet::new()));
        let flattened: Rc<RefCell<Vec<FlatEntry>>> = Rc::new(RefCell::new(Vec::new()));
        let version = Signal::new(0_u64);
        let version_counter = Rc::new(std::cell::Cell::new(0_u64));

        // Initial flatten
        Self::rebuild_flat_list(&tree, &expanded.borrow(), &mut flattened.borrow_mut());

        // Observe tree changes
        let exp = expanded.clone();
        let flat = flattened.clone();
        let tree_for_obs = tree.clone();
        let ver = version.clone();
        let vc = version_counter.clone();
        let observer = tree.observe_changes(move |_change| {
            Self::rebuild_flat_list(&tree_for_obs, &exp.borrow(), &mut flat.borrow_mut());
            let next = vc.get() + 1;
            vc.set(next);
            ver.set(next);
        });

        Self {
            tree,
            expanded,
            flattened,
            version,
            version_counter,
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
    pub fn flat_index_of(&self, node: NodeId) -> Option<usize> {
        self.flattened
            .borrow()
            .iter()
            .position(|e| e.node_id == node)
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
            version: self.version.clone(),
            version_counter: self.version_counter.clone(),
        }
    }

    // --- Internal ---

    fn reflatten_and_notify(&self) {
        Self::rebuild_flat_list(
            &self.tree,
            &self.expanded.borrow(),
            &mut self.flattened.borrow_mut(),
        );
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

    fn expand_subtree_recursive(tree: &TreeModel<T>, node: NodeId, expanded: &mut HashSet<NodeId>) {
        if tree.has_children(node) {
            expanded.insert(node);
            let children = tree.children(node);
            for child in children {
                Self::expand_subtree_recursive(tree, child, expanded);
            }
        }
    }

    fn rebuild_flat_list(
        tree: &TreeModel<T>,
        expanded: &HashSet<NodeId>,
        out: &mut Vec<FlatEntry>,
    ) {
        out.clear();
        let root_count = tree.root_count();
        for i in 0..root_count {
            let root = tree.root(i);
            Self::flatten_node(tree, root, 0, expanded, out);
        }
    }

    fn flatten_node(
        tree: &TreeModel<T>,
        node: NodeId,
        depth: usize,
        expanded: &HashSet<NodeId>,
        out: &mut Vec<FlatEntry>,
    ) {
        let has_children = tree.has_children(node);
        let is_expanded = expanded.contains(&node);

        out.push(FlatEntry {
            node_id: node,
            depth,
            has_children,
            is_expanded,
        });

        if is_expanded && has_children {
            let children = tree.children(node);
            for child in children {
                Self::flatten_node(tree, child, depth + 1, expanded, out);
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

/// Lightweight handle to a `TreeSlice`'s shared state, usable in closures.
/// Created via `TreeSlice::handle()`. Shares all Rc-based internals.
pub struct TreeSliceHandle<T: 'static> {
    tree: TreeModel<T>,
    expanded: Rc<RefCell<HashSet<NodeId>>>,
    flattened: Rc<RefCell<Vec<FlatEntry>>>,
    version: Signal<u64>,
    version_counter: Rc<std::cell::Cell<u64>>,
}

impl<T: 'static> TreeSliceHandle<T> {
    pub fn visible_count(&self) -> usize {
        self.flattened.borrow().len()
    }

    pub fn entry_at(&self, flat_index: usize) -> Option<FlatEntry> {
        self.flattened.borrow().get(flat_index).cloned()
    }

    pub fn visible_node_id(&self, flat_index: usize) -> Option<NodeId> {
        self.flattened.borrow().get(flat_index).map(|e| e.node_id)
    }

    pub fn expand(&self, node: NodeId) {
        let inserted = self.expanded.borrow_mut().insert(node);
        if inserted {
            self.reflatten_and_notify();
        }
    }

    pub fn collapse(&self, node: NodeId) {
        let removed = self.expanded.borrow_mut().remove(&node);
        if removed {
            self.reflatten_and_notify();
        }
    }

    pub fn is_expanded(&self, node: NodeId) -> bool {
        self.expanded.borrow().contains(&node)
    }

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

    pub fn tree(&self) -> &TreeModel<T> {
        &self.tree
    }

    fn reflatten_and_notify(&self) {
        TreeSlice::<T>::rebuild_flat_list(
            &self.tree,
            &self.expanded.borrow(),
            &mut self.flattened.borrow_mut(),
        );
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
            version: self.version.clone(),
            version_counter: self.version_counter.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
