// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Drag-to-reorder for `GridView`.
//!
//! Each tile can start a typed [`GridViewDragData`] drag; the container is a
//! drop target that computes a 2D insertion index from the pointer position
//! and either reorders the backing model (intra-grid) or forwards the drop
//! to the app (`on_item_drop`). A vertical insertion bar (painted by
//! `GridOverlay`) shows where the item will land.

use bastyde_canvas::Point;

use super::layout::GridLayoutStrategy;

/// Payload identifying an in-grid tile being dragged for reorder.
#[derive(Debug, Clone)]
pub(crate) struct GridViewDragData {
    pub(crate) source_index: usize,
    /// Disambiguates different `GridView` instances so a drop only reorders
    /// when it originated in the same grid.
    pub(crate) source_model_id: usize,
}

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
