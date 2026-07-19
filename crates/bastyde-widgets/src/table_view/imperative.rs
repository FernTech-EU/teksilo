// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Imperative-API helpers shared by [`TableView`](crate::TableView) and
//! [`TreeTableView`](crate::TreeTableView).
//!
//! Both widgets expose the same "drive the table from code" surface — scroll to
//! a row, override a column width, pin a column, open a cell editor. The public
//! methods stay inherent on each widget (that is the discoverable API), but the
//! *logic* lives here once, so a fix lands on both instead of drifting.
//!
//! Everything takes the widget's signals/handles as parameters rather than the
//! widget itself, which keeps this module free of both concrete types.

use std::collections::HashMap;

use bastyde_core::signal::Signal;

use crate::common::row_metrics::SharedRowMetrics;
use crate::table_view::column::{Column, PinnedSide};

/// Scroll so that `row` is aligned to the top of the viewport.
///
/// A no-op before the first layout pass: `max_scroll_y` is still `0`, so the
/// clamp collapses any target to `0`.
pub(crate) fn scroll_to_row(
    row: usize,
    row_metrics: &SharedRowMetrics,
    scroll_y: &Signal<f32>,
    max_scroll_y: &Signal<f32>,
) {
    // `try_borrow_mut`: the metrics cell is also borrowed during layout and by
    // `row_height_fn`, so a call from inside a cell delegate or an activation
    // handler could re-enter. Skipping the scroll beats panicking.
    let Ok(mut metrics) = row_metrics.try_borrow_mut() else {
        return;
    };
    let target = metrics.row_top(row);
    drop(metrics);
    let max = max_scroll_y.get();
    scroll_y.set(target.clamp(0.0, max));
}

/// Scroll the minimum distance needed to make `row` visible.
///
/// A no-op before the first layout pass — `viewport_height` still holds its
/// construction placeholder then, so the computed offset would be measured
/// against a viewport that was never laid out.
pub(crate) fn ensure_row_visible(
    row: usize,
    row_metrics: &SharedRowMetrics,
    scroll_y: &Signal<f32>,
    max_scroll_y: &Signal<f32>,
    viewport_height: f32,
    laid_out: bool,
) {
    if !laid_out {
        return;
    }
    let Ok(mut metrics) = row_metrics.try_borrow_mut() else {
        return;
    };
    let scroll = scroll_y.get();
    let new_scroll =
        metrics.scroll_for_ensure_visible(row, scroll, viewport_height, max_scroll_y.get());
    drop(metrics);
    if (new_scroll - scroll).abs() > f32::EPSILON {
        scroll_y.set(new_scroll);
    }
}

/// Set or remove a single column's user-resized width override. A non-positive
/// or non-finite `width` removes the entry, reverting the column to its
/// declared width policy.
pub(crate) fn set_column_width(signal: &Signal<HashMap<String, f32>>, col_id: &str, width: f32) {
    let mut m = signal.get();
    if width.is_finite() && width > 0.0 {
        m.insert(col_id.to_string(), width);
    } else {
        m.remove(col_id);
    }
    signal.set(m);
}

/// Pin or unpin a single column. [`PinnedSide::None`] removes the override,
/// reverting the column to its declared [`Column::pinned`].
pub(crate) fn set_column_pinning(
    signal: &Signal<HashMap<String, PinnedSide>>,
    col_id: &str,
    side: PinnedSide,
) {
    let mut m = signal.get();
    if matches!(side, PinnedSide::None) {
        m.remove(col_id);
    } else {
        m.insert(col_id.to_string(), side);
    }
    signal.set(m);
}

/// Set or clear the filter text for a single column. An empty `text` removes
/// the entry.
pub(crate) fn set_filter(signal: &Signal<HashMap<String, String>>, col_id: &str, text: &str) {
    let mut m = signal.get();
    if text.is_empty() {
        m.remove(col_id);
    } else {
        m.insert(col_id.to_string(), text.to_string());
    }
    signal.set(m);
}

/// Resolve `(row, col_id)` to a `(row, display_position)` edit target.
///
/// Returns `None` — leaving any existing editor untouched — when `col_id` is
/// not a declared column, when it is not currently displayed, or when `row` is
/// outside the visible range. Without the row check an out-of-range
/// `begin_edit` would strand `editing_cell` on a row that can never match,
/// which nothing but an explicit `end_edit` would clear.
pub(crate) fn resolve_edit_target<T: 'static>(
    row: usize,
    col_id: &str,
    columns: &[Column<T>],
    display_indices: &[usize],
    row_count: usize,
) -> Option<(usize, usize)> {
    if row >= row_count {
        return None;
    }
    let decl_index = columns.iter().position(|c| c.id == col_id)?;
    let display_pos = display_indices.iter().position(|&i| i == decl_index)?;
    Some((row, display_pos))
}
