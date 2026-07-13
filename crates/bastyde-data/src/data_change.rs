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

/// Map a **single** index anchor (not a selection set) through a
/// [`DataChange`], or `None` if the row the anchor pointed at no longer
/// exists (it was removed, or the whole list was reset).
///
/// This is the same shift semantics as [`map_index_after_move`] /
/// `SelectionModel::adjust_for_*` / `CheckedModel::adjust_for_*`, specialized
/// for a bare `Option<usize>` anchor that has no "membership" to prune —
/// e.g. `ListView`'s keyboard-focus index. Used so a single-anchor consumer
/// doesn't have to re-derive insert/remove/move shift logic by hand.
///
/// - `ItemsInserted`: the anchor shifts up by the inserted count if it sat
///   at or after the insertion point, otherwise it's untouched.
/// - `ItemsRemoved`: the anchor shifts down past the removed range; if the
///   anchor itself pointed *into* the removed range, it is dropped (`None`)
///   — the row it followed is gone.
/// - `ItemsMoved`: delegates to [`map_index_after_move`] (the anchor follows
///   its row, or shifts around the moved block like everyone else).
/// - `ItemUpdated` / `WindowLoaded`: no structural shift — the anchor is
///   unchanged.
/// - `Reset`: the anchor is dropped (`None`) — nothing about the old
///   indexing survives a wholesale replacement.
pub fn adjust_single_index_for_change(idx: usize, change: &DataChange) -> Option<usize> {
    match change {
        DataChange::ItemsInserted { range } => Some(if idx >= range.start {
            idx + (range.end - range.start)
        } else {
            idx
        }),
        DataChange::ItemsRemoved { range } => {
            if idx < range.start {
                Some(idx)
            } else if idx >= range.end {
                Some(idx - (range.end - range.start))
            } else {
                None
            }
        }
        DataChange::ItemsMoved { from, to, count } => {
            Some(map_index_after_move(idx, *from, *to, *count))
        }
        DataChange::ItemUpdated { .. } | DataChange::WindowLoaded { .. } => Some(idx),
        DataChange::Reset => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjust_single_index_inserted_shifts_at_or_after_start() {
        let change = DataChange::ItemsInserted { range: 2..4 };
        assert_eq!(adjust_single_index_for_change(0, &change), Some(0));
        assert_eq!(adjust_single_index_for_change(1, &change), Some(1));
        assert_eq!(adjust_single_index_for_change(2, &change), Some(4));
        assert_eq!(adjust_single_index_for_change(5, &change), Some(7));
    }

    #[test]
    fn adjust_single_index_removed_drops_within_range_shifts_after() {
        let change = DataChange::ItemsRemoved { range: 2..4 };
        assert_eq!(adjust_single_index_for_change(0, &change), Some(0));
        assert_eq!(adjust_single_index_for_change(1, &change), Some(1));
        assert_eq!(adjust_single_index_for_change(2, &change), None);
        assert_eq!(adjust_single_index_for_change(3, &change), None);
        assert_eq!(adjust_single_index_for_change(4, &change), Some(2));
        assert_eq!(adjust_single_index_for_change(10, &change), Some(8));
    }

    #[test]
    fn adjust_single_index_moved_delegates_to_map_index_after_move() {
        let change = DataChange::ItemsMoved {
            from: 1,
            to: 4,
            count: 2,
        };
        for idx in 0..8 {
            assert_eq!(
                adjust_single_index_for_change(idx, &change),
                Some(map_index_after_move(idx, 1, 4, 2))
            );
        }
    }

    #[test]
    fn adjust_single_index_updated_and_window_loaded_are_no_shift() {
        assert_eq!(
            adjust_single_index_for_change(3, &DataChange::ItemUpdated { index: 3 }),
            Some(3)
        );
        assert_eq!(
            adjust_single_index_for_change(3, &DataChange::WindowLoaded { range: 0..10 }),
            Some(3)
        );
    }

    #[test]
    fn adjust_single_index_reset_always_drops() {
        assert_eq!(adjust_single_index_for_change(0, &DataChange::Reset), None);
        assert_eq!(adjust_single_index_for_change(99, &DataChange::Reset), None);
    }
}
