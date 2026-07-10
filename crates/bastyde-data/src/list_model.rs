// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `ListModel<T>` — concrete reactive list backed by a `Vec<T>`.
//!
//! `ListModel<T>` stores items in a heap-allocated `Vec<T>` behind
//! `Rc<RefCell<…>>`. Cloning a handle shares the same underlying data — there
//! is no deep copy. Every mutation method (`push`, `insert`, `remove`, `set`,
//! `move_item`, `replace_all`, `clear`) drops the internal borrow before
//! notifying observers, so observer callbacks may safely call read methods
//! (`len`, `with_item`) without a re-entrant borrow.
//!
//! `ListModel<T>` implements [`ListDataSource`] directly, so it can be handed
//! to any `ListView` / `TableView` without adaption. For lists too large to
//! hold in memory, implement [`ListDataSource`] directly on your own type
//! (paged database cursor, windowed feed, etc.).
//!
//! ## When to use
//!
//! Use `ListModel<T>` when the full list fits in memory and you want automatic
//! change notifications with no extra setup. Use a custom [`ListDataSource`]
//! when the source is external, huge, or lazy-loaded.
//!
//! ## Notifications
//!
//! Observers registered via [`ListModel::observe_changes`] receive a
//! [`DataChange`] describing the minimal change: `ItemsInserted`,
//! `ItemsRemoved`, `ItemUpdated`, `ItemsMoved`, or `Reset`. The
//! [`ObserverHandle`] returned is RAII — dropping
//! it unregisters the callback immediately.
//!
//! ```rust
//! # use bastyde_data::ListModel;
//! let model: ListModel<&str> = ListModel::new();
//! model.push("alpha");
//! model.push("beta");
//! model.push("gamma");
//! assert_eq!(model.len(), 3);
//! let second = model.with_item(1, |s| *s);
//! assert_eq!(second, Some("beta"));
//! model.set(0, "ALPHA");
//! model.remove(2);
//! assert_eq!(model.len(), 2);
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_core::ObserverHandle;

use crate::data_change::DataChange;
use crate::dnd_types::{
    DragEligibility, DragSource, DropCommit, DropPosition, DropQuery, DropResponse,
};
use crate::list_data_source::ListDataSource;

struct ObserverEntry {
    id: u64,
    callback: Rc<dyn Fn(&DataChange)>,
}

struct ListModelInner<T> {
    items: Vec<T>,
    observers: Vec<ObserverEntry>,
    next_observer_id: u64,
    /// Strong handle to the debug-registry adapter for this model.
    /// Owned here so that the registration drops automatically when
    /// the inner is freed (the adapter holds only a `Weak` to inner,
    /// breaking the cycle). `None` until `.debug_named()` is called.
    /// Compiled out in release.
    #[cfg(debug_assertions)]
    debug_adapter: Option<Rc<dyn crate::debug_registry::ModelDebug>>,
}

/// A concrete reactive list that stores items in a `Vec<T>`.
///
/// `ListModel<T>` is `Clone` — cloning produces a second handle to the same
/// data. Multiple widgets can hold clones and all see the same items.
///
/// Every mutation method modifies the internal Vec, drops the mutable borrow,
/// then notifies observers. By the time any observer runs, the borrow is
/// released and shared borrows (`len()`, `with_item()`) are safe.
pub struct ListModel<T: 'static> {
    inner: Rc<RefCell<ListModelInner<T>>>,
}

impl<T: 'static> ListModel<T> {
    /// Create an empty list model.
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(ListModelInner {
                items: Vec::new(),
                observers: Vec::new(),
                next_observer_id: 1,
                #[cfg(debug_assertions)]
                debug_adapter: None,
            })),
        }
    }

    /// Create a list model from an existing vector.
    pub fn from_vec(items: Vec<T>) -> Self {
        Self {
            inner: Rc::new(RefCell::new(ListModelInner {
                items,
                observers: Vec::new(),
                next_observer_id: 1,
                #[cfg(debug_assertions)]
                debug_adapter: None,
            })),
        }
    }

    /// Number of items in the list.
    pub fn len(&self) -> usize {
        self.inner.borrow().items.len()
    }

    /// Whether the list is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.borrow().items.is_empty()
    }

    /// Access an item by index via a callback. Returns `None` if out of bounds.
    ///
    /// The callback pattern avoids returning a reference that would need to
    /// outlive the `RefCell` borrow guard.
    pub fn with_item<R>(&self, index: usize, f: impl FnOnce(&T) -> R) -> Option<R> {
        let guard = self.inner.borrow();
        guard.items.get(index).map(f)
    }

    /// Append an item to the end of the list.
    pub fn push(&self, item: T) {
        let index = {
            let mut guard = self.inner.borrow_mut();
            let index = guard.items.len();
            guard.items.push(item);
            index
        };
        self.notify(DataChange::ItemsInserted {
            range: index..index + 1,
        });
    }

    /// Insert an item at the given index.
    ///
    /// # Panics
    /// Panics if `index > len()`.
    pub fn insert(&self, index: usize, item: T) {
        {
            let mut guard = self.inner.borrow_mut();
            guard.items.insert(index, item);
        }
        self.notify(DataChange::ItemsInserted {
            range: index..index + 1,
        });
    }

    /// Remove and return the item at the given index.
    ///
    /// # Panics
    /// Panics if `index >= len()`.
    pub fn remove(&self, index: usize) -> T {
        let item = {
            let mut guard = self.inner.borrow_mut();
            guard.items.remove(index)
        };
        self.notify(DataChange::ItemsRemoved {
            range: index..index + 1,
        });
        item
    }

    /// Replace the item at the given index.
    ///
    /// # Panics
    /// Panics if `index >= len()`.
    pub fn set(&self, index: usize, item: T) {
        {
            let mut guard = self.inner.borrow_mut();
            guard.items[index] = item;
        }
        self.notify(DataChange::ItemUpdated { index });
    }

    /// Move an item from one index to another.
    ///
    /// The item at `from` is removed, then inserted at `to` (post-removal index).
    ///
    /// # Panics
    /// Panics if either index is out of bounds.
    pub fn move_item(&self, from: usize, to: usize) {
        if from == to {
            return;
        }
        {
            let mut guard = self.inner.borrow_mut();
            let item = guard.items.remove(from);
            guard.items.insert(to, item);
        }
        self.notify(DataChange::ItemsMoved { from, to, count: 1 });
    }

    /// Move a set of items so they land **contiguously** at a drop gap,
    /// preserving their relative order — the multi-row same-view reorder
    /// commit. `indices` are the items' current positions (any order;
    /// out-of-range entries are ignored); `insert_gap` is the destination in
    /// `0..=len` expressed in the pre-move indexing (i.e. "land before the item
    /// currently at `insert_gap`"; `len` = at the end).
    ///
    /// Returns whether anything moved (`false` if `indices` held no in-range
    /// entry). A **contiguous** source block emits a single
    /// [`DataChange::ItemsMoved`] — so index-based selection follows the moved
    /// rows; a non-contiguous set emits [`DataChange::Reset`] (that permutation
    /// is not expressible as one `ItemsMoved`, and selection is dropped). For a
    /// single index prefer [`move_item`](Self::move_item).
    pub fn move_items(&self, indices: &[usize], insert_gap: usize) -> bool {
        let len = self.len();
        let mut idx: Vec<usize> = indices.iter().copied().filter(|&i| i < len).collect();
        idx.sort_unstable();
        idx.dedup();
        if idx.is_empty() {
            return false;
        }
        let contiguous = idx.windows(2).all(|w| w[1] == w[0] + 1);
        let from0 = idx[0];
        let count = idx.len();
        let at;
        {
            let mut guard = self.inner.borrow_mut();
            // Remove from the back so earlier indices stay valid, then restore
            // ascending (original) order.
            let mut block: Vec<T> = idx.iter().rev().map(|&i| guard.items.remove(i)).collect();
            block.reverse();
            let removed_before = idx.iter().filter(|&&i| i < insert_gap).count();
            at = insert_gap
                .saturating_sub(removed_before)
                .min(guard.items.len());
            for (off, item) in block.into_iter().enumerate() {
                guard.items.insert(at + off, item);
            }
        }
        if contiguous && from0 != at {
            self.notify(DataChange::ItemsMoved {
                from: from0,
                to: at,
                count,
            });
        } else if contiguous {
            // No net movement (block landed where it started).
        } else {
            self.notify(DataChange::Reset);
        }
        true
    }

    /// Replace the entire list contents.
    pub fn replace_all(&self, items: Vec<T>) {
        {
            let mut guard = self.inner.borrow_mut();
            guard.items = items;
        }
        self.notify(DataChange::Reset);
    }

    /// Remove all items from the list.
    pub fn clear(&self) {
        {
            let mut guard = self.inner.borrow_mut();
            guard.items.clear();
        }
        self.notify(DataChange::Reset);
    }

    /// Register an observer that is called on every mutation.
    /// Returns an `ObserverHandle` — dropping it removes the callback.
    pub fn observe_changes(&self, f: impl Fn(&DataChange) + 'static) -> ObserverHandle {
        let mut guard = self.inner.borrow_mut();
        let id = guard.next_observer_id;
        guard.next_observer_id += 1;
        guard.observers.push(ObserverEntry {
            id,
            callback: Rc::new(f),
        });
        let inner = self.inner.clone();
        ObserverHandle::new(
            self.inner.clone(),
            id,
            Rc::new(move |observer_id| {
                inner.borrow_mut().observers.retain(|e| e.id != observer_id);
            }),
        )
    }

    fn notify(&self, change: DataChange) {
        let callbacks: Vec<Rc<dyn Fn(&DataChange)>> = self
            .inner
            .borrow()
            .observers
            .iter()
            .map(|e| e.callback.clone())
            .collect();
        for cb in &callbacks {
            cb(&change);
        }
    }
}

impl<T: std::fmt::Debug + 'static> ListModel<T> {
    /// Register this model with the debug inspector under `name`. In
    /// release builds (`!cfg(debug_assertions)`) this is a no-op
    /// pass-through so call sites stay free of `#[cfg]` lines.
    ///
    /// Idempotent on repeated calls — the latest registration wins.
    /// The registration drops automatically when the last `ListModel`
    /// handle is freed (the adapter the registry holds is `Weak`).
    pub fn debug_named(self, _name: impl Into<String>) -> Self {
        #[cfg(debug_assertions)]
        {
            let weak = Rc::downgrade(&self.inner);
            let adapter: Rc<dyn crate::debug_registry::ModelDebug> =
                Rc::new(ListModelDebug::<T> { weak });
            let name = _name.into();
            crate::debug_registry::register(name, Rc::downgrade(&adapter));
            self.inner.borrow_mut().debug_adapter = Some(adapter);
        }
        self
    }
}

#[cfg(debug_assertions)]
struct ListModelDebug<T> {
    weak: std::rc::Weak<RefCell<ListModelInner<T>>>,
}

#[cfg(debug_assertions)]
impl<T: std::fmt::Debug + 'static> crate::debug_registry::ModelDebug for ListModelDebug<T> {
    fn kind(&self) -> &'static str {
        "ListModel"
    }
    fn len(&self) -> usize {
        self.weak
            .upgrade()
            .map(|inner| inner.borrow().items.len())
            .unwrap_or(0)
    }
    fn debug_dump(&self, out: &mut dyn std::fmt::Write) {
        let Some(inner) = self.weak.upgrade() else {
            return;
        };
        let guard = inner.borrow();
        for (i, item) in guard.items.iter().enumerate() {
            let _ = writeln!(out, "[{}] {:?}", i, item);
        }
    }
}

impl<T: 'static> Default for ListModel<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: 'static> Clone for ListModel<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T: std::fmt::Debug + 'static> std::fmt::Debug for ListModel<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListModel")
            .field("len", &self.inner.borrow().items.len())
            .finish()
    }
}

/// `ListModel` is the built-in fully-resident, in-memory `ListDataSource`.
/// Identity is positional (`Key = usize`); a `SameView` drop reorders via
/// `move_item`. `Into` and `Foreign` drops are rejected (a flat list does not
/// nest, and a bare model knows no foreign payloads).
impl<T: 'static> ListDataSource for ListModel<T> {
    type Item = T;
    type Key = usize;

    fn len(&self) -> usize {
        ListModel::len(self)
    }

    fn with_item<R>(&self, index: usize, f: impl FnOnce(&T) -> R) -> Option<R> {
        ListModel::with_item(self, index, f)
    }

    fn key_at(&self, index: usize) -> Option<usize> {
        (index < ListModel::len(self)).then_some(index)
    }

    fn index_of(&self, key: &usize) -> Option<usize> {
        (*key < ListModel::len(self)).then_some(*key)
    }

    fn observe_changes(&self, f: impl Fn(&DataChange) + 'static) -> ObserverHandle {
        ListModel::observe_changes(self, f)
    }

    fn drag(&self, _key: &usize) -> DragEligibility {
        DragEligibility::CanDrag
    }

    fn can_accept(&self, query: &DropQuery<'_, usize>) -> DropResponse {
        match &query.source {
            DragSource::SameView { .. } => match query.position {
                DropPosition::Into => DropResponse::Reject,
                DropPosition::Before | DropPosition::After => DropResponse::Accept,
            },
            DragSource::Foreign { .. } => DropResponse::Reject,
        }
    }

    fn accept_drop(&self, commit: DropCommit<'_, usize>) -> bool {
        let DragSource::SameView { key: from } = commit.source else {
            return false;
        };
        let len = ListModel::len(self);
        if from >= len {
            return false;
        }
        let target = commit.target;
        // move_item removes `from` before inserting, so an insertion point above
        // the source's old slot shifts down by one.
        let shift = if from < target { 1 } else { 0 };
        let to = match commit.position {
            DropPosition::Before => target.saturating_sub(shift),
            DropPosition::After => (target + 1).saturating_sub(shift),
            DropPosition::Into => return false,
        };
        let to = to.min(len.saturating_sub(1));
        self.move_item(from, to);
        true
    }

    fn reorder_within(&self, sources: &[usize], target: &usize, position: DropPosition) -> bool {
        // A `ListModel`'s key IS the index, so the stable-key default (which
        // re-anchors on a just-moved key) would corrupt after the first move —
        // route the multi-row case through the index-safe block move instead.
        // A single row keeps the finer-grained `accept_drop`/`move_item` path.
        if sources.len() <= 1 {
            let Some(&from) = sources.first() else {
                return false;
            };
            return self.accept_drop(DropCommit {
                source: DragSource::SameView { key: from },
                target: *target,
                position,
            });
        }
        let gap = match position {
            DropPosition::Before => *target,
            DropPosition::After => *target + 1,
            DropPosition::Into => return false,
        };
        self.move_items(sources, gap)
    }

    fn on_drag_out(&self, key: &usize) {
        // Source-side completion for a foreign move: drop the row that was
        // accepted elsewhere. Callers remove in descending index order so
        // earlier keys stay valid across a multi-row transfer.
        if *key < ListModel::len(self) {
            let _ = self.remove(*key);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn new_is_empty() {
        let model: ListModel<String> = ListModel::new();
        assert!(model.is_empty());
        assert_eq!(model.len(), 0);
    }

    #[test]
    fn from_vec() {
        let model = ListModel::from_vec(vec![10, 20, 30]);
        assert_eq!(model.len(), 3);
        assert_eq!(model.with_item(1, |v| *v), Some(20));
    }

    #[test]
    fn push_emits_inserted() {
        let model = ListModel::new();
        let changes: Rc<RefCell<Vec<DataChange>>> = Rc::new(RefCell::new(Vec::new()));
        let c = changes.clone();
        let _handle = model.observe_changes(move |change| {
            c.borrow_mut().push(change.clone());
        });

        model.push("a");
        model.push("b");

        let log = changes.borrow();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0], DataChange::ItemsInserted { range: 0..1 });
        assert_eq!(log[1], DataChange::ItemsInserted { range: 1..2 });
    }

    #[test]
    fn insert_emits_inserted() {
        let model = ListModel::from_vec(vec![1, 2, 3]);
        let changes: Rc<RefCell<Vec<DataChange>>> = Rc::new(RefCell::new(Vec::new()));
        let c = changes.clone();
        let _handle = model.observe_changes(move |change| {
            c.borrow_mut().push(change.clone());
        });

        model.insert(1, 99);

        assert_eq!(model.len(), 4);
        assert_eq!(model.with_item(1, |v| *v), Some(99));
        let log = changes.borrow();
        assert_eq!(log.len(), 1, "insert should emit exactly one change");
        assert_eq!(log[0], DataChange::ItemsInserted { range: 1..2 });
    }

    #[test]
    fn remove_emits_removed() {
        let model = ListModel::from_vec(vec!["a", "b", "c"]);
        let changes: Rc<RefCell<Vec<DataChange>>> = Rc::new(RefCell::new(Vec::new()));
        let c = changes.clone();
        let _handle = model.observe_changes(move |change| {
            c.borrow_mut().push(change.clone());
        });

        let removed = model.remove(1);
        assert_eq!(removed, "b");
        assert_eq!(model.len(), 2);
        let log = changes.borrow();
        assert_eq!(log.len(), 1, "remove should emit exactly one change");
        assert_eq!(log[0], DataChange::ItemsRemoved { range: 1..2 });
    }

    #[test]
    fn set_emits_updated() {
        let model = ListModel::from_vec(vec![10, 20, 30]);
        let changes: Rc<RefCell<Vec<DataChange>>> = Rc::new(RefCell::new(Vec::new()));
        let c = changes.clone();
        let _handle = model.observe_changes(move |change| {
            c.borrow_mut().push(change.clone());
        });

        model.set(2, 99);
        assert_eq!(model.with_item(2, |v| *v), Some(99));
        let log = changes.borrow();
        assert_eq!(log.len(), 1, "set should emit exactly one change");
        assert_eq!(log[0], DataChange::ItemUpdated { index: 2 });
    }

    #[test]
    fn move_item_emits_moved() {
        let model = ListModel::from_vec(vec!["a", "b", "c", "d"]);
        let changes: Rc<RefCell<Vec<DataChange>>> = Rc::new(RefCell::new(Vec::new()));
        let c = changes.clone();
        let _handle = model.observe_changes(move |change| {
            c.borrow_mut().push(change.clone());
        });

        model.move_item(0, 2);
        // After: ["b", "c", "a", "d"]
        assert_eq!(model.with_item(0, |v| *v), Some("b"));
        assert_eq!(model.with_item(2, |v| *v), Some("a"));
        let log = changes.borrow();
        assert_eq!(log.len(), 1, "move_item should emit exactly one change");
        assert_eq!(
            log[0],
            DataChange::ItemsMoved {
                from: 0,
                to: 2,
                count: 1
            }
        );
    }

    #[test]
    fn move_item_same_index_is_noop() {
        let model = ListModel::from_vec(vec![1, 2, 3]);
        let count = Rc::new(Cell::new(0));
        let c = count.clone();
        let _handle = model.observe_changes(move |_| {
            c.set(c.get() + 1);
        });

        model.move_item(1, 1);
        assert_eq!(count.get(), 0);
    }

    #[test]
    fn replace_all_emits_reset() {
        let model = ListModel::from_vec(vec![1, 2]);
        let changes: Rc<RefCell<Vec<DataChange>>> = Rc::new(RefCell::new(Vec::new()));
        let c = changes.clone();
        let _handle = model.observe_changes(move |change| {
            c.borrow_mut().push(change.clone());
        });

        model.replace_all(vec![10, 20, 30]);
        assert_eq!(model.len(), 3);
        let log = changes.borrow();
        assert_eq!(log[0], DataChange::Reset);
    }

    #[test]
    fn clear_emits_reset() {
        let model = ListModel::from_vec(vec![1, 2, 3]);
        let changes: Rc<RefCell<Vec<DataChange>>> = Rc::new(RefCell::new(Vec::new()));
        let c = changes.clone();
        let _handle = model.observe_changes(move |change| {
            c.borrow_mut().push(change.clone());
        });

        model.clear();
        assert!(model.is_empty());
        let log = changes.borrow();
        assert_eq!(log[0], DataChange::Reset);
    }

    #[test]
    fn observer_removed_on_handle_drop() {
        let model = ListModel::new();
        let count = Rc::new(Cell::new(0));
        let c = count.clone();
        let handle = model.observe_changes(move |_| {
            c.set(c.get() + 1);
        });

        model.push(1);
        assert_eq!(count.get(), 1);

        drop(handle);
        model.push(2);
        assert_eq!(count.get(), 1); // Not called again
    }

    #[test]
    fn multiple_observers() {
        let model = ListModel::new();
        let count = Rc::new(Cell::new(0));
        let c1 = count.clone();
        let c2 = count.clone();
        let _h1 = model.observe_changes(move |_| c1.set(c1.get() + 1));
        let _h2 = model.observe_changes(move |_| c2.set(c2.get() + 1));

        model.push(42);
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn clone_shares_data() {
        let model = ListModel::from_vec(vec![1, 2, 3]);
        let clone = model.clone();

        model.push(4);
        assert_eq!(clone.len(), 4);
        assert_eq!(clone.with_item(3, |v| *v), Some(4));
    }

    #[test]
    fn clone_shares_observers() {
        let model = ListModel::from_vec(vec![1, 2]);
        let count = Rc::new(Cell::new(0));
        let c = count.clone();
        let _handle = model.observe_changes(move |_| c.set(c.get() + 1));

        let clone = model.clone();
        clone.push(3); // mutation on clone triggers observer registered on original
        assert_eq!(count.get(), 1);
    }

    #[test]
    fn with_item_out_of_bounds_returns_none() {
        let model = ListModel::from_vec(vec![1, 2]);
        assert_eq!(model.with_item(5, |v| *v), None);
    }

    #[test]
    #[should_panic]
    fn remove_out_of_bounds_panics() {
        let model = ListModel::from_vec(vec![1]);
        model.remove(5);
    }

    // ── ListDataSource capability protocol ──────────────────────────────

    fn order<T: Clone>(model: &ListModel<T>) -> Vec<T> {
        (0..model.len())
            .map(|i| model.with_item(i, |v| v.clone()).unwrap())
            .collect()
    }

    #[test]
    fn list_source_accept_drop_after_reorders() {
        // [a,b,c,d]; drag index 0 (a) After index 2 (c) → [b,c,a,d]
        let model = ListModel::from_vec(vec!["a", "b", "c", "d"]);
        assert!(model.accept_drop(DropCommit {
            source: DragSource::SameView { key: 0 },
            target: 2,
            position: DropPosition::After,
        }));
        assert_eq!(order(&model), vec!["b", "c", "a", "d"]);
    }

    #[test]
    fn list_source_accept_drop_before_reorders() {
        // [a,b,c,d]; drag index 3 (d) Before index 1 (b) → [a,d,b,c]
        let model = ListModel::from_vec(vec!["a", "b", "c", "d"]);
        assert!(model.accept_drop(DropCommit {
            source: DragSource::SameView { key: 3 },
            target: 1,
            position: DropPosition::Before,
        }));
        assert_eq!(order(&model), vec!["a", "d", "b", "c"]);
    }

    #[test]
    fn list_source_can_accept_rejects_into_accepts_sibling() {
        let model = ListModel::from_vec(vec![1, 2, 3]);
        // A flat list does not nest → Into is forbidden.
        assert_eq!(
            model.can_accept(&DropQuery {
                source: DragSource::SameView { key: 0 },
                target: 1,
                position: DropPosition::Into,
            }),
            DropResponse::Reject
        );
        // Sibling reorder is allowed.
        assert_eq!(
            model.can_accept(&DropQuery {
                source: DragSource::SameView { key: 0 },
                target: 1,
                position: DropPosition::After,
            }),
            DropResponse::Accept
        );
    }

    #[test]
    fn list_source_key_is_positional_identity() {
        let model = ListModel::from_vec(vec![10, 20, 30]);
        assert_eq!(model.key_at(1), Some(1));
        assert_eq!(model.index_of(&2), Some(2));
        assert_eq!(model.key_at(5), None);
        assert_eq!(model.index_of(&9), None);
    }

    fn snapshot<T: Clone>(model: &ListModel<T>) -> Vec<T> {
        (0..model.len())
            .filter_map(|i| model.with_item(i, |v| v.clone()))
            .collect()
    }

    #[test]
    fn move_items_block_move_contiguous_and_ordered() {
        // Move a non-contiguous set {A(0), C(2), E(4)} to land at gap 3
        // (before the item originally at index 3, i.e. D).
        let model = ListModel::from_vec(vec!['A', 'B', 'C', 'D', 'E', 'F']);
        model.move_items(&[0, 2, 4], 3);
        // gap 3 = "before the original item at index 3" (D). Remove A,C,E →
        // [B,D,F]; two removed before the gap → insert the block before D.
        assert_eq!(snapshot(&model), vec!['B', 'A', 'C', 'E', 'D', 'F']);
    }

    #[test]
    fn move_items_to_end() {
        let model = ListModel::from_vec(vec![1, 2, 3, 4]);
        model.move_items(&[0, 1], 4);
        assert_eq!(snapshot(&model), vec![3, 4, 1, 2]);
    }

    #[test]
    fn move_items_ignores_out_of_range_and_dedups() {
        let model = ListModel::from_vec(vec![1, 2, 3]);
        model.move_items(&[1, 1, 9], 3);
        assert_eq!(snapshot(&model), vec![1, 3, 2]);
    }

    #[test]
    fn move_items_single_matches_move_item() {
        let a = ListModel::from_vec(vec![1, 2, 3, 4, 5]);
        let b = ListModel::from_vec(vec![1, 2, 3, 4, 5]);
        // move_items([1], gap 4) == move a block of one from 1 to before-4.
        a.move_items(&[1], 4);
        // Equivalent single move: remove index 1, insert at post-removal index 3.
        b.move_item(1, 3);
        assert_eq!(snapshot(&a), snapshot(&b));
    }

    #[test]
    fn reorder_within_multi_row_lands_contiguously() {
        // Exercise the ListModel `reorder_within` override (multi → block move).
        let model = ListModel::from_vec(vec![0, 1, 2, 3, 4, 5]);
        // Drag rows {0,1} to drop After index 4.
        assert!(model.reorder_within(&[0, 1], &4, DropPosition::After));
        assert_eq!(snapshot(&model), vec![2, 3, 4, 0, 1, 5]);
    }

    #[test]
    fn reorder_within_single_row_uses_accept_drop() {
        let model = ListModel::from_vec(vec![10, 20, 30]);
        assert!(model.reorder_within(&[0], &2, DropPosition::After));
        assert_eq!(snapshot(&model), vec![20, 30, 10]);
    }

    #[test]
    fn reorder_within_into_is_rejected_for_flat_list() {
        let model = ListModel::from_vec(vec![1, 2, 3]);
        assert!(!model.reorder_within(&[0, 1], &2, DropPosition::Into));
        assert_eq!(snapshot(&model), vec![1, 2, 3]);
    }

    #[test]
    fn on_drag_out_removes_the_moved_row() {
        let model = ListModel::from_vec(vec!['a', 'b', 'c']);
        model.on_drag_out(&1);
        assert_eq!(snapshot(&model), vec!['a', 'c']);
    }

    #[test]
    fn move_items_contiguous_emits_moved_non_contiguous_resets() {
        let model = ListModel::from_vec(vec![0, 1, 2, 3, 4]);
        let log: Rc<RefCell<Vec<DataChange>>> = Rc::new(RefCell::new(Vec::new()));
        let l = log.clone();
        let _h = model.observe_changes(move |c| l.borrow_mut().push(c.clone()));
        // Contiguous block → ItemsMoved (so index selection can follow).
        assert!(model.move_items(&[1, 2], 5));
        assert!(matches!(
            log.borrow().last(),
            Some(DataChange::ItemsMoved { .. })
        ));
        log.borrow_mut().clear();
        // Non-contiguous set → Reset.
        assert!(model.move_items(&[0, 2], 0));
        assert!(matches!(log.borrow().last(), Some(DataChange::Reset)));
    }

    #[test]
    fn reorder_within_and_move_items_report_no_move_for_out_of_range() {
        let model = ListModel::from_vec(vec![1, 2, 3]);
        assert!(!model.reorder_within(&[99, 100], &0, DropPosition::Before));
        assert!(!model.move_items(&[99, 100], 0));
        assert_eq!(snapshot(&model), vec![1, 2, 3]);
    }
}
