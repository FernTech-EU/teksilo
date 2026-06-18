// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared substrate for the data views' source-owned drag-and-drop + lazy
//! loading.
//!
//! Centralizes the vocabulary the four data views (`ListView` / `TreeView` /
//! `TableView` / `TreeTableView`) share, so DnD validation (`can_accept`) and
//! the lazy placeholder are wired one way everywhere:
//!
//! - [`RowDrag`] — the non-generic intra-app drag payload a row emits. The
//!   receiving source distinguishes its OWN reorder (matching `source_view_id`)
//!   from a foreign drop, and translates `source_index` → its own key via
//!   `key_at`, so the source's `Key` type never leaks into the view.
//! - [`DropIndicator`] — what `paint` renders; `allowed == false` is the
//!   pre-commit forbidden affordance.
//! - [`flat_insertion_target`] — maps a flat insertion index to the
//!   `(target, position)` pair `can_accept` / `accept_drop` expect.
//! - [`default_placeholder`] — the skeleton for a `Loading` row.

use std::sync::atomic::{AtomicUsize, Ordering};

use bastyde_core::widget::Widget;
use bastyde_data::DropPosition;

/// The intra-app drag payload a data-view row emits. Non-generic: the receiving
/// source compares `source_view_id` to decide SameView-vs-Foreign and maps
/// `source_index` → its own key.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RowDrag {
    pub(crate) source_index: usize,
    pub(crate) source_view_id: usize,
}

/// A drop indicator the data views' `paint` renders. `allowed == false` paints a
/// muted line where an accepted-drop line would be — the pre-commit "you can't
/// drop here" affordance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DropIndicator {
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) allowed: bool,
}

/// A process-unique id distinguishing data-view instances (for SameView drop
/// detection when several views share one source).
pub(crate) fn next_view_id() -> usize {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Map a flat insertion index (`0..=len`) to the `(target_index, position)` pair
/// a `ListDataSource::can_accept` / `accept_drop` understands. `None` for an
/// empty list. Insertion *before* row `i` is `(i, Before)`; insertion past the
/// end is `(len-1, After)`.
pub(crate) fn flat_insertion_target(
    insertion: usize,
    len: usize,
) -> Option<(usize, DropPosition)> {
    if len == 0 {
        None
    } else if insertion >= len {
        Some((len - 1, DropPosition::After))
    } else {
        Some((insertion, DropPosition::Before))
    }
}

/// The default skeleton for a `Loading` row — a muted inset bar. The row's
/// placement sizes it to the row's height and width.
pub(crate) fn default_placeholder() -> Box<dyn Widget> {
    use crate::primitives::{Padding, RectWidget};
    Box::new(
        Padding::uniform(6.0).child(
            RectWidget::new()
                .background(bastyde_tokens::SurfaceRole::Hover)
                .corner_radius(bastyde_tokens::CornerRadius::uniform(4.0)),
        ),
    )
}
