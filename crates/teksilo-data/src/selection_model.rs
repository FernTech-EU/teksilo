// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! SelectionModel — index-based selection state for collection widgets.
//!
//! [`SelectionModel`] manages which flat indices are selected in a
//! `ListView`, `TreeView`, `TableView`, or `GridView`. It is a
//! share-by-clone handle (`Rc<RefCell<…>>` internally): pass a clone to
//! each view that should share selection state. The current selection is
//! exposed as a reactive `Signal<BTreeSet<usize>>` so widgets can bind to
//! it without polling.
//!
//! Three selection behaviours are available via [`SelectionMode`]: `None`
//! (read-only / no interaction), `Single` (at most one item), and `Multi`
//! (Ctrl+click toggle + Shift+click range extension via an internal anchor).
//! Mutators automatically notify all `Signal` observers after every change,
//! and the helper methods `adjust_for_insert` / `adjust_for_remove` /
//! `adjust_for_move` keep selected indices consistent when the underlying
//! source mutates.
//!
//! ## When to use `SelectionModel` vs `KeyedSelectionModel`
//!
//! Use `SelectionModel` (this type) for views that are backed by a plain
//! `ListModel<T>` or a `SortFilterListModel<T>` where *position* is the
//! natural identity. Use [`crate::KeyedSelectionModel`] when items carry a
//! stable app-defined key (e.g. a `NodeId` or a UUID) and selection must
//! survive sort/filter rebuilds or window slides that renumber visible indices.
//!
//! ```rust
//! # use teksilo_data::{SelectionModel, SelectionMode};
//! let sel = SelectionModel::new(SelectionMode::Multi);
//! sel.select(2);         // clear-and-select index 2, anchor = 2
//! sel.toggle(5);         // add index 5 (Ctrl+click behaviour)
//! sel.extend_to(8);      // extend from anchor 5 to 8 (Shift+click behaviour)
//! assert!(sel.is_selected(2));
//! assert_eq!(sel.count(), 5); // 2, 5, 6, 7, 8
//! sel.clear();
//! assert_eq!(sel.count(), 0);
//! ```

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use teksilo_core::signal::Signal;

/// Selection behavior mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    /// No selection allowed.
    None,
    /// At most one item selected at a time.
    Single,
    /// Multiple items can be selected (Ctrl+click toggles, Shift+click extends).
    Multi,
}

/// Manages selection state for a collection widget.
///
/// The selection is exposed as a `Signal<BTreeSet<usize>>` so widgets can
/// observe changes reactively.
pub struct SelectionModel {
    mode: SelectionMode,
    selection: Signal<BTreeSet<usize>>,
    /// Anchor index for Shift+click range extension.
    /// Shared via Rc so clones see the same anchor state.
    anchor: Rc<Cell<Option<usize>>>,
    /// The selection as it stood when the current Shift gesture began — the
    /// set a range extension is unioned *with*.
    ///
    /// Without it, `extend_to` can only grow: `Shift+End` followed by
    /// `Shift+Home` would leave the whole collection selected instead of
    /// reversing, because every extension would union into the previous one.
    /// Committing a base at each non-extending mutation makes the extension a
    /// pure function of `(base, anchor, target)`, so shrinking a range
    /// deselects what it shrank past.
    ///
    /// The commit rule is what makes the disjoint workflow work: `select`
    /// clears it (a plain click replaces everything), while `toggle` commits
    /// the resulting selection (so a Ctrl+click followed by a Shift+click
    /// keeps the earlier picks and adds the range). Same design as
    /// `CellSelectionModel`, which has had it since it shipped.
    base: Rc<RefCell<BTreeSet<usize>>>,
    /// Whether a range extension is already in progress, so
    /// [`SelectionModel::extend_to_additive`] captures its base once per
    /// gesture rather than on every keystroke.
    extending: Rc<Cell<bool>>,
    /// Strong holder for the debug-registry adapter. Shared across
    /// clones; once all `SelectionModel` handles drop, the holder Rc
    /// reaches zero and the adapter is freed, marking the registry
    /// entry dead. `None` until `.debug_named()` is called.
    /// Compiled out in release.
    #[cfg(debug_assertions)]
    debug_adapter_holder: Rc<RefCell<Option<Rc<dyn crate::debug_registry::ModelDebug>>>>,
}

impl SelectionModel {
    /// Create a new selection model with the given mode.
    pub fn new(mode: SelectionMode) -> Self {
        Self {
            mode,
            selection: Signal::new(BTreeSet::new()),
            anchor: Rc::new(Cell::new(None)),
            base: Rc::new(RefCell::new(BTreeSet::new())),
            extending: Rc::new(Cell::new(false)),
            #[cfg(debug_assertions)]
            debug_adapter_holder: Rc::new(RefCell::new(None)),
        }
    }

    /// Commit `base` and end any range gesture in progress.
    ///
    /// Every mutator that is not itself an extension calls this, so the next
    /// `Shift` gesture starts from a known set rather than from whatever the
    /// last extension happened to leave behind.
    fn commit_base(&self, base: BTreeSet<usize>) {
        *self.base.borrow_mut() = base;
        self.extending.set(false);
    }

    /// The selection mode.
    pub fn mode(&self) -> SelectionMode {
        self.mode
    }

    /// Get a clone of the selection signal for reactive binding.
    pub fn selection_signal(&self) -> Signal<BTreeSet<usize>> {
        self.selection.clone()
    }

    /// Whether the given index is currently selected.
    pub fn is_selected(&self, index: usize) -> bool {
        self.selection.get().contains(&index)
    }

    /// The currently selected indices, sorted.
    pub fn selected_indices(&self) -> Vec<usize> {
        self.selection.get().into_iter().collect()
    }

    /// Number of selected items.
    pub fn count(&self) -> usize {
        self.selection.get().len()
    }

    /// Select a single index. In Single mode, clears previous selection.
    /// In Multi mode, clears previous and selects just this one (use `toggle`
    /// for Ctrl+click behavior). Sets the anchor for subsequent Shift+click.
    pub fn select(&self, index: usize) {
        if self.mode == SelectionMode::None {
            return;
        }
        let mut set = BTreeSet::new();
        set.insert(index);
        self.selection.set(set);
        self.anchor.set(Some(index));
        // A plain click replaces the selection, so a Shift gesture starting
        // here has nothing to preserve.
        self.commit_base(BTreeSet::new());
    }

    /// Toggle selection of a single index (for Ctrl+click in Multi mode).
    /// In Single mode, behaves like `select()`.
    pub fn toggle(&self, index: usize) {
        match self.mode {
            SelectionMode::None => {}
            SelectionMode::Single => self.select(index),
            SelectionMode::Multi => {
                let mut set = self.selection.get();
                if set.contains(&index) {
                    set.remove(&index);
                } else {
                    set.insert(index);
                }
                self.selection.set(set.clone());
                // The anchor moves to the toggled item in *either* direction —
                // the clause almost every reimplementation misses, and the one
                // that makes "Ctrl+arrow away, Ctrl+Space, then Shift+arrow"
                // extend from the new region rather than from wherever the
                // user started.
                self.anchor.set(Some(index));
                // Ctrl+click keeps what is already picked, so a Shift gesture
                // starting here extends *around* it.
                self.commit_base(set);
            }
        }
    }

    /// Extend the selection from the anchor to the given index (for Shift+click
    /// and Shift+navigation). In Single mode, behaves like `select()`.
    ///
    /// The result is `base ∪ anchor..=index`, where `base` is the selection as
    /// it stood at the last non-extending mutation — **not** the current
    /// selection. So the range tracks the anchor in both directions: reversing
    /// a Shift gesture shrinks it, and `Shift+End` followed by `Shift+Home`
    /// leaves one row selected rather than the whole collection. The anchor
    /// itself does not move.
    pub fn extend_to(&self, index: usize) {
        self.extend_from_base(index, false);
    }

    /// Extend from the anchor to `index`, keeping whatever was selected when
    /// this gesture began (Ctrl+Shift+navigation).
    ///
    /// The difference from [`extend_to`](Self::extend_to) is only which set the
    /// range is unioned with: a plain gesture starts from the base committed by
    /// the last click or toggle, while this one captures the live selection on
    /// its first keystroke, so a second disjoint range can be built without
    /// losing the first. Subsequent keystrokes in the same gesture reuse that
    /// capture, so the range still shrinks when reversed.
    pub fn extend_to_additive(&self, index: usize) {
        self.extend_from_base(index, true);
    }

    fn extend_from_base(&self, index: usize, additive: bool) {
        match self.mode {
            SelectionMode::None => {}
            SelectionMode::Single => self.select(index),
            SelectionMode::Multi => {
                if additive && !self.extending.get() {
                    *self.base.borrow_mut() = self.selection.get();
                }
                let anchor = self.anchor.get().unwrap_or(index);
                let start = anchor.min(index);
                let end = anchor.max(index);
                let mut set = self.base.borrow().clone();
                set.extend(start..=end);
                self.selection.set(set);
                self.extending.set(true);
                // Anchor stays at the original position
            }
        }
    }

    /// Replace the selection with `indices` (or, when `additive`, union them
    /// into the current selection). Used by rubber-band / marquee selection,
    /// where the selected set is an arbitrary subset rather than a range. In
    /// `Single` mode the highest index wins; `None` mode is a no-op.
    pub fn select_indices(&self, indices: impl IntoIterator<Item = usize>, additive: bool) {
        if self.mode == SelectionMode::None {
            return;
        }
        let mut set = if additive {
            self.selection.get()
        } else {
            BTreeSet::new()
        };
        set.extend(indices);
        if self.mode == SelectionMode::Single {
            let last = set.iter().next_back().copied();
            set = last.into_iter().collect();
        }
        self.selection.set(set.clone());
        // A marquee that replaces reads like a click; one that adds reads like
        // a Ctrl+click, and a Shift gesture after it must keep what it caught.
        self.commit_base(if additive { set } else { BTreeSet::new() });
    }

    /// Select all indices from 0 to count-1.
    ///
    /// A no-op in `None` mode, and also in `Single` mode — "select all" has
    /// no coherent meaning for a control that holds at most one item, and
    /// silently selecting one arbitrary row would be worse than doing
    /// nothing. This mirrors what the gated call sites already do
    /// (`ListView`'s Ctrl+A handler, which documents it as "Multi selection
    /// only — a no-op for Single / None, matching every list control", and
    /// `TableView`'s `select_all` helper, which matches only the Multi
    /// modes). Enforcing it here too keeps an ungated caller — `GridView`'s
    /// Ctrl+A handler is one — from breaking the `Single` invariant that
    /// every other mutator on this type upholds.
    pub fn select_all(&self, count: usize) {
        if self.mode == SelectionMode::None || self.mode == SelectionMode::Single {
            return;
        }
        let set: BTreeSet<usize> = (0..count).collect();
        self.selection.set(set.clone());
        self.commit_base(set);
    }

    /// Clear the selection.
    pub fn clear(&self) {
        self.selection.set(BTreeSet::new());
        self.anchor.set(None);
        self.commit_base(BTreeSet::new());
    }

    /// Adjust selection indices after items are inserted.
    /// Indices >= `start` are shifted up by `count`.
    pub fn adjust_for_insert(&self, start: usize, count: usize) {
        let old = self.selection.get();
        let mut new_set = BTreeSet::new();
        for &idx in &old {
            if idx >= start {
                new_set.insert(idx + count);
            } else {
                new_set.insert(idx);
            }
        }
        if new_set != old {
            self.selection.set(new_set);
        }
        if let Some(a) = self.anchor.get()
            && a >= start
        {
            self.anchor.set(Some(a + count));
        }
        // The gesture base is index-space state exactly like the selection is,
        // so it has to follow the same shift — otherwise the next Shift
        // extension unions in rows the user never picked.
        self.remap_base(|idx| Some(if idx >= start { idx + count } else { idx }));
    }

    /// Adjust selection indices after items are removed.
    /// Indices in `start..start+count` are deselected; indices above are shifted down.
    pub fn adjust_for_remove(&self, start: usize, count: usize) {
        let old = self.selection.get();
        let end = start + count;
        let mut new_set = BTreeSet::new();
        for &idx in &old {
            if idx < start {
                new_set.insert(idx);
            } else if idx >= end {
                new_set.insert(idx - count);
            }
            // Indices in start..end are dropped
        }
        if new_set != old {
            self.selection.set(new_set);
        }
        if let Some(a) = self.anchor.get() {
            if a >= end {
                self.anchor.set(Some(a - count));
            } else if a >= start {
                self.anchor.set(None);
            }
        }
        self.remap_base(|idx| {
            if idx < start {
                Some(idx)
            } else if idx >= end {
                Some(idx - count)
            } else {
                None
            }
        });
    }

    /// Adjust selection indices after a block of `count` items moved from
    /// `from` to `to` (a post-removal index, matching `ListModel::move_item`).
    /// Selected indices follow their items, so a dragged row stays selected.
    pub fn adjust_for_move(&self, from: usize, to: usize, count: usize) {
        if from == to || count == 0 {
            return;
        }
        let old = self.selection.get();
        let new_set: BTreeSet<usize> = old
            .iter()
            .map(|&idx| crate::map_index_after_move(idx, from, to, count))
            .collect();
        if new_set != old {
            self.selection.set(new_set);
        }
        if let Some(a) = self.anchor.get() {
            self.anchor
                .set(Some(crate::map_index_after_move(a, from, to, count)));
        }
        self.remap_base(|idx| Some(crate::map_index_after_move(idx, from, to, count)));
    }

    /// Rewrite the gesture base through the same index map the selection just
    /// took, dropping the entries the map answers `None` for.
    fn remap_base(&self, map: impl Fn(usize) -> Option<usize>) {
        let mut base = self.base.borrow_mut();
        if base.is_empty() {
            return;
        }
        *base = base.iter().filter_map(|&idx| map(idx)).collect();
    }

    /// Drop the range anchor when a projection has renumbered the rows under
    /// it, so the next `Shift` gesture starts from the cursor rather than from
    /// a row that has since moved.
    ///
    /// A sort/filter proxy signals a blanket reset rather than a per-row
    /// delta, so `adjust_for_*` never runs and an index anchor silently comes
    /// to mean a different row. Views that read
    /// `first_changed_index()` from `SortFilterListModel` / `TreeSlice` /
    /// `TreeDataSlice` / `SortFilterTreeModel` call this with it: everything
    /// before that index still means what it meant, so an anchor there
    /// survives. Qt hit the same bug and fixed it by making the anchor a
    /// persistent index; `KeyedSelectionModel` avoids it by construction.
    pub fn invalidate_anchor_from(&self, first_changed: usize) {
        if self.anchor.get().is_some_and(|a| a >= first_changed) {
            self.anchor.set(None);
        }
        self.remap_base(|idx| (idx < first_changed).then_some(idx));
    }
}

impl Clone for SelectionModel {
    fn clone(&self) -> Self {
        Self {
            mode: self.mode,
            selection: self.selection.clone(),
            anchor: self.anchor.clone(),
            base: self.base.clone(),
            extending: self.extending.clone(),
            #[cfg(debug_assertions)]
            debug_adapter_holder: self.debug_adapter_holder.clone(),
        }
    }
}

impl SelectionModel {
    /// Register this selection model with the debug inspector under
    /// `name`. In release builds (`!cfg(debug_assertions)`) this is a
    /// no-op pass-through so call sites stay free of `#[cfg]` lines.
    ///
    /// Idempotent on repeated calls — the latest registration wins.
    /// The registration drops automatically when the last
    /// `SelectionModel` handle is freed (the strong adapter `Rc` lives
    /// inside a shared holder; the registry holds only a `Weak`).
    pub fn debug_named(self, _name: impl Into<String>) -> Self {
        #[cfg(debug_assertions)]
        {
            let adapter: Rc<dyn crate::debug_registry::ModelDebug> = Rc::new(SelectionModelDebug {
                selection: self.selection.clone(),
                mode: self.mode,
            });
            crate::debug_registry::register(_name.into(), Rc::downgrade(&adapter));
            *self.debug_adapter_holder.borrow_mut() = Some(adapter);
        }
        self
    }
}

#[cfg(debug_assertions)]
struct SelectionModelDebug {
    selection: Signal<BTreeSet<usize>>,
    mode: SelectionMode,
}

#[cfg(debug_assertions)]
impl crate::debug_registry::ModelDebug for SelectionModelDebug {
    fn kind(&self) -> &'static str {
        "SelectionModel"
    }
    fn len(&self) -> usize {
        self.selection.get().len()
    }
    fn debug_dump(&self, out: &mut dyn std::fmt::Write) {
        let _ = writeln!(out, "mode = {:?}", self.mode);
        let sel = self.selection.get();
        if sel.is_empty() {
            let _ = writeln!(out, "(empty)");
            return;
        }
        for i in sel.iter() {
            let _ = writeln!(out, "[{}]", i);
        }
    }
}

impl std::fmt::Debug for SelectionModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectionModel")
            .field("mode", &self.mode)
            .field("selected_count", &self.selection.get().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_select() {
        let model = SelectionModel::new(SelectionMode::Single);
        model.select(2);
        assert!(model.is_selected(2));
        assert!(!model.is_selected(0));
        assert_eq!(model.selected_indices(), vec![2]);

        model.select(5);
        assert!(!model.is_selected(2));
        assert!(model.is_selected(5));
    }

    #[test]
    fn multi_select_toggle() {
        let model = SelectionModel::new(SelectionMode::Multi);
        model.toggle(1);
        model.toggle(3);
        assert!(model.is_selected(1));
        assert!(model.is_selected(3));
        assert_eq!(model.count(), 2);

        model.toggle(1);
        assert!(!model.is_selected(1));
        assert!(model.is_selected(3));
    }

    #[test]
    fn multi_select_extend_range() {
        let model = SelectionModel::new(SelectionMode::Multi);
        model.select(2); // anchor at 2
        model.extend_to(5); // extend from 2 to 5
        assert_eq!(model.selected_indices(), vec![2, 3, 4, 5]);
    }

    #[test]
    fn extend_backwards() {
        let model = SelectionModel::new(SelectionMode::Multi);
        model.select(5);
        model.extend_to(2);
        assert_eq!(model.selected_indices(), vec![2, 3, 4, 5]);
    }

    #[test]
    fn reversing_a_shift_gesture_shrinks_the_range() {
        // Shift+Down four times then Shift+Up twice must give back the rows it
        // took, not keep them. Unioning into the live selection instead of
        // recomputing from the anchor is what made this grow-only.
        let model = SelectionModel::new(SelectionMode::Multi);
        model.select(2);
        model.extend_to(6);
        assert_eq!(model.selected_indices(), vec![2, 3, 4, 5, 6]);
        model.extend_to(4);
        assert_eq!(model.selected_indices(), vec![2, 3, 4]);
        model.extend_to(2);
        assert_eq!(model.selected_indices(), vec![2]);
    }

    #[test]
    fn extending_across_the_anchor_replaces_rather_than_unions() {
        let model = SelectionModel::new(SelectionMode::Multi);
        model.select(5);
        model.extend_to(8);
        assert_eq!(model.selected_indices(), vec![5, 6, 7, 8]);
        // Crossing back past the anchor drops the far side entirely.
        model.extend_to(3);
        assert_eq!(model.selected_indices(), vec![3, 4, 5]);
    }

    #[test]
    fn shift_end_then_shift_home_selects_one_row_not_the_whole_list() {
        // The user-visible shape of the same bug: End then Home used to leave
        // everything selected.
        let model = SelectionModel::new(SelectionMode::Multi);
        model.select(4);
        model.extend_to(9); // Shift+End
        model.extend_to(0); // Shift+Home
        assert_eq!(model.selected_indices(), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn a_ctrl_toggle_moves_the_anchor_and_survives_the_next_shift_range() {
        // The Explorer disjoint workflow: pick 1, Ctrl-pick 5, then Shift to 7.
        // The range runs from the *toggled* row, and row 1 stays selected.
        let model = SelectionModel::new(SelectionMode::Multi);
        model.select(1);
        model.toggle(5);
        model.extend_to(7);
        assert_eq!(model.selected_indices(), vec![1, 5, 6, 7]);
        // And it still shrinks, without eating the disjoint pick.
        model.extend_to(6);
        assert_eq!(model.selected_indices(), vec![1, 5, 6]);
    }

    #[test]
    fn the_anchor_moves_when_a_toggle_deselects_too() {
        let model = SelectionModel::new(SelectionMode::Multi);
        model.select(3);
        model.toggle(3); // now empty, anchor at 3
        model.extend_to(5);
        assert_eq!(model.selected_indices(), vec![3, 4, 5]);
    }

    #[test]
    fn an_additive_extend_keeps_the_range_built_by_the_previous_gesture() {
        // Ctrl+Shift builds a second range without losing the first.
        let model = SelectionModel::new(SelectionMode::Multi);
        model.select(0);
        model.extend_to(2);
        assert_eq!(model.selected_indices(), vec![0, 1, 2]);
        model.toggle(6); // Ctrl+Space moves the cursor's anchor
        model.extend_to_additive(8);
        assert_eq!(model.selected_indices(), vec![0, 1, 2, 6, 7, 8]);
        // Still shrinks within the gesture, and still keeps the first range.
        model.extend_to_additive(7);
        assert_eq!(model.selected_indices(), vec![0, 1, 2, 6, 7]);
    }

    #[test]
    fn a_plain_extend_after_a_click_discards_everything_else() {
        let model = SelectionModel::new(SelectionMode::Multi);
        model.select_indices([1, 2, 8], false);
        model.select(4); // a plain click replaces
        model.extend_to(6);
        assert_eq!(model.selected_indices(), vec![4, 5, 6]);
    }

    #[test]
    fn an_additive_marquee_is_kept_by_a_following_shift_range() {
        let model = SelectionModel::new(SelectionMode::Multi);
        model.select(0);
        model.select_indices([7, 8], true);
        model.toggle(2);
        model.extend_to(4);
        assert_eq!(model.selected_indices(), vec![0, 2, 3, 4, 7, 8]);
    }

    #[test]
    fn the_gesture_base_follows_an_insert_and_a_remove() {
        let model = SelectionModel::new(SelectionMode::Multi);
        model.select(1);
        model.toggle(5); // base = {1, 5}, anchor = 5
        model.adjust_for_insert(0, 2); // everything shifts up by two
        model.extend_to(9); // anchor is now 7
        assert_eq!(model.selected_indices(), vec![3, 7, 8, 9]);

        let model = SelectionModel::new(SelectionMode::Multi);
        model.select(1);
        model.toggle(5);
        model.adjust_for_remove(0, 1); // base {1,5} -> {0,4}
        model.extend_to(6);
        assert_eq!(model.selected_indices(), vec![0, 4, 5, 6]);
    }

    #[test]
    fn a_reprojection_drops_an_anchor_it_has_renumbered() {
        let model = SelectionModel::new(SelectionMode::Multi);
        model.select(2);
        model.toggle(6);
        // A sort/filter proxy reports that everything from row 4 changed.
        model.invalidate_anchor_from(4);
        // The next Shift starts from the target itself rather than from a row
        // that now means something else, and the stale half of the base is gone.
        model.extend_to(8);
        assert_eq!(model.selected_indices(), vec![2, 8]);
    }

    #[test]
    fn single_mode_ignores_the_additive_extend_like_every_other_mutator() {
        let model = SelectionModel::new(SelectionMode::Single);
        model.select(3);
        model.extend_to_additive(7);
        assert_eq!(model.selected_indices(), vec![7]);
    }

    #[test]
    fn select_all() {
        let model = SelectionModel::new(SelectionMode::Multi);
        model.select_all(5);
        assert_eq!(model.selected_indices(), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn select_indices_replaces_then_adds() {
        let model = SelectionModel::new(SelectionMode::Multi);
        model.select(1);
        // Non-additive replaces.
        model.select_indices([4, 5], false);
        assert_eq!(model.selected_indices(), vec![4, 5]);
        // Additive unions.
        model.select_indices([2], true);
        assert_eq!(model.selected_indices(), vec![2, 4, 5]);
    }

    #[test]
    fn clear() {
        let model = SelectionModel::new(SelectionMode::Multi);
        model.select_all(3);
        model.clear();
        assert_eq!(model.count(), 0);
    }

    #[test]
    fn none_mode_ignores_all() {
        let model = SelectionModel::new(SelectionMode::None);
        model.select(1);
        assert_eq!(model.count(), 0);
        model.toggle(2);
        assert_eq!(model.count(), 0);
        model.select_all(10);
        assert_eq!(model.count(), 0);
    }

    #[test]
    fn adjust_for_insert() {
        let model = SelectionModel::new(SelectionMode::Multi);
        model.toggle(1);
        model.toggle(3);
        // Insert 2 items at index 2
        model.adjust_for_insert(2, 2);
        // 1 stays, 3 shifts to 5
        assert_eq!(model.selected_indices(), vec![1, 5]);
    }

    #[test]
    fn adjust_for_remove() {
        let model = SelectionModel::new(SelectionMode::Multi);
        model.toggle(1);
        model.toggle(3);
        model.toggle(5);
        // Remove 1 item at index 3
        model.adjust_for_remove(3, 1);
        // 1 stays, 3 removed, 5 shifts to 4
        assert_eq!(model.selected_indices(), vec![1, 4]);
    }

    #[test]
    fn adjust_for_move_follows_the_moved_item() {
        // [A,B,C,D], select A(0). move A from 0 to 2 -> [B,C,A,D].
        let model = SelectionModel::new(SelectionMode::Multi);
        model.toggle(0);
        model.adjust_for_move(0, 2, 1);
        assert_eq!(model.selected_indices(), vec![2], "selection followed A");
    }

    #[test]
    fn adjust_for_move_shifts_a_bystander_selection() {
        // [A,B,C,D], select B(1). move A from 0 to 2 -> [B,C,A,D]; B is now 0.
        let model = SelectionModel::new(SelectionMode::Multi);
        model.toggle(1);
        model.adjust_for_move(0, 2, 1);
        assert_eq!(model.selected_indices(), vec![0], "B shifted down to 0");
    }

    #[test]
    fn adjust_for_move_backwards() {
        // [A,B,C,D], select D(3). move D from 3 to 1 -> [A,D,B,C].
        let model = SelectionModel::new(SelectionMode::Multi);
        model.toggle(3);
        model.adjust_for_move(3, 1, 1);
        assert_eq!(model.selected_indices(), vec![1]);
    }

    #[test]
    fn signal_reactivity() {
        use std::cell::Cell;
        use std::rc::Rc;

        let model = SelectionModel::new(SelectionMode::Single);
        let signal = model.selection_signal();
        let changed = Rc::new(Cell::new(false));
        let c = changed.clone();
        let _handle = signal.observe(move |_| c.set(true));

        model.select(3);
        assert!(changed.get());
    }

    #[test]
    fn single_mode_extend_acts_as_select() {
        let model = SelectionModel::new(SelectionMode::Single);
        model.select(1);
        model.extend_to(5);
        // In single mode, extend_to just selects
        assert_eq!(model.selected_indices(), vec![5]);
    }
}
