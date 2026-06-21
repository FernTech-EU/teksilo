// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Selection types for `TableView` and `TreeTableView`.
//!
//! For row selection (`SingleRow` / `MultiRow`) the table re-uses the
//! existing `bastyde_data::SelectionModel` keyed by visible row index.
//!
//! For cell selection (`SingleCell` / `MultiCell`) the table uses
//! [`CellSelectionModel`] which tracks `(row, col)` pairs as a
//! `Signal<BTreeSet<(usize, usize)>>`. Anchor-rectangle extension supports
//! Excel-style Shift-Arrow / Shift-Click semantics.

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use bastyde_core::signal::Signal;

/// Selection mode for a `TableView` or `TreeTableView`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TableSelectionMode {
    /// No selection allowed.
    None,
    /// At most one row selected at a time.
    SingleRow,
    /// Multiple rows selectable; Ctrl-click toggles, Shift-click extends.
    /// **Default.**
    #[default]
    MultiRow,
    /// Excel-style: at most one cell selected at a time.
    SingleCell,
    /// Excel-style: rectangular cell selection.
    MultiCell,
}

impl TableSelectionMode {
    /// Whether the mode operates on cells rather than entire rows.
    pub fn is_cell_mode(self) -> bool {
        matches!(self, Self::SingleCell | Self::MultiCell)
    }

    /// Whether the mode allows more than one entry to be selected.
    pub fn is_multi(self) -> bool {
        matches!(self, Self::MultiRow | Self::MultiCell)
    }
}

/// Cell-level selection state for `TableSelectionMode::SingleCell` /
/// `MultiCell`. Tracks `(row, col)` pairs in visible-index space.
///
/// Mirrors `bastyde_data::SelectionModel`'s API surface (signal-backed,
/// auto-adjustable on data mutations) but keyed by `(row, col)` instead of
/// `row` alone.
pub struct CellSelectionModel {
    mode: TableSelectionMode,
    selection: Signal<BTreeSet<(usize, usize)>>,
    anchor: Rc<Cell<Option<(usize, usize)>>>,
    /// Cells committed by prior clicks/toggles, kept *separate* from the live
    /// Shift-drag rectangle. `extend_to` recomputes the selection as
    /// `base ∪ rectangle(anchor, target)` each time, so a Shift+click to a
    /// smaller rectangle *shrinks* it (Excel semantics) instead of only ever
    /// growing it, while Ctrl-committed cells survive.
    base: Rc<RefCell<BTreeSet<(usize, usize)>>>,
}

impl CellSelectionModel {
    /// Construct a model. **Panics** if `mode` is not a cell mode —
    /// callers in row mode should use `bastyde_data::SelectionModel`.
    pub fn new(mode: TableSelectionMode) -> Self {
        assert!(
            mode.is_cell_mode(),
            "CellSelectionModel requires SingleCell or MultiCell mode (got {mode:?})"
        );
        Self {
            mode,
            selection: Signal::new(BTreeSet::new()),
            anchor: Rc::new(Cell::new(None)),
            base: Rc::new(RefCell::new(BTreeSet::new())),
        }
    }

    pub fn mode(&self) -> TableSelectionMode {
        self.mode
    }

    pub fn selection_signal(&self) -> Signal<BTreeSet<(usize, usize)>> {
        self.selection.clone()
    }

    pub fn is_selected(&self, row: usize, col: usize) -> bool {
        self.selection.get().contains(&(row, col))
    }

    pub fn count(&self) -> usize {
        self.selection.get().len()
    }

    /// Replace the selection with the single cell `(row, col)` and set
    /// the anchor.
    pub fn select(&self, row: usize, col: usize) {
        if self.mode == TableSelectionMode::None {
            return;
        }
        let mut s = BTreeSet::new();
        s.insert((row, col));
        self.selection.set(s);
        self.anchor.set(Some((row, col)));
        // A plain click starts a fresh range: nothing committed beneath the
        // (about-to-be-dragged) rectangle.
        self.base.borrow_mut().clear();
    }

    /// Toggle the cell `(row, col)` (Ctrl-click). In `SingleCell` mode
    /// this behaves like [`select`](Self::select).
    pub fn toggle(&self, row: usize, col: usize) {
        match self.mode {
            TableSelectionMode::None => {}
            TableSelectionMode::SingleCell => self.select(row, col),
            TableSelectionMode::MultiCell => {
                let mut s = self.selection.get();
                if !s.insert((row, col)) {
                    s.remove(&(row, col));
                }
                self.selection.set(s.clone());
                self.anchor.set(Some((row, col)));
                // Ctrl-click commits the whole current selection as the base,
                // so a subsequent Shift-extend keeps it while the new
                // rectangle (anchored here) can still grow and shrink.
                *self.base.borrow_mut() = s;
            }
            TableSelectionMode::SingleRow | TableSelectionMode::MultiRow => {}
        }
    }

    /// Extend the selection to include the rectangular range from the
    /// anchor to `(row, col)`. In `SingleCell` mode this falls back to
    /// [`select`](Self::select).
    pub fn extend_to(&self, row: usize, col: usize) {
        match self.mode {
            TableSelectionMode::None => {}
            TableSelectionMode::SingleCell => self.select(row, col),
            TableSelectionMode::MultiCell => {
                let anchor = self.anchor.get().unwrap_or((row, col));
                let r0 = anchor.0.min(row);
                let r1 = anchor.0.max(row);
                let c0 = anchor.1.min(col);
                let c1 = anchor.1.max(col);
                // Recompute from the committed base ∪ the current rectangle,
                // rather than merging into the previous selection — so moving
                // the Shift target inward SHRINKS the rectangle (Excel
                // semantics) instead of only ever accreting cells.
                let mut s = self.base.borrow().clone();
                for r in r0..=r1 {
                    for c in c0..=c1 {
                        s.insert((r, c));
                    }
                }
                self.selection.set(s);
                // Anchor stays in place.
            }
            TableSelectionMode::SingleRow | TableSelectionMode::MultiRow => {}
        }
    }

    /// Select every cell in `0..row_count × 0..col_count`.
    pub fn select_all(&self, row_count: usize, col_count: usize) {
        if self.mode == TableSelectionMode::None {
            return;
        }
        let mut s = BTreeSet::new();
        for r in 0..row_count {
            for c in 0..col_count {
                s.insert((r, c));
            }
        }
        self.selection.set(s.clone());
        // Treat select-all as a committed base, so a following Shift-extend
        // keeps it rather than collapsing to the bare rectangle.
        *self.base.borrow_mut() = s;
    }

    pub fn clear(&self) {
        self.selection.set(BTreeSet::new());
        self.anchor.set(None);
        self.base.borrow_mut().clear();
    }

    /// Re-key the committed `base` set with the same transform applied to the
    /// live selection on a row/column insert or remove, so a later Shift-extend
    /// unions a correctly-shifted base rather than stale coordinates.
    fn remap_base(&self, f: impl Fn((usize, usize)) -> Option<(usize, usize)>) {
        let mut b = self.base.borrow_mut();
        if b.is_empty() {
            return;
        }
        *b = b.iter().filter_map(|&cell| f(cell)).collect();
    }

    /// Adjust selection after `count` rows are inserted starting at
    /// `at_row`. Existing selections at indices `>= at_row` shift up.
    pub fn adjust_for_row_insert(&self, at_row: usize, count: usize) {
        let old = self.selection.get();
        let mut new = BTreeSet::new();
        for &(r, c) in &old {
            if r >= at_row {
                new.insert((r + count, c));
            } else {
                new.insert((r, c));
            }
        }
        if new != old {
            self.selection.set(new);
        }
        self.remap_base(|(r, c)| {
            Some(if r >= at_row { (r + count, c) } else { (r, c) })
        });
        if let Some((r, c)) = self.anchor.get()
            && r >= at_row
        {
            self.anchor.set(Some((r + count, c)));
        }
    }

    /// Adjust selection after `count` rows starting at `at_row` are
    /// removed. Selections within the removed range are dropped; later
    /// rows shift down.
    pub fn adjust_for_row_remove(&self, at_row: usize, count: usize) {
        let old = self.selection.get();
        let end = at_row + count;
        let mut new = BTreeSet::new();
        for &(r, c) in &old {
            if r < at_row {
                new.insert((r, c));
            } else if r >= end {
                new.insert((r - count, c));
            }
            // r in [at_row, end) is dropped
        }
        if new != old {
            self.selection.set(new);
        }
        self.remap_base(|(r, c)| {
            if r < at_row {
                Some((r, c))
            } else if r >= end {
                Some((r - count, c))
            } else {
                None
            }
        });
        if let Some((r, c)) = self.anchor.get() {
            if r >= end {
                self.anchor.set(Some((r - count, c)));
            } else if r >= at_row {
                self.anchor.set(None);
            }
        }
    }

    /// Adjust selection after a block of `count` rows moved from `from` to
    /// `to` (a post-removal index, matching `ListModel::move_item`). Selected
    /// cells follow their rows; columns are untouched.
    pub fn adjust_for_row_move(&self, from: usize, to: usize, count: usize) {
        if from == to || count == 0 {
            return;
        }
        let map = |r: usize| bastyde_data::map_index_after_move(r, from, to, count);
        let old = self.selection.get();
        let new: BTreeSet<(usize, usize)> = old.iter().map(|&(r, c)| (map(r), c)).collect();
        if new != old {
            self.selection.set(new);
        }
        self.remap_base(|(r, c)| Some((map(r), c)));
        if let Some((r, c)) = self.anchor.get() {
            self.anchor.set(Some((map(r), c)));
        }
    }

    /// Adjust selection after `count` columns are inserted at `at_col`.
    pub fn adjust_for_column_insert(&self, at_col: usize, count: usize) {
        let old = self.selection.get();
        let mut new = BTreeSet::new();
        for &(r, c) in &old {
            if c >= at_col {
                new.insert((r, c + count));
            } else {
                new.insert((r, c));
            }
        }
        if new != old {
            self.selection.set(new);
        }
        self.remap_base(|(r, c)| {
            Some(if c >= at_col { (r, c + count) } else { (r, c) })
        });
        if let Some((r, c)) = self.anchor.get()
            && c >= at_col
        {
            self.anchor.set(Some((r, c + count)));
        }
    }

    /// Adjust selection after `count` columns starting at `at_col` are
    /// removed.
    pub fn adjust_for_column_remove(&self, at_col: usize, count: usize) {
        let old = self.selection.get();
        let end = at_col + count;
        let mut new = BTreeSet::new();
        for &(r, c) in &old {
            if c < at_col {
                new.insert((r, c));
            } else if c >= end {
                new.insert((r, c - count));
            }
        }
        if new != old {
            self.selection.set(new);
        }
        self.remap_base(|(r, c)| {
            if c < at_col {
                Some((r, c))
            } else if c >= end {
                Some((r, c - count))
            } else {
                None
            }
        });
        if let Some((r, c)) = self.anchor.get() {
            if c >= end {
                self.anchor.set(Some((r, c - count)));
            } else if c >= at_col {
                self.anchor.set(None);
            }
        }
    }
}

impl Clone for CellSelectionModel {
    fn clone(&self) -> Self {
        Self {
            mode: self.mode,
            selection: self.selection.clone(),
            anchor: self.anchor.clone(),
            base: self.base.clone(),
        }
    }
}

impl std::fmt::Debug for CellSelectionModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CellSelectionModel")
            .field("mode", &self.mode)
            .field("selected_count", &self.selection.get().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_replaces_and_sets_anchor() {
        let m = CellSelectionModel::new(TableSelectionMode::MultiCell);
        m.select(2, 3);
        assert!(m.is_selected(2, 3));
        assert_eq!(m.count(), 1);
        m.select(5, 5);
        assert!(m.is_selected(5, 5));
        assert!(!m.is_selected(2, 3));
    }

    #[test]
    fn toggle_in_multi_cell_adds_and_removes() {
        let m = CellSelectionModel::new(TableSelectionMode::MultiCell);
        m.toggle(0, 0);
        m.toggle(1, 1);
        assert_eq!(m.count(), 2);
        m.toggle(0, 0);
        assert_eq!(m.count(), 1);
    }

    #[test]
    fn extend_in_multi_cell_fills_rectangle() {
        let m = CellSelectionModel::new(TableSelectionMode::MultiCell);
        m.select(2, 1);
        m.extend_to(4, 3);
        // 3 rows × 3 cols = 9 cells.
        assert_eq!(m.count(), 9);
        assert!(m.is_selected(3, 2));
    }

    #[test]
    fn extend_shrinks_when_target_moves_inward() {
        // Excel semantics: a second Shift extend to a smaller rectangle must
        // SHRINK the selection, not keep the larger one.
        let m = CellSelectionModel::new(TableSelectionMode::MultiCell);
        m.select(0, 0);
        m.extend_to(2, 2); // 3×3 = 9
        assert_eq!(m.count(), 9);
        m.extend_to(1, 1); // 2×2 = 4
        assert_eq!(m.count(), 4, "rectangle must shrink, not accrete");
        assert!(!m.is_selected(2, 2), "the dropped corner must be deselected");
    }

    #[test]
    fn ctrl_committed_cells_survive_a_later_shift_extend() {
        // Ctrl-click commits a base; a subsequent Shift-extend keeps it while
        // the new rectangle can still shrink.
        let m = CellSelectionModel::new(TableSelectionMode::MultiCell);
        m.select(0, 0); // {(0,0)}
        m.toggle(5, 5); // Ctrl-click → base now {(0,0),(5,5)}, anchor (5,5)
        m.extend_to(6, 6); // base ∪ rect((5,5),(6,6))
        assert!(m.is_selected(0, 0), "Ctrl-committed cell must survive");
        assert!(m.is_selected(6, 6));
        m.extend_to(5, 5); // shrink the rect back to a single cell
        assert!(m.is_selected(0, 0), "committed cell still there");
        assert!(!m.is_selected(6, 6), "shrunk-away cell gone");
        assert_eq!(m.count(), 2); // (0,0) committed + (5,5) rect
    }

    #[test]
    fn select_all_in_multi_cell() {
        let m = CellSelectionModel::new(TableSelectionMode::MultiCell);
        m.select_all(3, 4);
        assert_eq!(m.count(), 12);
    }

    #[test]
    fn single_cell_mode_keeps_one_selection() {
        let m = CellSelectionModel::new(TableSelectionMode::SingleCell);
        m.select(1, 1);
        m.toggle(2, 2);
        assert_eq!(m.count(), 1);
        assert!(m.is_selected(2, 2));
    }

    #[test]
    fn adjust_for_row_insert_shifts_higher_rows() {
        let m = CellSelectionModel::new(TableSelectionMode::MultiCell);
        m.select(2, 0);
        m.toggle(4, 0);
        m.adjust_for_row_insert(3, 2);
        assert!(m.is_selected(2, 0));
        assert!(m.is_selected(6, 0));
    }

    #[test]
    fn adjust_for_row_remove_drops_in_range() {
        let m = CellSelectionModel::new(TableSelectionMode::MultiCell);
        m.select(1, 0);
        m.toggle(3, 0);
        m.toggle(5, 0);
        m.adjust_for_row_remove(2, 2);
        // Row 1 stays, rows 2..4 are removed (so row 3 is dropped),
        // and row 5 shifts down by 2 to 3.
        assert!(m.is_selected(1, 0));
        // After the shift, row 3 is now occupied by what used to be row 5.
        assert!(m.is_selected(3, 0));
        let v: Vec<_> = m.selection_signal().get().into_iter().collect();
        assert_eq!(v, vec![(1, 0), (3, 0)]);
    }

    #[test]
    fn adjust_for_row_move_follows_cells() {
        let m = CellSelectionModel::new(TableSelectionMode::MultiCell);
        m.select(0, 1); // cell in row 0
        m.toggle(1, 2); // cell in row 1
        // Move row 0 to index 2: rows [B,C,A] — A's cell follows to row 2,
        // B's cell shifts down to row 0.
        m.adjust_for_row_move(0, 2, 1);
        let v: Vec<_> = m.selection_signal().get().into_iter().collect();
        assert_eq!(v, vec![(0, 2), (2, 1)]);
    }

    #[test]
    fn adjust_for_column_insert_and_remove() {
        let m = CellSelectionModel::new(TableSelectionMode::MultiCell);
        m.select(0, 1);
        m.toggle(0, 4);
        m.adjust_for_column_insert(2, 2);
        // col 1 stays, col 4 → 6
        let v: Vec<_> = m.selection_signal().get().into_iter().collect();
        assert_eq!(v, vec![(0, 1), (0, 6)]);

        m.adjust_for_column_remove(0, 2);
        let v: Vec<_> = m.selection_signal().get().into_iter().collect();
        // col 1 dropped (in range), col 6 shifts to 4.
        assert_eq!(v, vec![(0, 4)]);
    }

    #[test]
    #[should_panic]
    fn cell_model_rejects_row_mode() {
        let _ = CellSelectionModel::new(TableSelectionMode::MultiRow);
    }

    #[test]
    fn mode_is_cell_mode() {
        assert!(TableSelectionMode::SingleCell.is_cell_mode());
        assert!(TableSelectionMode::MultiCell.is_cell_mode());
        assert!(!TableSelectionMode::MultiRow.is_cell_mode());
    }
}
