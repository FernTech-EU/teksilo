// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Type-erased data source for list-backed widgets.
//!
//! Wraps both `ListModel<T>` and any `ListDataSource<Item = T>` behind a
//! uniform set of `Rc<dyn Fn(..)>` closures so consumers like `ListView` and
//! `ComboBox` don't have to carry a generic source parameter or duplicate the
//! wrapping code.
//!
//! The [`DndLazy`] bundle erases the source's DnD + lazy capability protocol the
//! same way: the view works in **indices** (geometry-derived), and each closure
//! translates index → the source's `Key` (via `key_at`) before calling the
//! source's `can_accept` / `accept_drop` / `row_state` / … . The `Key` type
//! therefore never escapes into the (key-less) `ListView<T>`.

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_core::ObserverHandle;
use bastyde_core::drag_payload::DragPayload;
use bastyde_core::widget::Widget;
use bastyde_data::{
    DataChange, DragEligibility, DragSource, DropCommit, DropPosition, DropQuery, DropResponse,
    ListDataSource, ListModel, RowState,
};

use crate::data_views::{RowDragData, ViewId};

/// The erased DnD + lazy capability closures for a list source. View-facing
/// arguments are indices + the view's id; the closures resolve keys internally.
///
/// `Clone` is a shallow `Rc` bump throughout — the closures (and the drag-key
/// stash they share) stay one instance, so a clone handed to a body pane sees
/// exactly the same state the owning view does.
#[derive(Clone)]
pub(crate) struct DndLazy {
    /// Whether the row at `index` may begin a drag.
    pub(crate) drag_fn: Rc<dyn Fn(usize) -> DragEligibility>,
    /// `(payload, target_index, position, this_view_id) -> verdict`.
    pub(crate) can_accept_fn: Rc<dyn Fn(&DragPayload, usize, DropPosition, ViewId) -> DropResponse>,
    /// `(payload, target_index, position, this_view_id) -> applied`.
    pub(crate) accept_drop_fn: Rc<dyn Fn(&DragPayload, usize, DropPosition, ViewId) -> bool>,
    /// Source-side completion: resolve stable keys for these rows NOW
    /// (drag-start) and return a thunk that removes exactly them (a foreign
    /// move-out). Resolving eagerly — rather than re-reading flat indices at
    /// completion — keeps a Move correct even if the view's flat indices
    /// reshuffle mid-drag; removal runs in descending-index order so an
    /// index-keyed model (`ListModel`, whose key *is* the index) stays valid.
    pub(crate) snapshot_out_fn: crate::data_views::SnapshotOutFn,
    /// Resolve + stash the dragged rows' stable keys for a **synthetic**
    /// same-view payload built outside `RowExport::build_payload` (the keyboard
    /// Alt+Arrow reorder). Pointer drags stash through
    /// [`snapshot_out_fn`](Self::snapshot_out_fn) at drag-start. The same-view
    /// accept path reads identity exclusively from this stash — see
    /// [`can_accept_fn`](Self::can_accept_fn).
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

impl DndLazy {
    /// Erase the DnD + lazy protocol of a concrete `ListDataSource`.
    ///
    /// `pub(crate)` so `TableView` can carry a `DndLazy` alongside its own
    /// multi-cell read erasure (its `with_item_fn` is the side-effect form,
    /// shaped differently from `ListSource`'s single-widget reader, so it
    /// doesn't fold into a `ListSource` — but the DnD + lazy protocol is
    /// identical and shared from here).
    pub(crate) fn from_source<T: 'static, S: ListDataSource<Item = T> + 'static>(s: Rc<S>) -> Self {
        // Stable keys of the rows in the in-flight same-view drag, resolved
        // from flat indices ONCE at payload construction (`snapshot_out_fn`
        // for pointer drags, `stash_drag_keys_fn` for synthetic keyboard
        // payloads). The accept path reads identity from here rather than
        // re-resolving `RowDragData::rows` at hover/drop time: those indices
        // freeze at drag-start, and the source can reflow mid-drag (a
        // spring-load auto-expand, a peer write) — whichever rows slid into
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
                        // Can't drop a selection onto one of its own rows —
                        // by key, so the check survives a mid-drag reflow.
                        if keys.contains(&target_key) {
                            return DropResponse::Reject;
                        }
                        // Homogeneous flat reorder: the first dragged row is
                        // a fair representative for the hover verdict (all
                        // move as a block).
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
                    // One call handles both single- and multi-row reorder; the
                    // source's `reorder_within` keeps the block contiguous.
                    return s3.reorder_within(&keys, &target_key, position);
                }
                s3.accept_drop(DropCommit {
                    source: DragSource::Foreign { payload },
                    target: target_key,
                    position,
                })
            }),
            snapshot_out_fn: Rc::new(move |indices: &[usize]| {
                // Resolve stable keys NOW: they feed both the same-view accept
                // path (via the drag-key stash) and the returned removal thunk
                // (descending index order so an index-keyed model stays valid
                // during removal).
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

    /// A fully-inert bundle for sources with no real backing (e.g. `ComboBox`'s
    /// cloning-accessor source): no drag, every drop rejected, fully resident.
    fn inert() -> Self {
        Self {
            drag_fn: Rc::new(|_| DragEligibility::NoDrag),
            can_accept_fn: Rc::new(|_, _, _, _| DropResponse::Reject),
            accept_drop_fn: Rc::new(|_, _, _, _| false),
            snapshot_out_fn: Rc::new(|_: &[usize]| Box::new(|| {}) as Box<dyn Fn()>),
            stash_drag_keys_fn: Rc::new(|_: &[usize]| {}),
            row_state_fn: Rc::new(|_| RowState::Ready),
            request_window_fn: Rc::new(|_| {}),
            can_fetch_more_fn: Rc::new(|| false),
            fetch_more_fn: Rc::new(|| {}),
        }
    }
}

pub(crate) struct ListSource<T: 'static> {
    pub(crate) len_fn: Rc<dyn Fn() -> usize>,
    pub(crate) with_item_fn:
        Rc<dyn Fn(usize, &dyn Fn(&T) -> Box<dyn Widget>) -> Option<Box<dyn Widget>>>,
    /// String-returning sibling of [`with_item_fn`](Self::with_item_fn) —
    /// reads an arbitrary `String` from a resident item (or `None` if the
    /// row isn't loaded). Powers type-ahead label extraction without
    /// forcing the delegate's widget-building path.
    pub(crate) with_item_str_fn: Rc<dyn Fn(usize, &dyn Fn(&T) -> String) -> Option<String>>,
    /// Read `&T` from the resident row at `index` via a side-effecting
    /// callback, returning whether it ran (row present + loaded). Powers
    /// export item-cloning (`.exportable(..)`) without the delegate's
    /// widget-building path.
    pub(crate) read_item_fn: Rc<dyn Fn(usize, &mut dyn FnMut(&T)) -> bool>,
    pub(crate) observe_fn: Rc<dyn Fn(Box<dyn Fn(&DataChange)>) -> ObserverHandle>,
    /// Only populated when backed by `ListModel` — external sources can't
    /// reorder in place.
    pub(crate) move_item_fn: Option<Rc<dyn Fn(usize, usize)>>,
    /// Only populated when backed by `ListModel` — external sources
    /// can't remove items in place. Used by `TabBar<T>` to provide
    /// a default close-tab behavior when the caller doesn't pass an
    /// explicit `on_close` handler.
    pub(crate) remove_item_fn: Option<Rc<dyn Fn(usize)>>,
    /// Divergence side-channel for `DataChange::Reset`-emitting proxies
    /// (`ListDataSource::first_changed_index`). Returns the first visible
    /// index whose content may have changed in the latest rebuild, or
    /// `None` for a genuine full change. Raw `ListModel`s report `None` —
    /// their observers already get fine-grained `DataChange` variants.
    pub(crate) first_changed_fn: Rc<dyn Fn() -> Option<usize>>,
    /// Resolve `index` to a [`RowAnchor`](crate::data_views::RowAnchor) that
    /// survives row movement. Keyless sources (a bare `ListModel`, or one that
    /// leaves `key_at` at its `None` default) get a fixed anchor.
    pub(crate) anchor_fn: Rc<dyn Fn(usize) -> crate::data_views::RowAnchor>,
    /// Erased DnD + lazy capability protocol (source-owned validation +
    /// windowing). Inert for `from_cloning_accessors` sources.
    pub(crate) dnd: DndLazy,
}

/// Shallow `Rc` bump of every erased accessor — hand-written rather than
/// derived so it carries no `T: Clone` bound (`T` appears only inside
/// `dyn Fn` signatures, never by value). Lets a view share its whole source
/// with its body pane instead of threading nine closures separately.
impl<T: 'static> Clone for ListSource<T> {
    fn clone(&self) -> Self {
        Self {
            len_fn: self.len_fn.clone(),
            with_item_fn: self.with_item_fn.clone(),
            with_item_str_fn: self.with_item_str_fn.clone(),
            read_item_fn: self.read_item_fn.clone(),
            observe_fn: self.observe_fn.clone(),
            move_item_fn: self.move_item_fn.clone(),
            remove_item_fn: self.remove_item_fn.clone(),
            first_changed_fn: self.first_changed_fn.clone(),
            anchor_fn: self.anchor_fn.clone(),
            dnd: self.dnd.clone(),
        }
    }
}

impl<T: 'static> ListSource<T> {
    /// A movement-proof handle to the row at `index`.
    pub(crate) fn anchor(&self, index: usize) -> crate::data_views::RowAnchor {
        (self.anchor_fn)(index)
    }
}

impl<T: 'static> ListSource<T> {
    pub(crate) fn from_model(model: ListModel<T>) -> Self {
        let m1 = model.clone();
        let m2 = model.clone();
        let m3 = model.clone();
        let m4 = model.clone();
        let m5 = model.clone();
        let m6 = model.clone();
        let m7 = model.clone();
        Self {
            // A bare `ListModel` has no row identity, so an anchor can only
            // report the index it was built with.
            anchor_fn: Rc::new(crate::data_views::RowAnchor::fixed),
            len_fn: Rc::new(move || m1.len()),
            with_item_fn: Rc::new(move |index, f| m2.with_item(index, |item| f(item))),
            with_item_str_fn: Rc::new(move |index, f| m6.with_item(index, |item| f(item))),
            read_item_fn: Rc::new(move |index, f| m7.with_item(index, |item| f(item)).is_some()),
            observe_fn: Rc::new(move |f| m3.observe_changes(move |c| f(c))),
            move_item_fn: Some(Rc::new(move |from, to| m4.move_item(from, to))),
            remove_item_fn: Some(Rc::new(move |index| {
                if index < m5.len() {
                    let _ = m5.remove(index);
                }
            })),
            first_changed_fn: Rc::new(|| None),
            dnd: DndLazy::from_source(Rc::new(model)),
        }
    }

    pub(crate) fn from_data_source<S: ListDataSource<Item = T>>(source: S) -> Self {
        Self::from_data_source_rc(Rc::new(source))
    }

    /// Like [`from_data_source`](Self::from_data_source) but takes a shared
    /// `Rc<S>`, so a caller that also needs the concrete source (e.g. to build
    /// a keyed-selection facade over the same `key_at` / `index_of`) can share
    /// one handle instead of erasing twice.
    pub(crate) fn from_data_source_rc<S: ListDataSource<Item = T>>(s: Rc<S>) -> Self {
        let s1 = s.clone();
        let s2 = s.clone();
        let s3 = s.clone();
        let s4 = s.clone();
        let s5 = s.clone();
        let s6 = s.clone();
        let s7 = s.clone();
        Self {
            anchor_fn: Rc::new(move |index| match s7.key_at(index) {
                Some(key) => {
                    let src = s7.clone();
                    crate::data_views::RowAnchor::new(Rc::new(move || {
                        if src.key_at(index).as_ref() == Some(&key) {
                            return Some(index);
                        }
                        src.index_of(&key)
                    }))
                }
                None => crate::data_views::RowAnchor::fixed(index),
            }),
            len_fn: Rc::new(move || s1.len()),
            with_item_fn: Rc::new(move |index, f| s2.with_item(index, |item| f(item))),
            with_item_str_fn: Rc::new(move |index, f| s5.with_item(index, |item| f(item))),
            read_item_fn: Rc::new(move |index, f| s6.with_item(index, |item| f(item)).is_some()),
            observe_fn: Rc::new(move |f| s3.observe_changes(move |c| f(c))),
            move_item_fn: None,
            remove_item_fn: None,
            first_changed_fn: Rc::new(move || s4.first_changed_index()),
            dnd: DndLazy::from_source(s),
        }
    }

    /// Build from a `len / item-at / observe` closure triple where the
    /// item getter returns an owned clone instead of borrowing. The
    /// resulting `with_item_fn` clones the item out, then hands a
    /// reference to the delegate. Used by `ComboBox`'s `ItemSource`,
    /// which fronts both `ListModel<T>` and `ListDataSource<T>` behind
    /// cloning accessors and doesn't carry a `&T` lifetime. DnD + lazy are
    /// inert for this path.
    pub(crate) fn from_cloning_accessors(
        len_fn: Rc<dyn Fn() -> usize>,
        item_at: Rc<dyn Fn(usize) -> Option<T>>,
        observe_fn: Rc<dyn Fn(Box<dyn Fn(&DataChange)>) -> ObserverHandle>,
    ) -> Self
    where
        T: Clone,
    {
        let item_at_str = item_at.clone();
        let item_at_read = item_at.clone();
        Self {
            // Accessor-backed sources expose no identity.
            anchor_fn: Rc::new(crate::data_views::RowAnchor::fixed),
            len_fn,
            with_item_fn: Rc::new(move |index, f| item_at(index).as_ref().map(f)),
            with_item_str_fn: Rc::new(move |index, f| item_at_str(index).as_ref().map(f)),
            read_item_fn: Rc::new(move |index, f| {
                if let Some(item) = item_at_read(index).as_ref() {
                    f(item);
                    true
                } else {
                    false
                }
            }),
            observe_fn,
            move_item_fn: None,
            remove_item_fn: None,
            first_changed_fn: Rc::new(|| None),
            dnd: DndLazy::inert(),
        }
    }

    pub(crate) fn len(&self) -> usize {
        (self.len_fn)()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod drag_identity_tests {
    use super::*;
    use bastyde_core::ObserverHandle;
    use bastyde_data::{
        DataChange, DragEligibility, DropPosition, DropQuery, DropResponse, ListModel,
    };
    use std::cell::RefCell;

    use crate::data_views::{RowDragData, ViewId, ViewKind};

    /// A keyed flat source that records every `reorder_within` call, so the
    /// tests can assert WHICH rows the erasure asked it to move.
    struct RecordingRows {
        rows: RefCell<Vec<u64>>,
        model: ListModel<u64>,
        reorders: RefCell<Vec<(Vec<u64>, u64, DropPosition)>>,
    }

    impl RecordingRows {
        fn new(ids: &[u64]) -> Self {
            Self {
                rows: RefCell::new(ids.to_vec()),
                model: ListModel::from_vec(ids.to_vec()),
                reorders: RefCell::new(Vec::new()),
            }
        }
        fn set(&self, ids: &[u64]) {
            *self.rows.borrow_mut() = ids.to_vec();
        }
    }

    impl ListDataSource for RecordingRows {
        type Item = u64;
        type Key = u64;
        fn len(&self) -> usize {
            self.rows.borrow().len()
        }
        fn with_item<R>(&self, index: usize, f: impl FnOnce(&u64) -> R) -> Option<R> {
            self.rows.borrow().get(index).map(f)
        }
        fn key_at(&self, index: usize) -> Option<u64> {
            self.rows.borrow().get(index).copied()
        }
        fn index_of(&self, key: &u64) -> Option<usize> {
            self.rows.borrow().iter().position(|k| k == key)
        }
        fn observe_changes(&self, f: impl Fn(&DataChange) + 'static) -> ObserverHandle {
            self.model.observe_changes(f)
        }
        fn drag(&self, _key: &u64) -> DragEligibility {
            DragEligibility::CanDrag
        }
        fn can_accept(&self, _query: &DropQuery<'_, u64>) -> DropResponse {
            DropResponse::Accept
        }
        fn reorder_within(&self, sources: &[u64], target: &u64, position: DropPosition) -> bool {
            self.reorders
                .borrow_mut()
                .push((sources.to_vec(), *target, position));
            true
        }
    }

    fn same_view_payload(view_id: ViewId, rows: Vec<usize>) -> DragPayload {
        DragPayload::typed(RowDragData::<u64> {
            source: view_id,
            rows,
            items: None,
        })
    }

    #[test]
    fn a_reorder_moves_the_rows_dragged_not_the_slots_they_left() {
        // Row 30 is grabbed at index 2, then the source reflows mid-drag (a
        // peer write / spring-load shape): a row appears above it. The drop
        // must move row 30 — not row 20, which now occupies index 2.
        let src = Rc::new(RecordingRows::new(&[10, 20, 30]));
        let list = ListSource::from_data_source_rc(src.clone());
        let vid = ViewId::next(ViewKind::List);

        // Drag-start, exactly as `RowExport::build_payload` does.
        let _thunk = (list.dnd.snapshot_out_fn)(&[2]);
        let payload = same_view_payload(vid, vec![2]);

        src.set(&[999, 10, 20, 30]);

        assert_eq!(
            (list.dnd.can_accept_fn)(&payload, 0, DropPosition::Before, vid),
            DropResponse::Accept
        );
        assert!((list.dnd.accept_drop_fn)(
            &payload,
            0,
            DropPosition::Before,
            vid
        ));
        assert_eq!(
            src.reorders.borrow().as_slice(),
            &[(vec![30], 999, DropPosition::Before)],
            "the dragged row's key must move, not whichever row slid into its old index"
        );
    }

    #[test]
    fn a_reflowed_own_row_still_rejects_a_drop_onto_itself() {
        // After the mid-drag reflow the dragged row sits at a NEW index; the
        // own-row rejection must follow it there (an index comparison against
        // the drag-start slot would wave the drop through).
        let src = Rc::new(RecordingRows::new(&[10, 20, 30]));
        let list = ListSource::from_data_source_rc(src.clone());
        let vid = ViewId::next(ViewKind::List);

        let _thunk = (list.dnd.snapshot_out_fn)(&[2]); // row 30
        let payload = same_view_payload(vid, vec![2]);

        src.set(&[999, 10, 20, 30]); // row 30 now at index 3

        assert_eq!(
            (list.dnd.can_accept_fn)(&payload, 3, DropPosition::Before, vid),
            DropResponse::Reject
        );
        assert!(!(list.dnd.accept_drop_fn)(
            &payload,
            3,
            DropPosition::Before,
            vid
        ));
        assert!(src.reorders.borrow().is_empty());
    }

    #[test]
    fn a_synthetic_keyboard_payload_stashes_at_construction() {
        // The Alt+Arrow path builds its payload outside `build_payload`; the
        // explicit stash call is what arms the accept path for it.
        let src = Rc::new(RecordingRows::new(&[10, 20, 30]));
        let list = ListSource::from_data_source_rc(src.clone());
        let vid = ViewId::next(ViewKind::List);

        (list.dnd.stash_drag_keys_fn)(&[1]);
        let payload = same_view_payload(vid, vec![1]);

        assert!((list.dnd.accept_drop_fn)(
            &payload,
            2,
            DropPosition::After,
            vid
        ));
        assert_eq!(
            src.reorders.borrow().as_slice(),
            &[(vec![20], 30, DropPosition::After)]
        );
    }
}

#[cfg(test)]
mod anchor_tests {
    use super::*;
    use bastyde_core::ObserverHandle;
    use bastyde_data::{DataChange, ListModel, SortFilterListModel};

    /// A flat source with REAL identity: rows carry an id independent of their
    /// position. This is the only shape a flat anchor can actually track --
    /// see `a_builtin_flat_source_has_no_identity_to_anchor_to` below.
    struct KeyedRows {
        rows: std::cell::RefCell<Vec<(u64, &'static str)>>,
        model: ListModel<u64>,
    }

    impl KeyedRows {
        fn new(ids: &[u64]) -> Self {
            Self {
                rows: std::cell::RefCell::new(ids.iter().map(|k| (*k, "row")).collect()),
                model: ListModel::from_vec(ids.to_vec()),
            }
        }
        fn set(&self, ids: &[u64]) {
            *self.rows.borrow_mut() = ids.iter().map(|k| (*k, "row")).collect();
        }
    }

    impl ListDataSource for KeyedRows {
        type Item = u64;
        type Key = u64;
        fn len(&self) -> usize {
            self.rows.borrow().len()
        }
        fn with_item<R>(&self, index: usize, f: impl FnOnce(&u64) -> R) -> Option<R> {
            self.rows.borrow().get(index).map(|(k, _)| f(k))
        }
        fn key_at(&self, index: usize) -> Option<u64> {
            self.rows.borrow().get(index).map(|(k, _)| *k)
        }
        fn index_of(&self, key: &u64) -> Option<usize> {
            self.rows.borrow().iter().position(|(k, _)| k == key)
        }
        fn observe_changes(&self, f: impl Fn(&DataChange) + 'static) -> ObserverHandle {
            self.model.observe_changes(f)
        }
    }

    #[test]
    fn a_keyed_flat_source_anchor_follows_its_row() {
        let src = Rc::new(KeyedRows::new(&[10, 20, 30]));
        let list = ListSource::from_data_source_rc(src.clone());
        let anchor = list.anchor(2); // row 30
        assert_eq!(anchor.index(), Some(2));

        src.set(&[1, 2, 10, 20, 30]); // two rows inserted above
        assert_eq!(
            anchor.index(),
            Some(4),
            "the anchor must track row 30 to its new index"
        );

        src.set(&[10, 20]); // row 30 removed
        assert_eq!(anchor.index(), None);
        assert!(!anchor.is_live());
    }

    #[test]
    fn a_sort_filter_projection_anchor_survives_a_filter_change() {
        // The flat fragility in practice: a filter changes underneath handlers
        // that were built against the previous projection. `SortFilterListModel`
        // keys rows by their SOURCE index, which reprojection does not renumber,
        // so the anchor tracks its row to the new visible position.
        let proj = Rc::new(
            SortFilterListModel::new(ListModel::from_vec(vec![10_i64, 20, 30])).with_predicate(
                "v",
                |t| {
                    let drop: i64 = t.parse().unwrap_or(i64::MIN);
                    Box::new(move |r: &i64| *r != drop)
                },
            ),
        );
        let list = ListSource::from_data_source_rc(proj.clone());
        let anchor = list.anchor(2); // value 30, source index 2
        assert_eq!(anchor.index(), Some(2));

        // Filter out the FIRST row: 30 slides to visible index 1.
        proj.set_filter("v", "10");
        assert_eq!(
            anchor.index(),
            Some(1),
            "the anchor must follow its row across the reprojection"
        );

        // Filter out the anchored row itself: the handler must no-op, not act
        // on whichever row now sits at its old position.
        proj.clear_filters();
        proj.set_filter("v", "30");
        assert_eq!(anchor.index(), None);
        assert!(!anchor.is_live());
    }

    #[test]
    fn keyed_selection_over_a_projection_now_works_and_prunes_filtered_rows() {
        // Same root cause as the anchor: with `key_at` defaulted to None, a
        // `KeyedSelectionModel` over a projection pruned EVERY key (nothing
        // could ever be found), so keyed selection was silently inert here.
        // Pin both the fix and the semantics it exposes.
        use bastyde_data::{KeyedSelectionModel, SelectionMode};

        let proj = Rc::new(
            SortFilterListModel::new(ListModel::from_vec(vec![10_i64, 20, 30])).with_predicate(
                "v",
                |t| {
                    let drop: i64 = t.parse().unwrap_or(i64::MIN);
                    Box::new(move |r: &i64| *r != drop)
                },
            ),
        );
        let keyed = KeyedSelectionModel::<usize>::new(SelectionMode::Multi);
        keyed.select(2); // source index of value 30

        // Presence is resolvable now, which it was not before.
        assert_eq!(
            ListDataSource::key_at(proj.as_ref(), 2),
            Some(2),
            "a visible row reports its source index"
        );
        assert!(keyed.is_selected(&2));

        // Filtering the row away makes it unresolvable: a pruning consumer
        // drops it. Documented rather than assumed -- unlike the tree side,
        // where `contains_key` checks the raw tree so a collapsed row keeps
        // its selection, a flat projection has no "hidden but present" notion.
        proj.set_filter("v", "30");
        assert_eq!(
            ListDataSource::index_of(proj.as_ref(), &2),
            None,
            "the filtered-out row has no visible position"
        );
    }

    #[test]
    fn a_bare_list_model_has_no_identity_to_anchor_to() {
        // Documents a real limit rather than asserting a wish: `ListModel` is a
        // Vec, so a row IS its position and `key_at` returns the index. Anchors
        // over one degrade to fixed -- no worse than capturing the index.
        let bare = ListSource::from_model(ListModel::from_vec(vec![1, 2, 3]));
        let a = bare.anchor(1);
        assert_eq!(a.index(), Some(1));
        assert!(a.is_live());
    }
}
