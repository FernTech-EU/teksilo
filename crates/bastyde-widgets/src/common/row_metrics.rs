// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! 1-D row-layout abstraction for the virtualizing row widgets
//! (`ListView`, `TreeView`, `TableView`, `TreeTableView`).
//!
//! Three height modes behind one call surface, so every consumer of row
//! geometry (visible range, placement, scrollbar totals, ensure-visible,
//! keyboard paging, DnD insertion, paint passes) is written once against
//! `RowMetrics` instead of `index * height` arithmetic:
//!
//! - **Uniform** — the default fast path. Pure arithmetic, no allocation,
//!   bit-identical to the historical `index * (height + spacing)` math.
//! - **Exact** — a pure `fn(row) -> height` callback seeds a
//!   [`PrefixSumOffsets`] table. Deterministic, no measurement pass.
//! - **AutoMeasure** — rows seed at an estimate and are corrected from
//!   real height-for-width measurements fed back through
//!   [`observe_measured`](RowMetrics::observe_measured), which returns a
//!   scroll-anchor delta so content above the viewport stays put.
//!
//! Borrow discipline: widgets hold a [`SharedRowMetrics`]
//! (`Rc<RefCell<RowMetrics>>`). Every method here is a single
//! self-contained operation — call sites must never hold a borrow across
//! a call into the framework (`ctx.child_size`, signal sets, …). Helpers
//! that need several lookups (`visible_range`, `insertion_index`,
//! `scroll_for_ensure_visible`) live *inside* this type so call sites
//! never compose borrows.

use std::cell::RefCell;
use std::rc::Rc;

use super::row_offsets::PrefixSumOffsets;

/// Shared handle to a widget's row metrics — cloned into scroll
/// observers, DnD handlers, keyboard configs, and (for `TableView` /
/// `TreeTableView`) the body pane. Same idiom as `SharedColumnWidths`.
pub(crate) type SharedRowMetrics = Rc<RefCell<RowMetrics>>;

/// Height source for [`RowMetrics`].
pub(crate) enum RowMode {
    /// Every row is `item_height` tall, `spacing` apart.
    Uniform { item_height: f32, spacing: f32 },
    /// Per-row height from a pure callback (same index + same data →
    /// same height). Re-swept from the first changed index on every
    /// model change.
    Exact(Rc<dyn Fn(usize) -> f32>),
    /// Rows seed at `estimated` and are corrected by measurement.
    AutoMeasure { estimated: f32 },
}

impl std::fmt::Debug for RowMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Uniform {
                item_height,
                spacing,
            } => f
                .debug_struct("Uniform")
                .field("item_height", item_height)
                .field("spacing", spacing)
                .finish(),
            Self::Exact(_) => f.debug_tuple("Exact").field(&"<fn>").finish(),
            Self::AutoMeasure { estimated } => f
                .debug_struct("AutoMeasure")
                .field("estimated", estimated)
                .finish(),
        }
    }
}

/// Builder-side height-mode selection shared by the four row widgets.
/// Widgets keep one of these plus their `item_height` / `spacing`
/// fields, and re-materialize their [`RowMetrics`] whenever any of the
/// three changes — so `.spacing(..)` after `.item_height_fn(..)` (or any
/// other ordering) composes correctly. Last mode setter wins.
#[derive(Default)]
pub(crate) enum HeightSource {
    #[default]
    Uniform,
    Exact(Rc<dyn Fn(usize) -> f32>),
    Auto {
        estimated: f32,
    },
}

impl HeightSource {
    /// Materialize fresh metrics for this mode. `item_height` is the
    /// Uniform row height (ignored by the other modes); `spacing` is the
    /// inter-row gap in every mode.
    pub(crate) fn make_metrics(&self, item_height: f32, spacing: f32) -> RowMetrics {
        match self {
            Self::Uniform => RowMetrics::uniform(item_height, spacing),
            Self::Exact(f) => RowMetrics::exact(f.clone(), spacing),
            Self::Auto { estimated } => RowMetrics::auto_measure(*estimated, spacing),
        }
    }
}

impl std::fmt::Debug for HeightSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Uniform => f.write_str("Uniform"),
            Self::Exact(_) => f.write_str("Exact(<fn>)"),
            Self::Auto { estimated } => write!(f, "Auto {{ estimated: {estimated} }}"),
        }
    }
}

/// Row geometry for one widget: heights, cumulative offsets, and the
/// derived queries every virtualization consumer needs.
#[derive(Debug)]
pub(crate) struct RowMetrics {
    mode: RowMode,
    /// Inter-row spacing (`PrefixSumOffsets::gap` in the offset modes).
    spacing: f32,
    /// `None` in `Uniform` mode — the fast path allocates nothing.
    offsets: Option<PrefixSumOffsets>,
    count: usize,
}

impl RowMetrics {
    pub(crate) fn uniform(item_height: f32, spacing: f32) -> Self {
        Self {
            mode: RowMode::Uniform {
                item_height,
                spacing,
            },
            spacing,
            offsets: None,
            count: 0,
        }
    }

    pub(crate) fn exact(height_fn: Rc<dyn Fn(usize) -> f32>, spacing: f32) -> Self {
        Self {
            mode: RowMode::Exact(height_fn),
            spacing,
            // The estimate is irrelevant in Exact mode (every row is
            // seeded from the callback), but `PrefixSumOffsets` wants one.
            offsets: Some(PrefixSumOffsets::new(0, 1.0, spacing, 0.0, 0.0)),
            count: 0,
        }
    }

    pub(crate) fn auto_measure(estimated: f32, spacing: f32) -> Self {
        let estimated = estimated.max(1.0);
        Self {
            mode: RowMode::AutoMeasure { estimated },
            spacing,
            offsets: Some(PrefixSumOffsets::new(0, estimated, spacing, 0.0, 0.0)),
            count: 0,
        }
    }

    /// Whether `place_children` must run the measure pass (AutoMeasure).
    pub(crate) fn needs_measure(&self) -> bool {
        matches!(self.mode, RowMode::AutoMeasure { .. })
    }

    /// Whether any non-uniform mode is active (offset-table-backed).
    #[allow(dead_code)]
    pub(crate) fn is_uniform(&self) -> bool {
        matches!(self.mode, RowMode::Uniform { .. })
    }

    fn step(&self) -> f32 {
        match &self.mode {
            RowMode::Uniform {
                item_height,
                spacing,
            } => item_height + spacing,
            _ => 0.0,
        }
    }

    /// Grow or shrink to `count` rows, preserving existing heights.
    /// Exact mode seeds appended rows from the callback (an append is
    /// O(appended), not O(n)).
    pub(crate) fn resize(&mut self, count: usize) {
        let old = self.count;
        self.count = count;
        if let Some(off) = &mut self.offsets {
            off.resize(count);
            if count > old
                && let RowMode::Exact(f) = &self.mode
            {
                for i in old..count {
                    off.set_row_height(i, f(i));
                }
            }
        }
    }

    /// Drop every height (back to estimate / full callback re-sweep) and
    /// resize to `count`.
    pub(crate) fn reset(&mut self, count: usize) {
        self.count = count;
        if let Some(off) = &mut self.offsets {
            off.reset(count);
            if let RowMode::Exact(f) = &self.mode {
                for i in 0..count {
                    off.set_row_height(i, f(i));
                }
            }
        }
    }

    /// Invalidate rows `[start, count)`: AutoMeasure drops them back to
    /// the estimate; Exact re-derives them from the callback (the
    /// estimate is meaningless there); Uniform is a no-op.
    pub(crate) fn invalidate_from(&mut self, start: usize) {
        let count = self.count;
        match (&self.mode, &mut self.offsets) {
            (RowMode::Exact(f), Some(off)) => {
                for i in start..count {
                    off.set_row_height(i, f(i));
                }
            }
            (RowMode::AutoMeasure { .. }, Some(off)) => {
                off.invalidate(start, count);
            }
            _ => {}
        }
    }

    /// The single entry point for data observers: `Some(d)` keeps the
    /// measured prefix `0..d`; `None` (unknown) drops everything.
    pub(crate) fn apply_divergence(&mut self, divergence: Option<usize>, new_count: usize) {
        match divergence {
            Some(d) => {
                self.resize(new_count);
                self.invalidate_from(d);
            }
            None => self.reset(new_count),
        }
    }

    /// Total content height for `count` rows (self-syncs the row count).
    pub(crate) fn total_height(&mut self, count: usize) -> f32 {
        self.resize(count);
        match &self.mode {
            RowMode::Uniform {
                item_height,
                spacing,
            } => {
                if count == 0 {
                    0.0
                } else {
                    count as f32 * (item_height + spacing) - spacing
                }
            }
            _ => self.offsets.as_mut().map(|o| o.total()).unwrap_or_default(),
        }
    }

    /// Content-space top Y of row `i`.
    pub(crate) fn row_top(&mut self, i: usize) -> f32 {
        match &self.mode {
            RowMode::Uniform { .. } => i as f32 * self.step(),
            _ => self
                .offsets
                .as_mut()
                .map(|o| o.row_top(i))
                .unwrap_or_default(),
        }
    }

    /// Height of row `i` (estimated when unmeasured in AutoMeasure mode).
    pub(crate) fn row_height(&mut self, i: usize) -> f32 {
        match &self.mode {
            RowMode::Uniform { item_height, .. } => *item_height,
            _ => self
                .offsets
                .as_ref()
                .map(|o| o.row_height(i))
                .unwrap_or_default(),
        }
    }

    /// The row whose vertical span (including its trailing gap) contains
    /// content-space `y`, clamped to a valid row.
    pub(crate) fn row_at(&mut self, y: f32) -> usize {
        if self.count == 0 {
            return 0;
        }
        match &self.mode {
            RowMode::Uniform { .. } => {
                let step = self.step();
                if step <= 0.0 {
                    return 0;
                }
                ((y.max(0.0) / step).floor() as usize).min(self.count - 1)
            }
            _ => self
                .offsets
                .as_mut()
                .map(|o| o.row_at(y))
                .unwrap_or_default(),
        }
    }

    /// Visible row range `[start, end)` for the given scroll position,
    /// padded by `buffer` rows each side (self-syncs the row count).
    ///
    /// Uniform mode reproduces the historical floor/ceil arithmetic
    /// bit-for-bit; the offset modes mirror its boundary semantics (a row
    /// whose top sits exactly at the viewport bottom is excluded).
    pub(crate) fn visible_range(
        &mut self,
        scroll: f32,
        viewport: f32,
        count: usize,
        buffer: usize,
    ) -> (usize, usize) {
        self.resize(count);
        if count == 0 {
            return (0, 0);
        }
        let scroll = scroll.max(0.0);
        match &self.mode {
            RowMode::Uniform { .. } => {
                let step = self.step();
                if step <= 0.0 {
                    return (0, count);
                }
                let first_visible = (scroll / step).floor() as usize;
                let last_visible = ((scroll + viewport) / step).ceil() as usize;
                let start = first_visible.saturating_sub(buffer);
                let end = (last_visible + buffer).min(count);
                (start, end)
            }
            _ => {
                let Some(off) = self.offsets.as_mut() else {
                    return (0, count);
                };
                let start = off.row_at(scroll).saturating_sub(buffer);
                // ε mirrors the `ceil` exclusion: a row whose top lands
                // exactly on the viewport bottom contributes zero pixels.
                let bottom = (scroll + viewport - 0.01).max(scroll);
                let end = (off.row_at(bottom) + 1 + buffer).min(count);
                (start, end)
            }
        }
    }

    /// DnD insertion point for content-space `y`: the boundary index in
    /// `0..=count`, snapping to the nearest row edge. Threshold form —
    /// `row_at(y + h/2)` is wrong for variable heights (it can overshoot
    /// a short row following a tall one). The threshold spans the row
    /// plus its trailing gap, preserving the historical
    /// `floor((y + step/2) / step)` behavior in Uniform mode.
    pub(crate) fn insertion_index(&mut self, y: f32) -> usize {
        if self.count == 0 || y < 0.0 {
            return 0;
        }
        let r = self.row_at(y);
        let top = self.row_top(r);
        let span = self.row_height(r) + self.spacing;
        // `>=` so the exact midpoint snaps forward, matching the
        // historical `floor((y + step/2) / step)` boundary.
        if y - top >= span * 0.5 {
            (r + 1).min(self.count)
        } else {
            r
        }
    }

    /// New scroll position that brings row `i` into view (unchanged when
    /// it already is). Mirrors the historical ensure-visible clamping.
    pub(crate) fn scroll_for_ensure_visible(
        &mut self,
        i: usize,
        scroll: f32,
        viewport: f32,
        max_scroll: f32,
    ) -> f32 {
        let top = self.row_top(i);
        let bottom = top + self.row_height(i);
        if top < scroll {
            top.max(0.0)
        } else if bottom > scroll + viewport {
            (bottom - viewport).clamp(0.0, max_scroll.max(0.0))
        } else {
            scroll
        }
    }

    /// Feed measured heights back (AutoMeasure only — returns `0.0`
    /// otherwise). Returns the scroll-anchor delta: the summed height
    /// change of rows strictly above the viewport top, i.e. how far
    /// `scroll_y` must shift so on-screen content doesn't jump. Row tops
    /// are snapshotted before any mutation, so an invalidate → re-measure
    /// cycle cannot double-count.
    pub(crate) fn observe_measured(&mut self, measured: &[(usize, f32)], scroll_y: f32) -> f32 {
        if !self.needs_measure() {
            return 0.0;
        }
        let Some(off) = self.offsets.as_mut() else {
            return 0.0;
        };
        // Force a clean table so the pre-change tops are consistent.
        let _ = off.total();
        let mut tops = Vec::with_capacity(measured.len());
        for &(r, h) in measured {
            tops.push((r, off.row_top(r), h));
        }
        let mut anchor_delta = 0.0_f32;
        for (r, top_before, h) in tops {
            let delta = off.set_row_height(r, h);
            if delta.abs() > 0.01 && top_before < scroll_y {
                anchor_delta += delta;
            }
        }
        anchor_delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uniform(h: f32, sp: f32, count: usize) -> RowMetrics {
        let mut m = RowMetrics::uniform(h, sp);
        m.resize(count);
        m
    }

    // ── Uniform parity with the historical arithmetic ───────────────────

    #[test]
    fn uniform_total_matches_old_formula() {
        let mut m = uniform(32.0, 0.0, 10);
        assert_eq!(m.total_height(10), 320.0);
        let mut m = uniform(40.0, 8.0, 3);
        // count * (h + sp) - sp = 3*48 - 8 = 136
        assert_eq!(m.total_height(3), 136.0);
        assert_eq!(m.total_height(0), 0.0);
    }

    #[test]
    fn uniform_visible_range_matches_old_floor_ceil() {
        let mut m = uniform(30.0, 0.0, 1000);
        // scroll 95, viewport 300: first = floor(95/30)=3, last = ceil(395/30)=14
        assert_eq!(m.visible_range(95.0, 300.0, 1000, 0), (3, 14));
        // With buffer 5: (0, 19) — saturating at the front.
        assert_eq!(m.visible_range(95.0, 300.0, 1000, 5), (0, 19));
        // Exact boundary: scroll+viewport = 390 → ceil(390/30) = 13 (the
        // row whose top is at the viewport bottom is excluded).
        assert_eq!(m.visible_range(90.0, 300.0, 1000, 0), (3, 13));
    }

    #[test]
    fn uniform_row_lookups() {
        let mut m = uniform(40.0, 8.0, 5);
        assert_eq!(m.row_top(2), 96.0); // 2 * 48
        assert_eq!(m.row_height(2), 40.0);
        assert_eq!(m.row_at(95.0), 1); // in row 1's trailing gap
        assert_eq!(m.row_at(96.0), 2);
        assert_eq!(m.row_at(-5.0), 0);
        assert_eq!(m.row_at(99999.0), 4);
    }

    #[test]
    fn uniform_insertion_index_matches_old_midpoint() {
        let mut m = uniform(30.0, 0.0, 10);
        // Old formula: floor((y + 15) / 30).
        assert_eq!(m.insertion_index(0.0), 0);
        assert_eq!(m.insertion_index(14.0), 0);
        assert_eq!(m.insertion_index(16.0), 1);
        assert_eq!(m.insertion_index(290.0), 10); // beyond last row → count
        assert_eq!(m.insertion_index(-3.0), 0);
        // With spacing the threshold is the step midpoint, like the old
        // floor((y + step/2)/step): h=40, sp=8 → step 48, midpoint 24.
        let mut m = uniform(40.0, 8.0, 10);
        assert_eq!(m.insertion_index(23.0), 0);
        assert_eq!(m.insertion_index(25.0), 1);
    }

    // ── Exact mode ───────────────────────────────────────────────────────

    fn heights_fn(hs: &'static [f32]) -> Rc<dyn Fn(usize) -> f32> {
        Rc::new(move |i| hs.get(i).copied().unwrap_or(10.0))
    }

    #[test]
    fn exact_positions_rows_from_callback() {
        let mut m = RowMetrics::exact(heights_fn(&[100.0, 20.0, 50.0]), 0.0);
        m.resize(3);
        assert_eq!(m.row_top(0), 0.0);
        assert_eq!(m.row_top(1), 100.0);
        assert_eq!(m.row_top(2), 120.0);
        assert_eq!(m.total_height(3), 170.0);
        assert_eq!(m.row_at(119.0), 1);
        assert_eq!(m.row_at(120.0), 2);
    }

    #[test]
    fn exact_with_spacing() {
        let mut m = RowMetrics::exact(heights_fn(&[100.0, 20.0, 50.0]), 8.0);
        m.resize(3);
        assert_eq!(m.row_top(1), 108.0);
        assert_eq!(m.row_top(2), 136.0);
        assert_eq!(m.total_height(3), 186.0);
    }

    #[test]
    fn exact_resize_seeds_only_appended_rows() {
        let mut m = RowMetrics::exact(heights_fn(&[100.0, 20.0, 50.0, 30.0]), 0.0);
        m.resize(2);
        assert_eq!(m.total_height(2), 120.0);
        m.resize(4); // appended rows come from the callback
        assert_eq!(m.row_top(3), 170.0);
        assert_eq!(m.total_height(4), 200.0);
    }

    #[test]
    fn exact_invalidate_from_reseeds_from_callback() {
        // The callback is the source of truth — invalidation must NOT
        // fall back to the (meaningless) estimate.
        let mut m = RowMetrics::exact(heights_fn(&[100.0, 20.0, 50.0]), 0.0);
        m.resize(3);
        m.invalidate_from(1);
        assert_eq!(m.row_top(2), 120.0);
        assert_eq!(m.total_height(3), 170.0);
    }

    // ── Insertion with variable heights (the midpoint-shift bug) ─────────

    #[test]
    fn insertion_index_threshold_form_with_tall_short_tall() {
        // Heights [40, 10, 40]: tops 0, 40, 50. The naive
        // row_at(y + h/2) form overshoots the short middle row.
        let mut m = RowMetrics::exact(heights_fn(&[40.0, 10.0, 40.0]), 0.0);
        m.resize(3);
        // y = 35: inside row 0's lower half → insert after row 0 = 1.
        assert_eq!(m.insertion_index(35.0), 1);
        // (Naive form: row_at(35 + 20) = row_at(55) = 2 — wrong.)
        // y = 42: upper half of the short row 1 → before row 1 = 1.
        assert_eq!(m.insertion_index(42.0), 1);
        // y = 47: lower half of row 1 → after row 1 = 2.
        assert_eq!(m.insertion_index(47.0), 2);
        // y = 85: past the midpoint of row 2 → end = 3.
        assert_eq!(m.insertion_index(85.0), 3);
    }

    // ── AutoMeasure mode ─────────────────────────────────────────────────

    #[test]
    fn auto_seeds_at_estimate_and_corrects_on_observe() {
        let mut m = RowMetrics::auto_measure(50.0, 0.0);
        m.resize(4);
        assert_eq!(m.total_height(4), 200.0);
        // Rows 0 and 1 measure to 30 — viewport at top, no anchor shift.
        let delta = m.observe_measured(&[(0, 30.0), (1, 30.0)], 0.0);
        assert_eq!(delta, 0.0);
        assert_eq!(m.row_top(1), 30.0);
        assert_eq!(m.row_top(2), 60.0);
        assert_eq!(m.total_height(4), 160.0); // 30+30+50+50
    }

    #[test]
    fn auto_anchor_delta_only_for_rows_strictly_above_viewport() {
        let mut m = RowMetrics::auto_measure(50.0, 0.0);
        m.resize(4);
        // Scrolled to row 2 (top = 100). Row 0 (top 0 < 100) grows by 40;
        // row 2 (top 100, not < 100) grows by 10.
        let delta = m.observe_measured(&[(0, 90.0), (2, 60.0)], 100.0);
        assert!((delta - 40.0).abs() < 0.01);
    }

    #[test]
    fn auto_invalidate_then_remeasure_does_not_double_count() {
        let mut m = RowMetrics::auto_measure(50.0, 0.0);
        m.resize(4);
        let d1 = m.observe_measured(&[(0, 90.0)], 100.0);
        assert!((d1 - 40.0).abs() < 0.01);
        m.invalidate_from(0); // back to estimate (top snapshot resets too)
        // Re-measuring to the same 90 from the estimated 50 is a fresh
        // +40 from the *estimated* position — correct, not a double-count
        // of the first pass (the table itself reverted by -40 at
        // invalidate time).
        let d2 = m.observe_measured(&[(0, 90.0)], 100.0);
        assert!((d2 - 40.0).abs() < 0.01);
    }

    #[test]
    fn auto_resize_preserves_measured_prefix() {
        let mut m = RowMetrics::auto_measure(50.0, 0.0);
        m.resize(2);
        m.observe_measured(&[(0, 90.0)], 0.0);
        m.resize(4);
        assert_eq!(m.row_top(1), 90.0); // measurement survives
        assert_eq!(m.row_top(2), 140.0); // appended rows at estimate
    }

    #[test]
    fn auto_sub_epsilon_jitter_is_absorbed() {
        let mut m = RowMetrics::auto_measure(50.0, 0.0);
        m.resize(2);
        m.observe_measured(&[(0, 90.0)], 0.0);
        let delta = m.observe_measured(&[(0, 90.005)], 100.0);
        assert_eq!(delta, 0.0);
        assert_eq!(m.row_top(1), 90.0);
    }

    // ── apply_divergence ─────────────────────────────────────────────────

    #[test]
    fn apply_divergence_keeps_measured_prefix() {
        let mut m = RowMetrics::auto_measure(50.0, 0.0);
        m.resize(3);
        m.observe_measured(&[(0, 90.0), (1, 70.0), (2, 60.0)], 0.0);
        // Append: divergence = old len 3, new count 5.
        m.apply_divergence(Some(3), 5);
        assert_eq!(m.row_top(1), 90.0);
        assert_eq!(m.row_top(2), 160.0);
        assert_eq!(m.row_top(3), 220.0); // 90+70+60, new rows at estimate
        // Change at index 1: prefix [0,1) survives, the rest reverts.
        m.apply_divergence(Some(1), 5);
        assert_eq!(m.row_top(1), 90.0);
        assert_eq!(m.row_top(2), 140.0); // 90 + estimate
    }

    #[test]
    fn apply_divergence_none_resets_everything() {
        let mut m = RowMetrics::auto_measure(50.0, 0.0);
        m.resize(2);
        m.observe_measured(&[(0, 90.0)], 0.0);
        m.apply_divergence(None, 2);
        assert_eq!(m.row_top(1), 50.0);
    }

    // ── ensure-visible ───────────────────────────────────────────────────

    #[test]
    fn scroll_for_ensure_visible_variable_heights() {
        let mut m = RowMetrics::exact(heights_fn(&[100.0, 20.0, 50.0, 200.0]), 0.0);
        m.resize(4);
        // Row 3 spans 170..370. Viewport 100 tall at scroll 0 → scroll to
        // bottom - viewport = 270 (clamped by max_scroll).
        assert_eq!(m.scroll_for_ensure_visible(3, 0.0, 100.0, 270.0), 270.0);
        // Row 0 above the viewport when scrolled to 150 → its top.
        assert_eq!(m.scroll_for_ensure_visible(0, 150.0, 100.0, 270.0), 0.0);
        // Row 2 (120..170) already inside viewport 100..200 → unchanged.
        assert_eq!(m.scroll_for_ensure_visible(2, 100.0, 100.0, 270.0), 100.0);
    }

    #[test]
    fn offsets_visible_range_boundary_matches_ceil_semantics() {
        // Uniform-equivalent heights through the Exact path must produce
        // the same range as Uniform at an exact boundary.
        let mut e = RowMetrics::exact(Rc::new(|_| 30.0), 0.0);
        e.resize(1000);
        let mut u = uniform(30.0, 0.0, 1000);
        assert_eq!(
            e.visible_range(90.0, 300.0, 1000, 0),
            u.visible_range(90.0, 300.0, 1000, 0)
        );
        assert_eq!(
            e.visible_range(95.0, 300.0, 1000, 0),
            u.visible_range(95.0, 300.0, 1000, 0)
        );
    }
}
