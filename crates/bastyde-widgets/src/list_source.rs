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
                if let Some(rd) = payload.get_typed::<RowDragData<T>>()
                    && rd.source == view_id
                {
                    // Can't drop a selection onto one of its own rows.
                    if rd.rows.contains(&target_index) {
                        return DropResponse::Reject;
                    }
                    // Homogeneous flat reorder: the first dragged row is a fair
                    // representative for the hover verdict (all move as a block).
                    let Some(source_key) = rd.rows.first().and_then(|&i| s2.key_at(i)) else {
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
                if let Some(rd) = payload.get_typed::<RowDragData<T>>()
                    && rd.source == view_id
                {
                    if rd.rows.contains(&target_index) {
                        return false;
                    }
                    let keys: Vec<S::Key> = rd.rows.iter().filter_map(|&i| s3.key_at(i)).collect();
                    if keys.is_empty() {
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
                // Resolve stable keys NOW (descending index order for the
                // index-keyed case); the returned thunk removes them later.
                let mut pairs: Vec<(usize, S::Key)> = indices
                    .iter()
                    .filter_map(|&i| s4.key_at(i).map(|k| (i, k)))
                    .collect();
                pairs.sort_by_key(|&(i, _)| std::cmp::Reverse(i));
                let s = s4.clone();
                Box::new(move || {
                    for (_, k) in &pairs {
                        s.on_drag_out(k);
                    }
                }) as Box<dyn Fn()>
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
    /// Erased DnD + lazy capability protocol (source-owned validation +
    /// windowing). Inert for `from_cloning_accessors` sources.
    pub(crate) dnd: DndLazy,
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
        Self {
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
