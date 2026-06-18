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

use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bastyde_core::ObserverHandle;
use bastyde_core::widget::Widget;
use bastyde_data::{
    DataChange, DropPosition, ItemKey, KeyedSelectionModel, SelectionMode, SelectionModel,
};

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

/// Index-facing row-selection facade backing the four data views.
///
/// An app installs *either* the index-based [`SelectionModel`] (positions) or a
/// [`KeyedSelectionModel<K>`] (stable identities that survive reorder / filter /
/// window-slide / multi-view). The views' click / keyboard / rebuild / paint
/// paths all work in **indices**, so this facade erases the difference: the
/// keyed variant carries the view's index↔key mapping (`key_at` / `len` /
/// `contains_key`) and translates internally. The method surface deliberately
/// mirrors `SelectionModel` so call sites read identically (`rs.select(i)`,
/// `rs.is_selected(i)`, …).
#[derive(Clone)]
pub(crate) struct RowSelection {
    mode: SelectionMode,
    is_selected: Rc<dyn Fn(usize) -> bool>,
    select_fn: Rc<dyn Fn(usize)>,
    toggle_fn: Rc<dyn Fn(usize)>,
    extend_fn: Rc<dyn Fn(usize)>,
    select_all_fn: Rc<dyn Fn(usize)>,
    selected_indices_fn: Rc<dyn Fn() -> Vec<usize>>,
    clear_fn: Rc<dyn Fn()>,
    observe_fn: Rc<dyn Fn(Box<dyn Fn()>) -> ObserverHandle>,
    on_change_fn: Rc<dyn Fn(&DataChange)>,
    /// Unconditional prune for the version-signal-driven tree views (which
    /// don't emit a `DataChange`): drop orphaned keys (keyed) or no-op (index).
    prune_fn: Rc<dyn Fn()>,
}

impl RowSelection {
    /// Back the facade with the index-based [`SelectionModel`]. Index ops pass
    /// straight through; `on_data_change` index-shifts (insert / remove) or
    /// clears (reset) the selection, matching the legacy inline behaviour.
    pub(crate) fn from_index(sel: SelectionModel) -> Self {
        let (s_is, s_sel, s_tog, s_ext, s_all, s_idx, s_clr, s_obs, s_chg) = (
            sel.clone(),
            sel.clone(),
            sel.clone(),
            sel.clone(),
            sel.clone(),
            sel.clone(),
            sel.clone(),
            sel.clone(),
            sel.clone(),
        );
        Self {
            mode: sel.mode(),
            is_selected: Rc::new(move |i| s_is.is_selected(i)),
            select_fn: Rc::new(move |i| s_sel.select(i)),
            toggle_fn: Rc::new(move |i| s_tog.toggle(i)),
            extend_fn: Rc::new(move |i| s_ext.extend_to(i)),
            select_all_fn: Rc::new(move |count| s_all.select_all(count)),
            selected_indices_fn: Rc::new(move || s_idx.selected_indices()),
            clear_fn: Rc::new(move || s_clr.clear()),
            observe_fn: Rc::new(move |cb| s_obs.selection_signal().observe(move |_| cb())),
            on_change_fn: Rc::new(move |change| match change {
                DataChange::ItemsInserted { range } => {
                    s_chg.adjust_for_insert(range.start, range.end - range.start);
                }
                DataChange::ItemsRemoved { range } => {
                    s_chg.adjust_for_remove(range.start, range.end - range.start);
                }
                DataChange::Reset => s_chg.clear(),
                _ => {}
            }),
            // The index model has no stable identity to prune against on a
            // bare version bump — tree structural adjustments stay no-ops here
            // (the legacy behaviour).
            prune_fn: Rc::new(|| {}),
        }
    }

    /// Back the facade with a [`KeyedSelectionModel<K>`] plus the view's
    /// index↔key mapping. `key_at(i)` is the key at visible index `i`, `len()`
    /// the visible count (for Shift-range ordering and `selected_indices`), and
    /// `contains_key(&k)` whether the *source* still holds the key (for
    /// prune-on-remove — a collapsed-but-present tree node must NOT be pruned,
    /// so this is supplied by the view, not derived from the visible window).
    pub(crate) fn from_keyed<K: ItemKey>(
        keyed: KeyedSelectionModel<K>,
        key_at: Rc<dyn Fn(usize) -> Option<K>>,
        len: Rc<dyn Fn() -> usize>,
        contains_key: Rc<dyn Fn(&K) -> bool>,
    ) -> Self {
        let mode = keyed.mode();
        Self {
            mode,
            is_selected: {
                let (k, ka) = (keyed.clone(), key_at.clone());
                Rc::new(move |i| ka(i).map(|key| k.is_selected(&key)).unwrap_or(false))
            },
            select_fn: {
                let (k, ka) = (keyed.clone(), key_at.clone());
                Rc::new(move |i| {
                    if let Some(key) = ka(i) {
                        k.select(key);
                    }
                })
            },
            toggle_fn: {
                let (k, ka) = (keyed.clone(), key_at.clone());
                Rc::new(move |i| {
                    if let Some(key) = ka(i) {
                        k.toggle(key);
                    }
                })
            },
            extend_fn: {
                let (k, ka, l) = (keyed.clone(), key_at.clone(), len.clone());
                Rc::new(move |i| {
                    if let Some(target) = ka(i) {
                        let ordered: Vec<K> = (0..l()).filter_map(|j| ka(j)).collect();
                        k.extend_to(target, &ordered);
                    }
                })
            },
            select_all_fn: {
                let (k, ka) = (keyed.clone(), key_at.clone());
                Rc::new(move |count| {
                    let keys: Vec<K> = (0..count).filter_map(|i| ka(i)).collect();
                    k.select_keys(keys, false);
                })
            },
            selected_indices_fn: {
                let (k, ka, l) = (keyed.clone(), key_at.clone(), len.clone());
                Rc::new(move || {
                    (0..l())
                        .filter(|&i| ka(i).map(|key| k.is_selected(&key)).unwrap_or(false))
                        .collect()
                })
            },
            clear_fn: {
                let k = keyed.clone();
                Rc::new(move || k.clear())
            },
            observe_fn: {
                let k = keyed.clone();
                Rc::new(move |cb| k.selection_signal().observe(move |_| cb()))
            },
            on_change_fn: {
                let (k, c) = (keyed.clone(), contains_key.clone());
                Rc::new(move |change| match change {
                    // Keys are stable across inserts / moves; only removals and
                    // resets can orphan a selected key.
                    DataChange::ItemsRemoved { .. } | DataChange::Reset => {
                        k.prune_missing(|key| c(key));
                    }
                    _ => {}
                })
            },
            prune_fn: {
                let (k, c) = (keyed, contains_key);
                Rc::new(move || k.prune_missing(|key| c(key)))
            },
        }
    }

    pub(crate) fn mode(&self) -> SelectionMode {
        self.mode
    }
    pub(crate) fn is_selected(&self, index: usize) -> bool {
        (self.is_selected)(index)
    }
    pub(crate) fn select(&self, index: usize) {
        (self.select_fn)(index)
    }
    pub(crate) fn toggle(&self, index: usize) {
        (self.toggle_fn)(index)
    }
    pub(crate) fn extend_to(&self, index: usize) {
        (self.extend_fn)(index)
    }
    pub(crate) fn select_all(&self, count: usize) {
        (self.select_all_fn)(count)
    }
    pub(crate) fn selected_indices(&self) -> Vec<usize> {
        (self.selected_indices_fn)()
    }
    pub(crate) fn clear(&self) {
        (self.clear_fn)()
    }
    /// Subscribe to selection changes (drives the view's rebuild). Owns the
    /// returned handle for the subscription's lifetime.
    pub(crate) fn observe_for_rebuild(&self, cb: impl Fn() + 'static) -> ObserverHandle {
        (self.observe_fn)(Box::new(cb))
    }
    /// React to a source data change (index-shift for the index model, prune
    /// for the keyed model).
    pub(crate) fn on_data_change(&self, change: &DataChange) {
        (self.on_change_fn)(change)
    }
    /// Prune orphaned keys (keyed model) — used by the tree views, which drive
    /// off a version signal rather than a `DataChange`. No-op for the index
    /// model.
    pub(crate) fn prune(&self) {
        (self.prune_fn)()
    }
}
