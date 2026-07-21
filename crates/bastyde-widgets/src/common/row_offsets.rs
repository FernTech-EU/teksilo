// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Cumulative row-offset table for variable-height virtualization.
//!
//! `PrefixSumOffsets` maps row index → content-space top-y in O(log n) (and
//! the inverse, y → row, by binary search), rebuilding lazily only from the
//! first dirtied row. Shared by `GridView`'s variable-height strategies and
//! the 1-D row widgets (`ListView` / `TreeView` / `TableView` / `TreeTableView`
//! via `RowMetrics`). Holders keep it behind a `RefCell` so the `&self`
//! `place_children` pass can feed measured heights back without `&mut`.

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

/// Property-based tests for [`PrefixSumOffsets`].
///
/// `PrefixSumOffsets` is `pub(crate)`, so a `tests/` integration file can't
/// reach it at all; this module lives inline (after the existing `mod
/// tests`, so the example-based coverage stays the first thing a reader
/// hits) and reaches straight into the struct's private fields to seed
/// heights directly — deliberately bypassing [`set_row_height`]'s
/// jitter-absorption (it silently no-ops when the new height is within
/// `0.01` of the seeded `estimated`), which would otherwise make an
/// oracle-driven test's own setup lie about what heights are actually
/// installed.
///
/// This table is a binary search over a monotone cumulative-sum array —
/// classic proptest territory: `row_at`'s `partition_point` call assumes
/// the offsets slice is sorted, and the one thing that can make several
/// consecutive offsets compare *equal* (still sorted, but with ties) is a
/// run of zero-height rows combined with a zero gap. An earlier,
/// unfinished investigation (`zzz_debug_probe_zero_height_rows`) suspected
/// this tie could push `row_at`/`insertion_index` out of range or off the
/// row they should report; the generators below deliberately construct
/// exactly that degenerate shape (heavily-weighted-toward-zero heights,
/// heavily-weighted-toward-zero gaps, explicit runs of zero-height rows
/// around a guaranteed positive one) so the properties are exercised
/// against it, not just against "nice" tables.
///
/// Contracts asserted:
///   1. `row_top` is monotonically non-decreasing across the row index,
///      even across a zero-height/zero-gap tie run.
///   2. `row_at` always returns an index in `0..rows` (or `0` for an empty
///      table) for every finite `y` — negative, zero, in-range, and far
///      beyond the table's total.
///   3. `row_at` is monotone non-decreasing as `y` increases.
///   4. `row_at` agrees with an independent linear-scan oracle for any `y`
///      strictly inside a row of POSITIVE height, even when that row is
///      flanked by runs of zero-height rows.
///   5. `total()` conserves the sum of every row height plus the `rows-1`
///      inter-row gaps plus the insets (CONSERVATION, f32 epsilon).
///
/// 256 cases per property (the workspace default); override with
/// `PROPTEST_CASES=4096 cargo test -p bastyde-widgets --lib row_offsets`.
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    // Zero heights are the degenerate case under investigation: with a
    // zero gap they make consecutive prefix-sum offsets compare equal,
    // which is exactly what stresses `partition_point`'s tie-breaking in
    // `row_at`. Weighted so runs of zero are common, not rare.
    fn arb_height() -> impl Strategy<Value = f32> {
        prop_oneof![
            4 => Just(0.0_f32),
            1 => Just(1.0_f32),
            3 => 0.5f32..400.0f32,
        ]
    }

    // Bounded to at most 40 rows (well under the task's 60-row ceiling);
    // each row is one f32 plus one bool, so the largest table here is a
    // few hundred bytes — the thing this suite needs generous length for
    // is the *chance* of a long zero-height run, not memory headroom.
    fn arb_heights(max_len: usize) -> impl Strategy<Value = Vec<f32>> {
        prop::collection::vec(arb_height(), 0..=max_len)
    }

    // A run of guaranteed-zero rows, used to build explicit zero-height
    // "moats" around a row we then probe with the linear-scan oracle.
    fn arb_zero_run(max_len: usize) -> impl Strategy<Value = Vec<f32>> {
        prop::collection::vec(Just(0.0_f32), 0..=max_len)
    }

    fn arb_positive_height() -> impl Strategy<Value = f32> {
        0.5f32..400.0f32
    }

    // A zero gap is required to actually collide consecutive offsets;
    // weighted heavily toward it for the same reason as `arb_height`, but
    // a nonzero gap must stay reachable so both regimes get covered.
    fn arb_gap() -> impl Strategy<Value = f32> {
        prop_oneof![3 => Just(0.0_f32), 1 => 0.5f32..20.0f32]
    }

    fn arb_inset() -> impl Strategy<Value = f32> {
        prop_oneof![1 => Just(0.0_f32), 1 => 0.0f32..50.0f32]
    }

    // Wide enough to comfortably exceed any total this suite can build
    // (<=40 rows * (400 max height + 20 max gap) ~= 16_800), and explicitly
    // includes negative y and the exact origin.
    fn arb_y() -> impl Strategy<Value = f32> {
        prop_oneof![
            1 => Just(0.0_f32),
            1 => -500.0f32..0.0f32,
            1 => 0.0f32..2000.0f32,
            1 => Just(100_000.0_f32),
        ]
    }

    /// Independent re-derivation of a row's top offset by direct
    /// summation — deliberately NOT sharing code with
    /// `PrefixSumOffsets::rebuild`'s incremental/dirty-region bookkeeping,
    /// so it can catch a bug in that incremental machinery rather than
    /// just re-running it.
    fn linear_row_top(heights: &[f32], gap: f32, top_inset: f32, row: usize) -> f32 {
        let mut acc = top_inset;
        for h in &heights[..row.min(heights.len())] {
            acc += h + gap;
        }
        acc
    }

    fn expected_total(heights: &[f32], gap: f32, top_inset: f32, bottom_inset: f32) -> f32 {
        let rows = heights.len();
        if rows == 0 {
            return 0.0;
        }
        let sum_heights: f32 = heights.iter().sum();
        top_inset + sum_heights + (rows as f32 - 1.0) * gap + bottom_inset
    }

    /// Builds a table with EXACTLY the given heights. Goes straight at the
    /// private fields (this module is a descendant of `row_offsets`, so
    /// that's ordinary Rust privacy, not a visibility change) instead of
    /// looping `set_row_height`, which would silently no-op — and thus
    /// desync the oracle from what's actually installed — whenever a
    /// generated height lands within `0.01` of the placeholder `estimated`
    /// seed.
    fn build(heights: &[f32], gap: f32, top_inset: f32, bottom_inset: f32) -> PrefixSumOffsets {
        let mut p = PrefixSumOffsets::new(heights.len(), 0.0, gap, top_inset, bottom_inset);
        p.heights = heights.to_vec();
        p.measured = vec![true; heights.len()];
        p.dirty_from = Some(0);
        p
    }

    // ── 1. offsets are monotonically non-decreasing, zero-height runs included ──
    proptest! {
        #[test]
        fn row_top_is_monotonically_non_decreasing_across_the_row_index(
            heights in arb_heights(40),
            gap in arb_gap(),
            top_inset in arb_inset(),
            bottom_inset in arb_inset(),
        ) {
            let mut p = build(&heights, gap, top_inset, bottom_inset);
            let rows = heights.len();
            for i in 0..rows.saturating_sub(1) {
                let a = p.row_top(i);
                let b = p.row_top(i + 1);
                prop_assert!(
                    a <= b,
                    "row_top regressed: row_top({})={} > row_top({})={} (heights={:?}, gap={})",
                    i, a, i + 1, b, heights, gap,
                );
            }
            if rows > 0 {
                let last_top = p.row_top(rows - 1);
                let total = p.total();
                // `total()` and `row_top()` reach the same value by different
                // accumulation orders, so they can disagree in the last f32 ulp
                // (observed: 4.56097 vs 4.5609703 for a single zero-height row
                // under a 4.56 top inset). Compare with a tolerance scaled to
                // the magnitude involved — asserting an exact float ordering
                // across two summation paths tests IEEE rounding, not the
                // offset table.
                let tol = 1e-4 * total.abs().max(last_top.abs()).max(1.0);
                prop_assert!(
                    total >= last_top - tol,
                    "total()={} fell below the last row's own top {} by more than {} (heights={:?}, gap={})",
                    total, last_top, tol, heights, gap,
                );
            }
        }
    }

    // ── 2. row_at always returns an in-range index for every finite y ──
    proptest! {
        #[test]
        fn row_at_returns_an_in_range_index_for_every_finite_y(
            heights in arb_heights(40),
            gap in arb_gap(),
            y in arb_y(),
        ) {
            let mut p = build(&heights, gap, 0.0, 0.0);
            let rows = heights.len();
            let r = p.row_at(y);
            if rows == 0 {
                prop_assert_eq!(r, 0, "row_at on an empty table must be 0, got {}", r);
            } else {
                prop_assert!(
                    r < rows,
                    "row_at({}) = {} is out of range for {} rows (heights={:?}, gap={})",
                    y, r, rows, heights, gap,
                );
            }
        }
    }

    // ── 3. row_at is monotone non-decreasing in y ──
    proptest! {
        #[test]
        fn row_at_is_monotone_non_decreasing_in_y(
            heights in arb_heights(40),
            gap in arb_gap(),
            y1 in arb_y(),
            y2 in arb_y(),
        ) {
            let (lo, hi) = if y1 <= y2 { (y1, y2) } else { (y2, y1) };
            let mut p = build(&heights, gap, 0.0, 0.0);
            let r_lo = p.row_at(lo);
            let r_hi = p.row_at(hi);
            prop_assert!(
                r_lo <= r_hi,
                "row_at({}) = {} > row_at({}) = {}, not monotone (heights={:?}, gap={})",
                lo, r_lo, hi, r_hi, heights, gap,
            );
        }
    }

    // ── 4. row_at oracle: a y strictly inside a positive-height row's band ──
    proptest! {
        #[test]
        fn row_at_matches_a_linear_scan_oracle_inside_a_positive_band(
            zeros_before in arb_zero_run(15),
            target_height in arb_positive_height(),
            zeros_after in arb_zero_run(15),
            gap in arb_gap(),
            frac in 0.001f32..0.999f32,
        ) {
            let target_index = zeros_before.len();
            let mut heights = zeros_before.clone();
            heights.push(target_height);
            heights.extend(zeros_after.iter().copied());

            let mut p = build(&heights, gap, 0.0, 0.0);
            let target_top = linear_row_top(&heights, gap, 0.0, target_index);
            let y = target_top + target_height * frac;

            let r = p.row_at(y);
            let band_end = target_top + target_height;
            prop_assert_eq!(
                r, target_index,
                "y={} sits strictly inside row {}'s band [{}, {}) but row_at returned {} \
                 (heights={:?}, gap={})",
                y, target_index, target_top, band_end, r, heights, gap,
            );
        }
    }

    // ── 5. total() conserves the sum of heights, inter-row gaps and insets ──
    proptest! {
        #[test]
        fn total_conserves_the_sum_of_heights_and_gaps_and_insets(
            heights in arb_heights(40),
            gap in arb_gap(),
            top_inset in arb_inset(),
            bottom_inset in arb_inset(),
        ) {
            let mut p = build(&heights, gap, top_inset, bottom_inset);
            let expected = expected_total(&heights, gap, top_inset, bottom_inset);
            let actual = p.total();
            prop_assert!(
                (actual - expected).abs() < 0.05,
                "total()={} but the direct sum of heights+gaps+insets is {} \
                 (heights={:?}, gap={}, top_inset={}, bottom_inset={})",
                actual, expected, heights, gap, top_inset, bottom_inset,
            );
        }
    }
}
