// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Drag-to-reorder for `GridView`.
//!
//! Each tile starts a shared [`RowDrag`](crate::data_views::RowDrag) drag (the
//! same payload `ListView` / `TreeView` / `TableView` emit, so a grid tile can
//! be dropped on any of them and vice-versa); the container is a drop target
//! that computes a 2D insertion index from the pointer position and routes the
//! drop through the **source's** `can_accept` / `accept_drop` (intra-grid
//! reorder) or forwards a foreign drop to the app (`on_item_drop`). A vertical
//! insertion bar (painted by `GridOverlay`) shows where the item will land.

use bastyde_canvas::Point;

use super::layout::GridLayoutStrategy;

/// The flat index a drop at `local` (widget-local point) would insert
/// *before*. Lands on the nearest tile edge; falls through to `len` (append)
/// when the point is past the last tile / in trailing empty space.
pub(crate) fn insertion_index(
    strategy: &dyn GridLayoutStrategy,
    local: Point,
    scroll_y: f32,
    viewport_width: f32,
    len: usize,
) -> usize {
    let cp = Point::new(local.x, local.y + scroll_y);
    match strategy.index_at_point(cp, len, viewport_width) {
        Some(i) => {
            let r = strategy.tile_rect(i, viewport_width);
            if cp.x > r.x + r.width * 0.5 {
                (i + 1).min(len)
            } else {
                i
            }
        }
        None => len,
    }
}
