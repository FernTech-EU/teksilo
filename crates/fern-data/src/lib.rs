//! Data model abstractions for FernUI.
//!
//! Provides reactive collection types (`ListModel<T>`, `TreeModel<T>`) and
//! change notification enums (`DataChange`, `TreeChange`) for data-driven
//! widgets like `ListView`, `TreeView`, and `Repeater`.

pub mod data_change;
pub mod list_data_source;
pub mod list_model;
pub mod selection_model;
pub mod sort_filter_list_model;
pub mod sort_filter_tree_model;
pub mod tree_change;
pub mod tree_model;
pub mod tree_slice;

pub use data_change::DataChange;
pub use list_data_source::ListDataSource;
pub use list_model::ListModel;
pub use selection_model::{SelectionMode, SelectionModel};
pub use sort_filter_list_model::{SortDirection, SortFilterListModel};
pub use sort_filter_tree_model::{SortFilterTreeModel, TreeFilterMode};
pub use tree_change::{NodeId, TreeChange};
pub use tree_model::TreeModel;
pub use tree_slice::{FlatEntry, TreeSlice, TreeSliceHandle};
