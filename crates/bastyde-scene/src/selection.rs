//! Selection model for `Scene` items.
//!
//! Mirrors the API of `bastyde_data::SelectionModel` but keyed by
//! [`ItemId`] instead of `usize` — the natural address for scene
//! entries. Click-to-select, Ctrl+click toggle, Shift+click range,
//! and marquee box-select all flow through this single model;
//! `SceneView` paints a marquee overlay during the drag and
//! commits the result via `Scene::items_in_rect`.
//!
//! The selection set is exposed as a `Signal<BTreeSet<ItemId>>`
//! so `SceneItem` paint code can render selected items differently
//! by binding their colors / strokes to a derived signal:
//!
//! ```ignore
//! let selected = view.selection().selection_signal();
//! let stroke_color = selected.map(move |s| {
//!     if s.contains(&card_id) { Color::BLUE } else { Color::TRANSPARENT }
//! });
//! ```

use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;

use bastyde_canvas::Rect;
use bastyde_core::signal::Signal;

use crate::item::ItemId;
use crate::scene::Scene;

/// Selection-mode discriminator. Mirrors `bastyde_data::SelectionMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneSelectionMode {
    /// Selection disabled. Click does nothing, marquee does nothing.
    None,
    /// At most one item selected at a time.
    Single,
    /// Multiple items can be selected; Ctrl+click toggles, Shift+click
    /// extends a range from the anchor.
    Multi,
}

/// Reactive selection state for a `Scene`.
///
/// Cheap-to-clone via `Rc` internals — all clones share the same
/// underlying signal. Pass clones into widget closures or item
/// `register_bindings` impls without worrying about ownership.
#[derive(Clone)]
pub struct SceneSelection {
    mode: SceneSelectionMode,
    selection: Signal<BTreeSet<ItemId>>,
    /// Anchor for Shift+click range extension. Shared via `Rc<Cell>`
    /// so clones see the same anchor.
    anchor: Rc<Cell<Option<ItemId>>>,
}

impl SceneSelection {
    /// New selection model with the given mode. Initially empty,
    /// no anchor.
    pub fn new(mode: SceneSelectionMode) -> Self {
        Self {
            mode,
            selection: Signal::new(BTreeSet::new()),
            anchor: Rc::new(Cell::new(None)),
        }
    }

    /// The configured selection mode.
    pub fn mode(&self) -> SceneSelectionMode {
        self.mode
    }

    /// Live selection signal. Bind reactive consumers (item paint,
    /// status-bar item-count labels) to this.
    pub fn selection_signal(&self) -> Signal<BTreeSet<ItemId>> {
        self.selection.clone()
    }

    /// Whether the given item id is currently selected.
    pub fn is_selected(&self, id: ItemId) -> bool {
        self.selection.get().contains(&id)
    }

    /// Selected item ids in sorted order.
    pub fn selected(&self) -> Vec<ItemId> {
        self.selection.get().into_iter().collect()
    }

    /// Number of selected items.
    pub fn count(&self) -> usize {
        self.selection.get().len()
    }

    /// Clear the selection. The anchor is also cleared so a
    /// subsequent Shift+click extends from a fresh starting point.
    pub fn clear(&self) {
        self.selection.set(BTreeSet::new());
        self.anchor.set(None);
    }

    /// Replace the selection with a single item; sets the anchor
    /// for subsequent range extension. No-op in `None` mode.
    pub fn select_one(&self, id: ItemId) {
        if matches!(self.mode, SceneSelectionMode::None) {
            return;
        }
        let mut set = BTreeSet::new();
        set.insert(id);
        self.selection.set(set);
        self.anchor.set(Some(id));
    }

    /// Toggle membership for the given id (Ctrl+click semantic).
    /// Sets the anchor on toggle-on; leaves it unchanged on
    /// toggle-off. No-op in `None` mode; in `Single` mode behaves
    /// like `select_one` if the item is currently unselected, or
    /// `clear` if it is.
    pub fn toggle(&self, id: ItemId) {
        match self.mode {
            SceneSelectionMode::None => (),
            SceneSelectionMode::Single => {
                if self.is_selected(id) {
                    self.clear();
                } else {
                    self.select_one(id);
                }
            }
            SceneSelectionMode::Multi => {
                let mut set = self.selection.get();
                if set.remove(&id) {
                    // Toggle-off: anchor unchanged.
                } else {
                    set.insert(id);
                    self.anchor.set(Some(id));
                }
                self.selection.set(set);
            }
        }
    }

    /// Replace the selection with the given set of ids. Used by
    /// marquee on commit. Anchor is cleared. No-op in `None`
    /// mode; in `Single` mode keeps at most one (the first id in
    /// `ids`).
    pub fn replace(&self, ids: impl IntoIterator<Item = ItemId>) {
        match self.mode {
            SceneSelectionMode::None => {}
            SceneSelectionMode::Single => {
                let mut iter = ids.into_iter();
                let mut set = BTreeSet::new();
                if let Some(first) = iter.next() {
                    set.insert(first);
                }
                self.selection.set(set);
                self.anchor.set(None);
            }
            SceneSelectionMode::Multi => {
                let set: BTreeSet<ItemId> = ids.into_iter().collect();
                self.selection.set(set);
                self.anchor.set(None);
            }
        }
    }

    /// Add `ids` to the existing selection (marquee with
    /// Ctrl-modifier — additive box-select). No-op in `None` mode;
    /// in `Single` mode reduces to `select_one(last)`.
    pub fn extend(&self, ids: impl IntoIterator<Item = ItemId>) {
        match self.mode {
            SceneSelectionMode::None => {}
            SceneSelectionMode::Single => {
                if let Some(last) = ids.into_iter().last() {
                    self.select_one(last);
                }
            }
            SceneSelectionMode::Multi => {
                let mut set = self.selection.get();
                set.extend(ids);
                self.selection.set(set);
            }
        }
    }

    /// Marquee commit helper: replace (or extend, if `additive`)
    /// the selection with every scene item whose AABB intersects
    /// `marquee_rect_in_scene`. Lightweight items and heavyweight
    /// widget entries are both candidates — the spatial index
    /// returns ids regardless of kind.
    pub fn commit_marquee(&self, scene: &Scene, marquee_rect: Rect, additive: bool) {
        // Filter to items carrying `IS_SELECTABLE`. The spatial
        // index returns every entry whose AABB intersects — both
        // selectable and non-selectable (locked layers, decoration-
        // only items, logical groups). The marquee commit must
        // respect the flag so single-click + marquee agree about
        // what can be selected. (Unit 9: was previously unfiltered;
        // see edge_cases::marquee_commit_respects_is_selectable_flag.)
        let hits: Vec<crate::item::ItemId> = scene
            .items_in_rect(marquee_rect)
            .into_iter()
            .filter(|id| {
                scene
                    .flags(*id)
                    .map(|f| f.contains(crate::flags::ItemFlags::IS_SELECTABLE))
                    .unwrap_or(false)
            })
            .collect();
        if additive {
            self.extend(hits);
        } else {
            self.replace(hits);
        }
    }
}

impl std::fmt::Debug for SceneSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneSelection")
            .field("mode", &self.mode)
            .field("count", &self.count())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_id() -> ItemId {
        ItemId::next()
    }

    #[test]
    fn none_mode_ignores_all_mutations() {
        let sel = SceneSelection::new(SceneSelectionMode::None);
        sel.select_one(fresh_id());
        sel.toggle(fresh_id());
        sel.replace([fresh_id(), fresh_id()]);
        assert_eq!(sel.count(), 0);
    }

    #[test]
    fn single_mode_keeps_at_most_one() {
        let sel = SceneSelection::new(SceneSelectionMode::Single);
        let a = fresh_id();
        let b = fresh_id();
        sel.select_one(a);
        sel.select_one(b);
        assert_eq!(sel.selected(), vec![b]);
    }

    #[test]
    fn multi_toggle_round_trip() {
        let sel = SceneSelection::new(SceneSelectionMode::Multi);
        let a = fresh_id();
        let b = fresh_id();
        sel.toggle(a);
        sel.toggle(b);
        assert_eq!(sel.count(), 2);
        sel.toggle(a);
        assert_eq!(sel.selected(), vec![b]);
    }

    #[test]
    fn replace_clears_then_inserts() {
        let sel = SceneSelection::new(SceneSelectionMode::Multi);
        let a = fresh_id();
        let b = fresh_id();
        let c = fresh_id();
        sel.select_one(a);
        sel.replace([b, c]);
        assert_eq!(sel.count(), 2);
        assert!(sel.is_selected(b));
        assert!(sel.is_selected(c));
        assert!(!sel.is_selected(a));
    }

    #[test]
    fn extend_is_additive() {
        let sel = SceneSelection::new(SceneSelectionMode::Multi);
        let a = fresh_id();
        let b = fresh_id();
        sel.select_one(a);
        sel.extend([b]);
        assert_eq!(sel.count(), 2);
    }

    #[test]
    fn signal_updates_observable() {
        let sel = SceneSelection::new(SceneSelectionMode::Multi);
        let signal = sel.selection_signal();
        let id = fresh_id();
        assert!(signal.get().is_empty());
        sel.select_one(id);
        assert!(signal.get().contains(&id));
    }
}
