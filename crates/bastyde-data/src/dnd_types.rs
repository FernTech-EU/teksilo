// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared capability types for the data-source drag-and-drop + lazy protocol.
//!
//! These types are the Bastyde-shaped equivalent of Qt's
//! `flags`/`canDropMimeData`/`dropMimeData` (DnD validation) and
//! `canFetchMore`/`fetchMore` (lazy loading), expressed as defaulted methods on
//! [`ListDataSource`](crate::ListDataSource) and
//! [`TreeDataSource`](crate::TreeDataSource). A source *owns* the answer to
//! "may this drop happen?" (`can_accept`) and "apply the move" (`accept_drop`);
//! the view merely renders the source's verdict and routes the commit. This is
//! what lets an external source of truth (e.g. a Qleany entity store) drive a
//! view without the view ever mutating a mirror model.
//!
//! ## Key types
//!
//! - [`ItemKey`] — blanket identity trait for any `Clone + Eq + Hash + Debug + 'static` type.
//! - [`RowState`] — whether a lazy row's data is resident (`Ready`) or still loading (`Loading`).
//! - [`DragEligibility`] — per-row drag gate returned by `ListDataSource::drag`.
//! - [`DropPosition`] — where a drop lands relative to the target row.
//! - [`DragSource`] — who is dragging: the same view (intra-view reorder) or a foreign view/OS drop.
//! - [`DropQuery`] / [`DropResponse`] — hover-time can-I-drop? query and verdict.
//! - [`DropCommit`] — the committed drop handed to `accept_drop`.
//!
//! ```ignore
//! // Example: implementing can_accept for a custom ListDataSource
//! fn can_accept(&self, query: &bastyde_data::DropQuery<'_, usize>) -> bastyde_data::DropResponse {
//!     match &query.source {
//!         bastyde_data::DragSource::SameView { .. } => bastyde_data::DropResponse::Accept,
//!         bastyde_data::DragSource::Foreign { .. } => bastyde_data::DropResponse::Reject,
//!     }
//! }
//! ```

use bastyde_core::DragPayload;

/// A stable, hashable identity for a row/node. Blanket-implemented for every
/// `Clone + Eq + Hash + Debug + 'static` type, so `usize`, `NodeId`, `i64`,
/// `String`, `Uuid`, … all qualify with no extra work.
///
/// In-memory models use positional keys (`usize` for `ListModel`, `NodeId` for
/// `TreeModel`); external sources use their own domain key (an entity id), which
/// is exactly what removes the need to mirror them into a built-in model.
pub trait ItemKey: Clone + Eq + std::hash::Hash + std::fmt::Debug + 'static {}
impl<T: Clone + Eq + std::hash::Hash + std::fmt::Debug + 'static> ItemKey for T {}

/// Whether a realized row's data is resident yet. A windowed/lazy source returns
/// `Loading` for indices outside its resident window; the view renders a
/// placeholder skeleton for those and calls `request_window` to pull them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    /// Item data is resident; `with_item`/`with_entry` returns `Some`.
    Ready,
    /// The row exists (counts against `len`/`visible_count`) but its data is not
    /// yet loaded; `with_item`/`with_entry` returns `None`.
    Loading,
}

/// Where, relative to a target row, a drop lands. `Into` (reparent) is only
/// meaningful for trees; flat lists reject it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropPosition {
    /// Immediately before the target (sibling, same level).
    Before,
    /// As a child of the target (reparent — trees only).
    Into,
    /// Immediately after the target (sibling, same level).
    After,
}

/// Whether a row may begin a drag at all (the per-item transferable gate, Qt's
/// `Qt::ItemIsDragEnabled` / `TabBar`'s `with_transferable_predicate`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragEligibility {
    /// The row can be dragged.
    CanDrag,
    /// The row cannot be dragged (the gesture is suppressed).
    NoDrag,
}

/// Who is dragging, from the receiving source's point of view.
///
/// `SameView` is an intra-view reorder identified by the dragged row's key.
/// `Foreign` is everything else — an in-app drag from *another* view or an OS
/// drop — carried as a type-erased [`DragPayload`] the source downcasts itself
/// (e.g. a designer source downcasts to its palette-drop type, a list source to
/// its item type, an OS drop to files). This single distinction is exactly what
/// `TabBar` already encodes via its `source_bar_id`.
pub enum DragSource<'a, K> {
    /// An intra-view reorder; `key` identifies the dragged row.
    SameView { key: K },
    /// A drag from another view or the OS; downcast `payload` to interpret it.
    Foreign { payload: &'a DragPayload },
}

/// A hover-time question posed to a source: "may `source` drop at `position`
/// relative to `target`?" The source answers with a [`DropResponse`].
pub struct DropQuery<'a, K> {
    /// Who is dragging.
    pub source: DragSource<'a, K>,
    /// The row currently hovered.
    pub target: K,
    /// Where, relative to `target`, the drop would land.
    pub position: DropPosition,
}

/// A source's verdict on a [`DropQuery`]. Drives the hover affordance and gates
/// the commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropResponse {
    /// Allowed: paint the insertion line / reparent box at this position.
    Accept,
    /// Forbidden: paint the no-drop affordance; the drop will be refused.
    Reject,
    /// Allowed, but only at a different position — the view snaps its indicator
    /// to `.0` (e.g. a container that accepts children but not sibling reorder
    /// redirects `Before`/`After` → `Into`).
    Redirect(DropPosition),
}

/// A drop the user actually committed, handed to `accept_drop` to apply.
pub struct DropCommit<'a, K> {
    /// Who dragged.
    pub source: DragSource<'a, K>,
    /// The row dropped onto.
    pub target: K,
    /// Where, relative to `target`, the drop landed (after any `Redirect`).
    pub position: DropPosition,
}
