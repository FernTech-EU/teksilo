// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Column-width resolution + per-pane horizontal layout.
//!
//! `ColumnSolver` resolves a list of `ColumnWidth` declarations against
//! the available pane width, in three passes:
//!
//! 1. `Fixed(px)` is clamped by `min_width` / `max_width`.
//! 2. `Auto` evaluates to a fallback width (the table's
//!    `min_column_width_default`; a future pass will probe the header label
//!    and visible cells).
//! 3. Remaining horizontal space is distributed among `Flex` columns
//!    proportional to their flex factor, iteratively: a column whose
//!    proportional share would violate its own `min_width`/`max_width`
//!    is pinned to that bound and drops out of the pool, and the
//!    leftover + flex-factor total it would have consumed is
//!    re-shared among the columns still in play. Repeats to a fixed
//!    point (the same clamp-and-redistribute shape as the framework's
//!    `LayoutResponse` shrink algorithm) so floor violations on one
//!    column don't starve or overrun its siblings.
//!
//! The output is a parallel `Vec<f32>` of resolved widths matching the
//! input column order.

use std::collections::HashMap;

use teksilo_canvas::Rect;

use super::PaneBoundaries;
use super::column::{Column, ColumnWidth};

/// Stateless solver — pure function in struct form so the call site reads
/// clearly and so future caching can hang off `&mut self` without breaking
/// the API.
pub(crate) struct ColumnSolver;

impl ColumnSolver {
    /// Test-only convenience: resolve in declaration order.
    #[cfg(test)]
    pub(crate) fn resolve<T: 'static>(
        columns: &[Column<T>],
        available_width: f32,
        min_width_default: f32,
        overrides: &HashMap<String, f32>,
    ) -> Vec<f32> {
        let order: Vec<usize> = (0..columns.len()).collect();
        Self::resolve_in_order(
            columns,
            &order,
            available_width,
            min_width_default,
            overrides,
        )
    }

    /// Resolve widths for the given columns, returning a `Vec<f32>` in
    /// **display order** (parallel to `display_order`). Each entry is
    /// the resolved width of `columns[display_order[i]]`.
    pub(crate) fn resolve_in_order<T: 'static>(
        columns: &[Column<T>],
        display_order: &[usize],
        available_width: f32,
        min_width_default: f32,
        overrides: &HashMap<String, f32>,
    ) -> Vec<f32> {
        if display_order.is_empty() {
            return Vec::new();
        }

        let mut widths = vec![0.0_f32; display_order.len()];
        let mut flex_total: f32 = 0.0;
        let mut consumed: f32 = 0.0;

        // Pass 1 + 2: Fixed, Auto, and any signal-overridden columns
        // resolve to concrete widths.
        for (slot, &col_idx) in display_order.iter().enumerate() {
            let col = &columns[col_idx];
            let floor = col.min_width.unwrap_or(min_width_default);
            if let Some(&override_w) = overrides.get(&col.id) {
                let clamped = clamp(override_w, floor, col.max_width);
                widths[slot] = clamped;
                consumed += clamped;
                continue;
            }
            match col.width {
                ColumnWidth::Fixed(px) => {
                    let clamped = clamp(px, floor, col.max_width);
                    widths[slot] = clamped;
                    consumed += clamped;
                }
                ColumnWidth::Auto => {
                    let clamped = clamp(floor, floor, col.max_width);
                    widths[slot] = clamped;
                    consumed += clamped;
                }
                ColumnWidth::Flex(factor) => {
                    flex_total += factor.max(0.0);
                }
            }
        }

        // Pass 3: distribute leftover space among un-overridden Flex
        // columns. See the module doc for the clamp-and-redistribute
        // shape; a single unclamped pass would let one column's floor
        // violation either starve its siblings (their share stays
        // computed against the pre-floor leftover) or, if the floor
        // exceeds the pre-floor share by only a little, silently push
        // the resolved total past the pane.
        let leftover = (available_width - consumed).max(0.0);
        if flex_total > 0.0 {
            struct FlexSlot {
                slot: usize,
                factor: f32,
                floor: f32,
                max: Option<f32>,
            }
            let mut pool: Vec<FlexSlot> = display_order
                .iter()
                .enumerate()
                .filter_map(|(slot, &col_idx)| {
                    let col = &columns[col_idx];
                    if overrides.contains_key(&col.id) {
                        return None;
                    }
                    match col.width {
                        ColumnWidth::Flex(factor) => Some(FlexSlot {
                            slot,
                            factor: factor.max(0.0),
                            floor: col.min_width.unwrap_or(min_width_default),
                            max: col.max_width,
                        }),
                        _ => None,
                    }
                })
                .collect();

            let mut pool_leftover = leftover;
            let mut pool_flex_total: f32 = pool.iter().map(|s| s.factor).sum();

            while !pool.is_empty() {
                if pool_flex_total <= 0.0 {
                    // No factor left to key a share off (every
                    // remaining column has a zero flex factor) —
                    // whatever's left falls back to each floor.
                    for slot in &pool {
                        widths[slot.slot] = slot.floor;
                    }
                    break;
                }
                // One round: shares are computed against this round's
                // leftover/total for every still-pooled column before
                // any of them are removed, so removal order within a
                // round never biases which columns clamp.
                let round_leftover = pool_leftover;
                let round_flex_total = pool_flex_total;
                let mut next_pool = Vec::with_capacity(pool.len());
                let mut any_clamped = false;
                for slot in pool {
                    let share = round_leftover * (slot.factor / round_flex_total);
                    let violates = share < slot.floor || slot.max.is_some_and(|m| share > m);
                    if violates {
                        let clamped = clamp(share, slot.floor, slot.max);
                        widths[slot.slot] = clamped;
                        pool_leftover -= clamped;
                        pool_flex_total -= slot.factor;
                        any_clamped = true;
                    } else {
                        next_pool.push(slot);
                    }
                }
                if !any_clamped {
                    // Fixed point: every remaining column's proportional
                    // share already fits within its bounds.
                    for slot in &next_pool {
                        let share = pool_leftover * (slot.factor / pool_flex_total);
                        widths[slot.slot] = share;
                    }
                    break;
                }
                pool_leftover = pool_leftover.max(0.0);
                pool = next_pool;
            }
        }

        widths
    }

    /// Sum of resolved widths. Used for pane partitioning.
    #[allow(dead_code)]
    pub(crate) fn total_width(widths: &[f32]) -> f32 {
        widths.iter().sum()
    }

    /// X-offset of column `i` relative to the pane's leading edge.
    /// Used for column-resize hit testing.
    #[allow(dead_code)]
    pub(crate) fn x_offset(widths: &[f32], i: usize) -> f32 {
        widths.iter().take(i).sum()
    }
}

fn clamp(value: f32, min: f32, max: Option<f32>) -> f32 {
    let m = max.unwrap_or(f32::INFINITY);
    value.max(min).min(m)
}

// ── Horizontal scroll / pane geometry ───────────────────────────────────────
//
// A row's (or the header's) `body_width`-wide band splits into up to three
// panes by `PaneBoundaries`: Leading-pinned columns anchor at the band's own
// leading edge, Trailing-pinned columns anchor at its trailing edge, and the
// columns in between (the Middle pane) scroll horizontally by `scroll_x`,
// clipped to whatever room the pinned panes leave. Every function below
// works in **logical** (reading-order) offsets — 0 is always the band's own
// leading edge — so a single physical mirror step at the call site (`rtl ?
// band_width - offset - width : offset`, the same convention `BodyRow` /
// `HeaderRow` already use for their flat, unpinned cumulative walk) handles
// RTL for pinned AND scrolled content alike; nothing here needs its own RTL
// branch.

/// Sum of the resolved widths in `widths[range]`, defensively clamped to the
/// slice length (a display-order / widths-vector length mismatch is a
/// pre-existing tolerated edge case elsewhere in this module — see
/// `BodyRow`/`HeaderRow`'s `fallback_w`).
fn sum_range(widths: &[f32], range: std::ops::Range<usize>) -> f32 {
    let start = range.start.min(widths.len());
    let end = range.end.min(widths.len()).max(start);
    widths[start..end].iter().sum()
}

/// `(leading_width, middle_content_width, trailing_width)` — the three
/// panes' resolved widths. `middle_content_width` is the *unclamped* sum of
/// the scrollable columns, i.e. the horizontal analogue of
/// `RowMetrics::total_height` — it can exceed the viewport, which is exactly
/// what makes scrolling necessary.
pub(crate) fn pane_widths(widths: &[f32], boundaries: PaneBoundaries) -> (f32, f32, f32) {
    let leading = sum_range(widths, 0..boundaries.leading_count);
    let middle = sum_range(widths, boundaries.leading_count..boundaries.middle_end);
    let trailing = sum_range(widths, boundaries.middle_end..widths.len());
    (leading, middle, trailing)
}

/// Width left for the Middle pane's own viewport once the pinned panes take
/// their share of `band_width`. Floors at 0 (pinned columns alone can
/// outgrow the band on a very narrow table — same "just overflow" fallback
/// the rest of this module already accepts for an over-subscribed pane).
pub(crate) fn middle_viewport_width(
    band_width: f32,
    widths: &[f32],
    boundaries: PaneBoundaries,
) -> f32 {
    let (leading, _, trailing) = pane_widths(widths, boundaries);
    (band_width - leading - trailing).max(0.0)
}

/// Maximum `scroll_x` — `middle_content_width − middle_viewport_width`,
/// floored at 0 (content that already fits needs no scroll headroom).
pub(crate) fn max_scroll_x(band_width: f32, widths: &[f32], boundaries: PaneBoundaries) -> f32 {
    let (_, middle_content, _) = pane_widths(widths, boundaries);
    let viewport = middle_viewport_width(band_width, widths, boundaries);
    (middle_content - viewport).max(0.0)
}

/// Physical rects for the Leading / Middle / Trailing bands within a header
/// or body row's own `bounds` — the geometry `BodyRow::place_children` /
/// `HeaderRow::place_children` hand to their `RowBand` children, and that
/// `TableView`/`TreeTableView`'s own `paint()` re-derives to clip
/// pane-crossing root-painted decorations (vertical grid lines, the cell
/// focus ring) to the pane the target column actually belongs to.
///
/// A pane with no columns collapses to a zero-width rect at its edge —
/// harmless, since `RowBand` skips a band whose cell list is empty and a
/// zero-width clip paints nothing.
pub(crate) fn band_rects(
    bounds: Rect,
    widths: &[f32],
    boundaries: PaneBoundaries,
    rtl: bool,
) -> (Rect, Rect, Rect) {
    let (leading_w, _, trailing_w) = pane_widths(widths, boundaries);
    let middle_w = middle_viewport_width(bounds.width, widths, boundaries);
    if rtl {
        let leading = Rect::new(
            bounds.right() - leading_w,
            bounds.y,
            leading_w,
            bounds.height,
        );
        let trailing = Rect::new(bounds.x, bounds.y, trailing_w, bounds.height);
        let middle = Rect::new(bounds.x + trailing_w, bounds.y, middle_w, bounds.height);
        (leading, middle, trailing)
    } else {
        let leading = Rect::new(bounds.x, bounds.y, leading_w, bounds.height);
        let middle = Rect::new(bounds.x + leading_w, bounds.y, middle_w, bounds.height);
        let trailing = Rect::new(
            bounds.x + bounds.width - trailing_w,
            bounds.y,
            trailing_w,
            bounds.height,
        );
        (leading, middle, trailing)
    }
}

/// Logical x-offset (from the band's own leading edge, pre-RTL-mirror) of
/// display slot `slot` — Leading columns are anchored at the band start
/// (unaffected by `scroll_x`), Trailing columns are anchored at the band
/// end (also unaffected), and Middle columns run in between, shifted by
/// `-scroll_x`. `None` when `slot` is out of range.
pub(crate) fn column_logical_x(
    widths: &[f32],
    boundaries: PaneBoundaries,
    scroll_x: f32,
    band_width: f32,
    slot: usize,
) -> Option<f32> {
    if slot >= widths.len() {
        return None;
    }
    if slot < boundaries.leading_count {
        return Some(sum_range(widths, 0..slot));
    }
    let (leading_w, _, trailing_w) = pane_widths(widths, boundaries);
    if slot < boundaries.middle_end {
        let within = sum_range(widths, boundaries.leading_count..slot);
        return Some(leading_w - scroll_x + within);
    }
    let within = sum_range(widths, boundaries.middle_end..slot);
    Some(band_width - trailing_w + within)
}

/// Inverse of [`column_logical_x`]: given a **logical** (already
/// RTL-un-mirrored) drop x, find the display slot whose column midpoint it
/// falls before — the same "first column whose midpoint exceeds x" rule the
/// column-reorder drop handler always used, generalized to account for
/// pinning + the Middle pane's scroll offset. Returns `widths.len()` (append)
/// when `x` is past every column.
pub(crate) fn insertion_slot_at_x(
    widths: &[f32],
    boundaries: PaneBoundaries,
    scroll_x: f32,
    band_width: f32,
    x: f32,
) -> usize {
    let leading_end = boundaries.leading_count.min(widths.len());
    let mut cursor = 0.0;
    for i in 0..leading_end {
        let w = widths[i];
        if x < cursor + w * 0.5 {
            return i;
        }
        cursor += w;
    }
    let (leading_w, _, trailing_w) = pane_widths(widths, boundaries);
    let middle_end = boundaries.middle_end.min(widths.len()).max(leading_end);
    let mut cursor = leading_w - scroll_x;
    for i in leading_end..middle_end {
        let w = widths[i];
        if x < cursor + w * 0.5 {
            return i;
        }
        cursor += w;
    }
    let mut cursor = band_width - trailing_w;
    for i in middle_end..widths.len() {
        let w = widths[i];
        if x < cursor + w * 0.5 {
            return i;
        }
        cursor += w;
    }
    widths.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::TextWidget;
    use crate::table_view::column::{CellContext, Column};
    use teksilo_i18n::lit;

    fn col(id: &str, w: ColumnWidth) -> Column<&'static str> {
        Column::<&str>::new(id, lit!("h"), |_, _: &CellContext| {
            Box::new(TextWidget::new(lit!("x")))
        })
        .width(w)
    }

    #[test]
    fn fixed_widths_pass_through() {
        let cols = vec![
            col("a", ColumnWidth::Fixed(80.0)),
            col("b", ColumnWidth::Fixed(120.0)),
        ];
        let widths = ColumnSolver::resolve(&cols, 400.0, 32.0, &HashMap::new());
        assert_eq!(widths, vec![80.0, 120.0]);
    }

    #[test]
    fn flex_columns_split_leftover() {
        let cols = vec![
            col("a", ColumnWidth::Fixed(100.0)),
            col("b", ColumnWidth::Flex(1.0)),
            col("c", ColumnWidth::Flex(2.0)),
        ];
        // Leftover = 400 - 100 = 300; split 1:2 = 100 / 200.
        let widths = ColumnSolver::resolve(&cols, 400.0, 32.0, &HashMap::new());
        assert_eq!(widths[0], 100.0);
        assert!((widths[1] - 100.0).abs() < 0.01);
        assert!((widths[2] - 200.0).abs() < 0.01);
    }

    #[test]
    fn flex_clamps_to_min_width() {
        let cols = vec![
            col("a", ColumnWidth::Fixed(380.0)),
            col("b", ColumnWidth::Flex(1.0)).min_width(60.0),
        ];
        // Leftover only 20 px but min 60 — clamped up.
        let widths = ColumnSolver::resolve(&cols, 400.0, 32.0, &HashMap::new());
        assert_eq!(widths[1], 60.0);
    }

    #[test]
    fn flex_clamps_to_max_width() {
        let cols = vec![col("a", ColumnWidth::Flex(1.0)).max_width(120.0)];
        let widths = ColumnSolver::resolve(&cols, 400.0, 32.0, &HashMap::new());
        assert_eq!(widths[0], 120.0);
    }

    #[test]
    fn fixed_clamps_to_min_when_below() {
        let cols = vec![col("a", ColumnWidth::Fixed(10.0)).min_width(60.0)];
        let widths = ColumnSolver::resolve(&cols, 400.0, 32.0, &HashMap::new());
        assert_eq!(widths[0], 60.0);
    }

    #[test]
    fn fixed_clamps_to_max_when_above() {
        let cols = vec![col("a", ColumnWidth::Fixed(500.0)).max_width(180.0)];
        let widths = ColumnSolver::resolve(&cols, 400.0, 32.0, &HashMap::new());
        assert_eq!(widths[0], 180.0);
    }

    #[test]
    fn auto_falls_back_to_min_default() {
        let cols = vec![col("a", ColumnWidth::Auto)];
        let widths = ColumnSolver::resolve(&cols, 400.0, 48.0, &HashMap::new());
        assert_eq!(widths[0], 48.0);
    }

    #[test]
    fn auto_with_min_uses_min() {
        let cols = vec![col("a", ColumnWidth::Auto).min_width(100.0)];
        let widths = ColumnSolver::resolve(&cols, 400.0, 32.0, &HashMap::new());
        assert_eq!(widths[0], 100.0);
    }

    #[test]
    fn no_flex_no_overflow() {
        // Total fixed = 200, available = 400, no flex — leftover 200 stays
        // unallocated; the table pane is wider than the column total, which
        // is fine.
        let cols = vec![
            col("a", ColumnWidth::Fixed(80.0)),
            col("b", ColumnWidth::Fixed(120.0)),
        ];
        let widths = ColumnSolver::resolve(&cols, 400.0, 32.0, &HashMap::new());
        assert_eq!(ColumnSolver::total_width(&widths), 200.0);
    }

    #[test]
    fn x_offset_walks_widths() {
        let widths = vec![80.0, 120.0, 60.0];
        assert_eq!(ColumnSolver::x_offset(&widths, 0), 0.0);
        assert_eq!(ColumnSolver::x_offset(&widths, 1), 80.0);
        assert_eq!(ColumnSolver::x_offset(&widths, 2), 200.0);
        assert_eq!(ColumnSolver::x_offset(&widths, 3), 260.0);
    }

    #[test]
    fn empty_columns_returns_empty() {
        let cols: Vec<Column<&'static str>> = vec![];
        let widths = ColumnSolver::resolve(&cols, 400.0, 32.0, &HashMap::new());
        assert!(widths.is_empty());
    }

    #[test]
    fn override_pins_column_regardless_of_width_policy() {
        let cols = vec![
            col("a", ColumnWidth::Flex(1.0)),
            col("b", ColumnWidth::Flex(1.0)),
        ];
        let mut over = HashMap::new();
        over.insert("a".to_string(), 250.0);
        let widths = ColumnSolver::resolve(&cols, 400.0, 32.0, &over);
        // a is pinned to 250, b gets the leftover 150.
        assert_eq!(widths[0], 250.0);
        assert!((widths[1] - 150.0).abs() < 0.01, "got {}", widths[1]);
    }

    #[test]
    fn override_clamps_to_min_max() {
        let cols = vec![
            col("a", ColumnWidth::Flex(1.0))
                .min_width(80.0)
                .max_width(200.0),
        ];
        let mut over = HashMap::new();
        over.insert("a".to_string(), 5.0); // below min
        let widths = ColumnSolver::resolve(&cols, 400.0, 32.0, &over);
        assert_eq!(widths[0], 80.0);

        let mut over = HashMap::new();
        over.insert("a".to_string(), 999.0); // above max
        let widths = ColumnSolver::resolve(&cols, 400.0, 32.0, &over);
        assert_eq!(widths[0], 200.0);
    }

    #[test]
    fn negative_leftover_keeps_min() {
        let cols = vec![
            col("a", ColumnWidth::Fixed(500.0)),
            col("b", ColumnWidth::Flex(1.0)).min_width(50.0),
        ];
        // Available 400, fixed 500 — overflow. Flex still respects min.
        let widths = ColumnSolver::resolve(&cols, 400.0, 32.0, &HashMap::new());
        assert_eq!(widths[1], 50.0);
    }

    #[test]
    fn zero_flex_factor_treated_as_zero_share() {
        let cols = vec![
            col("a", ColumnWidth::Flex(0.0)).min_width(40.0),
            col("b", ColumnWidth::Flex(1.0)),
        ];
        let widths = ColumnSolver::resolve(&cols, 400.0, 32.0, &HashMap::new());
        // Flex(0.0) gets a 0 share of the leftover (400), which is below
        // its min_width (40) — it's pinned to 40 and drops out of the
        // pool. That 40 px is subtracted from the leftover *before* `b`
        // (the only remaining pooled column) claims the rest, so the two
        // resolved widths sum to exactly the pane instead of overflowing
        // it.
        assert_eq!(widths[0], 40.0);
        assert_eq!(widths[1], 360.0);
    }

    #[test]
    fn flex_min_width_redistributes_to_siblings() {
        let cols = vec![
            col("fixed", ColumnWidth::Fixed(100.0)),
            col("a", ColumnWidth::Flex(1.0)),
            col("b", ColumnWidth::Flex(1.0)).min_width(200.0),
        ];
        // Leftover after the fixed column is 300, split evenly 1:1 —
        // 150 apiece — but `b`'s min_width (200) wins its round and
        // pins it there. The 200 it now consumes (not its 150 share) is
        // subtracted from the leftover before `a`'s share is
        // recomputed in the next round, so `a` settles at 100 instead
        // of its stale first-round share of 150 — and the three
        // resolved widths sum to exactly the 400 px pane rather than
        // overflowing it by `b`'s 50 px shortfall.
        let widths = ColumnSolver::resolve(&cols, 400.0, 32.0, &HashMap::new());
        assert_eq!(widths[0], 100.0);
        assert_eq!(widths[1], 100.0);
        assert_eq!(widths[2], 200.0);
        assert_eq!(ColumnSolver::total_width(&widths), 400.0);
    }

    #[test]
    fn flex_min_widths_that_oversubscribe_the_pane_still_overflow() {
        // When the floors alone exceed the available width, redistribution
        // can't help — floors win and the resolved total overflows the
        // pane, same as a single non-iterative clamp would produce.
        let cols = vec![
            col("a", ColumnWidth::Flex(1.0)).min_width(300.0),
            col("b", ColumnWidth::Flex(1.0)).min_width(300.0),
        ];
        let widths = ColumnSolver::resolve(&cols, 400.0, 32.0, &HashMap::new());
        assert_eq!(widths[0], 300.0);
        assert_eq!(widths[1], 300.0);
    }

    // ── Pane geometry / horizontal scroll ───────────────────────────────

    #[test]
    fn pane_widths_splits_leading_middle_trailing() {
        let widths = [50.0, 60.0, 70.0, 80.0, 90.0];
        // Slots 0..1 leading, 1..4 middle, 4.. trailing.
        let b = PaneBoundaries::new(1, 4);
        assert_eq!(pane_widths(&widths, b), (50.0, 210.0, 90.0));
    }

    #[test]
    fn pane_widths_all_middle_when_unpinned() {
        let widths = [50.0, 60.0, 70.0];
        let b = PaneBoundaries::new(0, 3);
        assert_eq!(pane_widths(&widths, b), (0.0, 180.0, 0.0));
    }

    #[test]
    fn middle_viewport_width_is_band_minus_pinned_panes() {
        let widths = [60.0, 100.0, 100.0, 100.0, 60.0];
        let b = PaneBoundaries::new(1, 4);
        // Band 400, leading 60, trailing 60 -> middle viewport 280.
        assert_eq!(middle_viewport_width(400.0, &widths, b), 280.0);
    }

    #[test]
    fn middle_viewport_width_floors_at_zero_when_pinned_panes_overflow() {
        let widths = [300.0, 100.0, 300.0];
        let b = PaneBoundaries::new(1, 2);
        // Pinned panes alone (600) already exceed the 400 px band.
        assert_eq!(middle_viewport_width(400.0, &widths, b), 0.0);
    }

    #[test]
    fn max_scroll_x_is_zero_when_content_fits() {
        let widths = [60.0, 100.0, 60.0];
        let b = PaneBoundaries::new(1, 2);
        // Middle content (100) fits the 280 px middle viewport (400-60-60).
        assert_eq!(max_scroll_x(400.0, &widths, b), 0.0);
    }

    #[test]
    fn max_scroll_x_clamps_after_a_pane_shrink() {
        // Wide band: plenty of scroll headroom.
        let widths = [60.0, 500.0, 60.0];
        let b = PaneBoundaries::new(1, 2);
        assert_eq!(max_scroll_x(400.0, &widths, b), 500.0 - 280.0);
        // The pane (band) shrinks — e.g. the window narrowed. A scroll
        // position computed against the old, larger max must still resolve
        // to a smaller-but-still-correct max against the new band width, not
        // go negative or panic.
        let narrower = max_scroll_x(300.0, &widths, b);
        assert_eq!(narrower, 500.0 - (300.0 - 120.0));
        assert!(narrower > 0.0);
        // Shrink until the pinned panes alone consume the whole band (the
        // middle viewport itself floors at 0) — the scroll headroom becomes
        // the full content width, never negative.
        assert_eq!(max_scroll_x(50.0, &widths, b), 500.0);
    }

    #[test]
    fn band_rects_ltr_places_leading_left_middle_center_trailing_right() {
        let widths = [60.0, 200.0, 60.0];
        let b = PaneBoundaries::new(1, 2);
        let bounds = Rect::new(10.0, 20.0, 400.0, 30.0);
        let (leading, middle, trailing) = band_rects(bounds, &widths, b, false);
        assert_eq!(leading, Rect::new(10.0, 20.0, 60.0, 30.0));
        assert_eq!(middle, Rect::new(70.0, 20.0, 280.0, 30.0));
        assert_eq!(trailing, Rect::new(350.0, 20.0, 60.0, 30.0));
    }

    #[test]
    fn band_rects_rtl_mirrors_leading_to_the_physical_right() {
        let widths = [60.0, 200.0, 60.0];
        let b = PaneBoundaries::new(1, 2);
        let bounds = Rect::new(10.0, 20.0, 400.0, 30.0);
        let (leading, middle, trailing) = band_rects(bounds, &widths, b, true);
        // Leading pinned -> physical right edge of the band.
        assert_eq!(leading, Rect::new(350.0, 20.0, 60.0, 30.0));
        // Trailing pinned -> physical left edge.
        assert_eq!(trailing, Rect::new(10.0, 20.0, 60.0, 30.0));
        assert_eq!(middle, Rect::new(70.0, 20.0, 280.0, 30.0));
    }

    #[test]
    fn column_logical_x_pinned_columns_ignore_scroll() {
        let widths = [60.0, 80.0, 200.0, 60.0];
        let b = PaneBoundaries::new(1, 3);
        for scroll in [0.0, 40.0, 999.0] {
            assert_eq!(
                column_logical_x(&widths, b, scroll, 400.0, 0),
                Some(0.0),
                "leading column never moves"
            );
            assert_eq!(
                column_logical_x(&widths, b, scroll, 400.0, 3),
                Some(400.0 - 60.0),
                "trailing column never moves"
            );
        }
    }

    #[test]
    fn column_logical_x_middle_column_shifts_left_by_scroll() {
        let widths = [60.0, 80.0, 200.0, 60.0];
        let b = PaneBoundaries::new(1, 3);
        // Middle pane starts right after the 60px leading pane.
        assert_eq!(column_logical_x(&widths, b, 0.0, 400.0, 1), Some(60.0));
        assert_eq!(column_logical_x(&widths, b, 25.0, 400.0, 1), Some(35.0));
        assert_eq!(
            column_logical_x(&widths, b, 25.0, 400.0, 2),
            Some(60.0 - 25.0 + 80.0)
        );
    }

    #[test]
    fn column_logical_x_out_of_range_is_none() {
        let widths = [60.0, 80.0];
        let b = PaneBoundaries::new(0, 2);
        assert_eq!(column_logical_x(&widths, b, 0.0, 400.0, 2), None);
    }

    #[test]
    fn insertion_slot_at_x_finds_pinned_and_scrolled_columns() {
        let widths = [60.0, 80.0, 200.0, 60.0];
        let b = PaneBoundaries::new(1, 3);
        // x = 0 is inside the (only) leading column's first half.
        assert_eq!(insertion_slot_at_x(&widths, b, 0.0, 400.0, 0.0), 0);
        // Well past everything -> append.
        assert_eq!(insertion_slot_at_x(&widths, b, 0.0, 400.0, 10_000.0), 4);
        // With no scroll, x just past the leading pane (60) lands in the
        // first middle column's first half (60..60+40).
        assert_eq!(insertion_slot_at_x(&widths, b, 0.0, 400.0, 65.0), 1);
        // Scrolling the middle pane right by 70 slides that same physical x
        // into what is now the SECOND middle column (slot 2): logical
        // column 1 now starts at 60-70 = -10, ends at 70; x=65 falls in its
        // second half, so the insertion point becomes column 2's slot.
        assert_eq!(insertion_slot_at_x(&widths, b, 70.0, 400.0, 65.0), 2);
    }
}
