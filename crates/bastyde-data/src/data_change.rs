// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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

    /// A window of previously-`Loading` rows became `Ready` (lazy / windowed
    /// sources). Semantically like `ItemsInserted` for a row-height cache
    /// (divergence = `range.start`), but no rows were added — the count was
    /// already declared — so a `SelectionModel` must NOT index-shift for it.
    WindowLoaded { range: Range<usize> },

    /// The entire list was replaced. Consumers should discard all state and rebuild.
    Reset,
}

/// Map an index through a `DataChange::ItemsMoved { from, to, count }`.
///
/// Mirrors `ListModel::move_item`: the contiguous block `from..from+count` is
/// removed, then reinserted so its first item lands at `to` (a *post-removal*
/// index). Returns where `idx` ends up after the move. Used by index-based
/// state (selection, checked-set) to follow items across a reorder.
pub fn map_index_after_move(idx: usize, from: usize, to: usize, count: usize) -> usize {
    // Items inside the moved block travel with it, preserving their offset.
    if idx >= from && idx < from + count {
        return to + (idx - from);
    }
    // Everyone else: apply the removal of the block, then its reinsertion.
    let after_remove = if idx >= from + count { idx - count } else { idx };
    if after_remove >= to {
        after_remove + count
    } else {
        after_remove
    }
}
