// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The uniform-tile grid strategy: every row has the same fixed height.
//!
//! This is the exact, O(1) common case — photo galleries, icon views,
//! file-manager grids. The column count is derived from a fixed tile width
//! (`Fixed`), an explicit count (`FixedColumnCount`), or a minimum tile
//! width (`Adaptive`, the CSS `repeat(auto-fill, minmax(...))` model).

use bastyde_canvas::EdgeInsets;

use super::columns::ColumnGeometry;
use super::strategy::{BUFFER_ROWS, GridLayoutStrategy, GridSizing, TileRect, VisibleTileRange};

/// A uniform grid: fixed row height, columns derived per [`GridSizing`].
#[derive(Debug, Clone)]
pub struct UniformGrid {
    columns: ColumnGeometry,
    tile_height: f32,
    row_gap: f32,
    inset: EdgeInsets,
}

impl UniformGrid {
    /// Build from the public sizing description plus spacing/insets.
    pub(crate) fn new(sizing: GridSizing, col_gap: f32, row_gap: f32, inset: EdgeInsets) -> Self {
        Self {
            columns: ColumnGeometry::new(sizing, col_gap, inset),
            tile_height: sizing.tile_height().max(0.0),
            row_gap: row_gap.max(0.0),
            inset,
        }
    }

    fn row_step(&self) -> f32 {
        self.tile_height + self.row_gap
    }

    fn row_count(&self, item_count: usize, viewport_width: f32) -> usize {
        if item_count == 0 {
            return 0;
        }
        item_count.div_ceil(self.column_count(viewport_width).max(1))
    }
}

impl GridLayoutStrategy for UniformGrid {
    fn column_count(&self, viewport_width: f32) -> usize {
        self.columns.column_count(viewport_width)
    }

    fn column_x(&self, col: usize, viewport_width: f32) -> (f32, f32) {
        self.columns.column_x(col, viewport_width)
    }

    fn total_content_height(&self, item_count: usize, viewport_width: f32) -> f32 {
        let rows = self.row_count(item_count, viewport_width);
        if rows == 0 {
            return 0.0;
        }
        self.inset.top + rows as f32 * self.row_step() - self.row_gap + self.inset.bottom
    }

    fn visible_range(
        &self,
        scroll_y: f32,
        viewport_height: f32,
        viewport_width: f32,
        item_count: usize,
    ) -> VisibleTileRange {
        if item_count == 0 || self.row_step() <= 0.0 {
            return VisibleTileRange { start: 0, end: 0 };
        }
        let cols = self.column_count(viewport_width).max(1);
        let row_step = self.row_step();
        let content_scroll = (scroll_y - self.inset.top).max(0.0);
        let first_row = (content_scroll / row_step).floor() as usize;
        let last_row = ((content_scroll + viewport_height) / row_step).ceil() as usize;
        let start_row = first_row.saturating_sub(BUFFER_ROWS);
        let end_row = last_row + BUFFER_ROWS;
        let start = (start_row * cols).min(item_count);
        let end = (end_row.saturating_add(1).saturating_mul(cols)).min(item_count);
        VisibleTileRange { start, end }
    }

    fn tile_rect(&self, index: usize, viewport_width: f32) -> TileRect {
        let cols = self.column_count(viewport_width).max(1);
        let row = index / cols;
        let col = index % cols;
        let (x, width) = self.column_x(col, viewport_width);
        let y = self.inset.top + row as f32 * self.row_step();
        TileRect {
            x,
            y,
            width,
            height: self.tile_height,
        }
    }

    fn estimated_row_height(&self) -> f32 {
        self.tile_height
    }
}
