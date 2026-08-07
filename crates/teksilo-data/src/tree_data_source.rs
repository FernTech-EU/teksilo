// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TreeDataSource` — read-and-command interface for hierarchical data behind a
//! `TreeView` / `TreeTableView`.
//!
//! `TreeDataSource` is to trees what [`ListDataSource`](crate::ListDataSource)
//! is to flat lists: a projected, per-view, flattened read API plus the
//! capability protocol for identity, DnD validation, and lazy loading.
//! The built-in [`TreeSlice`](crate::TreeSlice) and
//! [`SortFilterTreeModel`](crate::SortFilterTreeModel) implement it over an
//! in-memory [`TreeModel`]; an external source of truth
//! (e.g. a Qleany entity store) implements it directly with its own `Key` type
//! and so never needs to mirror itself into a `TreeModel`.
//!
//! ## When to use
//!
//! Implement `TreeDataSource` directly when your data already lives outside an
//! in-memory tree (a database, a virtual filesystem, a remote store) and you
//! do not want to mirror it into a `TreeModel`. Use [`TreeSlice`](crate::TreeSlice)
//! when you have a `TreeModel<T>` and want per-view expand state.
//!
//! ## Example
//!
//! ```ignore
//! use teksilo_data::{TreeDataSource, FlatEntry, NodeId};
//! use teksilo_data::dnd_types::{DragEligibility, DropQuery, DropResponse, DropCommit, RowState};
//! use teksilo_core::signal::Signal;
//!
//! struct MySource { version: Signal<u64> }
//!
//! impl TreeDataSource for MySource {
//!     type Item = String;
//!     type Key = NodeId;
//!
//!     fn visible_count(&self) -> usize { 0 }
//!     fn with_entry<R>(&self, _i: usize, _f: impl FnOnce(&String, &FlatEntry<NodeId>) -> R) -> Option<R> { None }
//!     fn key_at(&self, _i: usize) -> Option<NodeId> { None }
//!     fn flat_index_of(&self, _k: &NodeId) -> Option<usize> { None }
//!     fn parent(&self, _k: &NodeId) -> Option<NodeId> { None }
//!     fn child_keys(&self, _k: &NodeId) -> Vec<NodeId> { vec![] }
//!     fn version_signal(&self) -> Signal<u64> { self.version.clone() }
//!     fn is_expanded(&self, _k: &NodeId) -> bool { false }
//!     fn set_expanded(&self, _k: &NodeId, _expanded: bool) {}
//! }
//! ```

use teksilo_core::signal::Signal;

use crate::dnd_types::ItemKey;
use crate::dnd_types::{
    DragEligibility, DragSource, DropCommit, DropPosition, DropQuery, DropResponse, RowState,
};
use crate::tree_change::NodeId;
use crate::tree_model::TreeModel;

/// A single entry in a tree's flattened, currently-visible row list.
///
/// Generic over the key type so external sources carry their own identity
/// (`K = NodeId` for `TreeModel`-backed sources, `K = i64` for an entity-id
/// store, …). The default `K = NodeId` keeps every in-tree `FlatEntry` mention
/// and `entry.node_id` read compiling unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatEntry<K: ItemKey = NodeId> {
    /// The row's stable key in its source.
    pub node_id: K,
    /// Depth in the tree (0 for roots).
    pub depth: usize,
    /// Whether this row has children in the source.
    pub has_children: bool,
    /// Whether this row is currently expanded (children visible).
    pub is_expanded: bool,
}

/// A per-view flattened, projectable view over hierarchical data.
///
/// Not object-safe (associated types + `impl FnOnce`); views consume it
/// generically and erase it into a closure bundle, exactly as `ListView` does
/// with `ListDataSource`. The DnD (`drag`/`can_accept`/`accept_drop`/
/// `on_drag_out`) and lazy (`row_state`/`request_window`/`can_fetch_more`/
/// `fetch_more`) methods default to inert/fully-resident, so a read-only source
/// implements only the core read + nav surface.
pub trait TreeDataSource: 'static {
    /// The item type stored at each node.
    type Item: 'static;
    /// The stable per-node identity (`NodeId` for in-memory trees, an entity id
    /// for an external store).
    type Key: ItemKey;

    // ── Core read ─────────────────────────────────────────────────────────
    /// Number of currently-visible (flattened) rows.
    fn visible_count(&self) -> usize;
    /// Access the item + flat metadata at a visible index via callback.
    fn with_entry<R>(
        &self,
        flat_index: usize,
        f: impl FnOnce(&Self::Item, &FlatEntry<Self::Key>) -> R,
    ) -> Option<R>;
    /// The key of the row at a visible index.
    fn key_at(&self, flat_index: usize) -> Option<Self::Key>;
    /// The visible index of a key, if currently visible.
    fn flat_index_of(&self, key: &Self::Key) -> Option<usize>;
    /// The parent of a node (`None` for a root) — drives sibling nav + the
    /// drop cycle-guard.
    fn parent(&self, key: &Self::Key) -> Option<Self::Key>;
    /// The children of a node, in order.
    fn child_keys(&self, key: &Self::Key) -> Vec<Self::Key>;
    /// A version signal that bumps on every structural/projection change — the
    /// view binds it at `BindingLevel::Rebuild`.
    fn version_signal(&self) -> Signal<u64>;

    // ── Expand / collapse (per-view) ──────────────────────────────────────
    /// Whether the node is expanded.
    fn is_expanded(&self, key: &Self::Key) -> bool;
    /// Expand (`true`) or collapse (`false`) the node.
    fn set_expanded(&self, key: &Self::Key, expanded: bool);

    /// First visible index whose content may differ after the latest change —
    /// rows `0..index` are unchanged, so per-row derived state (e.g. a measured
    /// height) remains valid. `None` means unknown (treat as a full change).
    fn first_changed_index(&self) -> Option<usize> {
        None
    }

    /// Whether `key` still exists in the source, **independent of visibility** —
    /// a node hidden under a collapsed ancestor (or scrolled out of a lazy
    /// window) still exists. Drives keyed-selection pruning, so that a
    /// collapsed-but-present node keeps its selection and only a *deleted* node
    /// is dropped. Default: visible-only (`flat_index_of(key).is_some()`);
    /// sources whose nodes persist while collapsed/scrolled out should override
    /// this to consult their full store.
    fn contains_key(&self, key: &Self::Key) -> bool {
        self.flat_index_of(key).is_some()
    }

    // ── DnD (default: inert) ──────────────────────────────────────────────
    /// Whether the node may begin a drag (the transferable gate).
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
    /// Reorder a whole set of this source's OWN nodes so they land contiguously
    /// at a drop gap — the multi-row same-view reorder commit. `sources` are the
    /// dragged nodes' keys in visible order; `target` / `position` name the drop
    /// gap. Returns whether anything moved.
    ///
    /// The default first drops any `sources` node that is a **descendant of
    /// another** `sources` node (moving an ancestor already carries its
    /// subtree), then moves the remaining top-level nodes one at a time,
    /// re-anchoring each after the previous. Tree keys are stable, so the
    /// re-anchoring is correct without index bookkeeping.
    fn reorder_within(
        &self,
        sources: &[Self::Key],
        target: &Self::Key,
        position: DropPosition,
    ) -> bool {
        // Dropping INTO one of the dragged subtrees (target is a dragged node
        // or a descendant of one) is invalid for the whole gesture — reject
        // rather than partially apply, matching what the hover verdict shows.
        let mut t = Some(target.clone());
        while let Some(node) = t {
            if sources.iter().any(|s| s == &node) {
                return false;
            }
            t = self.parent(&node);
        }
        // Keep only nodes that are not a descendant of another selected node.
        let top: Vec<Self::Key> = sources
            .iter()
            .filter(|k| {
                let mut p = self.parent(k);
                while let Some(ancestor) = p {
                    if sources.iter().any(|s| s == &ancestor) {
                        return false;
                    }
                    p = self.parent(&ancestor);
                }
                true
            })
            .cloned()
            .collect();
        let mut anchor = target.clone();
        let mut pos = position;
        let mut moved = false;
        for key in &top {
            if key == &anchor {
                continue;
            }
            if self.accept_drop(DropCommit {
                source: DragSource::SameView { key: key.clone() },
                target: anchor.clone(),
                position: pos,
            }) {
                moved = true;
                anchor = key.clone();
                pos = DropPosition::After;
            }
        }
        moved
    }
    /// Called on the *origin* source after one of its rows was accepted by a
    /// different view (source-side completion). Sources backed by a shared /
    /// command model no-op this; independent models use it to drop the moved
    /// row.
    fn on_drag_out(&self, _key: &Self::Key) {}

    // ── Lazy (default: fully resident) ────────────────────────────────────
    /// Whether the row at a visible index is loaded.
    fn row_state(&self, _flat_index: usize) -> RowState {
        RowState::Ready
    }
    /// Nudge the source to load the given visible range (the view calls this
    /// each build with its visible + buffer window).
    fn request_window(&self, _range: std::ops::Range<usize>) {}
    /// Whether more rows can be appended (infinite scroll).
    fn can_fetch_more(&self) -> bool {
        false
    }
    /// Fetch the next page (append-only growth).
    fn fetch_more(&self) {}
}

/// Whether `node` is `ancestor` or one of its descendants — the move cycle
/// guard (you cannot drop a node into its own subtree).
pub fn tree_is_desc_or_self<T: 'static>(
    tree: &TreeModel<T>,
    node: NodeId,
    ancestor: NodeId,
) -> bool {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if n == ancestor {
            return true;
        }
        cur = tree.parent(n);
    }
    false
}

/// Apply a tree reorder by `NodeId`, with the cycle guard and the
/// remove-then-insert index adjustment `TreeModel::move_node` requires. Shared
/// by the `TreeSlice` / `SortFilterTreeModel` `accept_drop` impls. Returns
/// whether the move was applied (false = rejected, e.g. cycle or self-drop).
pub fn tree_apply_reorder<T: 'static>(
    tree: &TreeModel<T>,
    source: NodeId,
    target: NodeId,
    position: DropPosition,
) -> bool {
    if source == target {
        return false;
    }
    // Reject dropping a node anywhere inside its own subtree (covers Into a
    // descendant and reorder relative to a descendant).
    if tree_is_desc_or_self(tree, target, source) {
        return false;
    }
    match position {
        DropPosition::Into => {
            // Append as last child. move_node removes `source` first, so if it
            // was already a child of `target` the post-removal length is one
            // smaller.
            let mut idx = tree.child_count(target);
            if tree.parent(source) == Some(target) {
                idx -= 1;
            }
            tree.move_node(source, target, idx);
            true
        }
        DropPosition::Before | DropPosition::After => {
            let new_parent = tree.parent(target);
            let siblings: Vec<NodeId> = match new_parent {
                Some(p) => tree.children(p),
                None => (0..tree.root_count()).map(|i| tree.root(i)).collect(),
            };
            // `target` must be among its own parent's children. If it isn't,
            // the tree is inconsistent — reject the drop rather than silently
            // falling back to index 0, which would reorder the node to the
            // start and corrupt the sibling order.
            let Some(pos) = siblings.iter().position(|&s| s == target) else {
                return false;
            };
            let mut idx = if position == DropPosition::After {
                pos + 1
            } else {
                pos
            };
            // Same-parent removal shift: move_node removes `source` before
            // inserting, so an insertion point above the source's old slot
            // shifts down by one.
            if tree.parent(source) == new_parent
                && let Some(sp) = siblings.iter().position(|&s| s == source)
                && sp < idx
            {
                idx -= 1;
            }
            match new_parent {
                Some(p) => tree.move_node(source, p, idx),
                None => tree.move_to_root(source, idx),
            }
            true
        }
    }
}
