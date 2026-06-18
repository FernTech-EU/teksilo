// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Data model abstractions for Bastyde.
//!
//! Provides reactive collection types (`ListModel<T>`, `TreeModel<T>`) and
//! change notification enums (`DataChange`, `TreeChange`) for data-driven
//! widgets like `ListView`, `TreeView`, and `Repeater`.
//!
//! The two source traits — [`ListDataSource`] (flat) and [`TreeDataSource`]
//! (hierarchical) — are the read-and-command interfaces every data view talks
//! to. `ListModel` / `TreeModel` are built-in implementations; an external
//! source of truth (e.g. a Qleany entity store) implements a source trait
//! directly and so never mirrors itself into a built-in model. Each trait
//! carries a capability protocol — identity (`key_at`/`index_of`), DnD
//! validation (`drag`/`can_accept`/`accept_drop`/`on_drag_out`), and lazy
//! loading (`row_state`/`request_window`/`can_fetch_more`/`fetch_more`) — as
//! defaulted methods (see [`dnd_types`]).
#![allow(clippy::type_complexity)]

pub mod check_state;
pub mod checked_model;
pub mod data_change;
#[cfg(debug_assertions)]
pub mod debug_registry;
pub mod dnd_types;
pub mod keyed_selection_model;
pub mod list_data_source;
pub mod list_model;
pub mod reconcile;
pub mod selection_model;
pub mod sort_filter_list_model;
pub mod sort_filter_tree_model;
pub mod tree_change;
pub mod tree_checked_model;
pub mod tree_data_source;
pub mod tree_model;
pub mod tree_slice;

pub use check_state::CheckState;
pub use checked_model::CheckedModel;
pub use data_change::DataChange;
pub use dnd_types::{
    DragEligibility, DragSource, DropCommit, DropPosition, DropQuery, DropResponse, ItemKey,
    RowState,
};
pub use keyed_selection_model::KeyedSelectionModel;
pub use list_data_source::ListDataSource;
pub use list_model::ListModel;
pub use reconcile::{ReconcileIndex, reconcile_list, reconcile_tree};
pub use selection_model::{SelectionMode, SelectionModel};
pub use sort_filter_list_model::{SortDirection, SortFilterListModel};
pub use sort_filter_tree_model::{SortFilterTreeModel, TreeFilterMode};
pub use tree_change::{NodeId, TreeChange};
pub use tree_checked_model::{AggregateMode, TreeCheckedModel};
pub use tree_data_source::{FlatEntry, TreeDataSource};
pub use tree_model::TreeModel;
pub use tree_slice::{TreeSlice, TreeSliceHandle};
