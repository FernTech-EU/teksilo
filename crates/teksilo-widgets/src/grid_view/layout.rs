// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Layout strategies for [`GridView`](crate::grid_view::GridView).
//!
//! See `GridLayoutStrategy` for the contract and the module
//! docs there for the shipped strategies.

pub(crate) mod columns;
pub(crate) mod masonry;
pub(crate) mod offsets;
pub(crate) mod sectioned;
pub(crate) mod strategy;
pub(crate) mod uniform;
pub(crate) mod variable_row;

pub use masonry::VirtualizedMasonry;
pub(crate) use strategy::GridLayoutStrategy;
pub use strategy::{GridSizing, ScrollAnchor, TileRect, VisibleTileRange};
pub use uniform::UniformGrid;
pub use variable_row::VariableRowGrid;
