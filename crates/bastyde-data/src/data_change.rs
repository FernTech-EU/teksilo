// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `DataChange` — change notifications for flat collections.
//!
//! Describes the mutations that [`crate::ListModel`] (and [`crate::ListDataSource`]
//! implementors) emit to their subscribers. Consumers such as `ListView`,
//! `TableView`, and `SortFilterListModel` receive a `DataChange` through their
//! observer and update their internal state (measured row heights, selection
//! indices, sort projections) incrementally rather than rebuilding from scratch.
//!
//! Most variants carry index ranges so that observers can perform O(affected)
//! work. `Reset` is the fallback when the change cannot be expressed
//! incrementally; consumers must discard all cached state and re-query the source.
//!
//! Also provided: [`map_index_after_move`], a pure function that maps a single
//! index through an `ItemsMoved` operation — used by [`crate::CheckedModel`] and
//! [`crate::SelectionModel`] to keep index-based state in sync after reorders.
//!
//! ```rust
//! # use bastyde_data::data_change::{DataChange, map_index_after_move};
//! // An insertion at row 2 shifts index 5 to 6.
//! let change = DataChange::ItemsInserted { range: 2..3 };
//! // map_index_after_move: move row 0 to position 2 (post-removal index).
//! let new_idx = map_index_after_move(0, 0, 2, 1);
//! assert_eq!(new_idx, 2);
//! ```

use std::ops::Range;

/// Describes a mutation to a flat list. Emitted by [`crate::ListModel`] automatically
/// and by [`crate::ListDataSource`] implementors manually.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataChange {
    /// Rows were inserted; `range` holds the indices of the newly inserted items.
    ItemsInserted { range: Range<usize> },

    /// Rows were removed; `range` holds the indices they occupied *before* removal.
    ItemsRemoved { range: Range<usize> },

    /// A contiguous block of `count` rows moved from `from` to `to` (post-removal index).
    ItemsMoved {
        from: usize,
        to: usize,
        count: usize,
    },

    /// A single row's data changed in place without any structural shift.
    ItemUpdated { index: usize },

    /// A window of previously-`Loading` rows became `Ready` (lazy / windowed
    /// sources). Semantically like `ItemsInserted` for a row-height cache
    /// (divergence = `range.start`), but no rows were added — the count was
    /// already declared — so a `SelectionModel` must NOT index-shift for it.
    WindowLoaded { range: Range<usize> },

    /// The entire list was replaced; consumers must discard all cached state and rebuild.
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
    let after_remove = if idx >= from + count {
        idx - count
    } else {
        idx
    };
    if after_remove >= to {
        after_remove + count
    } else {
        after_remove
    }
}
