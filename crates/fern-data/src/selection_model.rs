//! Selection state for collection widgets.
//!
//! `SelectionModel` manages which indices are selected in a `ListView` or
//! `TreeView`. Supports single-select, multi-select (Ctrl+click toggle),
//! and range-select (Shift+click extension).

use std::cell::Cell;
#[cfg(debug_assertions)]
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use fern_core::signal::Signal;

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
            #[cfg(debug_assertions)]
            debug_adapter_holder: Rc::new(RefCell::new(None)),
        }
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
                self.selection.set(set);
                self.anchor.set(Some(index));
            }
        }
    }

    /// Extend the selection from the anchor to the given index (for Shift+click).
    /// In Single mode, behaves like `select()`.
    pub fn extend_to(&self, index: usize) {
        match self.mode {
            SelectionMode::None => {}
            SelectionMode::Single => self.select(index),
            SelectionMode::Multi => {
                let anchor = self.anchor.get().unwrap_or(index);
                let start = anchor.min(index);
                let end = anchor.max(index);
                let mut set = self.selection.get();
                for i in start..=end {
                    set.insert(i);
                }
                self.selection.set(set);
                // Anchor stays at the original position
            }
        }
    }

    /// Select all indices from 0 to count-1.
    pub fn select_all(&self, count: usize) {
        if self.mode == SelectionMode::None {
            return;
        }
        let set: BTreeSet<usize> = (0..count).collect();
        self.selection.set(set);
    }

    /// Clear the selection.
    pub fn clear(&self) {
        self.selection.set(BTreeSet::new());
        self.anchor.set(None);
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
        if let Some(a) = self.anchor.get() {
            if a >= start {
                self.anchor.set(Some(a + count));
            }
        }
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
    }
}

impl Clone for SelectionModel {
    fn clone(&self) -> Self {
        Self {
            mode: self.mode,
            selection: self.selection.clone(),
            anchor: self.anchor.clone(),
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
            let adapter: Rc<dyn crate::debug_registry::ModelDebug> =
                Rc::new(SelectionModelDebug {
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
    fn select_all() {
        let model = SelectionModel::new(SelectionMode::Multi);
        model.select_all(5);
        assert_eq!(model.selected_indices(), vec![0, 1, 2, 3, 4]);
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
