// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Drag-to-reorder for `GridView`.
//!
//! Each tile starts a shared [`RowDragData`](crate::data_views::RowDragData)
//! drag (the same payload `ListView` / `TreeView` / `TableView` emit, so a
//! grid tile can be dropped on any of them and vice-versa); the container is
//! a drop target
//! that computes a 2D insertion index from the pointer position and routes the
//! drop through the **source's** `can_accept` / `accept_drop` (intra-grid
//! reorder) or forwards a foreign drop to the app (`on_item_drop`). A vertical
//! insertion bar (painted by `GridOverlay`) shows where the item will land.

use bastyde_canvas::Point;

use super::layout::GridLayoutStrategy;

/// The flat index a drop at `local` (widget-local point) would insert
/// *before*. Delegates to [`GridLayoutStrategy::insertion_index_at`], which
/// (unlike `index_at_point`) always resolves to a real insertion point — a
/// point in an inter-tile gap (row-gap, column-gap) lands on the nearest
/// tile rather than falling through to "append at the end".
pub(crate) fn insertion_index(
    strategy: &dyn GridLayoutStrategy,
    local: Point,
    scroll_y: f32,
    viewport_width: f32,
    len: usize,
) -> usize {
    let cp = Point::new(local.x, local.y + scroll_y);
    strategy.insertion_index_at(cp, len, viewport_width)
}
