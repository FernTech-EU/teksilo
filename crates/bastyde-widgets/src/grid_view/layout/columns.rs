// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`GridSizing`] → [`WidthPolicy`] mapping for the grid strategies.
//!
//! Every strategy derives its column count and per-column `(x, width)` the
//! same way from a [`GridSizing`]; only the *vertical* layout (uniform vs
//! variable vs waterfall) differs. The horizontal math itself lives in
//! [`crate::common::column_geometry`] — shared with `ColumnFlow`, which needs
//! the identical `auto-fill` column-count rule but knows nothing about tiles.
//! This module is only the `GridSizing`-shaped door onto it.

use bastyde_canvas::EdgeInsets;

use super::strategy::GridSizing;
pub(crate) use crate::common::column_geometry::ColumnGeometry;
use crate::common::column_geometry::WidthPolicy;

impl From<GridSizing> for WidthPolicy {
    fn from(sizing: GridSizing) -> Self {
        match sizing {
            GridSizing::Fixed { width, .. } => WidthPolicy::Fixed(width),
            GridSizing::FixedColumnCount { count, .. } => WidthPolicy::Count(count.max(1)),
            GridSizing::Adaptive {
                min_width,
                max_width,
                ..
            } => WidthPolicy::Adaptive {
                min: min_width,
                max: max_width,
            },
        }
    }
}

/// Build a [`ColumnGeometry`] from a [`GridSizing`]. The grid strategies'
/// entry point; equivalent to `ColumnGeometry::from_policy(sizing.into(), ..)`.
pub(crate) fn geometry_for(sizing: GridSizing, col_gap: f32, inset: EdgeInsets) -> ColumnGeometry {
    ColumnGeometry::from_policy(sizing.into(), col_gap, inset)
}

/// The column index containing content-space `x`, or `None` when `x` falls
/// in a column-gap, before the leading inset, or past the last column. The
/// closed-form inverse of [`ColumnGeometry::column_x`] — every strategy's
/// O(1)/O(log n) `index_at_point` override uses this instead of scanning
/// `tile_rect` per item. Derives the leading offset and column step from
/// `column_x(0, ..)` / `column_x(1, ..)` rather than `ColumnGeometry`'s
/// private fields, since this lives outside that module.
pub(crate) fn column_at(geometry: &ColumnGeometry, x: f32, viewport_width: f32) -> Option<usize> {
    let cols = geometry.column_count(viewport_width);
    let (x0, col_w) = geometry.column_x(0, viewport_width);
    if col_w <= 0.0 {
        return None;
    }
    let step = if cols > 1 {
        geometry.column_x(1, viewport_width).0 - x0
    } else {
        col_w
    };
    if step <= 0.0 {
        return None;
    }
    let rel = x - x0;
    if rel < 0.0 {
        return None;
    }
    let col = (rel / step) as usize;
    if col >= cols {
        return None;
    }
    // Reject a column-gap: the point must land within the tile itself, not
    // the gap trailing it.
    if rel - col as f32 * step > col_w {
        return None;
    }
    Some(col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_maps_min_and_max() {
        let p: WidthPolicy = GridSizing::Adaptive {
            min_width: 200.0,
            max_width: Some(400.0),
            height: 50.0,
        }
        .into();
        assert_eq!(
            p,
            WidthPolicy::Adaptive {
                min: 200.0,
                max: Some(400.0)
            }
        );
    }

    #[test]
    fn fixed_maps_width_and_drops_height() {
        let p: WidthPolicy = GridSizing::Fixed {
            width: 120.0,
            height: 90.0,
        }
        .into();
        assert_eq!(p, WidthPolicy::Fixed(120.0));
    }

    #[test]
    fn fixed_column_count_clamps_zero_to_one() {
        let p: WidthPolicy = GridSizing::FixedColumnCount {
            count: 0,
            height: 50.0,
        }
        .into();
        assert_eq!(p, WidthPolicy::Count(1));
    }

    #[test]
    fn geometry_for_matches_from_policy() {
        let sizing = GridSizing::Adaptive {
            min_width: 240.0,
            max_width: None,
            height: 50.0,
        };
        let g = geometry_for(sizing, 16.0, EdgeInsets::ZERO);
        // floor((1000 + 16) / (240 + 16)) = 3
        assert_eq!(g.column_count(1000.0), 3);
    }

    fn gapped_columns() -> ColumnGeometry {
        // 100-wide columns, 10px gap → 4 columns fit in 430px, step = 110.
        ColumnGeometry::from_policy(WidthPolicy::Fixed(100.0), 10.0, EdgeInsets::ZERO)
    }

    #[test]
    fn column_at_finds_column_and_exact_edges() {
        let g = gapped_columns();
        assert_eq!(column_at(&g, 0.0, 430.0), Some(0), "leading edge of col 0");
        assert_eq!(
            column_at(&g, 100.0, 430.0),
            Some(0),
            "trailing edge of col 0 is inclusive"
        );
        assert_eq!(column_at(&g, 50.0, 430.0), Some(0));
        assert_eq!(
            column_at(&g, 110.0, 430.0),
            Some(1),
            "leading edge of col 1"
        );
        assert_eq!(
            column_at(&g, 429.0, 430.0),
            Some(3),
            "inside the last column"
        );
    }

    #[test]
    fn column_at_returns_none_in_a_gap() {
        let g = gapped_columns();
        assert_eq!(column_at(&g, 105.0, 430.0), None, "midway through the gap");
    }

    #[test]
    fn column_at_returns_none_before_the_leading_inset_and_past_the_last_column() {
        let g = ColumnGeometry::from_policy(
            WidthPolicy::Fixed(100.0),
            10.0,
            EdgeInsets {
                leading: 20.0,
                trailing: 0.0,
                top: 0.0,
                bottom: 0.0,
            },
        );
        assert_eq!(column_at(&g, 10.0, 450.0), None, "before the leading inset");
        assert_eq!(column_at(&g, 10000.0, 450.0), None, "past the last column");
    }
}
