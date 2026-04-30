//! Trait abstracting per-row navigation for `TableView` and `TreeTable`.
//!
//! Flat tables answer arrow-up/down by stepping a contiguous index;
//! tree tables need to ask their `TreeSlice` for the next visible
//! flat-index. The shared keyboard handler in `keyboard.rs` is generic
//! over this trait so both widgets re-use the same key-matrix logic.

use std::rc::Rc;

/// Per-row navigator. Implementations live alongside the consumer
/// widget — `FlatNavigator` here for `TableView`; `TreeNavigator` in
/// `tree_table.rs` for `TreeTable`.
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

    /// Tree-only — depth of the row in the hierarchy. `None` for
    /// flat tables. Currently only exercised by `TreeNavigator`'s own
    /// test path; reserved for upcoming Shift+ArrowLeft "jump to
    /// parent" navigation in the shared keyboard module.
    #[allow(dead_code)]
    fn depth(&self, _row: usize) -> Option<usize> {
        None
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
