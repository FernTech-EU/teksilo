// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Trait abstracting per-row navigation for `TableView` and `TreeTableView`.
//!
//! Flat tables answer arrow-up/down by stepping a contiguous index;
//! tree tables need to ask their `TreeSlice` for the next visible
//! flat-index. The shared keyboard handler in `keyboard.rs` is generic
//! over this trait so both widgets re-use the same key-matrix logic.

use std::rc::Rc;

/// Per-row navigator. Implementations live alongside the consumer
/// widget — `FlatNavigator` here for `TableView`; `TreeNavigator` in
/// `tree_table_view.rs` for `TreeTableView`.
pub(crate) trait RowNavigator {
    fn row_count(&self) -> usize;

    fn next_row(&self, current: usize) -> Option<usize> {
        let n = self.row_count();
        if n == 0 {
            return None;
        }
        let next = current + 1;
        if next < n { Some(next) } else { None }
    }

    fn prev_row(&self, current: usize) -> Option<usize> {
        if current == 0 {
            None
        } else {
            Some(current - 1)
        }
    }

    fn first_row(&self) -> Option<usize> {
        if self.row_count() == 0 { None } else { Some(0) }
    }

    fn last_row(&self) -> Option<usize> {
        let n = self.row_count();
        if n == 0 { None } else { Some(n - 1) }
    }

    /// Tree-only — depth of the row in the hierarchy. `None` for flat tables,
    /// which is what makes [`parent_row`](Self::parent_row) inert there.
    fn depth(&self, _row: usize) -> Option<usize> {
        None
    }

    /// The row's parent in the current flattening, or `None` at the root (and
    /// always, for a flat table).
    ///
    /// Derived by scanning back for the nearest shallower row rather than
    /// asked of the source, because the flattening is the only shape both
    /// navigators expose — a parent is by construction the closest preceding
    /// row at a smaller depth. Bounded by the distance to the parent, on a
    /// keypress.
    fn parent_row(&self, row: usize) -> Option<usize> {
        let depth = self.depth(row)?;
        if depth == 0 {
            return None;
        }
        (0..row)
            .rev()
            .find(|&i| self.depth(i).is_some_and(|d| d < depth))
    }

    fn has_children(&self, _row: usize) -> bool {
        false
    }

    fn is_expanded(&self, _row: usize) -> bool {
        false
    }

    fn toggle_expanded(&self, _row: usize) {}
}

/// Trivial navigator over a length-providing closure. Used by
/// `TableView` directly.
pub(crate) struct FlatNavigator {
    pub(crate) len_fn: Rc<dyn Fn() -> usize>,
}

impl FlatNavigator {
    pub(crate) fn new(len_fn: Rc<dyn Fn() -> usize>) -> Self {
        Self { len_fn }
    }
}

impl RowNavigator for FlatNavigator {
    fn row_count(&self) -> usize {
        (self.len_fn)()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn flat_navigator_steps_through_rows() {
        let count = Rc::new(Cell::new(5));
        let c = count.clone();
        let nav = FlatNavigator::new(Rc::new(move || c.get()));
        assert_eq!(nav.row_count(), 5);
        assert_eq!(nav.first_row(), Some(0));
        assert_eq!(nav.last_row(), Some(4));
        assert_eq!(nav.next_row(0), Some(1));
        assert_eq!(nav.next_row(4), None);
        assert_eq!(nav.prev_row(0), None);
        assert_eq!(nav.prev_row(2), Some(1));
    }

    #[test]
    fn empty_navigator_returns_none_for_first_last() {
        let nav = FlatNavigator::new(Rc::new(|| 0));
        assert_eq!(nav.first_row(), None);
        assert_eq!(nav.last_row(), None);
        assert_eq!(nav.next_row(0), None);
        assert_eq!(nav.prev_row(0), None);
    }

    #[test]
    fn flat_default_tree_methods_are_noops() {
        let nav = FlatNavigator::new(Rc::new(|| 5));
        assert_eq!(nav.depth(0), None);
        assert!(!nav.has_children(0));
        assert!(!nav.is_expanded(0));
        nav.toggle_expanded(0); // no-op
    }
}
