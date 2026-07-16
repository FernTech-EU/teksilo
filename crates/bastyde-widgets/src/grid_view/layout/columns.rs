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
}
