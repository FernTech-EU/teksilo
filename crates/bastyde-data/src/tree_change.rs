//! Change notifications and node identifiers for tree collections.

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

    #[test]
    fn tree_change_debug() {
        let change = TreeChange::NodeInserted {
            parent: None,
            index: 0,
            node: NodeId::from_key(slotmap::KeyData::from_ffi(1).into()),
        };
        assert!(format!("{:?}", change).contains("NodeInserted"));
    }
}
