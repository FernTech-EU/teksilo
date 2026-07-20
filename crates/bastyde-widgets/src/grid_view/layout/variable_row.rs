// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Variable-row-height grid: each row is sized to its tallest tile.
//!
//! Columns are uniform (same policy as [`UniformGrid`](super::uniform::UniformGrid)),
//! but every row takes the height of its tallest tile — the SwiftUI
//! `LazyVGrid` model. Because off-screen tiles aren't built, heights are
//! learned one of two ways:
//!
//! * **Auto-measure** (default): the body pane measures each realized tile
//!   and feeds the heights back via [`observe_measured`]; unmeasured rows
//!   use an estimate and the scroll position is anchored when an estimate is
//!   corrected (see the anchor-delta return value).
//! * **Exact** (`item_height(index)` supplied): row heights are computed
//!   exactly as `max(item_height(i))` over the row — no measurement, no
//!   anchoring, an exact scrollbar.
//!
//! [`observe_measured`]: super::strategy::GridLayoutStrategy::observe_measured

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use bastyde_canvas::{EdgeInsets, Point};

use super::columns::{ColumnGeometry, column_at, geometry_for};
use super::offsets::PrefixSumOffsets;
use super::strategy::{BUFFER_ROWS, GridLayoutStrategy, GridSizing, TileRect, VisibleTileRange};

type ExactHeightFn = Rc<dyn Fn(usize) -> f32>;

/// A grid whose rows are each sized to their tallest tile.
pub struct VariableRowGrid {
    columns: ColumnGeometry,
    row_gap: f32,
    estimated: f32,
    /// Optional exact per-item natural height. When present, rows are seeded
    /// exactly (no measurement / anchoring).
    exact_height: Option<ExactHeightFn>,
    offsets: RefCell<PrefixSumOffsets>,
    /// Current logical item count, kept in sync by `resize` / the
    /// `item_count`-bearing trait methods.
    item_count: Cell<usize>,
    /// Column count the prefix sum was last built for (a change forces a
    /// full reseed — a width reflow regroups items into different rows).
    stored_cols: Cell<usize>,
}

impl std::fmt::Debug for VariableRowGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VariableRowGrid")
            .field("rows", &self.offsets.borrow().rows())
            .field("exact", &self.exact_height.is_some())
            .finish()
    }
}

impl VariableRowGrid {
    pub(crate) fn new(
        sizing: GridSizing,
        col_gap: f32,
        row_gap: f32,
        inset: EdgeInsets,
        estimated: f32,
        exact_height: Option<ExactHeightFn>,
    ) -> Self {
        let estimated = if estimated > 0.0 {
            estimated
        } else {
            sizing.tile_height().max(1.0)
        };
        Self {
            columns: geometry_for(sizing, col_gap, inset),
            row_gap: row_gap.max(0.0),
            estimated,
            exact_height,
            offsets: RefCell::new(PrefixSumOffsets::new(
                0,
                estimated,
                row_gap.max(0.0),
                inset.top,
                inset.bottom,
            )),
            item_count: Cell::new(0),
            stored_cols: Cell::new(0),
        }
    }

    /// Re-seed exactly from `item_height` for every row (only when exact
    /// heights are supplied). O(item_count) — called on structural changes.
    fn reseed_exact(&self, cols: usize) {
        let Some(ref ef) = self.exact_height else {
            return;
        };
        let n = self.item_count.get();
        let mut off = self.offsets.borrow_mut();
        let rows = off.rows();
        for r in 0..rows {
            let mut h = 0.0_f32;
            for i in (r * cols)..((r + 1) * cols).min(n) {
                h = h.max(ef(i));
            }
            off.set_row_height(r, h);
        }
    }

    /// Ensure the prefix sum matches the current `(item_count, cols)`.
    /// Cheap (early-returns) when nothing changed. A column-count change
    /// fully reseeds (rows regroup); an item-count change resizes in place,
    /// preserving prior measurements.
    fn sync(&self, viewport_width: f32) {
        let cols = self.columns.column_count(viewport_width).max(1);
        let n = self.item_count.get();
        let rows = n.div_ceil(cols);

        if cols != self.stored_cols.get() {
            self.offsets.borrow_mut().reset(rows);
            self.stored_cols.set(cols);
            self.reseed_exact(cols);
        } else if rows != self.offsets.borrow().rows() {
            self.offsets.borrow_mut().resize(rows);
            self.reseed_exact(cols);
        }
    }
}

impl GridLayoutStrategy for VariableRowGrid {
    fn column_count(&self, viewport_width: f32) -> usize {
        self.columns.column_count(viewport_width)
    }

    fn column_x(&self, col: usize, viewport_width: f32) -> (f32, f32) {
        self.columns.column_x(col, viewport_width)
    }

    fn total_content_height(&self, item_count: usize, viewport_width: f32) -> f32 {
        self.item_count.set(item_count);
        self.sync(viewport_width);
        self.offsets.borrow_mut().total()
    }

    fn visible_range(
        &self,
        scroll_y: f32,
        viewport_height: f32,
        viewport_width: f32,
        item_count: usize,
    ) -> VisibleTileRange {
        self.item_count.set(item_count);
        self.sync(viewport_width);
        if item_count == 0 {
            return VisibleTileRange { start: 0, end: 0 };
        }
        let cols = self.stored_cols.get().max(1);
        let mut off = self.offsets.borrow_mut();
        let first_row = off.row_at(scroll_y);
        let last_row = off.row_at(scroll_y + viewport_height);
        let start_row = first_row.saturating_sub(BUFFER_ROWS);
        let end_row = last_row + BUFFER_ROWS;
        let start = (start_row * cols).min(item_count);
        let end = (end_row.saturating_add(1).saturating_mul(cols)).min(item_count);
        VisibleTileRange { start, end }
    }

    fn tile_rect(&self, index: usize, viewport_width: f32) -> TileRect {
        self.sync(viewport_width);
        let cols = self.stored_cols.get().max(1);
        let row = index / cols;
        let col = index % cols;
        let (x, width) = self.columns.column_x(col, viewport_width);
        let mut off = self.offsets.borrow_mut();
        let y = off.row_top(row);
        let height = off.row_height(row);
        TileRect {
            x,
            y,
            width,
            height,
        }
    }

    fn estimated_row_height(&self) -> f32 {
        self.estimated
    }

    fn measures_tiles(&self) -> bool {
        // Only the auto-measure path needs tile measurement; the exact-
        // height fast-path seeds rows deterministically.
        self.exact_height.is_none()
    }

    fn observe_measured(
        &self,
        measured: &[(usize, f32)],
        scroll_y: f32,
        viewport_width: f32,
    ) -> f32 {
        if self.exact_height.is_some() {
            return 0.0;
        }
        self.sync(viewport_width);
        let cols = self.stored_cols.get().max(1);

        // Fold per-tile measurements into a per-row max.
        let mut row_max: HashMap<usize, f32> = HashMap::new();
        for &(idx, h) in measured {
            let r = idx / cols;
            let e = row_max.entry(r).or_insert(0.0);
            if h > *e {
                *e = h;
            }
        }

        let mut off = self.offsets.borrow_mut();
        // Read every affected row's pre-change top while the table is clean,
        // so the anchor decision doesn't churn the lazy rebuild.
        off.total();
        let tops: Vec<(usize, f32, f32)> = row_max
            .iter()
            .map(|(&r, &h)| (r, off.row_top(r), h))
            .collect();
        let mut anchor_delta = 0.0_f32;
        for (r, top_before, h) in tops {
            let delta = off.set_row_height(r, h);
            // Rows strictly above the viewport top shift the content the user
            // is pinned to; correct the scroll to keep it visually stationary.
            // A row whose top is exactly at `scroll_y` is the topmost visible
            // row — its top doesn't move when it grows, so no correction.
            if delta.abs() > 0.01 && top_before < scroll_y {
                anchor_delta += delta;
            }
        }
        anchor_delta
    }

    fn invalidate_rows(&self, item_range: std::ops::Range<usize>) {
        let cols = self.stored_cols.get().max(1);
        let start_row = item_range.start / cols;
        let end_row = if item_range.end == usize::MAX {
            self.offsets.borrow().rows()
        } else {
            item_range.end.div_ceil(cols)
        };
        self.offsets.borrow_mut().invalidate(start_row, end_row);
    }

    fn resize(&self, item_count: usize) {
        self.item_count.set(item_count);
        let cols = self.stored_cols.get().max(1);
        let rows = item_count.div_ceil(cols);
        self.offsets.borrow_mut().resize(rows);
        self.reseed_exact(cols);
    }

    fn index_at_point(
        &self,
        content_point: Point,
        item_count: usize,
        viewport_width: f32,
    ) -> Option<usize> {
        if item_count == 0 {
            return None;
        }
        self.item_count.set(item_count);
        self.sync(viewport_width);
        let cols = self.stored_cols.get().max(1);
        let (row, row_top, row_h) = {
            let mut off = self.offsets.borrow_mut();
            let row = off.row_at(content_point.y);
            (row, off.row_top(row), off.row_height(row))
        };
        // `row_at` clamps to a valid row even when the point is above the
        // first row or below the last — the explicit span check below is
        // what actually rejects those (and any row-gap in between).
        if content_point.y < row_top || content_point.y > row_top + row_h {
            return None;
        }
        let col = column_at(&self.columns, content_point.x, viewport_width)?;
        let idx = row * cols + col;
        (idx < item_count).then_some(idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> VariableRowGrid {
        // 100-wide tiles, 10px gaps → 2 columns in 210px. Exact 40px item
        // height (no measurement pass needed): row_step = 40 + 10 = 50.
        VariableRowGrid::new(
            GridSizing::Fixed {
                width: 100.0,
                height: 40.0,
            },
            10.0,
            10.0,
            EdgeInsets::ZERO,
            40.0,
            Some(Rc::new(|_i| 40.0)),
        )
    }

    #[test]
    fn index_at_point_closed_form_matches_measured_rows() {
        let g = grid();
        // 6 items, 2 cols → 3 rows. Row 0 spans y 0..40; row 1 spans
        // 50..90 (the row-gap band is 40..50).
        assert_eq!(g.index_at_point(Point::new(0.0, 0.0), 6, 210.0), Some(0));
        assert_eq!(g.index_at_point(Point::new(0.0, 50.0), 6, 210.0), Some(2));
    }

    #[test]
    fn index_at_point_closed_form_returns_none_in_gaps() {
        let g = grid();
        // Row-gap band.
        assert_eq!(g.index_at_point(Point::new(0.0, 45.0), 6, 210.0), None);
        // Column-gap band (x 100..110).
        assert_eq!(g.index_at_point(Point::new(105.0, 10.0), 6, 210.0), None);
        // Past the last row.
        assert_eq!(g.index_at_point(Point::new(0.0, 9000.0), 6, 210.0), None);
    }
}
