// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Virtualized waterfall (Pinterest-style) grid strategy.
//!
//! Fixed/adaptive column count, per-item variable height, shortest-column
//! placement: each item drops into the currently-shortest column, so columns
//! stay balanced. Placement is index-order but depends on every prior item's
//! height, so the placement map is rebuilt (O(n)) whenever a height changes —
//! fine for the hundreds-to-low-thousands of items a waterfall gallery holds.
//!
//! Heights come from the same two paths as [`VariableRowGrid`](super::variable_row::VariableRowGrid):
//! exact `item_height(index)` or auto-measure. Unlike the row grid, the
//! waterfall does **not** scroll-anchor on late measurement (items reflow
//! across columns); a good estimate keeps the typical top-down scroll smooth.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::EdgeInsets;

use super::columns::{ColumnGeometry, geometry_for};
use super::strategy::{BUFFER_ROWS, GridLayoutStrategy, GridSizing, TileRect, VisibleTileRange};

type ExactHeightFn = Rc<dyn Fn(usize) -> f32>;

/// Per-item placement map for the waterfall layout.
#[derive(Debug)]
struct Placement {
    heights: Vec<f32>,
    measured: Vec<bool>,
    col_of: Vec<usize>,
    top_of: Vec<f32>,
    total_height: f32,
    cols: usize,
    gap: f32,
    inset_top: f32,
    inset_bottom: f32,
    estimated: f32,
    dirty: bool,
}

impl Placement {
    fn new(estimated: f32, gap: f32, inset_top: f32, inset_bottom: f32) -> Self {
        Self {
            heights: Vec::new(),
            measured: Vec::new(),
            col_of: Vec::new(),
            top_of: Vec::new(),
            total_height: 0.0,
            cols: 1,
            gap,
            inset_top,
            inset_bottom,
            estimated,
            dirty: true,
        }
    }

    fn len(&self) -> usize {
        self.heights.len()
    }

    fn set_count(&mut self, n: usize) {
        if n != self.heights.len() {
            self.heights.resize(n, self.estimated);
            self.measured.resize(n, false);
            self.dirty = true;
        }
    }

    fn set_cols(&mut self, cols: usize) {
        if cols != self.cols {
            self.cols = cols.max(1);
            self.dirty = true;
        }
    }

    fn set_height(&mut self, i: usize, h: f32) {
        if i < self.heights.len() && (self.heights[i] - h).abs() > 0.01 {
            self.heights[i] = h;
            self.measured[i] = true;
            self.dirty = true;
        } else if i < self.measured.len() {
            self.measured[i] = true;
        }
    }

    fn invalidate(&mut self, start: usize, end: usize) {
        let end = end.min(self.heights.len());
        for i in start..end {
            self.heights[i] = self.estimated;
            self.measured[i] = false;
        }
        if start < end {
            self.dirty = true;
        }
    }

    fn rebuild(&mut self) {
        if !self.dirty {
            return;
        }
        let n = self.heights.len();
        self.col_of.resize(n, 0);
        self.top_of.resize(n, 0.0);
        let cols = self.cols.max(1);
        let mut bottoms = vec![self.inset_top; cols];
        for i in 0..n {
            // Shortest column (lowest current bottom), leftmost on ties.
            let c = bottoms
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.col_of[i] = c;
            self.top_of[i] = bottoms[c];
            bottoms[c] += self.heights[i] + self.gap;
        }
        let max_bottom = bottoms.iter().cloned().fold(self.inset_top, f32::max);
        self.total_height = if n == 0 {
            0.0
        } else {
            (max_bottom - self.gap).max(self.inset_top) + self.inset_bottom
        };
        self.dirty = false;
    }
}

/// A virtualized waterfall grid.
pub struct VirtualizedMasonry {
    columns: ColumnGeometry,
    exact_height: Option<ExactHeightFn>,
    placement: RefCell<Placement>,
    estimated: f32,
}

impl std::fmt::Debug for VirtualizedMasonry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VirtualizedMasonry")
            .field("items", &self.placement.borrow().len())
            .field("exact", &self.exact_height.is_some())
            .finish()
    }
}

impl VirtualizedMasonry {
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
            exact_height,
            placement: RefCell::new(Placement::new(
                estimated,
                row_gap.max(0.0),
                inset.top,
                inset.bottom,
            )),
            estimated,
        }
    }

    fn reseed_exact(&self) {
        let Some(ref ef) = self.exact_height else {
            return;
        };
        let mut p = self.placement.borrow_mut();
        let n = p.len();
        for i in 0..n {
            let h = ef(i);
            p.set_height(i, h);
        }
    }

    /// Keep the placement's item count / column count in sync.
    fn sync(&self, item_count: usize, viewport_width: f32) {
        let cols = self.columns.column_count(viewport_width).max(1);
        {
            let mut p = self.placement.borrow_mut();
            p.set_count(item_count);
            p.set_cols(cols);
        }
        if self.exact_height.is_some() {
            self.reseed_exact();
        }
        self.placement.borrow_mut().rebuild();
    }
}

impl GridLayoutStrategy for VirtualizedMasonry {
    // `index_at_point` intentionally keeps the trait's O(n) `tile_rect` scan
    // default: unlike the row-major strategies, item order here isn't
    // visually monotonic in `y` (each item drops into the currently-
    // shortest column), so there's no closed-form inverse of `tile_rect` to
    // exploit. Acceptable for the hundreds-to-low-thousands of items a
    // waterfall gallery holds (see the module doc comment).

    fn column_count(&self, viewport_width: f32) -> usize {
        self.columns.column_count(viewport_width)
    }

    fn column_x(&self, col: usize, viewport_width: f32) -> (f32, f32) {
        self.columns.column_x(col, viewport_width)
    }

    fn total_content_height(&self, item_count: usize, viewport_width: f32) -> f32 {
        self.sync(item_count, viewport_width);
        self.placement.borrow().total_height
    }

    fn visible_range(
        &self,
        scroll_y: f32,
        viewport_height: f32,
        viewport_width: f32,
        item_count: usize,
    ) -> VisibleTileRange {
        self.sync(item_count, viewport_width);
        if item_count == 0 {
            return VisibleTileRange { start: 0, end: 0 };
        }
        let p = self.placement.borrow();
        let cols = p.cols.max(1);
        let top = scroll_y;
        let bot = scroll_y + viewport_height;
        // Items intersecting the viewport. Because column tops aren't strictly
        // monotonic in index, scan for the min/max intersecting index and
        // realize that contiguous span (a superset; the buffer absorbs slack).
        let mut min_i = None;
        let mut max_i = None;
        for i in 0..item_count {
            let t = p.top_of[i];
            let b = t + p.heights[i];
            if b >= top && t <= bot {
                min_i.get_or_insert(i);
                max_i = Some(i);
            }
        }
        match (min_i, max_i) {
            (Some(lo), Some(hi)) => {
                let buf = BUFFER_ROWS * cols;
                let start = lo.saturating_sub(buf);
                let end = (hi + 1 + buf).min(item_count);
                VisibleTileRange { start, end }
            }
            _ => VisibleTileRange { start: 0, end: 0 },
        }
    }

    fn tile_rect(&self, index: usize, viewport_width: f32) -> TileRect {
        self.placement.borrow_mut().rebuild();
        let p = self.placement.borrow();
        let col = p.col_of.get(index).copied().unwrap_or(0);
        let (x, width) = self.columns.column_x(col, viewport_width);
        let y = p.top_of.get(index).copied().unwrap_or(0.0);
        let height = p.heights.get(index).copied().unwrap_or(self.estimated);
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
        self.exact_height.is_none()
    }

    fn observe_measured(
        &self,
        measured: &[(usize, f32)],
        _scroll_y: f32,
        viewport_width: f32,
    ) -> f32 {
        if self.exact_height.is_some() {
            return 0.0;
        }
        // Item count is set by the layout pass before this runs; just feed
        // per-item heights. No scroll anchoring (items reflow across columns).
        let _ = viewport_width;
        let mut p = self.placement.borrow_mut();
        for &(i, h) in measured {
            p.set_height(i, h);
        }
        0.0
    }

    fn invalidate_rows(&self, item_range: std::ops::Range<usize>) {
        let end = if item_range.end == usize::MAX {
            self.placement.borrow().len()
        } else {
            item_range.end
        };
        self.placement
            .borrow_mut()
            .invalidate(item_range.start, end);
    }

    fn resize(&self, item_count: usize) {
        self.placement.borrow_mut().set_count(item_count);
        if self.exact_height.is_some() {
            self.reseed_exact();
        }
    }
}
