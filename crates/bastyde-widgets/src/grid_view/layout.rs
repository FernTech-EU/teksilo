//! Layout strategies for [`GridView`](crate::grid_view::GridView).
//!
//! See [`strategy::GridLayoutStrategy`] for the contract and the module
//! docs there for the shipped strategies.

pub(crate) mod columns;
pub(crate) mod masonry;
pub(crate) mod offsets;
pub(crate) mod sectioned;
pub(crate) mod strategy;
pub(crate) mod uniform;
pub(crate) mod variable_row;

pub use masonry::VirtualizedMasonry;
pub use strategy::{GridSizing, ScrollAnchor, TileRect, VisibleTileRange};
pub(crate) use strategy::GridLayoutStrategy;
pub use uniform::UniformGrid;
pub use variable_row::VariableRowGrid;
