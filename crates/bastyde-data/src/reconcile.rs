//! Incremental keyed reconcile — bring a model in line with a fresh keyed list
//! by emitting the *minimal* structural changes, instead of clearing and
//! rebuilding (which is a [`DataChange::Reset`] / [`TreeChange::Reset`] that
//! discards every consumer's per-item state).
//!
//! This is the missing half of the model/view story for the common case where
//! the **domain owns the data** (a DB, a Qleany entity store, a Kafka feed) and
//! the bastyde model is a *projection* over it — the Qt `QAbstractItemModel`
//! shape rather than the `QStandardItemModel` "model owns the data" shape (see
//! `docs/data-models.md` §7 and §10). Re-reading the whole domain list and
//! `reconcile`-ing it keeps:
//!
//! - **trees:** every surviving node's `NodeId`, so the [`TreeSlice`] expand
//!   set, the `SelectionModel`, and scroll all follow the change (a move stays
//!   a move, not a remove+insert);
//! - **lists:** surviving items' widget subtrees in a `Repeater` (an
//!   `ItemsMoved` reorders without recreation), plus scroll anchoring and the
//!   `first_changed_index()` divergence prefix for row-height caches.
//!
//! ## Tables
//!
//! There is deliberately no table-specific reconcile. A `TableView`'s rows are a
//! [`ListModel`] (or a `SortFilterListModel` over one) — reconcile that with
//! [`reconcile_list`]. A `TreeTableView`'s rows are a [`TreeModel`] — reconcile that
//! with [`reconcile_tree`]. A cell edit is just a value change on the row, which
//! both reconcilers emit as an in-place update (`set` / `update`). Columns are
//! configuration, not data, so they are never reconciled.
//!
//! [`DataChange::Reset`]: crate::DataChange
//! [`TreeChange::Reset`]: crate::TreeChange
//! [`TreeSlice`]: crate::TreeSlice

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use crate::list_model::ListModel;
use crate::tree_change::NodeId;
use crate::tree_model::TreeModel;

// ── trees ────────────────────────────────────────────────────────────

/// `(NodeId, last value)` per key — the persistent map [`reconcile_tree`]
/// maintains between calls. The stored value lets it detect an in-place change
/// without reading the model back.
pub type ReconcileIndex<K, T> = HashMap<K, (NodeId, T)>;

/// Reconcile `tree` to match `rows`, a **pre-order** flat list (`depth == 0` is
/// a root; each row's parent is the nearest earlier row of smaller depth).
/// Updates `index` in place and returns the `NodeId`s of newly-inserted nodes
/// (handy for an "expand freshly-revealed subtrees" view policy).
///
/// Reuses the `NodeId` of every key present in both states, emitting only the
/// `insert` / `remove` / `move_node` / `update` needed. Reparenting that
/// *inverts* an ancestor/descendant pair is not supported (it would trip
/// [`TreeModel::move_node`]'s cycle assertion) — domain edits like move / wrap /
/// unwrap never do this.
pub fn reconcile_tree<K, T>(
    tree: &TreeModel<T>,
    index: &mut ReconcileIndex<K, T>,
    rows: &[(K, usize, T)],
) -> Vec<NodeId>
where
    K: Eq + Hash + Clone,
    T: Clone + PartialEq,
{
    let new_keys: HashSet<&K> = rows.iter().map(|(k, _, _)| k).collect();

    // Phase 1 — place survivors + insert new nodes in pre-order. Moving a
    // survivor to its new parent also pulls it OUT of any vanishing subtree, so
    // Phase 2 only ever deletes genuinely-gone nodes.
    let mut new_nodes = Vec::new();
    let mut stack: Vec<(usize, NodeId)> = Vec::new();
    let mut next_index: HashMap<Option<NodeId>, usize> = HashMap::new();

    for (key, depth, value) in rows {
        while stack.last().is_some_and(|&(d, _)| d >= *depth) {
            stack.pop();
        }
        let parent = stack.last().map(|&(_, n)| n);
        let idx = *next_index.get(&parent).unwrap_or(&0);

        let node = if let Some((existing, old_value)) = index.get(key).cloned() {
            if tree.parent(existing) != parent || tree_child_index(tree, parent, existing) != Some(idx)
            {
                match parent {
                    Some(p) => tree.move_node(existing, p, idx),
                    None => tree.move_to_root(existing, idx),
                }
            }
            if old_value != *value {
                tree.update(existing, value.clone());
            }
            index.insert(key.clone(), (existing, value.clone()));
            existing
        } else {
            let node = match parent {
                Some(p) => tree.insert_child(p, idx, value.clone()),
                None => tree.insert_root(idx, value.clone()),
            };
            index.insert(key.clone(), (node, value.clone()));
            new_nodes.push(node);
            node
        };

        *next_index.entry(parent).or_insert(0) += 1;
        stack.push((*depth, node));
    }

    // Phase 2 — remove vanished nodes. Compute the removal roots (no vanished
    // ancestor) against the *intact* tree first; removing during the walk would
    // invalidate the ids of vanished nodes in subtrees already deleted. The
    // roots are pairwise non-ancestor, so removing them afterwards is safe.
    let vanished: Vec<(K, NodeId)> = index
        .iter()
        .filter(|(k, _)| !new_keys.contains(*k))
        .map(|(k, (n, _))| (k.clone(), *n))
        .collect();
    let vanished_ids: HashSet<NodeId> = vanished.iter().map(|(_, n)| *n).collect();
    let removal_roots: Vec<NodeId> = vanished
        .iter()
        .filter(|(_, node)| {
            let mut ancestor = tree.parent(*node);
            while let Some(p) = ancestor {
                if vanished_ids.contains(&p) {
                    return false;
                }
                ancestor = tree.parent(p);
            }
            true
        })
        .map(|(_, node)| *node)
        .collect();
    for node in removal_roots {
        tree.remove(node);
    }
    for (key, _) in vanished {
        index.remove(&key);
    }

    new_nodes
}

fn tree_child_index<T>(tree: &TreeModel<T>, parent: Option<NodeId>, node: NodeId) -> Option<usize> {
    match parent {
        Some(p) => tree.children(p).iter().position(|&c| c == node),
        None => (0..tree.root_count()).find(|&i| tree.root(i) == node),
    }
}

// ── lists ────────────────────────────────────────────────────────────

/// Reconcile `list` to match `rows`, keyed by `key_of`. Emits only the
/// `insert` / `remove` / `move_item` / `set` needed; surviving items keep their
/// position-relative identity, so a `Repeater` reorders subtrees instead of
/// recreating them. Front-to-back placement — `O(n²)` position lookups, fine for
/// the bounded collections lists are used for; large/virtualized sources that
/// don't need this can keep emitting `Reset`.
pub fn reconcile_list<K, T>(list: &ListModel<T>, rows: &[(K, T)], key_of: impl Fn(&T) -> K)
where
    K: Eq + Hash + Clone,
    T: Clone + PartialEq,
{
    // A local mirror of the model's current key order — lets us find positions
    // without re-reading the model, and stays in lock-step with every op below.
    let mut order: Vec<K> = (0..list.len())
        .filter_map(|i| list.with_item(i, &key_of))
        .collect();
    let new_keys: HashSet<&K> = rows.iter().map(|(k, _)| k).collect();

    // Remove vanished, back-to-front so indices stay valid.
    for i in (0..order.len()).rev() {
        if !new_keys.contains(&order[i]) {
            list.remove(i);
            order.remove(i);
        }
    }

    // Place each row front-to-back at its target index.
    for (j, (key, value)) in rows.iter().enumerate() {
        if let Some(pos) = order.iter().position(|k| k == key) {
            let changed = list.with_item(pos, |old| old != value).unwrap_or(true);
            if pos != j {
                list.move_item(pos, j);
                let k = order.remove(pos);
                order.insert(j, k);
            }
            if changed {
                list.set(j, value.clone());
            }
        } else {
            list.insert(j, value.clone());
            order.insert(j, key.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_change::DataChange;
    use crate::tree_change::TreeChange;
    use std::cell::RefCell;
    use std::rc::Rc;

    // ── tree ──────────────────────────────────────────────────────────

    fn build_tree(rows: &[(i32, usize, &'static str)]) -> (TreeModel<&'static str>, ReconcileIndex<i32, &'static str>) {
        let tree = TreeModel::new();
        let mut index = ReconcileIndex::new();
        reconcile_tree(&tree, &mut index, rows);
        (tree, index)
    }

    fn flat_tree(tree: &TreeModel<&'static str>) -> Vec<(&'static str, usize)> {
        fn go(tree: &TreeModel<&'static str>, node: NodeId, depth: usize, out: &mut Vec<(&'static str, usize)>) {
            out.push((tree.with_item(node, |v| *v).unwrap(), depth));
            for c in tree.children(node) {
                go(tree, c, depth + 1, out);
            }
        }
        let mut out = Vec::new();
        for i in 0..tree.root_count() {
            go(tree, tree.root(i), 0, &mut out);
        }
        out
    }

    #[test]
    fn tree_initial_build() {
        let (tree, _) = build_tree(&[(1, 0, "V"), (2, 1, "A"), (3, 1, "Card"), (4, 2, "T")]);
        assert_eq!(flat_tree(&tree), vec![("V", 0), ("A", 1), ("Card", 1), ("T", 2)]);
    }

    #[test]
    fn tree_reorder_preserves_ids() {
        let (tree, mut index) = build_tree(&[(1, 0, "V"), (2, 1, "A"), (3, 1, "B"), (4, 1, "C")]);
        let ids = (index[&2].0, index[&3].0, index[&4].0);
        reconcile_tree(&tree, &mut index, &[(1, 0, "V"), (4, 1, "C"), (2, 1, "A"), (3, 1, "B")]);
        assert_eq!(flat_tree(&tree), vec![("V", 0), ("C", 1), ("A", 1), ("B", 1)]);
        assert_eq!((index[&2].0, index[&3].0, index[&4].0), ids);
    }

    #[test]
    fn tree_subtree_removal() {
        let (tree, mut index) =
            build_tree(&[(1, 0, "V"), (2, 1, "Card"), (3, 2, "T1"), (4, 2, "T2"), (5, 1, "B")]);
        reconcile_tree(&tree, &mut index, &[(1, 0, "V"), (5, 1, "B")]);
        assert_eq!(flat_tree(&tree), vec![("V", 0), ("B", 1)]);
        for gone in [2, 3, 4] {
            assert!(!index.contains_key(&gone));
        }
    }

    #[test]
    fn tree_wrap_and_unwrap_keep_ids() {
        // wrap B in Card
        let (tree, mut index) = build_tree(&[(1, 0, "V"), (2, 1, "A"), (3, 1, "B")]);
        let b = index[&3].0;
        reconcile_tree(&tree, &mut index, &[(1, 0, "V"), (2, 1, "A"), (99, 1, "Card"), (3, 2, "B")]);
        assert_eq!(index[&3].0, b);
        assert_eq!(tree.parent(b), Some(index[&99].0));
        // unwrap it again
        reconcile_tree(&tree, &mut index, &[(1, 0, "V"), (2, 1, "A"), (3, 1, "B")]);
        assert_eq!(index[&3].0, b, "B keeps its id across wrap+unwrap");
        assert!(!index.contains_key(&99));
    }

    #[test]
    fn tree_value_change_is_in_place() {
        let (tree, mut index) = build_tree(&[(1, 0, "V"), (2, 1, "A")]);
        let a = index[&2].0;
        let log: Rc<RefCell<Vec<TreeChange>>> = Rc::new(RefCell::new(Vec::new()));
        let l = log.clone();
        let _h = tree.observe_changes(move |c| l.borrow_mut().push(c.clone()));
        reconcile_tree(&tree, &mut index, &[(1, 0, "V"), (2, 1, "A!")]);
        assert_eq!(tree.with_item(a, |v| *v), Some("A!"));
        assert_eq!(*log.borrow(), vec![TreeChange::NodeUpdated { node: a }]);
    }

    // ── list ──────────────────────────────────────────────────────────

    fn list_keys(list: &ListModel<&'static str>) -> Vec<&'static str> {
        (0..list.len()).filter_map(|i| list.with_item(i, |v| *v)).collect()
    }

    #[test]
    fn list_insert_remove_reorder() {
        let list: ListModel<&str> = ListModel::new();
        let id = |s: &&'static str| *s;
        reconcile_list(&list, &[("A", "A"), ("B", "B"), ("C", "C")], id);
        assert_eq!(list_keys(&list), vec!["A", "B", "C"]);

        // remove B, insert X first, reorder to [X, C, A]
        reconcile_list(&list, &[("X", "X"), ("C", "C"), ("A", "A")], id);
        assert_eq!(list_keys(&list), vec!["X", "C", "A"]);
    }

    #[test]
    fn list_minimal_changes_not_reset() {
        // Keyed by a stable id; the label is the value (so we can see updates).
        let list: ListModel<(i32, &'static str)> = ListModel::new();
        let id = |v: &(i32, &'static str)| v.0;
        reconcile_list(&list, &[(1, (1, "a")), (2, (2, "b"))], id);

        let log: Rc<RefCell<Vec<DataChange>>> = Rc::new(RefCell::new(Vec::new()));
        let l = log.clone();
        let _h = list.observe_changes(move |c| l.borrow_mut().push(c.clone()));

        // Relabel item 2 only — must be a single ItemUpdated, never a Reset.
        reconcile_list(&list, &[(1, (1, "a")), (2, (2, "B!"))], id);
        assert_eq!(list.with_item(1, |v| v.1), Some("B!"));
        assert_eq!(*log.borrow(), vec![DataChange::ItemUpdated { index: 1 }]);
    }
}
