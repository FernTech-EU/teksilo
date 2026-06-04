//! Cumulative row-offset table for variable-height grid strategies.
//!
//! `PrefixSumOffsets` maps row index → content-space top-y in O(log n) (and
//! the inverse, y → row, by binary search), rebuilding lazily only from the
//! first dirtied row. Variable-row and waterfall strategies hold one of
//! these behind a `RefCell` so the `&self` `place_children` pass can feed
//! measured heights back without `&mut`.

/// Per-row cumulative offsets with lazy, incremental rebuild.
#[derive(Debug)]
pub(crate) struct PrefixSumOffsets {
    /// Per-row content height (no inter-row gap).
    heights: Vec<f32>,
    /// Whether each row's height has been measured (vs. estimated).
    measured: Vec<bool>,
    gap: f32,
    top_inset: f32,
    bottom_inset: f32,
    estimated: f32,
    /// `offsets[r]` = top-y of row `r` for `r < rows`; `offsets[rows]` =
    /// total content height. Length `rows + 1`. Rebuilt lazily.
    offsets: Vec<f32>,
    /// First row needing recomputation; `None` when clean.
    dirty_from: Option<usize>,
}

impl PrefixSumOffsets {
    pub(crate) fn new(
        rows: usize,
        estimated: f32,
        gap: f32,
        top_inset: f32,
        bottom_inset: f32,
    ) -> Self {
        Self {
            heights: vec![estimated; rows],
            measured: vec![false; rows],
            gap,
            top_inset,
            bottom_inset,
            estimated,
            offsets: vec![0.0; rows + 1],
            dirty_from: Some(0),
        }
    }

    pub(crate) fn rows(&self) -> usize {
        self.heights.len()
    }

    /// Whether row `r` carries a measured (vs estimated) height.
    pub(crate) fn is_measured(&self, r: usize) -> bool {
        self.measured.get(r).copied().unwrap_or(false)
    }

    /// Drop every height back to the estimate and resize to `rows`.
    pub(crate) fn reset(&mut self, rows: usize) {
        self.heights = vec![self.estimated; rows];
        self.measured = vec![false; rows];
        self.offsets = vec![0.0; rows + 1];
        self.dirty_from = Some(0);
    }

    /// Grow or shrink to `rows`, preserving existing heights (new rows seed
    /// to the estimate). Cheaper than [`reset`](Self::reset) when only the
    /// item count changed and prior measurements should survive.
    pub(crate) fn resize(&mut self, rows: usize) {
        let old = self.heights.len();
        if rows == old {
            return;
        }
        self.heights.resize(rows, self.estimated);
        self.measured.resize(rows, false);
        self.offsets.resize(rows + 1, 0.0);
        self.mark_dirty(old.min(rows));
    }

    fn mark_dirty(&mut self, from: usize) {
        self.dirty_from = Some(self.dirty_from.map_or(from, |d| d.min(from)));
    }

    /// Set row `r`'s height; returns the delta (`new - old`) when it
    /// changed beyond a sub-pixel epsilon (else `0.0`, with no dirtying so
    /// re-measures don't oscillate). Marks the row measured.
    pub(crate) fn set_row_height(&mut self, r: usize, h: f32) -> f32 {
        if r >= self.heights.len() {
            return 0.0;
        }
        let old = self.heights[r];
        let delta = h - old;
        self.measured[r] = true;
        if delta.abs() > 0.01 {
            self.heights[r] = h;
            self.mark_dirty(r);
            delta
        } else {
            0.0
        }
    }

    /// Invalidate rows in `[start, end)` (clamped) back to the estimate.
    pub(crate) fn invalidate(&mut self, start: usize, end: usize) {
        let end = end.min(self.heights.len());
        if start >= end {
            return;
        }
        for r in start..end {
            self.heights[r] = self.estimated;
            self.measured[r] = false;
        }
        self.mark_dirty(start);
    }

    fn rebuild(&mut self) {
        let Some(from) = self.dirty_from.take() else {
            return;
        };
        let rows = self.heights.len();
        if self.offsets.len() != rows + 1 {
            self.offsets.resize(rows + 1, 0.0);
        }
        if rows == 0 {
            self.offsets[0] = 0.0;
            return;
        }
        // Derive row `from`'s top from the clean previous row rather than the
        // stale `offsets[from]` slot, which after a resize-grow holds the old
        // *total* (baked the bottom inset, not the inter-row gap).
        let from = from.min(rows);
        let mut acc = if from == 0 {
            self.top_inset
        } else {
            self.offsets[from - 1] + self.heights[from - 1] + self.gap
        };
        for r in from..rows {
            self.offsets[r] = acc;
            acc += self.heights[r] + self.gap;
        }
        // Bottom of the last row (drop the trailing gap) plus the bottom inset.
        self.offsets[rows] = acc - self.gap + self.bottom_inset;
    }

    pub(crate) fn total(&mut self) -> f32 {
        self.rebuild();
        let rows = self.heights.len();
        if rows == 0 { 0.0 } else { self.offsets[rows] }
    }

    pub(crate) fn row_top(&mut self, r: usize) -> f32 {
        self.rebuild();
        let rows = self.heights.len();
        if rows == 0 {
            return self.top_inset;
        }
        self.offsets[r.min(rows)]
    }

    pub(crate) fn row_height(&self, r: usize) -> f32 {
        self.heights.get(r).copied().unwrap_or(self.estimated)
    }

    /// The row whose vertical span contains `y` (clamped to a valid row).
    pub(crate) fn row_at(&mut self, y: f32) -> usize {
        self.rebuild();
        let rows = self.heights.len();
        if rows == 0 {
            return 0;
        }
        // Number of row-tops <= y; the containing row is one less.
        let pp = self.offsets[..rows].partition_point(|&o| o <= y);
        pp.saturating_sub(1).min(rows - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimated_offsets_and_total() {
        // 4 rows, height 50, gap 8, no insets → tops 0, 58, 116, 174;
        // total = bottom of last row = 174 + 50 = 224.
        let mut p = PrefixSumOffsets::new(4, 50.0, 8.0, 0.0, 0.0);
        assert_eq!(p.row_top(0), 0.0);
        assert_eq!(p.row_top(1), 58.0);
        assert_eq!(p.row_top(2), 116.0);
        assert_eq!(p.row_top(3), 174.0);
        assert_eq!(p.total(), 224.0);
    }

    #[test]
    fn insets_offset_the_content() {
        // top inset 10, bottom inset 6.
        let mut p = PrefixSumOffsets::new(2, 50.0, 8.0, 10.0, 6.0);
        assert_eq!(p.row_top(0), 10.0);
        assert_eq!(p.row_top(1), 68.0);
        // bottom of last row = 68 + 50 = 118; + bottom inset 6 = 124.
        assert_eq!(p.total(), 124.0);
    }

    #[test]
    fn set_row_height_returns_delta_and_shifts_following() {
        let mut p = PrefixSumOffsets::new(3, 50.0, 8.0, 0.0, 0.0);
        // Row 0 grows 50 → 90 (delta +40); rows 1, 2 shift down by 40.
        let delta = p.set_row_height(0, 90.0);
        assert!((delta - 40.0).abs() < 0.01);
        assert_eq!(p.row_top(1), 98.0); // 90 + 8
        assert_eq!(p.row_top(2), 156.0); // 98 + 50 + 8
        // Re-setting to the same value yields zero delta (no oscillation).
        assert_eq!(p.set_row_height(0, 90.0), 0.0);
    }

    #[test]
    fn row_at_maps_y_to_row() {
        let mut p = PrefixSumOffsets::new(4, 50.0, 8.0, 0.0, 0.0);
        assert_eq!(p.row_at(0.0), 0);
        assert_eq!(p.row_at(57.0), 0);
        assert_eq!(p.row_at(58.0), 1);
        assert_eq!(p.row_at(200.0), 3);
        assert_eq!(p.row_at(99999.0), 3); // clamps to last row
    }

    #[test]
    fn resize_preserves_measured_heights() {
        let mut p = PrefixSumOffsets::new(2, 50.0, 8.0, 0.0, 0.0);
        p.set_row_height(0, 90.0);
        p.resize(4); // append two estimated rows
        assert_eq!(p.rows(), 4);
        assert_eq!(p.row_top(1), 98.0); // row 0's measured height preserved
        assert!(p.is_measured(0));
        assert!(!p.is_measured(2));
    }

    #[test]
    fn resize_grow_uses_gap_not_bottom_inset_for_appended_rows() {
        // Regression: after a grow, the newly-appended row's top must continue
        // the inter-row gap, not reuse the stale total (which had baked the
        // bottom inset). Heights 50, gap 8, bottom inset 100.
        let mut p = PrefixSumOffsets::new(2, 50.0, 8.0, 0.0, 100.0);
        let _ = p.total(); // force a rebuild so offsets[2] = old total
        p.resize(4);
        // Row 2 top = row1_top(58) + height(50) + gap(8) = 116 — NOT
        // 58 + 50 + 100 (bottom inset).
        assert_eq!(p.row_top(2), 116.0);
        assert_eq!(p.row_top(3), 174.0);
        // New total adds the bottom inset once, at the end.
        assert_eq!(p.total(), 174.0 + 50.0 + 100.0);
    }

    #[test]
    fn invalidate_resets_to_estimate() {
        let mut p = PrefixSumOffsets::new(3, 50.0, 8.0, 0.0, 0.0);
        p.set_row_height(0, 90.0);
        p.invalidate(0, 1);
        assert!(!p.is_measured(0));
        assert_eq!(p.row_top(1), 58.0); // back to estimate
    }
}
