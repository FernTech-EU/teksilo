// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The uniform-tile grid strategy: every row has the same fixed height.
//!
//! This is the exact, O(1) common case — photo galleries, icon views,
//! file-manager grids. The column count is derived from a fixed tile width
//! (`Fixed`), an explicit count (`FixedColumnCount`), or a minimum tile
//! width (`Adaptive`, the CSS `repeat(auto-fill, minmax(...))` model).

use bastyde_canvas::{EdgeInsets, Point};

use super::columns::{ColumnGeometry, column_at, geometry_for};
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
            columns: geometry_for(sizing, col_gap, inset),
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

    fn index_at_point(
        &self,
        content_point: Point,
        item_count: usize,
        viewport_width: f32,
    ) -> Option<usize> {
        if item_count == 0 || self.row_step() <= 0.0 {
            return None;
        }
        let y = content_point.y - self.inset.top;
        if y < 0.0 {
            return None;
        }
        let row = (y / self.row_step()) as usize;
        if y - row as f32 * self.row_step() > self.tile_height {
            return None; // row-gap
        }
        let col = column_at(&self.columns, content_point.x, viewport_width)?;
        let cols = self.column_count(viewport_width);
        let idx = row * cols + col;
        (idx < item_count).then_some(idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> UniformGrid {
        // 100×50 tiles, 10px gaps, no insets → 4 columns in 430px.
        UniformGrid::new(
            GridSizing::Fixed {
                width: 100.0,
                height: 50.0,
            },
            10.0,
            10.0,
            EdgeInsets::ZERO,
        )
    }

    #[test]
    fn index_at_point_closed_form_matches_exact_edges() {
        let g = grid();
        // Tile 0 spans x 0..100, y 0..50 — both edges inclusive.
        assert_eq!(g.index_at_point(Point::new(0.0, 0.0), 12, 430.0), Some(0));
        assert_eq!(
            g.index_at_point(Point::new(100.0, 50.0), 12, 430.0),
            Some(0)
        );
        // Row 1 starts at y = 50 + 10 (gap) = 60.
        assert_eq!(g.index_at_point(Point::new(0.0, 60.0), 12, 430.0), Some(4));
    }

    #[test]
    fn index_at_point_closed_form_returns_none_in_gaps() {
        let g = grid();
        // Row-gap band (50..60).
        assert_eq!(g.index_at_point(Point::new(50.0, 55.0), 12, 430.0), None);
        // Column-gap band (100..110).
        assert_eq!(g.index_at_point(Point::new(105.0, 25.0), 12, 430.0), None);
        // Past the last row.
        assert_eq!(g.index_at_point(Point::new(0.0, 9000.0), 12, 430.0), None);
        // Above the first row.
        assert_eq!(g.index_at_point(Point::new(0.0, -5.0), 12, 430.0), None);
    }
}
