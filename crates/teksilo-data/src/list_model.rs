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
//! # use teksilo_data::ListModel;
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
use std::collections::HashSet;
use std::hash::Hash;
use std::ops::Range;
use std::rc::Rc;

use teksilo_core::ObserverHandle;

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

impl<T: PartialEq + 'static> ListModel<T> {
    /// Reconcile the list's contents with `new_items`, matching old and new
    /// rows **by key** (`key_fn`) instead of wholesale-replacing them, and
    /// emitting the minimal set of granular [`DataChange`]s needed to reach
    /// that state — never [`DataChange::Reset`].
    ///
    /// This is the primitive a live view needs when a peer process (or any
    /// other out-of-band writer) reloads a backing file and the merged
    /// result must land in a `ListModel` that a `ListView` is *currently
    /// displaying*, without wiping the user's selection or keyboard focus
    /// mid-interaction. `replace_all`/`clear` always emit `Reset`, and a
    /// `Reset` unconditionally clears a positional `SelectionModel`
    /// (`RowSelection::from_index`) — `reconcile_by_key` is how a caller
    /// avoids that.
    ///
    /// Emits, in this order, coalescing contiguous runs into a single event
    /// each:
    /// - [`DataChange::ItemsRemoved`] for keys present in the old list but
    ///   absent from `new_items`;
    /// - [`DataChange::ItemsMoved`] (single-row blocks) to re-order the
    ///   surviving rows into `new_items`'s relative order — skipped
    ///   entirely for rows already in the right place, so an append-only or
    ///   remove-only reload emits **no** moves at all;
    /// - [`DataChange::ItemsInserted`] for keys present in `new_items` but
    ///   not in the old list;
    /// - [`DataChange::ItemUpdated`] for a row whose key is unchanged but
    ///   whose content differs (`T: PartialEq`) — the row's stored value is
    ///   replaced with the incoming one.
    ///
    /// If `new_items` is identical (same keys, same order, same content, by
    /// `PartialEq`) to the current contents, **no** change is emitted and no
    /// observer runs — reconciling with unchanged data is silent.
    ///
    /// # Preconditions
    /// `key_fn` must be a pure, stable function of an item's identity (not
    /// its content) and keys must be **unique** within both the current list
    /// and `new_items`. See `# Panics` below — violating either is a caller
    /// bug, not a silently-tolerated edge case.
    ///
    /// # Panics
    /// Panics (via an internal `.expect`) if `key_fn` is not stable — it
    /// returns a different key for the same item across the two calls this
    /// method makes to it (once while snapshotting the current list's keys,
    /// once while re-deriving a key during the write pass) — or if a key is
    /// **duplicated** within the current list or within `new_items`. Both
    /// break the same invariant the write pass relies on: "the item that
    /// was accounted for under this key is still findable at or after the
    /// write cursor." A duplicate key means two different items raced to
    /// claim one key slot, so by the time the second one is processed the
    /// slot the accounting expected is already gone. This is this crate's
    /// usual documented-panic-on-contract-violation style (see e.g.
    /// [`TreeModel::remove`](crate::TreeModel::remove)) — a caller-side bug
    /// surfaced immediately as a panic, not silently wrong data.
    ///
    /// # Complexity
    /// Re-ordering is a straightforward left-to-right pass that moves each
    /// out-of-place survivor into its target slot; it is correct and always
    /// granular, but is not guaranteed to emit the mathematically fewest
    /// possible `ItemsMoved` events for an adversarial permutation (an
    /// LIS-based scheme could do slightly better there). For the common
    /// case this primitive targets — a peer append/remove/edit merged back
    /// in — the existing relative order of untouched rows is preserved
    /// as-is, so no moves are emitted at all.
    pub fn reconcile_by_key<K: Eq + Hash>(&self, new_items: Vec<T>, key_fn: impl Fn(&T) -> K) {
        let mut changes: Vec<DataChange> = Vec::new();
        {
            let mut guard = self.inner.borrow_mut();
            reconcile_vec(&mut guard.items, new_items, &key_fn, &mut changes);
        }
        for change in changes {
            self.notify(change);
        }
    }
}

/// Diff `items` (current contents) against `new_items` by key, mutating
/// `items` in place to match `new_items` exactly and pushing one
/// [`DataChange`] per atomic step taken. Kept as a free function (rather
/// than inlined into `reconcile_by_key`) so it can be exercised directly —
/// and so it operates on the bare `Vec<T>` while the caller still holds the
/// `RefCell` borrow, before any observer notification fires.
fn reconcile_vec<T: PartialEq, K: Eq + Hash>(
    items: &mut Vec<T>,
    new_items: Vec<T>,
    key_fn: &impl Fn(&T) -> K,
    out: &mut Vec<DataChange>,
) {
    // Pair every incoming item with its key up front: we need the key
    // before we decide anything, and this avoids re-deriving ownership of
    // the item later.
    let new_pairs: Vec<(K, T)> = new_items.into_iter().map(|it| (key_fn(&it), it)).collect();
    let new_key_set: HashSet<&K> = new_pairs.iter().map(|(k, _)| k).collect();

    // ---- Phase 1: drop old items whose key no longer exists in the
    // incoming list. Computed against a pre-removal key snapshot so later
    // removals don't perturb earlier indices; applied back-to-front so
    // each removed range's indices stay valid at the point it's removed. ----
    let old_keys: Vec<K> = items.iter().map(key_fn).collect();
    let remove_idxs: Vec<usize> = (0..items.len())
        .filter(|&i| !new_key_set.contains(&old_keys[i]))
        .collect();
    for range in coalesce_ranges(&remove_idxs).into_iter().rev() {
        items.drain(range.clone());
        out.push(DataChange::ItemsRemoved { range });
    }

    // From here on, `items` holds exactly the surviving ("common") rows, in
    // their original relative order.
    let remaining_keys: HashSet<K> = items.iter().map(key_fn).collect();

    // ---- Phase 2/3: walk the target order left to right. A common key is
    // moved into place (if it isn't already there) and its content updated
    // in place if it changed; a brand-new key is buffered and flushed as a
    // single contiguous `ItemsInserted` run as soon as a common key (or the
    // end of the list) is reached. ----
    let mut cursor = 0usize;
    let mut pending_inserts: Vec<T> = Vec::new();

    for (key, new_item) in new_pairs {
        if remaining_keys.contains(&key) {
            flush_inserts(items, &mut cursor, &mut pending_inserts, out);

            // Invariant: items[0..cursor] already matches the target
            // prefix, so this key — being common and not yet placed — must
            // sit somewhere at or after `cursor`.
            let pos = items
                .iter()
                .skip(cursor)
                .position(|it| key_fn(it) == key)
                .expect(
                    "reconcile_by_key: key reported common but not found — \
                     key_fn must be stable and keys unique",
                );
            let actual = cursor + pos;
            if actual != cursor {
                let val = items.remove(actual);
                items.insert(cursor, val);
                out.push(DataChange::ItemsMoved {
                    from: actual,
                    to: cursor,
                    count: 1,
                });
            }
            if items[cursor] != new_item {
                items[cursor] = new_item;
                out.push(DataChange::ItemUpdated { index: cursor });
            }
            cursor += 1;
        } else {
            pending_inserts.push(new_item);
        }
    }
    flush_inserts(items, &mut cursor, &mut pending_inserts, out);
}

/// Splice any buffered new rows into `items` at `*cursor` as one contiguous
/// block, emit the single `ItemsInserted` covering them, and advance the
/// cursor past them. No-op (no event) if nothing is pending.
fn flush_inserts<T>(
    items: &mut Vec<T>,
    cursor: &mut usize,
    pending: &mut Vec<T>,
    out: &mut Vec<DataChange>,
) {
    if pending.is_empty() {
        return;
    }
    let start = *cursor;
    let n = pending.len();
    items.splice(start..start, pending.drain(..));
    out.push(DataChange::ItemsInserted {
        range: start..start + n,
    });
    *cursor += n;
}

/// Group a sorted, deduplicated slice of indices into maximal contiguous
/// ranges, e.g. `[1, 2, 3, 7, 8]` → `[1..4, 7..9]`.
fn coalesce_ranges(sorted_idxs: &[usize]) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut iter = sorted_idxs.iter().peekable();
    while let Some(&start) = iter.next() {
        let mut end = start + 1;
        while iter.peek().is_some_and(|&&n| n == end) {
            end += 1;
            iter.next();
        }
        ranges.push(start..end);
    }
    ranges
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

    // ── reconcile_by_key ─────────────────────────────────────────────────

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Row {
        id: u64,
        val: &'static str,
    }

    fn row(id: u64, val: &'static str) -> Row {
        Row { id, val }
    }

    fn key(r: &Row) -> u64 {
        r.id
    }

    fn ids(model: &ListModel<Row>) -> Vec<u64> {
        (0..model.len())
            .map(|i| model.with_item(i, |r| r.id).unwrap())
            .collect()
    }

    fn vals(model: &ListModel<Row>) -> Vec<&'static str> {
        (0..model.len())
            .map(|i| model.with_item(i, |r| r.val).unwrap())
            .collect()
    }

    fn record_changes(model: &ListModel<Row>) -> (Rc<RefCell<Vec<DataChange>>>, ObserverHandle) {
        let log: Rc<RefCell<Vec<DataChange>>> = Rc::new(RefCell::new(Vec::new()));
        let l = log.clone();
        let handle = model.observe_changes(move |c| l.borrow_mut().push(c.clone()));
        (log, handle)
    }

    #[test]
    fn reconcile_pure_insert_emits_one_coalesced_range() {
        let model = ListModel::from_vec(vec![row(1, "a"), row(2, "b")]);
        let (log, _h) = record_changes(&model);

        model.reconcile_by_key(
            vec![row(1, "a"), row(2, "b"), row(3, "c"), row(4, "d")],
            key,
        );

        assert_eq!(ids(&model), vec![1, 2, 3, 4]);
        assert_eq!(vals(&model), vec!["a", "b", "c", "d"]);
        let entries = log.borrow();
        assert_eq!(entries.len(), 1, "contiguous inserts coalesce: {entries:?}");
        assert_eq!(entries[0], DataChange::ItemsInserted { range: 2..4 });
    }

    #[test]
    fn reconcile_insert_at_front_and_middle() {
        let model = ListModel::from_vec(vec![row(1, "a"), row(2, "b")]);
        let (log, _h) = record_changes(&model);

        // 0 is new (front), 1 stays, 5 is new (middle), 2 stays.
        model.reconcile_by_key(
            vec![row(0, "z"), row(1, "a"), row(5, "m"), row(2, "b")],
            key,
        );

        assert_eq!(ids(&model), vec![0, 1, 5, 2]);
        let entries = log.borrow();
        // Two non-adjacent insert runs, no move needed (1 and 2 already in
        // relative order).
        assert_eq!(entries.len(), 2, "{entries:?}");
        assert_eq!(entries[0], DataChange::ItemsInserted { range: 0..1 });
        assert_eq!(entries[1], DataChange::ItemsInserted { range: 2..3 });
    }

    #[test]
    fn reconcile_pure_remove_emits_one_coalesced_range() {
        let model = ListModel::from_vec(vec![row(1, "a"), row(2, "b"), row(3, "c"), row(4, "d")]);
        let (log, _h) = record_changes(&model);

        model.reconcile_by_key(vec![row(1, "a"), row(4, "d")], key);

        assert_eq!(ids(&model), vec![1, 4]);
        let entries = log.borrow();
        assert_eq!(entries.len(), 1, "contiguous removes coalesce: {entries:?}");
        assert_eq!(entries[0], DataChange::ItemsRemoved { range: 1..3 });
    }

    #[test]
    fn reconcile_remove_scattered_emits_separate_ranges() {
        let model = ListModel::from_vec(vec![row(1, "a"), row(2, "b"), row(3, "c"), row(4, "d")]);
        let (log, _h) = record_changes(&model);

        model.reconcile_by_key(vec![row(1, "a"), row(3, "c")], key);

        assert_eq!(ids(&model), vec![1, 3]);
        let entries = log.borrow();
        assert_eq!(entries.len(), 2, "{entries:?}");
        // Emitted back-to-front so earlier indices stay valid at removal time.
        assert_eq!(entries[0], DataChange::ItemsRemoved { range: 3..4 });
        assert_eq!(entries[1], DataChange::ItemsRemoved { range: 1..2 });
    }

    #[test]
    fn reconcile_reorder_emits_only_moves() {
        let model = ListModel::from_vec(vec![row(1, "a"), row(2, "b"), row(3, "c")]);
        let (log, _h) = record_changes(&model);

        model.reconcile_by_key(vec![row(3, "c"), row(2, "b"), row(1, "a")], key);

        assert_eq!(ids(&model), vec![3, 2, 1]);
        let entries = log.borrow();
        assert!(
            entries
                .iter()
                .all(|c| matches!(c, DataChange::ItemsMoved { .. })),
            "a pure reorder must only emit moves: {entries:?}"
        );
        assert!(!entries.is_empty());
    }

    #[test]
    fn reconcile_no_reorder_when_relative_order_already_matches() {
        // Peer removed the middle row but didn't touch the order of the rest —
        // this must NOT emit any ItemsMoved, only the removal.
        let model = ListModel::from_vec(vec![row(1, "a"), row(2, "b"), row(3, "c"), row(4, "d")]);
        let (log, _h) = record_changes(&model);

        model.reconcile_by_key(vec![row(1, "a"), row(3, "c"), row(4, "d")], key);

        assert_eq!(ids(&model), vec![1, 3, 4]);
        let entries = log.borrow();
        assert!(
            entries
                .iter()
                .all(|c| !matches!(c, DataChange::ItemsMoved { .. })),
            "untouched relative order must not emit moves: {entries:?}"
        );
    }

    #[test]
    fn reconcile_in_place_update_emits_item_updated() {
        let model = ListModel::from_vec(vec![row(1, "a"), row(2, "b"), row(3, "c")]);
        let (log, _h) = record_changes(&model);

        model.reconcile_by_key(vec![row(1, "a"), row(2, "CHANGED"), row(3, "c")], key);

        assert_eq!(vals(&model), vec!["a", "CHANGED", "c"]);
        let entries = log.borrow();
        assert_eq!(entries.len(), 1, "{entries:?}");
        assert_eq!(entries[0], DataChange::ItemUpdated { index: 1 });
    }

    #[test]
    fn reconcile_combined_insert_remove_reorder_update() {
        // old: [1,2,3,4] -> new: [4, 2*, 5, 1]
        //  - 3 removed
        //  - 4 moves to front
        //  - 2's content changes in place
        //  - 5 is inserted
        //  - 1 stays last (relative order of 1 vs 2 vs 4 reshuffled)
        let model = ListModel::from_vec(vec![row(1, "a"), row(2, "b"), row(3, "c"), row(4, "d")]);
        let (log, _h) = record_changes(&model);

        model.reconcile_by_key(
            vec![row(4, "d"), row(2, "B!"), row(5, "e"), row(1, "a")],
            key,
        );

        assert_eq!(ids(&model), vec![4, 2, 5, 1]);
        assert_eq!(vals(&model), vec!["d", "B!", "e", "a"]);
        let entries = log.borrow();
        assert!(!entries.is_empty());
        assert!(
            entries
                .iter()
                .any(|c| matches!(c, DataChange::ItemsMoved { .. })),
            "{entries:?}"
        );
        assert!(
            entries
                .iter()
                .any(|c| matches!(c, DataChange::ItemsInserted { .. })),
            "{entries:?}"
        );
        assert!(
            entries
                .iter()
                .any(|c| matches!(c, DataChange::ItemsRemoved { .. })),
            "{entries:?}"
        );
        assert!(
            entries
                .iter()
                .any(|c| matches!(c, DataChange::ItemUpdated { .. })),
            "{entries:?}"
        );
        assert!(
            !entries.iter().any(|c| matches!(c, DataChange::Reset)),
            "{entries:?}"
        );
    }

    #[test]
    fn reconcile_empty_to_full() {
        let model: ListModel<Row> = ListModel::new();
        let (log, _h) = record_changes(&model);

        model.reconcile_by_key(vec![row(1, "a"), row(2, "b"), row(3, "c")], key);

        assert_eq!(ids(&model), vec![1, 2, 3]);
        let entries = log.borrow();
        assert_eq!(entries.len(), 1, "{entries:?}");
        assert_eq!(entries[0], DataChange::ItemsInserted { range: 0..3 });
    }

    #[test]
    fn reconcile_full_to_empty() {
        let model = ListModel::from_vec(vec![row(1, "a"), row(2, "b"), row(3, "c")]);
        let (log, _h) = record_changes(&model);

        model.reconcile_by_key(vec![], key);

        assert!(model.is_empty());
        let entries = log.borrow();
        assert_eq!(entries.len(), 1, "{entries:?}");
        assert_eq!(entries[0], DataChange::ItemsRemoved { range: 0..3 });
    }

    #[test]
    fn reconcile_identical_input_emits_nothing() {
        let model = ListModel::from_vec(vec![row(1, "a"), row(2, "b"), row(3, "c")]);
        let (log, _h) = record_changes(&model);

        model.reconcile_by_key(vec![row(1, "a"), row(2, "b"), row(3, "c")], key);

        assert_eq!(ids(&model), vec![1, 2, 3]);
        assert!(
            log.borrow().is_empty(),
            "identical reconcile must not notify: {:?}",
            log.borrow()
        );
    }

    #[test]
    fn reconcile_never_emits_reset() {
        // Exercise every scenario above (plus a full rewrite / total
        // replacement, the case most tempted to fall back to `Reset`) and
        // assert `Reset` is never among the emitted changes.
        let scenarios: Vec<(Vec<Row>, Vec<Row>)> = vec![
            (
                vec![row(1, "a"), row(2, "b")],
                vec![row(1, "a"), row(2, "b"), row(3, "c")],
            ),
            (
                vec![row(1, "a"), row(2, "b"), row(3, "c")],
                vec![row(1, "a")],
            ),
            (
                vec![row(1, "a"), row(2, "b"), row(3, "c")],
                vec![row(3, "c"), row(1, "a"), row(2, "b")],
            ),
            (vec![], vec![row(1, "a"), row(2, "b")]),
            (vec![row(1, "a"), row(2, "b")], vec![]),
            (
                vec![row(1, "a"), row(2, "b"), row(3, "c"), row(4, "d")],
                vec![row(9, "x"), row(8, "y"), row(7, "z")],
            ),
        ];
        for (before, after) in scenarios {
            let model = ListModel::from_vec(before.clone());
            let (log, _h) = record_changes(&model);
            model.reconcile_by_key(after.clone(), key);
            assert!(
                !log.borrow().iter().any(|c| matches!(c, DataChange::Reset)),
                "reconcile must never emit Reset — before {before:?}, after {after:?}, got {:?}",
                log.borrow()
            );
        }
    }

    #[test]
    #[should_panic(expected = "reconcile_by_key")]
    fn reconcile_duplicate_key_in_new_items_panics() {
        let model = ListModel::from_vec(vec![row(1, "a"), row(2, "b")]);
        // Two incoming rows both claim key 1 — the second can't be found at
        // or after the write cursor once the first has already consumed
        // that key's slot (see `# Panics` on `reconcile_by_key`).
        model.reconcile_by_key(vec![row(1, "a"), row(1, "a-dup"), row(2, "b")], key);
    }
}
