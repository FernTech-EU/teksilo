// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! TreeChange — change notifications and stable node identifiers for tree collections.
//!
//! [`NodeId`] is an opaque, stable handle for a node in a [`crate::TreeModel`].
//! Because `TreeModel` is backed by a slotmap, `NodeId` values survive arbitrary
//! insertions, removals, and moves — only deleting the node itself invalidates it.
//! [`TreeChange`] describes exactly what mutated in the tree so that projections
//! (`SortFilterTreeModel`, `TreeSlice`) can refresh efficiently and emit
//! fine-grained divergence hints.
//!
//! Consumers typically receive `TreeChange` values through an observer registered
//! via [`crate::TreeModel::observe_changes`], which fires synchronously (before
//! the registering call returns) after each mutation. The projections listed above
//! subscribe internally; app code rarely needs to subscribe directly.
//!
//! ```ignore
//! // TreeModel::observe_changes returns an ObserverHandle whose drop
//! // unregisters the callback — keep it alive for the observer's lifetime.
//! use bastyde_data::{TreeModel, TreeChange};
//! let tree: TreeModel<String> = TreeModel::new();
//! let _handle = tree.observe_changes(|change| {
//!     println!("{change:?}");
//! });
//! tree.insert_root(0, "root".to_string());
//! // prints: NodeInserted { parent: None, index: 0, node: NodeId(...) }
//! ```

/// Opaque identifier for a node in a `TreeModel`.
///
/// `NodeId` values are stable across mutations — inserting or removing other
/// nodes does not invalidate existing `NodeId` handles (they are SlotMap keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(slotmap::DefaultKey);

impl NodeId {
    pub(crate) fn from_key(key: slotmap::DefaultKey) -> Self {
        Self(key)
    }

    pub(crate) fn key(self) -> slotmap::DefaultKey {
        self.0
    }
}

/// Describes a mutation to a tree structure. Emitted by `TreeModel<T>` automatically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeChange {
    /// A node was inserted as a child of `parent` at the given index.
    /// `parent` is `None` for root-level insertions.
    NodeInserted {
        parent: Option<NodeId>,
        index: usize,
        node: NodeId,
    },

    /// A node (and its entire subtree) was removed.
    /// `parent` is `None` if it was a root-level node.
    NodeRemoved {
        parent: Option<NodeId>,
        node: NodeId,
    },

    /// A node was moved to a new parent at the given index.
    NodeMoved {
        node: NodeId,
        old_parent: Option<NodeId>,
        new_parent: Option<NodeId>,
        new_index: usize,
    },

    /// A node's data was updated in place.
    NodeUpdated { node: NodeId },

    /// The entire tree was replaced. Consumers should discard all state and rebuild.
    Reset,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_id_equality() {
        use slotmap::SlotMap;
        let mut sm: SlotMap<slotmap::DefaultKey, ()> = SlotMap::new();
        let k1 = sm.insert(());
        let k2 = sm.insert(());
        let id1 = NodeId::from_key(k1);
        let id1_clone = NodeId::from_key(k1);
        let id2 = NodeId::from_key(k2);

        assert_eq!(id1, id1_clone);
        assert_ne!(id1, id2);
    }
}
