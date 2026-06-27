// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Type-erased data source for `TreeView`.
//!
//! The tree analogue of [`ListSource`](crate::list_source::ListSource): wraps any
//! [`TreeDataSource`] behind a uniform set of
//! `Rc<dyn Fn(..)>` closures keyed on the **visible flat index**, so `TreeView`
//! stays `TreeView<T>` (no `Key` / source type parameter) and works in indices
//! throughout. Each closure resolves index → the source's `Key` (via `key_at`)
//! before calling the source's `parent` / `set_expanded` / `can_accept` / … .
//!
//! Both backings flow through the single [`TreeSource::from_data_source`]
//! constructor: the built-in `TreeView::new(TreeModel)` path wraps a
//! `Rc<TreeSlice<T>>` (which itself implements `TreeDataSource` with
//! `Key = NodeId`), while `TreeView::from_source` wraps an external source with
//! its own `Key` (e.g. an entity id). The only built-in-vs-external difference —
//! the `NodeId`-typed `TreeRowContext` handed to the legacy delegate — lives in
//! `tree_view.rs`'s row-delegate wrapper, not here.

use std::rc::Rc;

use bastyde_core::drag_payload::DragPayload;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{EventContext, Widget};
use bastyde_data::{
    DragEligibility, DragSource, DropCommit, DropPosition, DropQuery, DropResponse, RowState,
    TreeDataSource,
};

use crate::data_views::RowDrag;

/// Key-erased per-row flat metadata, derived from the source's `FlatEntry`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeRowMeta {
    /// Depth in the tree (0 for roots).
    pub depth: usize,
    /// Whether this row has children in the source.
    pub has_children: bool,
    /// Whether this row is currently expanded.
    pub is_expanded: bool,
}

/// Per-row context handed to a [`TreeView::from_source`](crate::TreeView::from_source)
/// delegate — the key-erased counterpart of the built-in
/// [`TreeRowContext`](crate::TreeRowContext). Carries the row's flat metadata
/// plus a one-call chevron toggle that flips the row's expansion through the
/// source (by index → key → `set_expanded`).
pub struct TreeRow {
    /// Depth in the tree (0 for roots).
    pub depth: usize,
    /// Whether this row has children in the source.
    pub has_children: bool,
    /// Whether this row is currently expanded.
    pub is_expanded: bool,
    toggle: Rc<dyn Fn(&mut EventContext)>,
}

impl TreeRow {
    /// Toggle callback for this row's chevron. Wires in one line:
    /// `.on_toggle_rc(row.toggle_callback())`.
    pub fn toggle_callback(&self) -> Rc<dyn Fn(&mut EventContext)> {
        self.toggle.clone()
    }
}

/// Erased DnD + lazy capability closures for a tree source. View-facing
/// arguments are visible flat indices + the view's id; the closures resolve keys
/// internally. Mirrors [`DndLazy`](crate::list_source::DndLazy) for trees, so the
/// `Key` never escapes into `TreeView<T>`.
pub(crate) struct TreeDndLazy {
    /// Whether the row at `index` may begin a drag.
    pub(crate) drag_fn: Rc<dyn Fn(usize) -> DragEligibility>,
    /// `(payload, target_index, position, this_view_id) -> verdict`.
    pub(crate) can_accept_fn: Rc<dyn Fn(&DragPayload, usize, DropPosition, usize) -> DropResponse>,
    /// `(payload, target_index, position, this_view_id) -> applied`.
    pub(crate) accept_drop_fn: Rc<dyn Fn(&DragPayload, usize, DropPosition, usize) -> bool>,
    /// Source-side completion: row at `index` was accepted elsewhere.
    pub(crate) on_drag_out_fn: Rc<dyn Fn(usize)>,
    /// Whether the row at `index` is loaded.
    pub(crate) row_state_fn: Rc<dyn Fn(usize) -> RowState>,
    /// Nudge the source to load a visible range.
    pub(crate) request_window_fn: Rc<dyn Fn(std::ops::Range<usize>)>,
    /// Whether more rows can be appended.
    pub(crate) can_fetch_more_fn: Rc<dyn Fn() -> bool>,
    /// Fetch the next page.
    pub(crate) fetch_more_fn: Rc<dyn Fn()>,
}

impl TreeDndLazy {
    fn from_source<T: 'static, S: TreeDataSource<Item = T> + 'static>(s: Rc<S>) -> Self {
        let (s1, s2, s3, s4, s5, s6, s7, s8) = (
            s.clone(),
            s.clone(),
            s.clone(),
            s.clone(),
            s.clone(),
            s.clone(),
            s.clone(),
            s,
        );
        Self {
            drag_fn: Rc::new(move |index| match s1.key_at(index) {
                Some(k) => s1.drag(&k),
                None => DragEligibility::NoDrag,
            }),
            can_accept_fn: Rc::new(move |payload, target_index, position, view_id| {
                let Some(target_key) = s2.key_at(target_index) else {
                    return DropResponse::Reject;
                };
                if let Some(rd) = payload.get_typed::<RowDrag>()
                    && rd.source_view_id == view_id
                {
                    let Some(source_key) = s2.key_at(rd.source_index) else {
                        return DropResponse::Reject;
                    };
                    return s2.can_accept(&DropQuery {
                        source: DragSource::SameView { key: source_key },
                        target: target_key,
                        position,
                    });
                }
                s2.can_accept(&DropQuery {
                    source: DragSource::Foreign { payload },
                    target: target_key,
                    position,
                })
            }),
            accept_drop_fn: Rc::new(move |payload, target_index, position, view_id| {
                let Some(target_key) = s3.key_at(target_index) else {
                    return false;
                };
                if let Some(rd) = payload.get_typed::<RowDrag>()
                    && rd.source_view_id == view_id
                {
                    let Some(source_key) = s3.key_at(rd.source_index) else {
                        return false;
                    };
                    return s3.accept_drop(DropCommit {
                        source: DragSource::SameView { key: source_key },
                        target: target_key,
                        position,
                    });
                }
                s3.accept_drop(DropCommit {
                    source: DragSource::Foreign { payload },
                    target: target_key,
                    position,
                })
            }),
            on_drag_out_fn: Rc::new(move |index| {
                if let Some(k) = s4.key_at(index) {
                    s4.on_drag_out(&k);
                }
            }),
            row_state_fn: Rc::new(move |index| s5.row_state(index)),
            request_window_fn: Rc::new(move |range| s6.request_window(range)),
            can_fetch_more_fn: Rc::new(move || s7.can_fetch_more()),
            fetch_more_fn: Rc::new(move || s8.fetch_more()),
        }
    }
}

/// Erased tree backing consumed by `TreeView`. All accessors are keyed on the
/// visible flat index; the `Key` type is captured at construction and never
/// surfaces in `TreeView<T>`.
pub(crate) struct TreeSource<T: 'static> {
    visible_count_fn: Rc<dyn Fn() -> usize>,
    /// Build a widget for the row at `index`: hands the builder `(&T, &TreeRowMeta)`.
    /// `None` when the index is out of range OR its data is still `Loading`.
    with_row_fn:
        Rc<dyn Fn(usize, &dyn Fn(&T, &TreeRowMeta) -> Box<dyn Widget>) -> Option<Box<dyn Widget>>>,
    /// String-returning sibling of [`with_row_fn`](Self::with_row_fn) — reads
    /// an arbitrary `String` from a resident row's item, for type-ahead label
    /// extraction. `None` when out of range or still loading.
    with_row_str_fn: Rc<dyn Fn(usize, &dyn Fn(&T) -> String) -> Option<String>>,
    /// Flat metadata for `index` without building a widget (a11y, keyboard).
    meta_fn: Rc<dyn Fn(usize) -> Option<TreeRowMeta>>,
    /// Expand (`true`) / collapse (`false`) the row at `index` (index → key).
    set_expanded_at_fn: Rc<dyn Fn(usize, bool)>,
    /// Whether the row at `index` is expanded.
    is_expanded_at_fn: Rc<dyn Fn(usize) -> bool>,
    /// The visible flat index of the row's parent, if visible (ArrowLeft-to-parent).
    parent_index_fn: Rc<dyn Fn(usize) -> Option<usize>>,
    /// `(pos_in_set_1based, set_size)` among the row's siblings (a11y).
    sibling_pos_fn: Rc<dyn Fn(usize) -> (usize, usize)>,
    /// Alt+Arrow sibling reorder: `(index, down) -> new flat index` (or `None`
    /// if at an edge / rejected). The key-typed sibling logic stays internal.
    keyboard_reorder_fn: Rc<dyn Fn(usize, bool) -> Option<usize>>,
    version_fn: Rc<dyn Fn() -> Signal<u64>>,
    first_changed_fn: Rc<dyn Fn() -> Option<usize>>,
    pub(crate) dnd: TreeDndLazy,
}

impl<T: 'static> TreeSource<T> {
    /// Erase any concrete [`TreeDataSource`]. The built-in path passes a
    /// `Rc<TreeSlice<T>>`; an external source passes its own `Rc<S>`.
    pub(crate) fn from_data_source<S: TreeDataSource<Item = T> + 'static>(s: Rc<S>) -> Self {
        let dnd = TreeDndLazy::from_source(s.clone());
        let (s1, s2, s3, s4, s5, s6, s7, s8, s9, s10, s11) = (
            s.clone(),
            s.clone(),
            s.clone(),
            s.clone(),
            s.clone(),
            s.clone(),
            s.clone(),
            s.clone(),
            s.clone(),
            s.clone(),
            s,
        );
        Self {
            visible_count_fn: Rc::new(move || s1.visible_count()),
            with_row_fn: Rc::new(move |index, build| {
                s2.with_entry(index, |item, entry| {
                    let meta = TreeRowMeta {
                        depth: entry.depth,
                        has_children: entry.has_children,
                        is_expanded: entry.is_expanded,
                    };
                    build(item, &meta)
                })
            }),
            with_row_str_fn: Rc::new(move |index, f| s11.with_entry(index, |item, _entry| f(item))),
            meta_fn: Rc::new(move |index| {
                s3.with_entry(index, |_item, entry| TreeRowMeta {
                    depth: entry.depth,
                    has_children: entry.has_children,
                    is_expanded: entry.is_expanded,
                })
            }),
            set_expanded_at_fn: Rc::new(move |index, expanded| {
                if let Some(k) = s4.key_at(index) {
                    s4.set_expanded(&k, expanded);
                }
            }),
            is_expanded_at_fn: Rc::new(move |index| {
                s5.key_at(index)
                    .map(|k| s5.is_expanded(&k))
                    .unwrap_or(false)
            }),
            parent_index_fn: Rc::new(move |index| {
                let k = s6.key_at(index)?;
                let p = s6.parent(&k)?;
                s6.flat_index_of(&p)
            }),
            sibling_pos_fn: Rc::new(move |index| {
                let Some(k) = s7.key_at(index) else {
                    return (1, 1);
                };
                match s7.parent(&k) {
                    Some(p) => {
                        let sibs = s7.child_keys(&p);
                        let pos = sibs.iter().position(|x| *x == k).unwrap_or(0) + 1;
                        (pos, sibs.len().max(1))
                    }
                    None => {
                        // Roots are always visible (depth 0): count visible
                        // depth-0 rows and find this one's position among them.
                        let n = s7.visible_count();
                        let mut roots = 0usize;
                        let mut pos = 1usize;
                        for j in 0..n {
                            let is_root = s7.with_entry(j, |_it, e| e.depth == 0).unwrap_or(false);
                            if is_root {
                                roots += 1;
                                if j == index {
                                    pos = roots;
                                }
                            }
                        }
                        (pos, roots.max(1))
                    }
                }
            }),
            keyboard_reorder_fn: Rc::new(move |index, down| {
                let k = s10.key_at(index)?;
                // Ordered sibling keys at `k`'s level. Roots are always visible
                // (depth 0 never collapses out), so the root list is the visible
                // depth-0 scan — no root-enumeration method needed on the trait.
                let siblings: Vec<S::Key> = match s10.parent(&k) {
                    Some(p) => s10.child_keys(&p),
                    None => (0..s10.visible_count())
                        .filter_map(|j| {
                            let is_root = s10.with_entry(j, |_it, e| e.depth == 0).unwrap_or(false);
                            if is_root { s10.key_at(j) } else { None }
                        })
                        .collect(),
                };
                let pos = siblings.iter().position(|x| *x == k)?;
                let (target, position) = if down {
                    if pos + 1 >= siblings.len() {
                        return None;
                    }
                    (siblings[pos + 1].clone(), DropPosition::After)
                } else {
                    if pos == 0 {
                        return None;
                    }
                    (siblings[pos - 1].clone(), DropPosition::Before)
                };
                let applied = s10.accept_drop(DropCommit {
                    source: DragSource::SameView { key: k.clone() },
                    target,
                    position,
                });
                if applied { s10.flat_index_of(&k) } else { None }
            }),
            version_fn: Rc::new(move || s8.version_signal()),
            first_changed_fn: Rc::new(move || s9.first_changed_index()),
            dnd,
        }
    }

    pub(crate) fn visible_count(&self) -> usize {
        (self.visible_count_fn)()
    }

    pub(crate) fn with_row(
        &self,
        index: usize,
        build: &dyn Fn(&T, &TreeRowMeta) -> Box<dyn Widget>,
    ) -> Option<Box<dyn Widget>> {
        (self.with_row_fn)(index, build)
    }

    pub(crate) fn meta(&self, index: usize) -> Option<TreeRowMeta> {
        (self.meta_fn)(index)
    }

    /// Read a `String` from the resident row at `index` (type-ahead label).
    pub(crate) fn with_row_str(&self, index: usize, f: &dyn Fn(&T) -> String) -> Option<String> {
        (self.with_row_str_fn)(index, f)
    }

    pub(crate) fn set_expanded_at(&self, index: usize, expanded: bool) {
        (self.set_expanded_at_fn)(index, expanded)
    }

    pub(crate) fn is_expanded_at(&self, index: usize) -> bool {
        (self.is_expanded_at_fn)(index)
    }

    pub(crate) fn toggle_at(&self, index: usize) {
        let expanded = (self.is_expanded_at_fn)(index);
        (self.set_expanded_at_fn)(index, !expanded);
    }

    pub(crate) fn parent_index(&self, index: usize) -> Option<usize> {
        (self.parent_index_fn)(index)
    }

    pub(crate) fn sibling_pos(&self, index: usize) -> (usize, usize) {
        (self.sibling_pos_fn)(index)
    }

    /// Move the row at `index` up (`down=false`) or down among its siblings,
    /// routed through the source's own `accept_drop`. Returns the moved row's
    /// new flat index, or `None` at an edge / if rejected.
    pub(crate) fn keyboard_reorder(&self, index: usize, down: bool) -> Option<usize> {
        (self.keyboard_reorder_fn)(index, down)
    }

    pub(crate) fn version_signal(&self) -> Signal<u64> {
        (self.version_fn)()
    }

    pub(crate) fn first_changed_index(&self) -> Option<usize> {
        (self.first_changed_fn)()
    }

    /// Build a per-row [`TreeRow`] context (key-erased toggle) for the
    /// `from_source` delegate.
    pub(crate) fn row_context(self_rc: &Rc<TreeSource<T>>, index: usize) -> TreeRow {
        let meta = self_rc.meta(index).unwrap_or(TreeRowMeta {
            depth: 0,
            has_children: false,
            is_expanded: false,
        });
        let src = self_rc.clone();
        TreeRow {
            depth: meta.depth,
            has_children: meta.has_children,
            is_expanded: meta.is_expanded,
            toggle: Rc::new(move |_ctx| src.toggle_at(index)),
        }
    }
}
