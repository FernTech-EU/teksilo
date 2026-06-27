// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Read-and-command interface for a flat collection behind a `ListView` /
//! `TableView`.
//!
//! `ListDataSource` is the flat-list peer of
//! [`TreeDataSource`](crate::TreeDataSource): a positional read API plus the
//! capability protocol (identity, DnD validation, lazy loading). It is the
//! input every flat data view reads through. The built-in `ListModel<T>` and
//! `SortFilterListModel<T>` implement it; an external/huge source (a paged
//! database cursor, a 1M-row windowed feed) implements it directly and owns its
//! own paging behind `row_state`/`request_window`/`fetch_more`.
//!
//! Not object-safe (associated types + generic `with_item`); `ListView`
//! consumes it generically via `ListView::from_source` and erases it into a
//! closure bundle. The DnD and lazy methods default to inert / fully-resident,
//! so a read-only in-memory source implements only `len` + `with_item` +
//! `observe_changes`.

use std::ops::Range;

use bastyde_core::ObserverHandle;

use crate::data_change::DataChange;
use crate::dnd_types::{DragEligibility, DropCommit, DropQuery, DropResponse, ItemKey, RowState};

/// A data source for a flat collection.
pub trait ListDataSource: 'static {
    /// The item type exposed by this data source.
    type Item: 'static;
    /// The stable per-row identity. In-memory `ListModel` uses `usize` (the
    /// index); external sources use their own domain key so keyed selection /
    /// DnD survive reorders without a mirror model.
    type Key: ItemKey;

    /// Number of rows (the **total**, including not-yet-loaded ones for a
    /// windowed source — the scrollbar needs it).
    fn len(&self) -> usize;

    /// Whether the source is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Access the item at `index` via a callback. Returns `None` for an
    /// out-of-bounds index OR an in-bounds index whose data is still
    /// `Loading` (see `row_state`).
    fn with_item<R>(&self, index: usize, f: impl FnOnce(&Self::Item) -> R) -> Option<R>;

    /// The stable key of the row at `index`. Default `None` (no identity);
    /// sources that support keyed selection / DnD override it.
    fn key_at(&self, _index: usize) -> Option<Self::Key> {
        None
    }

    /// The index of a key, if currently present. Default `None`.
    fn index_of(&self, _key: &Self::Key) -> Option<usize> {
        None
    }

    /// Register an observer for change notifications.
    fn observe_changes(&self, f: impl Fn(&DataChange) + 'static) -> ObserverHandle;

    /// First index whose content may differ after the change just delivered —
    /// rows `0..index` are unchanged. `None` means unknown (full change).
    fn first_changed_index(&self) -> Option<usize> {
        None
    }

    // ── DnD (default: inert) ──────────────────────────────────────────────
    /// Whether the row may begin a drag (the transferable gate).
    fn drag(&self, _key: &Self::Key) -> DragEligibility {
        DragEligibility::NoDrag
    }
    /// Whether a hovered drop is permitted (and where) — the pre-commit verdict.
    fn can_accept(&self, _query: &DropQuery<'_, Self::Key>) -> DropResponse {
        DropResponse::Reject
    }
    /// Apply a committed drop. Returns whether it was applied.
    fn accept_drop(&self, _commit: DropCommit<'_, Self::Key>) -> bool {
        false
    }
    /// Called on the *origin* source after one of its rows was accepted by a
    /// different view (source-side completion). Shared/command-backed sources
    /// no-op this; independent models use it to drop the moved row.
    fn on_drag_out(&self, _key: &Self::Key) {}

    // ── Lazy (default: fully resident) ────────────────────────────────────
    /// Whether the row at `index` is loaded.
    fn row_state(&self, _index: usize) -> RowState {
        RowState::Ready
    }
    /// Nudge the source to load the given range (the view calls this each build
    /// with its visible + buffer window).
    fn request_window(&self, _range: Range<usize>) {}
    /// Whether more rows can be appended (infinite scroll).
    fn can_fetch_more(&self) -> bool {
        false
    }
    /// Fetch the next page (append-only growth).
    fn fetch_more(&self) {}
}
