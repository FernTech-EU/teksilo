//! Change notifications for flat collections.

use std::ops::Range;

/// Describes a mutation to a flat list. Emitted by `ListModel<T>` automatically
/// and by `ListDataSource` implementors manually.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataChange {
    /// Items were inserted at the given range.
    /// The range represents the indices of the newly inserted items.
    ItemsInserted { range: Range<usize> },

    /// Items were removed from the given range.
    /// The range represents the indices the items occupied before removal.
    ItemsRemoved { range: Range<usize> },

    /// Items were moved within the list.
    ItemsMoved {
        from: usize,
        to: usize,
        count: usize,
    },

    /// A single item was updated in place.
    ItemUpdated { index: usize },

    /// The entire list was replaced. Consumers should discard all state and rebuild.
    Reset,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_change_debug() {
        let change = DataChange::ItemsInserted { range: 0..3 };
        assert!(format!("{:?}", change).contains("ItemsInserted"));
    }

    #[test]
    fn data_change_eq() {
        let a = DataChange::ItemsMoved {
            from: 2,
            to: 5,
            count: 1,
        };
        let b = DataChange::ItemsMoved {
            from: 2,
            to: 5,
            count: 1,
        };
        assert_eq!(a, b);
    }
}
