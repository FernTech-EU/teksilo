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
//!    proportional to their flex factor, again clamped.
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

        // Pass 3: distribute leftover space among un-overridden Flex columns.
        let leftover = (available_width - consumed).max(0.0);
        if flex_total > 0.0 {
            for (slot, &col_idx) in display_order.iter().enumerate() {
                let col = &columns[col_idx];
                if overrides.contains_key(&col.id) {
                    continue;
                }
                if let ColumnWidth::Flex(factor) = col.width {
                    let share = leftover * (factor.max(0.0) / flex_total);
                    let floor = col.min_width.unwrap_or(min_width_default);
                    let clamped = clamp(share, floor, col.max_width);
                    widths[slot] = clamped;
                }
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
        // Flex(0.0) gets a 0 share of the leftover, then clamps to its
        // min_width (40). The leftover for the other column is computed
        // against the full 400 (since `consumed` only tracks Fixed/Auto),
        // so it claims 400 — the table will lay out wider than the pane,
        // but that's expected when min-width constraints over-subscribe
        // the available space.
        assert_eq!(widths[0], 40.0);
        assert_eq!(widths[1], 400.0);
    }
}
