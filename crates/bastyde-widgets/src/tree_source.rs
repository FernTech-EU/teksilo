// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Type-erased data source adapter for [`TreeView`](crate::TreeView).
//!
//! Wraps any [`TreeDataSource`] behind a uniform set of `Rc<dyn Fn(..)>` closures
//! keyed on the **visible flat index**, so `TreeView<T>` requires no extra type
//! parameter for the source's `Key`. Each closure resolves index → `Key` (via
//! `key_at`) before forwarding to the source's `parent`, `set_expanded`,
//! `can_accept`, etc. The `Key` type is fully captured here and never surfaces
//! in the view.
//!
//! Both built-in and external backings flow through
//! [`TreeSource::from_data_source`]: the `TreeView::new(TreeModel)` path wraps a
//! `Rc<TreeSlice<T>>` (which implements `TreeDataSource<Key = NodeId>`), while
//! `TreeView::from_source` wraps an external `TreeDataSource` with its own `Key`.
//! The only built-in-vs-external difference — the `NodeId`-typed `TreeRowContext`
//! handed to the legacy delegate — lives in `tree_view.rs`, not here.

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_core::drag_payload::DragPayload;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{EventContext, Widget};
use bastyde_data::{
    DragEligibility, DragSource, DropCommit, DropPosition, DropQuery, DropResponse, RowState,
    TreeDataSource,
};

use crate::data_views::{RowDragData, ViewId};

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

/// Cache of the ascending flat indices of every visible depth-0 (root) row,
/// valid for one source version. Roots have no `parent` to enumerate
/// siblings through, so both `sibling_pos`'s root branch and Alt+Arrow's
/// root-sibling reorder fall back to scanning the WHOLE visible range for
/// `depth == 0` rows — O(realized root rows × visible count) per rebuild for
/// a flat-ish tree with many roots, since every realized root row repeats
/// the full scan. Rebuilt once per version bump (shared by both call sites)
/// instead, then answered by a binary search (`sibling_pos`) or a direct
/// re-map (`keyboard_reorder`).
type RootIndexCache = RefCell<Option<(u64, Rc<Vec<usize>>)>>;

/// The cached root indices for `source`'s current version, rescanning only
/// when the version has moved on since the last call.
fn root_indices<S: TreeDataSource>(source: &S, cache: &RootIndexCache) -> Rc<Vec<usize>> {
    let version = source.version_signal().get();
    {
        let cached = cache.borrow();
        if let Some((v, flat)) = cached.as_ref()
            && *v == version
        {
            return flat.clone();
        }
    }
    let n = source.visible_count();
    let flat = Rc::new(
        (0..n)
            .filter(|&j| source.with_entry(j, |_it, e| e.depth == 0).unwrap_or(false))
            .collect::<Vec<usize>>(),
    );
    *cache.borrow_mut() = Some((version, flat.clone()));
    flat
}

/// Erased DnD + lazy capability closures for a tree source. View-facing
/// arguments are visible flat indices + the view's id; the closures resolve keys
/// internally. Mirrors [`DndLazy`](crate::list_source::DndLazy) for trees, so the
/// `Key` never escapes into `TreeView<T>`.
pub(crate) struct TreeDndLazy {
    /// Whether the row at `index` may begin a drag.
    pub(crate) drag_fn: Rc<dyn Fn(usize) -> DragEligibility>,
    /// `(payload, target_index, position, this_view_id) -> verdict`.
    pub(crate) can_accept_fn: Rc<dyn Fn(&DragPayload, usize, DropPosition, ViewId) -> DropResponse>,
    /// `(payload, target_index, position, this_view_id) -> applied`.
    pub(crate) accept_drop_fn: Rc<dyn Fn(&DragPayload, usize, DropPosition, ViewId) -> bool>,
    /// Source-side completion: resolve stable node keys for these rows NOW
    /// (drag-start) and return a thunk that removes them (a foreign move-out).
    /// Resolving eagerly keeps a Move correct even when the tree's flat indices
    /// reshuffle mid-drag (spring-load auto-expand), since the stable `NodeId`s
    /// were already captured.
    pub(crate) snapshot_out_fn: crate::data_views::SnapshotOutFn,
    /// Resolve + stash the dragged rows' stable node keys for a **synthetic**
    /// same-view payload built outside `RowExport::build_payload`. Pointer
    /// drags stash through [`snapshot_out_fn`](Self::snapshot_out_fn) at
    /// drag-start; the same-view accept path reads identity exclusively from
    /// this stash — see [`can_accept_fn`](Self::can_accept_fn).
    pub(crate) stash_drag_keys_fn: Rc<dyn Fn(&[usize])>,
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
        // Stable node keys of the in-flight same-view drag, resolved from flat
        // indices ONCE at payload construction (`snapshot_out_fn` for pointer
        // drags, `stash_drag_keys_fn` for synthetic keyboard payloads). The
        // accept path reads identity from here rather than re-resolving
        // `RowDragData::rows` at hover/drop time: a tree's flat indices
        // reshuffle mid-drag (the spring-load auto-expand is triggered by the
        // very hover that precedes the drop), and whichever nodes slid into
        // the stale slots must not stand in for the dragged ones.
        let drag_keys: Rc<RefCell<Option<Vec<S::Key>>>> = Rc::new(RefCell::new(None));
        let (keys_ca, keys_ad, keys_snap, keys_stash) = (
            drag_keys.clone(),
            drag_keys.clone(),
            drag_keys.clone(),
            drag_keys,
        );
        let (s1, s2, s3, s4, s5, s6, s7, s8, s9) = (
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
            drag_fn: Rc::new(move |index| match s1.key_at(index) {
                Some(k) => s1.drag(&k),
                None => DragEligibility::NoDrag,
            }),
            can_accept_fn: Rc::new(move |payload, target_index, position, view_id| {
                let Some(target_key) = s2.key_at(target_index) else {
                    return DropResponse::Reject;
                };
                if let Some(rd) = payload.get_typed::<RowDragData<T>>()
                    && rd.source == view_id
                {
                    let source_key = {
                        let stash = keys_ca.borrow();
                        let Some(keys) = stash.as_ref().filter(|k| !k.is_empty()) else {
                            debug_assert!(false, "same-view drag without a drag-start key stash");
                            return DropResponse::Reject;
                        };
                        // Own-row rejection by key, so it survives a mid-drag
                        // reflow.
                        if keys.contains(&target_key) {
                            return DropResponse::Reject;
                        }
                        keys[0].clone()
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
                if let Some(rd) = payload.get_typed::<RowDragData<T>>()
                    && rd.source == view_id
                {
                    // Consume the drag-start stash (a construction path that
                    // forgot to stash then fails loudly on its next drop
                    // instead of silently reusing a previous drag's keys).
                    let taken = keys_ad.borrow_mut().take();
                    let Some(keys) = taken.filter(|k| !k.is_empty()) else {
                        debug_assert!(false, "same-view drop without a drag-start key stash");
                        return false;
                    };
                    if keys.contains(&target_key) {
                        return false;
                    }
                    // `reorder_within` drops descendants-of-selected and keeps
                    // the remaining nodes contiguous (single- or multi-row).
                    return s3.reorder_within(&keys, &target_key, position);
                }
                s3.accept_drop(DropCommit {
                    source: DragSource::Foreign { payload },
                    target: target_key,
                    position,
                })
            }),
            snapshot_out_fn: Rc::new(move |indices: &[usize]| {
                // Resolves stable keys NOW: they feed both the same-view accept
                // path (via the drag-key stash) and the returned removal thunk.
                let mut pairs: Vec<(usize, S::Key)> = indices
                    .iter()
                    .filter_map(|&i| s4.key_at(i).map(|k| (i, k)))
                    .collect();
                *keys_snap.borrow_mut() = Some(pairs.iter().map(|(_, k)| k.clone()).collect());
                pairs.sort_by_key(|&(i, _)| std::cmp::Reverse(i));
                let s = s4.clone();
                Box::new(move || {
                    for (_, k) in &pairs {
                        s.on_drag_out(k);
                    }
                }) as Box<dyn Fn()>
            }),
            stash_drag_keys_fn: Rc::new(move |indices: &[usize]| {
                *keys_stash.borrow_mut() =
                    Some(indices.iter().filter_map(|&i| s9.key_at(i)).collect());
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
    /// Read `&T` from the resident row at `index` via a side-effecting
    /// callback, returning whether it ran. Powers export item-cloning
    /// (`.exportable(..)`) without the delegate's widget-building path.
    pub(crate) read_item_fn: Rc<dyn Fn(usize, &mut dyn FnMut(&T)) -> bool>,
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
    /// Resolve `index` to a [`RowAnchor`](crate::data_views::RowAnchor) that
    /// survives row movement. Captures the source's key at build time; the key
    /// stays inside the closure, so `TreeSource<T>` remains key-agnostic.
    anchor_fn: Rc<dyn Fn(usize) -> crate::data_views::RowAnchor>,
    version_fn: Rc<dyn Fn() -> Signal<u64>>,
    first_changed_fn: Rc<dyn Fn() -> Option<usize>>,
    pub(crate) dnd: TreeDndLazy,
}

impl<T: 'static> TreeSource<T> {
    /// Erase any concrete [`TreeDataSource`]. The built-in path passes a
    /// `Rc<TreeSlice<T>>`; an external source passes its own `Rc<S>`.
    pub(crate) fn from_data_source<S: TreeDataSource<Item = T> + 'static>(s: Rc<S>) -> Self {
        let dnd = TreeDndLazy::from_source(s.clone());
        // Shared by `sibling_pos_fn` and `keyboard_reorder_fn` below — both
        // need "all visible roots, in order" and a version bump invalidates
        // both alike, so one scan per version serves either caller.
        let root_cache: Rc<RootIndexCache> = Rc::new(RefCell::new(None));
        let (root_cache_sib, root_cache_kbd) = (root_cache.clone(), root_cache);
        let (s1, s2, s3, s4, s5, s6, s7, s8, s9, s10, s11, s12, s13) = (
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
            s.clone(),
            s.clone(),
            s,
        );
        Self {
            visible_count_fn: Rc::new(move || s1.visible_count()),
            anchor_fn: Rc::new(move |index| match s13.key_at(index) {
                Some(key) => {
                    let src = s13.clone();
                    crate::data_views::RowAnchor::new(Rc::new(move || {
                        // Fast path: the captured slot still holds this row.
                        if src.key_at(index).as_ref() == Some(&key) {
                            return Some(index);
                        }
                        src.flat_index_of(&key)
                    }))
                }
                None => crate::data_views::RowAnchor::fixed(index),
            }),
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
            read_item_fn: Rc::new(move |index, f| {
                s12.with_entry(index, |item, _entry| f(item)).is_some()
            }),
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
                        // Roots are always visible (depth 0). The cached scan
                        // (one per source version) avoids re-deriving "all
                        // visible roots" for every realized root row.
                        let roots = root_indices(&*s7, &root_cache_sib);
                        let pos = roots.binary_search(&index).map(|p| p + 1).unwrap_or(1);
                        (pos, roots.len().max(1))
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
                    None => root_indices(&*s10, &root_cache_kbd)
                        .iter()
                        .filter_map(|&j| s10.key_at(j))
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

    /// A movement-proof handle to the row at `index`.
    pub(crate) fn anchor(&self, index: usize) -> crate::data_views::RowAnchor {
        (self.anchor_fn)(index)
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
        // Anchored, not index-captured: the chevron keeps toggling ITS row even
        // if rows above it appear or vanish before the click lands, and no-ops
        // if the row is gone rather than toggling whoever took its place.
        let anchor = self_rc.anchor(index);
        TreeRow {
            depth: meta.depth,
            has_children: meta.has_children,
            is_expanded: meta.is_expanded,
            toggle: Rc::new(move |_ctx| {
                if let Some(i) = anchor.index() {
                    src.toggle_at(i);
                }
            }),
        }
    }
}

#[cfg(test)]
mod drag_identity_tests {
    use super::*;
    use bastyde_data::{TreeDataSlice, TreeRow};
    use std::cell::RefCell;

    use crate::data_views::{RowDragData, ViewId, ViewKind};

    fn slice_of(keys: &[u64]) -> TreeDataSlice<u64, u64> {
        let slice = TreeDataSlice::<u64, u64>::new();
        let owned: Vec<u64> = keys.to_vec();
        slice.set_source(move || {
            owned
                .iter()
                .map(|k| TreeRow {
                    key: *k,
                    item: *k,
                    depth: 0,
                })
                .collect()
        });
        slice.reload();
        slice
    }

    fn reshape(slice: &TreeDataSlice<u64, u64>, keys: &[u64]) {
        let owned: Vec<u64> = keys.to_vec();
        slice.set_source(move || {
            owned
                .iter()
                .map(|k| TreeRow {
                    key: *k,
                    item: *k,
                    depth: 0,
                })
                .collect()
        });
        slice.reload();
    }

    fn same_view_payload(view_id: ViewId, rows: Vec<usize>) -> DragPayload {
        DragPayload::typed(RowDragData::<u64> {
            source: view_id,
            rows,
            items: None,
        })
    }

    #[test]
    fn a_reorder_moves_the_node_dragged_not_the_slot_it_left() {
        // Node 30 is grabbed at flat index 2, then the tree reflows mid-drag —
        // the exact shape a spring-load auto-expand produces, since the dwell
        // that expands a collapsed branch happens during the very drag. The
        // drop must move node 30, not whichever node now sits at index 2.
        let slice = slice_of(&[10, 20, 30]);
        let recorded: Rc<RefCell<Vec<(u64, u64, DropPosition)>>> =
            Rc::new(RefCell::new(Vec::new()));
        let rec = recorded.clone();
        slice.set_reorder(move |dragged, target, pos| {
            rec.borrow_mut().push((dragged, target, pos));
            true
        });
        let src = Rc::new(TreeSource::from_data_source(Rc::new(slice.clone())));
        let vid = ViewId::next(ViewKind::Tree);

        let _thunk = (src.dnd.snapshot_out_fn)(&[2]); // drag-start on node 30
        let payload = same_view_payload(vid, vec![2]);

        reshape(&slice, &[1, 2, 10, 20, 30]); // rows appear above mid-drag

        assert_eq!(
            (src.dnd.can_accept_fn)(&payload, 0, DropPosition::Before, vid),
            DropResponse::Accept
        );
        assert!((src.dnd.accept_drop_fn)(
            &payload,
            0,
            DropPosition::Before,
            vid
        ));
        assert_eq!(
            recorded.borrow().as_slice(),
            &[(30, 1, DropPosition::Before)],
            "the dragged node's key must move, not whichever node slid into its old index"
        );
    }

    #[test]
    fn a_reflowed_own_node_still_rejects_a_drop_onto_itself() {
        // After the mid-drag reflow the dragged node sits at a NEW flat index;
        // the own-row rejection must follow it there.
        let slice = slice_of(&[10, 20, 30]);
        slice.set_reorder(|_, _, _| true);
        let src = Rc::new(TreeSource::from_data_source(Rc::new(slice.clone())));
        let vid = ViewId::next(ViewKind::Tree);

        let _thunk = (src.dnd.snapshot_out_fn)(&[2]); // node 30
        let payload = same_view_payload(vid, vec![2]);

        reshape(&slice, &[1, 2, 10, 20, 30]); // node 30 now at index 4

        assert_eq!(
            (src.dnd.can_accept_fn)(&payload, 4, DropPosition::Before, vid),
            DropResponse::Reject
        );
        assert!(!(src.dnd.accept_drop_fn)(
            &payload,
            4,
            DropPosition::Before,
            vid
        ));
    }
}

#[cfg(test)]
mod anchor_tests {
    use super::*;
    use bastyde_data::{TreeDataSlice, TreeRow};

    fn slice_of(keys: &[u64]) -> TreeDataSlice<u64, u64> {
        let slice = TreeDataSlice::<u64, u64>::new();
        let owned: Vec<u64> = keys.to_vec();
        slice.set_source(move || {
            owned
                .iter()
                .map(|k| TreeRow {
                    key: *k,
                    item: *k,
                    depth: 0,
                })
                .collect()
        });
        slice.reload();
        slice
    }

    #[test]
    fn an_anchor_follows_its_row_when_rows_shift_above_it() {
        // Row 30 starts at index 2. After two rows are inserted above it, a
        // captured index would point at a different row entirely; the anchor
        // resolves to 30's new position.
        let slice = slice_of(&[10, 20, 30]);
        let src = Rc::new(TreeSource::from_data_source(Rc::new(slice.clone())));
        let anchor = src.anchor(2);
        assert_eq!(anchor.index(), Some(2));

        let shifted: Vec<u64> = vec![1, 2, 10, 20, 30];
        slice.set_source(move || {
            shifted
                .iter()
                .map(|k| TreeRow {
                    key: *k,
                    item: *k,
                    depth: 0,
                })
                .collect()
        });
        slice.reload();

        assert_eq!(
            anchor.index(),
            Some(4),
            "the anchor must track row 30 to its new index, not stay at 2"
        );
    }

    #[test]
    fn an_anchor_reports_none_once_its_row_is_gone() {
        // Deleting the row must make the handler a no-op, not redirect it onto
        // whichever row slid into the vacated slot.
        let slice = slice_of(&[10, 20, 30]);
        let src = Rc::new(TreeSource::from_data_source(Rc::new(slice.clone())));
        let anchor = src.anchor(1); // row 20

        let remaining: Vec<u64> = vec![10, 30];
        slice.set_source(move || {
            remaining
                .iter()
                .map(|k| TreeRow {
                    key: *k,
                    item: *k,
                    depth: 0,
                })
                .collect()
        });
        slice.reload();

        assert_eq!(anchor.index(), None, "row 20 is gone");
        assert!(!anchor.is_live());
    }

    #[test]
    fn a_keyless_source_degrades_to_a_fixed_anchor() {
        // No identity available: the anchor is no worse than capturing the
        // index, and must not pretend the row vanished.
        let anchor = crate::data_views::RowAnchor::fixed(7);
        assert_eq!(anchor.index(), Some(7));
        assert!(anchor.is_live());
    }

    #[test]
    fn an_editing_reconcile_converges_in_one_pass() {
        // `reconcile_editing_row` writes `editing_cell` from inside a pane's
        // build. That is safe only because it settles: once the row index has
        // been corrected, a second pass must write nothing. Pin that, so the
        // write-during-build never becomes a rebuild loop.
        use bastyde_core::signal::Signal;
        use std::cell::RefCell;

        let slice = slice_of(&[10, 20, 30]);
        let src = Rc::new(TreeSource::from_data_source(Rc::new(slice.clone())));
        let editing: Signal<Option<(usize, usize)>> = Signal::new(Some((2, 0)));
        let slot = Rc::new(RefCell::new(None));
        let anchor_of = |i: usize| src.anchor(i);

        // Pass 1 captures the anchor for row 30.
        crate::data_views::reconcile_editing_row(&editing, &slot, &anchor_of);
        assert_eq!(editing.get(), Some((2, 0)));

        // Row 30 moves to index 4.
        let shifted: Vec<u64> = vec![1, 2, 10, 20, 30];
        slice.set_source(move || {
            shifted
                .iter()
                .map(|k| TreeRow {
                    key: *k,
                    item: *k,
                    depth: 0,
                })
                .collect()
        });
        slice.reload();

        crate::data_views::reconcile_editing_row(&editing, &slot, &anchor_of);
        assert_eq!(editing.get(), Some((4, 0)), "corrected once");

        // The settling pass must be a no-op.
        let before = editing.get();
        crate::data_views::reconcile_editing_row(&editing, &slot, &anchor_of);
        assert_eq!(editing.get(), before, "second pass must write nothing");
    }
}
