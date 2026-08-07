// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TreeModel` — concrete reactive tree with shared, cloneable handles.
//!
//! `TreeModel<T>` owns a hierarchy of `T` items in a flat SlotMap arena with
//! parent-child links. Every structural mutation (`insert_root`, `insert_child`,
//! `remove`, `move_node`, `update`) emits a [`TreeChange`] to all registered
//! observers before returning. Node identity is a stable, versioned `NodeId`
//! (a SlotMap key) that is never reused after removal.
//!
//! Cloning produces a second handle to the **same** data — all handles see the
//! same hierarchy and receive the same change notifications. Register observers
//! via [`observe_changes`](TreeModel::observe_changes); the returned
//! [`ObserverHandle`] is RAII — dropping it
//! unregisters the callback.
//!
//! For per-view expand/collapse state wrap the model in a
//! [`TreeSlice`](crate::TreeSlice). For sort/filter projections use
//! [`SortFilterTreeModel`](crate::SortFilterTreeModel).
//!
//! ## Example
//!
//! ```rust
//! # use teksilo_data::{TreeModel, TreeChange};
//! let tree = TreeModel::new();
//! let root = tree.insert_root(0, "root");
//! let child = tree.insert_child(root, 0, "child");
//!
//! assert_eq!(tree.root_count(), 1);
//! assert_eq!(tree.child_count(root), 1);
//! assert_eq!(tree.parent(child), Some(root));
//!
//! let clone = tree.clone();
//! clone.insert_root(1, "root2");
//! assert_eq!(tree.root_count(), 2); // both handles share the same data
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use slotmap::SlotMap;

use teksilo_core::ObserverHandle;

use crate::tree_change::{NodeId, TreeChange};

struct TreeNode<T> {
    data: T,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
}

struct TreeObserverEntry {
    id: u64,
    callback: Rc<dyn Fn(&TreeChange)>,
}

struct TreeModelInner<T> {
    arena: SlotMap<slotmap::DefaultKey, TreeNode<T>>,
    roots: Vec<NodeId>,
    observers: Vec<TreeObserverEntry>,
    next_observer_id: u64,
    /// Strong handle to the debug-registry adapter for this tree.
    /// Owned here so the registration drops automatically when the
    /// inner is freed (the adapter holds only a `Weak` to inner,
    /// breaking the cycle). `None` until `.debug_named()` is called.
    /// Compiled out in release.
    #[cfg(debug_assertions)]
    debug_adapter: Option<Rc<dyn crate::debug_registry::ModelDebug>>,
}

/// A concrete reactive tree that stores a hierarchy of `T` items in a flat arena.
///
/// `TreeModel<T>` is `Clone` — cloning produces a second handle to the same
/// underlying data. All handles see the same hierarchy and receive the same
/// [`TreeChange`] notifications from [`observe_changes`](Self::observe_changes).
/// Nodes are identified by opaque [`NodeId`] handles that are stable and
/// non-reusable across mutations (versioned SlotMap keys).
pub struct TreeModel<T: 'static> {
    inner: Rc<RefCell<TreeModelInner<T>>>,
}

impl<T: 'static> TreeModel<T> {
    /// Create an empty tree model with no roots and no observers.
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(TreeModelInner {
                arena: SlotMap::new(),
                roots: Vec::new(),
                observers: Vec::new(),
                next_observer_id: 1,
                #[cfg(debug_assertions)]
                debug_adapter: None,
            })),
        }
    }

    // --- Structural queries ---

    /// Number of root-level nodes.
    pub fn root_count(&self) -> usize {
        self.inner.borrow().roots.len()
    }

    /// Get the `NodeId` of a root-level node by index.
    ///
    /// # Panics
    /// Panics if `index >= root_count()`.
    pub fn root(&self, index: usize) -> NodeId {
        self.inner.borrow().roots[index]
    }

    /// Number of children of the given node.
    pub fn child_count(&self, parent: NodeId) -> usize {
        let guard = self.inner.borrow();
        guard
            .arena
            .get(parent.key())
            .map(|n| n.children.len())
            .unwrap_or(0)
    }

    /// Get the `NodeId` of a child by parent and index.
    ///
    /// # Panics
    /// Panics if the parent or index is invalid.
    pub fn child(&self, parent: NodeId, index: usize) -> NodeId {
        let guard = self.inner.borrow();
        guard.arena[parent.key()].children[index]
    }

    /// Get the parent of a node, or `None` if it is a root.
    pub fn parent(&self, node: NodeId) -> Option<NodeId> {
        let guard = self.inner.borrow();
        guard.arena.get(node.key()).and_then(|n| n.parent)
    }

    /// Compute the depth of a node (0 for roots).
    pub fn depth(&self, node: NodeId) -> usize {
        let guard = self.inner.borrow();
        let mut depth = 0;
        let mut current = guard.arena.get(node.key()).and_then(|n| n.parent);
        while let Some(pid) = current {
            depth += 1;
            current = guard.arena.get(pid.key()).and_then(|n| n.parent);
        }
        depth
    }

    /// Whether the given node has any children.
    pub fn has_children(&self, node: NodeId) -> bool {
        self.child_count(node) > 0
    }

    /// Get the children of a node as a vector of `NodeId`.
    pub fn children(&self, node: NodeId) -> Vec<NodeId> {
        let guard = self.inner.borrow();
        guard
            .arena
            .get(node.key())
            .map(|n| n.children.clone())
            .unwrap_or_default()
    }

    /// Access a node's data via a callback. Returns `None` if the node doesn't exist.
    pub fn with_item<R>(&self, node: NodeId, f: impl FnOnce(&T) -> R) -> Option<R> {
        let guard = self.inner.borrow();
        guard.arena.get(node.key()).map(|n| f(&n.data))
    }

    /// Find the first node matching a predicate (depth-first from roots).
    pub fn find_by(&self, predicate: impl Fn(&T) -> bool) -> Option<NodeId> {
        let guard = self.inner.borrow();
        let mut stack: Vec<NodeId> = guard.roots.iter().rev().copied().collect();
        while let Some(nid) = stack.pop() {
            if let Some(node) = guard.arena.get(nid.key()) {
                if predicate(&node.data) {
                    return Some(nid);
                }
                for &child_id in node.children.iter().rev() {
                    stack.push(child_id);
                }
            }
        }
        None
    }

    // --- Mutations ---

    /// Insert a new root-level node at the given index.
    ///
    /// # Panics
    /// Panics if `index > root_count()`.
    pub fn insert_root(&self, index: usize, item: T) -> NodeId {
        let node_id = {
            let mut guard = self.inner.borrow_mut();
            let key = guard.arena.insert(TreeNode {
                data: item,
                parent: None,
                children: Vec::new(),
            });
            let node_id = NodeId::from_key(key);
            guard.roots.insert(index, node_id);
            node_id
        };
        self.notify(TreeChange::NodeInserted {
            parent: None,
            index,
            node: node_id,
        });
        node_id
    }

    /// Insert a new child node under the given parent at the given index.
    ///
    /// # Panics
    /// Panics if the parent is invalid or `index > child_count(parent)`.
    pub fn insert_child(&self, parent: NodeId, index: usize, item: T) -> NodeId {
        let node_id = {
            let mut guard = self.inner.borrow_mut();
            let key = guard.arena.insert(TreeNode {
                data: item,
                parent: Some(parent),
                children: Vec::new(),
            });
            let node_id = NodeId::from_key(key);
            guard.arena[parent.key()].children.insert(index, node_id);
            node_id
        };
        self.notify(TreeChange::NodeInserted {
            parent: Some(parent),
            index,
            node: node_id,
        });
        node_id
    }

    /// Remove a node and its entire subtree.
    ///
    /// # Panics
    /// Panics if the node is invalid.
    pub fn remove(&self, node: NodeId) {
        let parent = {
            let mut guard = self.inner.borrow_mut();
            let parent = guard.arena[node.key()].parent;
            // Remove from parent's children list or from roots
            if let Some(pid) = parent {
                guard.arena[pid.key()].children.retain(|&c| c != node);
            } else {
                guard.roots.retain(|&r| r != node);
            }
            // Recursively remove subtree from arena
            Self::remove_subtree(&mut guard.arena, node);
            parent
        };
        self.notify(TreeChange::NodeRemoved { parent, node });
    }

    /// Move a node (and its subtree) to a new parent at the given index.
    ///
    /// # Panics
    /// Panics if any of the nodes are invalid, or if the target is a
    /// descendant of the source (would create a cycle).
    pub fn move_node(&self, node: NodeId, new_parent: NodeId, new_index: usize) {
        let old_parent = {
            let mut guard = self.inner.borrow_mut();
            // Ensure we're not creating a cycle
            assert!(
                !Self::is_descendant_of(&guard.arena, new_parent, node),
                "cannot move a node into its own subtree"
            );

            let old_parent = guard.arena[node.key()].parent;

            // Remove from old parent
            if let Some(pid) = old_parent {
                guard.arena[pid.key()].children.retain(|&c| c != node);
            } else {
                guard.roots.retain(|&r| r != node);
            }

            // Insert into new parent
            guard.arena[node.key()].parent = Some(new_parent);
            guard.arena[new_parent.key()]
                .children
                .insert(new_index, node);

            old_parent
        };
        self.notify(TreeChange::NodeMoved {
            node,
            old_parent,
            new_parent: Some(new_parent),
            new_index,
        });
    }

    /// Move a node to the root level at the given index.
    pub fn move_to_root(&self, node: NodeId, new_index: usize) {
        let old_parent = {
            let mut guard = self.inner.borrow_mut();
            let old_parent = guard.arena[node.key()].parent;

            // Remove from old parent
            if let Some(pid) = old_parent {
                guard.arena[pid.key()].children.retain(|&c| c != node);
            } else {
                guard.roots.retain(|&r| r != node);
            }

            // Insert into roots
            guard.arena[node.key()].parent = None;
            guard.roots.insert(new_index, node);

            old_parent
        };
        self.notify(TreeChange::NodeMoved {
            node,
            old_parent,
            new_parent: None,
            new_index,
        });
    }

    /// Update a node's data in place.
    ///
    /// # Panics
    /// Panics if the node is invalid.
    pub fn update(&self, node: NodeId, item: T) {
        {
            let mut guard = self.inner.borrow_mut();
            guard.arena[node.key()].data = item;
        }
        self.notify(TreeChange::NodeUpdated { node });
    }

    // --- Observation ---

    /// Register an observer for tree change notifications.
    /// Returns an `ObserverHandle` — dropping it removes the callback.
    pub fn observe_changes(&self, f: impl Fn(&TreeChange) + 'static) -> ObserverHandle {
        let mut guard = self.inner.borrow_mut();
        let id = guard.next_observer_id;
        guard.next_observer_id += 1;
        guard.observers.push(TreeObserverEntry {
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

    // --- Internal helpers ---

    fn notify(&self, change: TreeChange) {
        let callbacks: Vec<Rc<dyn Fn(&TreeChange)>> = self
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

    /// Explicit-stack walk: collect every id in the subtree first (reading
    /// `.children` before anything is removed), then free them all. Removal
    /// order doesn't matter — freeing a slot doesn't touch any other
    /// entry's `children` list — so this stays depth-bounded by the
    /// subtree's node count rather than the call stack.
    fn remove_subtree(arena: &mut SlotMap<slotmap::DefaultKey, TreeNode<T>>, node: NodeId) {
        let mut stack = vec![node];
        let mut to_remove = Vec::new();
        while let Some(current) = stack.pop() {
            if let Some(n) = arena.get(current.key()) {
                stack.extend(n.children.iter().copied());
            }
            to_remove.push(current);
        }
        for id in to_remove {
            arena.remove(id.key());
        }
    }

    fn is_descendant_of(
        arena: &SlotMap<slotmap::DefaultKey, TreeNode<T>>,
        candidate: NodeId,
        ancestor: NodeId,
    ) -> bool {
        let mut current = Some(candidate);
        while let Some(nid) = current {
            if nid == ancestor {
                return true;
            }
            current = arena.get(nid.key()).and_then(|n| n.parent);
        }
        false
    }
}

impl<T: std::fmt::Debug + 'static> TreeModel<T> {
    /// Register this tree with the debug inspector under `name`. In
    /// release builds (`!cfg(debug_assertions)`) this is a no-op
    /// pass-through so call sites stay free of `#[cfg]` lines.
    ///
    /// Idempotent on repeated calls — the latest registration wins.
    /// The registration drops automatically when the last `TreeModel`
    /// handle is freed (the adapter the registry holds is `Weak`).
    pub fn debug_named(self, _name: impl Into<String>) -> Self {
        #[cfg(debug_assertions)]
        {
            let weak = Rc::downgrade(&self.inner);
            let adapter: Rc<dyn crate::debug_registry::ModelDebug> =
                Rc::new(TreeModelDebug::<T> { weak });
            let name = _name.into();
            crate::debug_registry::register(name, Rc::downgrade(&adapter));
            self.inner.borrow_mut().debug_adapter = Some(adapter);
        }
        self
    }
}

#[cfg(debug_assertions)]
struct TreeModelDebug<T> {
    weak: std::rc::Weak<RefCell<TreeModelInner<T>>>,
}

#[cfg(debug_assertions)]
impl<T: std::fmt::Debug + 'static> crate::debug_registry::ModelDebug for TreeModelDebug<T> {
    fn kind(&self) -> &'static str {
        "TreeModel"
    }
    fn len(&self) -> usize {
        self.weak
            .upgrade()
            .map(|inner| inner.borrow().arena.len())
            .unwrap_or(0)
    }
    fn debug_dump(&self, out: &mut dyn std::fmt::Write) {
        let Some(inner) = self.weak.upgrade() else {
            return;
        };
        let guard = inner.borrow();
        // Depth-first walk from each root, indenting by depth.
        let roots: Vec<NodeId> = guard.roots.clone();
        for root in roots {
            dump_subtree(&guard, root, 0, out);
        }
    }
}

#[cfg(debug_assertions)]
fn dump_subtree<T: std::fmt::Debug>(
    guard: &TreeModelInner<T>,
    node: NodeId,
    depth: usize,
    out: &mut dyn std::fmt::Write,
) {
    let Some(n) = guard.arena.get(node.key()) else {
        return;
    };
    let _ = writeln!(out, "{:indent$}{:?}", "", n.data, indent = depth * 2);
    let children = n.children.clone();
    for child in children {
        dump_subtree(guard, child, depth + 1, out);
    }
}

impl<T: 'static> Default for TreeModel<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: 'static> Clone for TreeModel<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T: std::fmt::Debug + 'static> std::fmt::Debug for TreeModel<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = self.inner.borrow();
        f.debug_struct("TreeModel")
            .field("root_count", &guard.roots.len())
            .field("total_nodes", &guard.arena.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn sample_tree() -> (TreeModel<&'static str>, NodeId, NodeId, NodeId, NodeId) {
        let tree = TreeModel::new();
        let a = tree.insert_root(0, "A");
        let b = tree.insert_root(1, "B");
        let a1 = tree.insert_child(a, 0, "A1");
        let a2 = tree.insert_child(a, 1, "A2");
        (tree, a, b, a1, a2)
    }

    #[test]
    fn empty_tree() {
        let tree: TreeModel<i32> = TreeModel::new();
        assert_eq!(tree.root_count(), 0);
    }

    #[test]
    fn insert_roots() {
        let tree = TreeModel::new();
        let a = tree.insert_root(0, "A");
        let b = tree.insert_root(1, "B");
        assert_eq!(tree.root_count(), 2);
        assert_eq!(tree.root(0), a);
        assert_eq!(tree.root(1), b);
    }

    #[test]
    fn insert_children() {
        let (tree, a, _, a1, a2) = sample_tree();
        assert_eq!(tree.child_count(a), 2);
        assert_eq!(tree.child(a, 0), a1);
        assert_eq!(tree.child(a, 1), a2);
    }

    #[test]
    fn parent_and_depth() {
        let (tree, a, _, a1, _) = sample_tree();
        assert_eq!(tree.parent(a), None);
        assert_eq!(tree.parent(a1), Some(a));
        assert_eq!(tree.depth(a), 0);
        assert_eq!(tree.depth(a1), 1);
    }

    #[test]
    fn has_children_query() {
        let (tree, a, b, _, _) = sample_tree();
        assert!(tree.has_children(a));
        assert!(!tree.has_children(b));
    }

    #[test]
    fn with_item() {
        let (tree, a, _, _, _) = sample_tree();
        assert_eq!(tree.with_item(a, |v| *v), Some("A"));
    }

    #[test]
    fn find_by() {
        let (tree, _, _, a1, _) = sample_tree();
        let found = tree.find_by(|v| *v == "A1");
        assert_eq!(found, Some(a1));

        let not_found = tree.find_by(|v| *v == "Z");
        assert_eq!(not_found, None);
    }

    #[test]
    fn insert_root_emits_change() {
        let tree = TreeModel::new();
        let changes: Rc<RefCell<Vec<TreeChange>>> = Rc::new(RefCell::new(Vec::new()));
        let c = changes.clone();
        let _handle = tree.observe_changes(move |change| {
            c.borrow_mut().push(change.clone());
        });

        let a = tree.insert_root(0, "A");
        let log = changes.borrow();
        assert_eq!(
            log[0],
            TreeChange::NodeInserted {
                parent: None,
                index: 0,
                node: a
            }
        );
    }

    #[test]
    fn insert_child_emits_change() {
        let tree = TreeModel::new();
        let a = tree.insert_root(0, "A");

        let changes: Rc<RefCell<Vec<TreeChange>>> = Rc::new(RefCell::new(Vec::new()));
        let c = changes.clone();
        let _handle = tree.observe_changes(move |change| {
            c.borrow_mut().push(change.clone());
        });

        let a1 = tree.insert_child(a, 0, "A1");
        let log = changes.borrow();
        assert_eq!(log.len(), 1, "insert_child should emit exactly one change");
        assert_eq!(
            log[0],
            TreeChange::NodeInserted {
                parent: Some(a),
                index: 0,
                node: a1
            }
        );
    }

    #[test]
    fn remove_emits_change() {
        let (tree, a, _, a1, _) = sample_tree();
        let changes: Rc<RefCell<Vec<TreeChange>>> = Rc::new(RefCell::new(Vec::new()));
        let c = changes.clone();
        let _handle = tree.observe_changes(move |change| {
            c.borrow_mut().push(change.clone());
        });

        tree.remove(a1);
        assert_eq!(tree.child_count(a), 1);
        let log = changes.borrow();
        assert_eq!(log.len(), 1, "remove should emit exactly one change");
        assert_eq!(
            log[0],
            TreeChange::NodeRemoved {
                parent: Some(a),
                node: a1
            }
        );
    }

    #[test]
    fn remove_subtree() {
        let tree = TreeModel::new();
        let a = tree.insert_root(0, "A");
        let a1 = tree.insert_child(a, 0, "A1");
        let a1a = tree.insert_child(a1, 0, "A1a");

        tree.remove(a1);
        assert_eq!(tree.child_count(a), 0);
        // Both a1 and its descendant a1a should be gone from the arena
        assert_eq!(tree.with_item(a1, |_| ()), None, "a1 should be removed");
        assert_eq!(
            tree.with_item(a1a, |_| ()),
            None,
            "a1a (grandchild) should also be removed"
        );
    }

    #[test]
    fn remove_root() {
        let (tree, a, b, _, _) = sample_tree();
        tree.remove(a);
        assert_eq!(tree.root_count(), 1);
        assert_eq!(tree.root(0), b);
    }

    #[test]
    fn move_node() {
        let (tree, a, b, a1, _) = sample_tree();
        let changes: Rc<RefCell<Vec<TreeChange>>> = Rc::new(RefCell::new(Vec::new()));
        let c = changes.clone();
        let _handle = tree.observe_changes(move |change| {
            c.borrow_mut().push(change.clone());
        });

        tree.move_node(a1, b, 0);

        assert_eq!(tree.child_count(a), 1); // only a2 left
        assert_eq!(tree.child_count(b), 1); // a1 moved here
        assert_eq!(tree.child(b, 0), a1);
        assert_eq!(tree.parent(a1), Some(b));

        let log = changes.borrow();
        assert_eq!(
            log[0],
            TreeChange::NodeMoved {
                node: a1,
                old_parent: Some(a),
                new_parent: Some(b),
                new_index: 0,
            }
        );
    }

    #[test]
    fn move_to_root() {
        let (tree, a, _, a1, _) = sample_tree();
        tree.move_to_root(a1, 0);

        assert_eq!(tree.root_count(), 3); // a1, A, B
        assert_eq!(tree.root(0), a1);
        assert_eq!(tree.parent(a1), None);
        assert_eq!(tree.child_count(a), 1); // only a2 left
    }

    #[test]
    #[should_panic(expected = "cannot move a node into its own subtree")]
    fn move_into_own_subtree_panics() {
        let (tree, a, _, a1, _) = sample_tree();
        tree.move_node(a, a1, 0); // A into A1 would create a cycle
    }

    #[test]
    fn update_emits_change() {
        let (tree, a, _, _, _) = sample_tree();
        let changes: Rc<RefCell<Vec<TreeChange>>> = Rc::new(RefCell::new(Vec::new()));
        let c = changes.clone();
        let _handle = tree.observe_changes(move |change| {
            c.borrow_mut().push(change.clone());
        });

        tree.update(a, "A-updated");
        assert_eq!(tree.with_item(a, |v| *v), Some("A-updated"));
        let log = changes.borrow();
        assert_eq!(log[0], TreeChange::NodeUpdated { node: a });
    }

    #[test]
    fn observer_removed_on_handle_drop() {
        let tree = TreeModel::new();
        let count = Rc::new(Cell::new(0));
        let c = count.clone();
        let handle = tree.observe_changes(move |_| c.set(c.get() + 1));

        tree.insert_root(0, "A");
        assert_eq!(count.get(), 1);

        drop(handle);
        tree.insert_root(1, "B");
        assert_eq!(count.get(), 1); // Not called again
    }

    #[test]
    fn clone_shares_data() {
        let tree = TreeModel::new();
        let a = tree.insert_root(0, "A");

        let clone = tree.clone();
        assert_eq!(clone.root_count(), 1);
        assert_eq!(clone.with_item(a, |v| *v), Some("A"));

        clone.insert_root(1, "B");
        assert_eq!(tree.root_count(), 2);
    }

    #[test]
    fn deep_tree_depth() {
        let tree = TreeModel::new();
        let r = tree.insert_root(0, "r");
        let c1 = tree.insert_child(r, 0, "c1");
        let c2 = tree.insert_child(c1, 0, "c2");
        let c3 = tree.insert_child(c2, 0, "c3");
        assert_eq!(tree.depth(r), 0);
        assert_eq!(tree.depth(c1), 1);
        assert_eq!(tree.depth(c2), 2);
        assert_eq!(tree.depth(c3), 3);
    }

    #[test]
    fn children_returns_correct_ids() {
        let (tree, a, _, a1, a2) = sample_tree();
        let children = tree.children(a);
        assert_eq!(children, vec![a1, a2]);
    }

    /// `remove` walks the whole subtree via `remove_subtree`; a 50,000-deep
    /// single-child chain must not overflow the call stack (it's an
    /// explicit-stack walk, not recursion).
    #[test]
    fn remove_deep_chain_does_not_overflow() {
        const DEPTH: usize = 50_000;
        let tree = TreeModel::new();
        let root = tree.insert_root(0, 0usize);
        let mut leaf = root;
        for i in 1..DEPTH {
            leaf = tree.insert_child(leaf, 0, i);
        }
        tree.remove(root);
        assert_eq!(tree.root_count(), 0);
        assert_eq!(tree.with_item(leaf, |_| ()), None);
    }
}
