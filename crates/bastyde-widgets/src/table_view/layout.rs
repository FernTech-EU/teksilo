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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::TextWidget;
    use crate::table_view::column::{CellContext, Column};
    use bastyde_i18n::lit;

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
}
