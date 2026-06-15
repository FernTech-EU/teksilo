// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared column geometry for grid strategies.
//!
//! Every strategy derives its column count and per-column `(x, width)` the
//! same way from a [`GridSizing`]; only the *vertical* layout (uniform vs
//! variable vs waterfall) differs. `ColumnGeometry` factors out that shared
//! horizontal math.

use bastyde_canvas::EdgeInsets;

use super::strategy::GridSizing;

/// How a column's width is determined.
#[derive(Debug, Clone, Copy)]
pub(crate) enum WidthPolicy {
    /// Fixed tile width; tiles are not stretched.
    Fixed(f32),
    /// Explicit column count; tiles stretch to an equal share.
    Count(usize),
    /// Min tile width; tiles stretch to fill, clamped to `max`.
    Adaptive { min: f32, max: Option<f32> },
}

/// Horizontal layout: column count + per-column geometry.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ColumnGeometry {
    policy: WidthPolicy,
    col_gap: f32,
    inset: EdgeInsets,
}

impl ColumnGeometry {
    pub(crate) fn new(sizing: GridSizing, col_gap: f32, inset: EdgeInsets) -> Self {
        let policy = match sizing {
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
        };
        Self {
            policy,
            col_gap: col_gap.max(0.0),
            inset,
        }
    }

    /// Usable content width inside the leading/trailing insets.
    pub(crate) fn available_width(&self, viewport_width: f32) -> f32 {
        (viewport_width - self.inset.horizontal()).max(0.0)
    }

    pub(crate) fn column_count(&self, viewport_width: f32) -> usize {
        match self.policy {
            WidthPolicy::Count(n) => n.max(1),
            WidthPolicy::Fixed(w) | WidthPolicy::Adaptive { min: w, .. } => {
                let avail = self.available_width(viewport_width);
                if w <= 0.0 {
                    return 1;
                }
                let n = ((avail + self.col_gap) / (w + self.col_gap)).floor() as i64;
                n.max(1) as usize
            }
        }
    }

    pub(crate) fn column_x(&self, col: usize, viewport_width: f32) -> (f32, f32) {
        let cols = self.column_count(viewport_width).max(1);
        let avail = self.available_width(viewport_width);
        let stretched = (avail - self.col_gap * (cols as f32 - 1.0)) / cols as f32;
        let col_w = match self.policy {
            WidthPolicy::Fixed(w) => w,
            WidthPolicy::Count(_) => stretched,
            WidthPolicy::Adaptive { max, .. } => match max {
                Some(mx) => stretched.min(mx),
                None => stretched,
            },
        }
        .max(0.0);
        let x = self.inset.leading + col as f32 * (col_w + self.col_gap);
        (x, col_w)
    }
}
