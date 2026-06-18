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
