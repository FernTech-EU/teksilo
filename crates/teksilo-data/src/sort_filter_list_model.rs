// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Composable sort + filter projection over a flat list source.
//!
//! `SortFilterListModel<T>` wraps a [`ListModel<T>`] or any
//! [`ListDataSource<Item = T>`] and exposes a `ListDataSource<Item = T>`
//! whose visible item order is determined by:
//!
//! 1. **Filtering**: each column may register a predicate factory; rows that
//!    fail any non-empty filter are hidden.
//! 2. **Sorting**: at most one column may carry an active sort direction;
//!    rows are reordered by the column's registered comparator.
//!
//! Filter is applied first, sort second. The result is a flat reactive view
//! that drops directly into `TableView`, `ListView`, or `Repeater` via
//! `from_source(...)`.
//!
//! ## Reactivity
//!
//! Three independent change vectors trigger a rebuild of the visible-index
//! map:
//!
//! - The upstream source emits any [`DataChange`]. Most changes collapse to
//!   a single [`DataChange::Reset`] for the proxy's own observers —
//!   translating fine-grained inserts / removes / moves through a sort
//!   projection is correctness-fragile (an item's sort key can move it to a
//!   different visible row), so `Reset` is the safe default contract. The
//!   one exception is [`DataChange::ItemUpdated`]: the proxy re-evaluates
//!   just that row's filter verdict and its position against its current
//!   visible neighbours (not the whole list), and if neither changed,
//!   forwards a scoped `ItemUpdated` at the mapped visible index instead of
//!   paying for a full re-filter + re-sort + `Reset` on every edit to a
//!   live-updating source. Any verdict change (entering/leaving the visible
//!   set, or needing to move past a neighbour) still falls back to the full
//!   rebuild.
//! - A bound sort signal updates: rebuild and emit `Reset`.
//! - A bound filters signal updates: rebuild and emit `Reset`.
//!
//! ## Selection semantics
//!
//! Selection on a sorted/filtered view is naturally tracked by **visible
//! index**, not by item identity. After a projection rebuild, a downstream
//! [`SelectionModel`](crate::SelectionModel) keeps the same numerical
//! indices selected — meaning the visual selection stays in place even
//! though it now points at different underlying rows. Apps that want
//! identity-based selection should observe their model directly and rewrite
//! the selection from source identifiers on each rebuild.
//!
//! ```rust
//! # use teksilo_data::{ListModel, SortFilterListModel, SortDirection};
//! # use teksilo_data::ListDataSource; // brings `len()` into scope
//! #[derive(Clone, Debug)]
//! struct Person { name: String, age: u32 }
//!
//! let model: ListModel<Person> = ListModel::new();
//! model.push(Person { name: "Carol".into(), age: 30 });
//! model.push(Person { name: "Alice".into(), age: 25 });
//! model.push(Person { name: "Bob".into(), age: 28 });
//!
//! let proxy = SortFilterListModel::new(model)
//!     .with_comparator("name", |a: &Person, b| a.name.cmp(&b.name))
//!     .with_predicate("name", |text| {
//!         let t = text.to_lowercase();
//!         Box::new(move |p: &Person| p.name.to_lowercase().contains(&t))
//!     });
//!
//! proxy.set_sort(Some("name"), SortDirection::Ascending);
//! assert_eq!(proxy.len(), 3); // Alice, Bob, Carol
//!
//! proxy.set_filter("name", "a");
//! assert_eq!(proxy.len(), 2); // Alice, Carol
//! ```

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;

use teksilo_core::ObserverHandle;
use teksilo_core::signal::Signal;

use crate::data_change::DataChange;
use crate::list_data_source::ListDataSource;
use crate::list_model::ListModel;

/// Sort direction emitted by `TableView` / `TreeTableView` headers and consumed by sort projections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    /// Sort from smallest to largest (A → Z, 0 → 9).
    Ascending,
    /// Sort from largest to smallest (Z → A, 9 → 0).
    Descending,
}

type LenFn = Rc<dyn Fn() -> usize>;
type WithItemFn<T> = Rc<dyn Fn(usize, &dyn Fn(&T))>;
type ObserveFn = Rc<dyn Fn(Box<dyn Fn(&DataChange)>) -> ObserverHandle>;
type Comparator<T> = Rc<dyn Fn(&T, &T) -> Ordering>;
type PredicateFactory<T> = Rc<dyn Fn(&str) -> Box<dyn Fn(&T) -> bool>>;

struct ObserverEntry {
    id: u64,
    callback: Rc<dyn Fn(&DataChange)>,
}

struct Inner<T: 'static> {
    len_fn: LenFn,
    with_item_fn: WithItemFn<T>,
    comparators: HashMap<String, Comparator<T>>,
    predicate_factories: HashMap<String, PredicateFactory<T>>,
    sort: Option<(String, SortDirection)>,
    filters: HashMap<String, String>,
    visible_to_source: Vec<usize>,
    /// Lazy reverse map; rebuilt on demand by `visible_index_of`.
    source_to_visible: RefCell<Option<Vec<Option<usize>>>>,
    /// First visible index whose content may differ after the latest
    /// rebuild. See [`SortFilterListModel::first_changed_index`].
    last_divergence: Option<usize>,
    observers: Vec<ObserverEntry>,
    next_observer_id: u64,
    /// External signal handles. Held to keep observation alive and to route
    /// imperative `set_*` calls back through the signal when bound.
    sort_signal: Option<Signal<Option<(String, SortDirection)>>>,
    filters_signal: Option<Signal<HashMap<String, String>>>,
    _upstream_handle: Option<ObserverHandle>,
    _sort_handle: Option<ObserverHandle>,
    _filters_handle: Option<ObserverHandle>,
}

impl<T: 'static> Inner<T> {
    fn rebuild(&mut self, upstream: Option<&DataChange>) {
        let n = (self.len_fn)();
        let predicates: Vec<Box<dyn Fn(&T) -> bool>> = self
            .filters
            .iter()
            .filter(|(_, text)| !text.is_empty())
            .filter_map(|(col_id, text)| self.predicate_factories.get(col_id).map(|f| f(text)))
            .collect();

        let mut visible: Vec<usize> = Vec::with_capacity(n);
        for src_idx in 0..n {
            let keep = Cell::new(true);
            (self.with_item_fn)(src_idx, &|item: &T| {
                for pred in &predicates {
                    if !pred(item) {
                        keep.set(false);
                        return;
                    }
                }
            });
            if keep.get() {
                visible.push(src_idx);
            }
        }

        if let Some((col_id, dir)) = &self.sort
            && let Some(cmp) = self.comparators.get(col_id).cloned()
        {
            let with_item_fn = self.with_item_fn.clone();
            let descending = *dir == SortDirection::Descending;
            visible.sort_by(|&a, &b| {
                let ord = Cell::new(Ordering::Equal);
                (with_item_fn)(a, &|va| {
                    (with_item_fn)(b, &|vb| {
                        ord.set(cmp(va, vb));
                    });
                });
                let mut o = ord.get();
                if descending {
                    o = o.reverse();
                }
                o
            });
        }

        // Divergence: the first visible index whose content may differ.
        // `visible_to_source` holds *positional* source indices, not item
        // identities — an upstream insert/remove/move renumbers every
        // source index at or above its change point, so equal index
        // values only guarantee identical content while they sit *below*
        // that floor. Sort/filter-only rebuilds (no upstream change)
        // don't renumber, so the floor is unbounded there.
        let floor = match upstream {
            None | Some(DataChange::ItemUpdated { .. }) => usize::MAX,
            Some(DataChange::ItemsInserted { range })
            | Some(DataChange::ItemsRemoved { range }) => range.start,
            Some(DataChange::ItemsMoved { from, to, .. }) => (*from).min(*to),
            Some(DataChange::WindowLoaded { range }) => range.start,
            Some(DataChange::Reset) => 0,
        };
        let mut d = self
            .visible_to_source
            .iter()
            .zip(visible.iter())
            .take_while(|(a, b)| a == b && **a < floor)
            .count();
        // An ItemUpdated leaves the index map unchanged when the sort key
        // didn't move it, but the row's content changed — fold in its
        // visible position. A moved/filtered-out item is caught by the
        // prefix compare instead.
        if let Some(DataChange::ItemUpdated { index }) = upstream
            && let Some(p) = visible.iter().position(|s| s == index)
        {
            d = d.min(p);
        }
        self.last_divergence = Some(d);

        self.visible_to_source = visible;
        self.source_to_visible.replace(None);
    }

    fn snapshot_callbacks(&self) -> Vec<Rc<dyn Fn(&DataChange)>> {
        self.observers.iter().map(|o| o.callback.clone()).collect()
    }
}

/// Flat list source projecting an upstream `ListModel<T>` /
/// `ListDataSource<Item = T>` through sort + filter.
///
/// See module-level documentation for semantics.
pub struct SortFilterListModel<T: 'static> {
    inner: Rc<RefCell<Inner<T>>>,
}

impl<T: 'static> SortFilterListModel<T> {
    /// Wrap a `ListModel<T>`.
    pub fn new(model: ListModel<T>) -> Self {
        let m_len = model.clone();
        let m_read = model.clone();
        let m_obs = model;
        let len_fn: LenFn = Rc::new(move || m_len.len());
        let with_item_fn: WithItemFn<T> = Rc::new(move |idx, f| {
            m_read.with_item(idx, |item| f(item));
        });
        let observe_fn: ObserveFn =
            Rc::new(move |callback| m_obs.observe_changes(move |change| callback(change)));
        Self::create(len_fn, with_item_fn, observe_fn)
    }

    /// Wrap any `ListDataSource<Item = T>`.
    pub fn from_source<S: ListDataSource<Item = T>>(source: S) -> Self {
        let s = Rc::new(source);
        let s_len = s.clone();
        let s_read = s.clone();
        let s_obs = s;
        let len_fn: LenFn = Rc::new(move || s_len.len());
        let with_item_fn: WithItemFn<T> = Rc::new(move |idx, f| {
            s_read.with_item(idx, |item| f(item));
        });
        let observe_fn: ObserveFn =
            Rc::new(move |callback| s_obs.observe_changes(move |change| callback(change)));
        Self::create(len_fn, with_item_fn, observe_fn)
    }

    fn create(len_fn: LenFn, with_item_fn: WithItemFn<T>, observe_fn: ObserveFn) -> Self {
        let inner = Rc::new(RefCell::new(Inner {
            len_fn,
            with_item_fn,
            comparators: HashMap::new(),
            predicate_factories: HashMap::new(),
            sort: None,
            filters: HashMap::new(),
            visible_to_source: Vec::new(),
            source_to_visible: RefCell::new(None),
            last_divergence: None,
            observers: Vec::new(),
            next_observer_id: 1,
            sort_signal: None,
            filters_signal: None,
            _upstream_handle: None,
            _sort_handle: None,
            _filters_handle: None,
        }));

        // Register upstream observer; on any source change rebuild + Reset.
        let weak = Rc::downgrade(&inner);
        let upstream_handle = (observe_fn)(Box::new(move |change| {
            if let Some(strong) = weak.upgrade() {
                rebuild_and_notify_with(&strong, Some(change));
            }
        }));
        inner.borrow_mut()._upstream_handle = Some(upstream_handle);

        // Initial visible map (no filter, no sort = identity).
        rebuild_and_notify(&inner);

        Self { inner }
    }

    /// Register a comparator for a column id. Chainable.
    pub fn with_comparator(
        self,
        col_id: impl Into<String>,
        cmp: impl Fn(&T, &T) -> Ordering + 'static,
    ) -> Self {
        self.inner
            .borrow_mut()
            .comparators
            .insert(col_id.into(), Rc::new(cmp));
        rebuild_and_notify(&self.inner);
        self
    }

    /// Register a predicate factory for a column id. The factory receives the
    /// current filter text (empty = no filter, never invoked) and returns a
    /// boxed predicate evaluated against each row. Chainable.
    pub fn with_predicate(
        self,
        col_id: impl Into<String>,
        factory: impl Fn(&str) -> Box<dyn Fn(&T) -> bool> + 'static,
    ) -> Self {
        self.inner
            .borrow_mut()
            .predicate_factories
            .insert(col_id.into(), Rc::new(factory));
        rebuild_and_notify(&self.inner);
        self
    }

    /// Bind a sort signal — typically `TableView::sort_signal()`. Updates
    /// re-project the view. The current value is read once at bind time.
    pub fn sort_signal(&self, signal: Signal<Option<(String, SortDirection)>>) {
        self.inner.borrow_mut().sort = signal.get();
        rebuild_and_notify(&self.inner);
        let weak = Rc::downgrade(&self.inner);
        let handle = signal.observe(move |new| {
            if let Some(strong) = weak.upgrade() {
                strong.borrow_mut().sort = new.clone();
                rebuild_and_notify(&strong);
            }
        });
        let mut guard = self.inner.borrow_mut();
        guard.sort_signal = Some(signal);
        guard._sort_handle = Some(handle);
    }

    /// Bind a filters signal — typically `TableView::filters_signal()`.
    /// Updates re-project the view. The current value is read once at bind
    /// time.
    pub fn filters_signal(&self, signal: Signal<HashMap<String, String>>) {
        self.inner.borrow_mut().filters = signal.get();
        rebuild_and_notify(&self.inner);
        let weak = Rc::downgrade(&self.inner);
        let handle = signal.observe(move |new| {
            if let Some(strong) = weak.upgrade() {
                strong.borrow_mut().filters = new.clone();
                rebuild_and_notify(&strong);
            }
        });
        let mut guard = self.inner.borrow_mut();
        guard.filters_signal = Some(signal);
        guard._filters_handle = Some(handle);
    }

    /// Set the active sort imperatively. If a sort signal is bound this
    /// writes through the signal; otherwise it mutates internal state and
    /// emits `DataChange::Reset` directly.
    pub fn set_sort(&self, col_id: Option<&str>, dir: SortDirection) {
        let new = col_id.map(|c| (c.to_string(), dir));
        // Clone the signal handle without keeping the borrow alive — the
        // observer registered by `sort_signal` will call back into
        // `inner.borrow_mut()` from `sig.set(...)`.
        let sig = self.inner.borrow().sort_signal.clone();
        if let Some(sig) = sig {
            sig.set(new);
            return;
        }
        self.inner.borrow_mut().sort = new;
        rebuild_and_notify(&self.inner);
    }

    /// Clear the active sort.
    pub fn clear_sort(&self) {
        let sig = self.inner.borrow().sort_signal.clone();
        if let Some(sig) = sig {
            sig.set(None);
            return;
        }
        self.inner.borrow_mut().sort = None;
        rebuild_and_notify(&self.inner);
    }

    /// Set or clear a single column's filter. An empty `text` removes the
    /// entry. If a filters signal is bound this writes through the signal.
    pub fn set_filter(&self, col_id: &str, text: &str) {
        let sig = self.inner.borrow().filters_signal.clone();
        if let Some(sig) = sig {
            let mut new = sig.get();
            if text.is_empty() {
                new.remove(col_id);
            } else {
                new.insert(col_id.to_string(), text.to_string());
            }
            sig.set(new);
            return;
        }
        {
            let mut guard = self.inner.borrow_mut();
            if text.is_empty() {
                guard.filters.remove(col_id);
            } else {
                guard.filters.insert(col_id.to_string(), text.to_string());
            }
        }
        rebuild_and_notify(&self.inner);
    }

    /// Clear every column's filter.
    pub fn clear_filters(&self) {
        let sig = self.inner.borrow().filters_signal.clone();
        if let Some(sig) = sig {
            sig.set(HashMap::new());
            return;
        }
        self.inner.borrow_mut().filters.clear();
        rebuild_and_notify(&self.inner);
    }

    /// First visible index whose content may differ from before the
    /// latest projection rebuild — rows `0..index` show the same items in
    /// the same order as before, so per-row derived state (e.g. a
    /// measured row height) remains valid for them. Equal to `len()` when
    /// the visible list is unchanged. Renumbering from upstream
    /// inserts/removes/moves is accounted for (equal source-index values
    /// above the change point are not trusted).
    ///
    /// `None` means unknown (no rebuild observed yet) — treat as a full
    /// change. The value describes the **latest** rebuild only; read it
    /// synchronously from a `DataChange` observer (callbacks fire inline
    /// on every rebuild, so per-change reads cannot miss a value). The
    /// `DataChange::Reset` contract for observers is unchanged — this is
    /// a side-channel for consumers that can exploit a valid prefix.
    pub fn first_changed_index(&self) -> Option<usize> {
        self.inner.borrow().last_divergence
    }

    /// Map a visible (post sort+filter) index to its source index.
    pub fn source_index_of(&self, visible: usize) -> Option<usize> {
        self.inner.borrow().visible_to_source.get(visible).copied()
    }

    /// Map an underlying source index to its visible position, if shown.
    /// Builds a reverse-index lazily; subsequent calls in the same projection
    /// epoch are O(1).
    pub fn visible_index_of(&self, source: usize) -> Option<usize> {
        let guard = self.inner.borrow();
        let mut rev = guard.source_to_visible.borrow_mut();
        if rev.is_none() {
            let n = (guard.len_fn)();
            let mut v = vec![None; n];
            for (vi, &si) in guard.visible_to_source.iter().enumerate() {
                if si < n {
                    v[si] = Some(vi);
                }
            }
            *rev = Some(v);
        }
        rev.as_ref()
            .expect("rev was just initialized above when None")
            .get(source)
            .copied()
            .flatten()
    }
}

fn rebuild_and_notify<T: 'static>(inner: &Rc<RefCell<Inner<T>>>) {
    rebuild_and_notify_with(inner, None);
}

fn rebuild_and_notify_with<T: 'static>(
    inner: &Rc<RefCell<Inner<T>>>,
    upstream: Option<&DataChange>,
) {
    // Fast path: a single-row content edit that neither enters/leaves the
    // visible set nor needs to move relative to its sorted neighbours can
    // skip the full O(n) filter pass + O(n log n) sort — patch the one row
    // instead of collapsing to `Reset`.
    if let Some(DataChange::ItemUpdated { index }) = upstream
        && let Some(outcome) = try_incremental_item_update(inner, *index)
    {
        if let IncrementalOutcome::Updated { visible_index } = outcome {
            let callbacks = inner.borrow().snapshot_callbacks();
            let change = DataChange::ItemUpdated {
                index: visible_index,
            };
            for cb in &callbacks {
                cb(&change);
            }
        }
        return;
    }

    // Drop the borrow before invoking observer callbacks so they may freely
    // call back into the proxy (`with_item`, `len`, etc.).
    let callbacks = {
        let mut guard = inner.borrow_mut();
        guard.rebuild(upstream);
        guard.snapshot_callbacks()
    };
    let change = DataChange::Reset;
    for cb in &callbacks {
        cb(&change);
    }
}

/// Outcome of [`try_incremental_item_update`]. Both variants mean the fast
/// path succeeded (no rebuild ran); the caller only needs to notify for
/// `Updated`.
enum IncrementalOutcome {
    /// The item was and remains visible at `visible_index` — its content
    /// changed, so observers get a scoped `ItemUpdated` there.
    Updated { visible_index: usize },
    /// The item was and remains filtered out — nothing observable changed.
    StillHidden,
}

/// Fast path for a single-row `DataChange::ItemUpdated`: re-evaluate just
/// this row's filter verdict and sort position against its current visible
/// neighbours, instead of re-filtering and re-sorting every row. Returns
/// `None` when either verdict changed (entering/leaving the visible set, or
/// needing to move past an adjacent neighbour) — the caller falls back to
/// the full rebuild, which is the only way to get a correct renumbering in
/// that case. On success `source_to_visible`'s lazy reverse map is left
/// untouched: nothing's position moved, so it's still valid.
fn try_incremental_item_update<T: 'static>(
    inner: &Rc<RefCell<Inner<T>>>,
    index: usize,
) -> Option<IncrementalOutcome> {
    let mut guard = inner.borrow_mut();

    let predicates: Vec<Box<dyn Fn(&T) -> bool>> = guard
        .filters
        .iter()
        .filter(|(_, text)| !text.is_empty())
        .filter_map(|(col_id, text)| guard.predicate_factories.get(col_id).map(|f| f(text)))
        .collect();
    let with_item_fn = guard.with_item_fn.clone();

    let passes_now = {
        let keep = Cell::new(true);
        (with_item_fn)(index, &|item: &T| {
            for pred in &predicates {
                if !pred(item) {
                    keep.set(false);
                    return;
                }
            }
        });
        keep.get()
    };

    let old_visible_pos = guard.visible_to_source.iter().position(|&s| s == index);
    if passes_now != old_visible_pos.is_some() {
        return None; // entering or leaving the visible set — needs a full renumber
    }

    let Some(visible_pos) = old_visible_pos else {
        // Was, and remains, filtered out.
        guard.last_divergence = Some(guard.visible_to_source.len());
        return Some(IncrementalOutcome::StillHidden);
    };

    if let Some((col_id, dir)) = guard.sort.clone()
        && let Some(cmp) = guard.comparators.get(&col_id).cloned()
    {
        let descending = dir == SortDirection::Descending;
        let ordered = |a: usize, b: usize| -> Ordering {
            let ord = Cell::new(Ordering::Equal);
            (with_item_fn)(a, &|va| {
                (with_item_fn)(b, &|vb| {
                    ord.set(cmp(va, vb));
                });
            });
            if descending {
                ord.get().reverse()
            } else {
                ord.get()
            }
        };
        let visible = &guard.visible_to_source;
        // A neighbour that now compares `Equal` matters as much as one we've
        // moved past. The full reprojection sorts with `Vec::sort_by`, which
        // is stable, so within a run of equal keys the visible order is
        // ascending *source* index. Keeping the row where it happens to sit
        // would leave the fast path disagreeing with the rebuild it is meant
        // to be an optimisation of, and the row would then appear to jump the
        // next time any unrelated edit triggered a full reprojection. Falling
        // back on the ties that actually violate that ordering preserves the
        // optimisation for every other case.
        if visible_pos > 0 {
            let pred = visible[visible_pos - 1];
            match ordered(pred, index) {
                Ordering::Greater => return None, // moved before its predecessor
                Ordering::Equal if pred > index => return None, // tie, wrong stable order
                _ => {}
            }
        }
        if visible_pos + 1 < visible.len() {
            let succ = visible[visible_pos + 1];
            match ordered(index, succ) {
                Ordering::Greater => return None, // moved past its successor
                Ordering::Equal if index > succ => return None, // tie, wrong stable order
                _ => {}
            }
        }
    }

    guard.last_divergence = Some(visible_pos);
    Some(IncrementalOutcome::Updated {
        visible_index: visible_pos,
    })
}

impl<T: 'static> Clone for SortFilterListModel<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T: 'static> ListDataSource for SortFilterListModel<T> {
    type Item = T;
    /// The **source index** behind a visible row.
    ///
    /// This is the strongest identity a flat projection can offer: the
    /// underlying `ListModel` has none of its own (a `Vec` row *is* its
    /// position). It is stable across every sort/filter reprojection, which is
    /// what a projection actually does to rows, so a
    /// [`RowAnchor`](../../teksilo_widgets/data_views/struct.RowAnchor.html)
    /// built over this view tracks its row when a filter changes underneath it.
    ///
    /// It is **not** stable across an upstream mutation: an insert, remove or
    /// move renumbers every source index at or above its change point (see the
    /// divergence note in `rebuild`). Anchors only have to survive the window
    /// between a change and the rebuild it schedules, and an upstream mutation
    /// is exactly what forces that rebuild — but a handler firing inside that
    /// window may still resolve to a neighbour. That is no worse than the
    /// captured index it replaced, and strictly better for the sort/filter case.
    type Key = usize;

    fn key_at(&self, index: usize) -> Option<usize> {
        self.source_index_of(index)
    }

    fn index_of(&self, key: &usize) -> Option<usize> {
        // O(1) after the first call in a projection epoch (lazy reverse index).
        self.visible_index_of(*key)
    }

    fn len(&self) -> usize {
        self.inner.borrow().visible_to_source.len()
    }

    fn with_item<R>(&self, index: usize, f: impl FnOnce(&Self::Item) -> R) -> Option<R> {
        let (with_item_fn, src_idx) = {
            let guard = self.inner.borrow();
            let src = *guard.visible_to_source.get(index)?;
            (guard.with_item_fn.clone(), src)
        };
        let f_cell: Cell<Option<_>> = Cell::new(Some(f));
        let slot: Cell<Option<R>> = Cell::new(None);
        (with_item_fn)(src_idx, &|item: &T| {
            if let Some(f) = f_cell.take() {
                slot.set(Some(f(item)));
            }
        });
        slot.into_inner()
    }

    fn first_changed_index(&self) -> Option<usize> {
        SortFilterListModel::first_changed_index(self)
    }

    fn observe_changes(&self, f: impl Fn(&DataChange) + 'static) -> ObserverHandle {
        let id = {
            let mut guard = self.inner.borrow_mut();
            let id = guard.next_observer_id;
            guard.next_observer_id += 1;
            guard.observers.push(ObserverEntry {
                id,
                callback: Rc::new(f),
            });
            id
        };
        let weak = Rc::downgrade(&self.inner);
        ObserverHandle::new(
            self.inner.clone(),
            id,
            Rc::new(move |observer_id| {
                if let Some(strong) = weak.upgrade() {
                    strong
                        .borrow_mut()
                        .observers
                        .retain(|e| e.id != observer_id);
                }
            }),
        )
    }
}

impl<T: 'static> std::fmt::Debug for SortFilterListModel<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let g = self.inner.borrow();
        f.debug_struct("SortFilterListModel")
            .field("visible_count", &g.visible_to_source.len())
            .field("sort", &g.sort)
            .field("filter_count", &g.filters.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct Row {
        id: u32,
        name: String,
    }

    fn sample() -> ListModel<Row> {
        ListModel::from_vec(vec![
            Row {
                id: 3,
                name: "carol".into(),
            },
            Row {
                id: 1,
                name: "alice".into(),
            },
            Row {
                id: 2,
                name: "bob".into(),
            },
            Row {
                id: 4,
                name: "dan".into(),
            },
        ])
    }

    fn collect_names<S: ListDataSource<Item = Row>>(s: &S) -> Vec<String> {
        (0..s.len())
            .map(|i| s.with_item(i, |r| r.name.clone()).unwrap())
            .collect()
    }

    #[test]
    fn passthrough_when_no_sort_or_filter() {
        let proxy = SortFilterListModel::new(sample());
        assert_eq!(proxy.len(), 4);
        assert_eq!(collect_names(&proxy), vec!["carol", "alice", "bob", "dan"]);
    }

    #[test]
    fn sort_ascending_by_name() {
        let proxy = SortFilterListModel::new(sample())
            .with_comparator("name", |a: &Row, b| a.name.cmp(&b.name));
        proxy.set_sort(Some("name"), SortDirection::Ascending);
        assert_eq!(collect_names(&proxy), vec!["alice", "bob", "carol", "dan"]);
    }

    #[test]
    fn sort_descending_by_id() {
        let proxy =
            SortFilterListModel::new(sample()).with_comparator("id", |a: &Row, b| a.id.cmp(&b.id));
        proxy.set_sort(Some("id"), SortDirection::Descending);
        let ids: Vec<u32> = (0..proxy.len())
            .map(|i| proxy.with_item(i, |r| r.id).unwrap())
            .collect();
        assert_eq!(ids, vec![4, 3, 2, 1]);
    }

    #[test]
    fn clear_sort_restores_source_order() {
        let proxy = SortFilterListModel::new(sample())
            .with_comparator("name", |a: &Row, b| a.name.cmp(&b.name));
        proxy.set_sort(Some("name"), SortDirection::Ascending);
        proxy.clear_sort();
        assert_eq!(collect_names(&proxy), vec!["carol", "alice", "bob", "dan"]);
    }

    #[test]
    fn filter_excludes_rows() {
        let proxy = SortFilterListModel::new(sample()).with_predicate("name", |text| {
            let t = text.to_lowercase();
            Box::new(move |r: &Row| r.name.to_lowercase().contains(&t))
        });
        proxy.set_filter("name", "a");
        // alice, carol, dan all contain 'a'; bob does not.
        let names = collect_names(&proxy);
        assert!(names.contains(&"alice".to_string()));
        assert!(names.contains(&"carol".to_string()));
        assert!(names.contains(&"dan".to_string()));
        assert!(!names.contains(&"bob".to_string()));
        assert_eq!(proxy.len(), 3);
    }

    #[test]
    fn empty_filter_text_clears_predicate() {
        let proxy = SortFilterListModel::new(sample()).with_predicate("name", |t| {
            let t = t.to_string();
            Box::new(move |r: &Row| r.name.contains(&t))
        });
        proxy.set_filter("name", "alice");
        assert_eq!(proxy.len(), 1);
        proxy.set_filter("name", "");
        assert_eq!(proxy.len(), 4);
    }

    #[test]
    fn clear_filters_resets_all() {
        let proxy = SortFilterListModel::new(sample())
            .with_predicate("name", |t| {
                let t = t.to_string();
                Box::new(move |r: &Row| r.name.contains(&t))
            })
            .with_predicate("id", |t| {
                let needle = t.to_string();
                Box::new(move |r: &Row| r.id.to_string().contains(&needle))
            });
        proxy.set_filter("name", "a");
        proxy.set_filter("id", "1");
        assert_eq!(proxy.len(), 1); // alice (id=1, name has 'a')
        proxy.clear_filters();
        assert_eq!(proxy.len(), 4);
    }

    #[test]
    fn sort_after_filter_applies_to_visible_only() {
        let proxy = SortFilterListModel::new(sample())
            .with_comparator("name", |a: &Row, b| a.name.cmp(&b.name))
            .with_predicate("name", |t| {
                let t = t.to_string();
                Box::new(move |r: &Row| r.name.contains(&t))
            });
        proxy.set_filter("name", "a"); // alice, carol, dan
        proxy.set_sort(Some("name"), SortDirection::Ascending);
        assert_eq!(collect_names(&proxy), vec!["alice", "carol", "dan"]);
    }

    #[test]
    fn upstream_insert_triggers_reset() {
        let model = sample();
        let proxy = SortFilterListModel::new(model.clone());
        let count = Rc::new(Cell::new(0));
        let c = count.clone();
        let _h = proxy.observe_changes(move |c2| {
            if let DataChange::Reset = c2 {
                c.set(c.get() + 1);
            }
        });
        // Construction issued a Reset already; clear our counter.
        count.set(0);
        model.push(Row {
            id: 5,
            name: "eve".into(),
        });
        assert_eq!(count.get(), 1);
        assert_eq!(proxy.len(), 5);
    }

    #[test]
    fn clone_shares_state() {
        let proxy =
            SortFilterListModel::new(sample()).with_comparator("id", |a: &Row, b| a.id.cmp(&b.id));
        let c1 = proxy.clone();
        c1.set_sort(Some("id"), SortDirection::Ascending);
        let names_via_clone = collect_names(&c1);
        let names_via_orig = collect_names(&proxy);
        assert_eq!(names_via_clone, names_via_orig);
        assert_eq!(names_via_orig, vec!["alice", "bob", "carol", "dan"]);
    }

    #[test]
    fn bound_sort_signal_drives_view() {
        let proxy = SortFilterListModel::new(sample())
            .with_comparator("name", |a: &Row, b| a.name.cmp(&b.name));
        let sig: Signal<Option<(String, SortDirection)>> = Signal::new(None);
        proxy.sort_signal(sig.clone());

        sig.set(Some(("name".to_string(), SortDirection::Ascending)));
        assert_eq!(collect_names(&proxy), vec!["alice", "bob", "carol", "dan"]);

        sig.set(Some(("name".to_string(), SortDirection::Descending)));
        assert_eq!(collect_names(&proxy), vec!["dan", "carol", "bob", "alice"]);

        sig.set(None);
        assert_eq!(collect_names(&proxy), vec!["carol", "alice", "bob", "dan"]);
    }

    #[test]
    fn bound_filters_signal_drives_view() {
        let proxy = SortFilterListModel::new(sample()).with_predicate("name", |t| {
            let t = t.to_string();
            Box::new(move |r: &Row| r.name.contains(&t))
        });
        let sig: Signal<HashMap<String, String>> = Signal::new(HashMap::new());
        proxy.filters_signal(sig.clone());

        let mut m = HashMap::new();
        m.insert("name".to_string(), "a".to_string());
        sig.set(m);
        assert_eq!(proxy.len(), 3);

        sig.set(HashMap::new());
        assert_eq!(proxy.len(), 4);
    }

    #[test]
    fn set_sort_writes_through_bound_signal() {
        let proxy =
            SortFilterListModel::new(sample()).with_comparator("id", |a: &Row, b| a.id.cmp(&b.id));
        let sig: Signal<Option<(String, SortDirection)>> = Signal::new(None);
        proxy.sort_signal(sig.clone());
        proxy.set_sort(Some("id"), SortDirection::Ascending);
        assert_eq!(
            sig.get(),
            Some(("id".to_string(), SortDirection::Ascending)),
            "set_sort must route through the bound signal so signal observers see the change"
        );
    }

    #[test]
    fn source_index_of_round_trips() {
        let proxy = SortFilterListModel::new(sample())
            .with_comparator("name", |a: &Row, b| a.name.cmp(&b.name));
        proxy.set_sort(Some("name"), SortDirection::Ascending);
        // After sort: [alice(src=1), bob(src=2), carol(src=0), dan(src=3)]
        assert_eq!(proxy.source_index_of(0), Some(1));
        assert_eq!(proxy.source_index_of(2), Some(0));
        assert_eq!(proxy.visible_index_of(1), Some(0));
        assert_eq!(proxy.visible_index_of(0), Some(2));
        assert_eq!(proxy.visible_index_of(99), None);
    }

    // ── first_changed_index (divergence) ────────────────────────────────

    #[test]
    fn divergence_on_append_is_old_len() {
        let model = sample();
        let proxy = SortFilterListModel::new(model.clone());
        model.push(Row {
            id: 5,
            name: "eve".into(),
        });
        assert_eq!(proxy.first_changed_index(), Some(4));
    }

    #[test]
    fn divergence_on_insert_at_front_is_zero() {
        let model = sample();
        let proxy = SortFilterListModel::new(model.clone());
        // Renumbering: every position now shows a shifted item even though
        // the identity projection's index values mostly coincide.
        model.insert(
            0,
            Row {
                id: 5,
                name: "eve".into(),
            },
        );
        assert_eq!(proxy.first_changed_index(), Some(0));
    }

    #[test]
    fn divergence_on_update_in_place_is_its_visible_index() {
        let model = sample();
        let proxy = SortFilterListModel::new(model.clone());
        model.set(
            2,
            Row {
                id: 2,
                name: "bobby".into(),
            },
        );
        assert_eq!(proxy.first_changed_index(), Some(2));
    }

    #[test]
    fn divergence_on_update_that_moves_under_sort() {
        let model = sample();
        let proxy = SortFilterListModel::new(model.clone())
            .with_comparator("name", |a: &Row, b| a.name.cmp(&b.name));
        proxy.set_sort(Some("name"), SortDirection::Ascending);
        // Sorted: alice(1), bob(2), carol(0), dan(3). Rename dan → "aaa":
        // it moves to the front, shifting everything.
        model.set(
            3,
            Row {
                id: 4,
                name: "aaa".into(),
            },
        );
        assert_eq!(proxy.first_changed_index(), Some(0));
    }

    #[test]
    fn divergence_on_sort_flip_is_first_reordered_row() {
        let model = sample();
        let proxy = SortFilterListModel::new(model)
            .with_comparator("name", |a: &Row, b| a.name.cmp(&b.name));
        // carol, alice, bob, dan → alice, bob, carol, dan: position 0 changes.
        proxy.set_sort(Some("name"), SortDirection::Ascending);
        assert_eq!(proxy.first_changed_index(), Some(0));
    }

    #[test]
    fn divergence_on_filter_removing_mid_row() {
        let model = sample();
        let proxy = SortFilterListModel::new(model).with_predicate("name", |t| {
            let t = t.to_string();
            Box::new(move |r: &Row| r.name.contains(&t))
        });
        // carol(0), alice(1), bob(2), dan(3); 'a' filters out bob (visible 2).
        proxy.set_filter("name", "a");
        assert_eq!(proxy.first_changed_index(), Some(2));
    }

    #[test]
    fn divergence_on_noop_rebuild_is_len() {
        let model = sample();
        let proxy = SortFilterListModel::new(model);
        proxy.clear_filters(); // rebuild with an identical projection
        assert_eq!(proxy.first_changed_index(), Some(4));
    }

    #[test]
    fn divergence_on_move_is_min_endpoint() {
        let model = sample();
        let proxy = SortFilterListModel::new(model.clone());
        model.move_item(1, 3);
        assert_eq!(proxy.first_changed_index(), Some(1));
    }

    #[test]
    fn unregistered_filter_column_is_ignored() {
        let proxy = SortFilterListModel::new(sample());
        // No predicate factory was registered for "name", so this filter
        // entry is silently ignored — the row count stays at 4.
        proxy.set_filter("name", "a");
        assert_eq!(proxy.len(), 4);
    }

    #[test]
    fn unregistered_sort_column_is_ignored() {
        let proxy = SortFilterListModel::new(sample());
        proxy.set_sort(Some("name"), SortDirection::Ascending);
        // No comparator registered → fall back to source order.
        assert_eq!(collect_names(&proxy), vec!["carol", "alice", "bob", "dan"]);
    }

    #[test]
    fn from_data_source_works() {
        // Re-use ListModel as a stand-in for ListDataSource (it's not one
        // directly, but this validates the from_source path indirectly via
        // the ItemSource trait if we wrap it. For now just verify wrapper
        // construction compiles and runs over a ListDataSource impl: see
        // `crate::filterable_list_source` if present, otherwise use a
        // minimal hand-rolled impl below.)
        struct VecSource(
            Rc<RefCell<Vec<Row>>>,
            #[allow(dead_code)] Rc<RefCell<Vec<ObserverEntry>>>,
        );
        impl ListDataSource for VecSource {
            type Item = Row;
            type Key = usize;
            fn len(&self) -> usize {
                self.0.borrow().len()
            }
            fn with_item<R>(&self, idx: usize, f: impl FnOnce(&Row) -> R) -> Option<R> {
                self.0.borrow().get(idx).map(f)
            }
            fn observe_changes(&self, _f: impl Fn(&DataChange) + 'static) -> ObserverHandle {
                // Minimal stub: observer never fires (test doesn't mutate).
                let inner: Rc<dyn std::any::Any> = Rc::new(());
                ObserverHandle::new(inner, 0, Rc::new(|_| {}))
            }
        }
        let src = VecSource(
            Rc::new(RefCell::new(vec![
                Row {
                    id: 9,
                    name: "x".into(),
                },
                Row {
                    id: 8,
                    name: "y".into(),
                },
            ])),
            Rc::new(RefCell::new(Vec::new())),
        );
        let proxy = SortFilterListModel::from_source(src);
        assert_eq!(proxy.len(), 2);
    }

    // ── Incremental ItemUpdated fast path ───────────────────────────────

    #[test]
    fn content_only_update_emits_item_updated_not_reset_and_preserves_order() {
        let model = sample();
        let proxy = SortFilterListModel::new(model.clone())
            .with_comparator("name", |a: &Row, b| a.name.cmp(&b.name));
        proxy.set_sort(Some("name"), SortDirection::Ascending);
        // Sorted: alice(1), bob(2), carol(0), dan(3).
        let before = collect_names(&proxy);

        let changes: Rc<RefCell<Vec<DataChange>>> = Rc::new(RefCell::new(Vec::new()));
        let c = changes.clone();
        let _h = proxy.observe_changes(move |change| c.borrow_mut().push(change.clone()));

        // Same sort key initial letter ('b'), so "bob" stays between
        // "alice" and "carol" — content changed, order didn't.
        model.set(
            2,
            Row {
                id: 2,
                name: "bobby".into(),
            },
        );

        assert_eq!(
            changes.borrow().as_slice(),
            &[DataChange::ItemUpdated { index: 1 }],
            "a stable-position content edit must emit a scoped ItemUpdated, not Reset"
        );
        assert_eq!(proxy.first_changed_index(), Some(1));
        let after = collect_names(&proxy);
        assert_eq!(after, vec!["alice", "bobby", "carol", "dan"]);
        assert_eq!(before.len(), after.len());
    }

    #[test]
    fn update_that_changes_sort_position_falls_back_to_reset_with_correct_ordering() {
        let model = sample();
        let proxy = SortFilterListModel::new(model.clone())
            .with_comparator("name", |a: &Row, b| a.name.cmp(&b.name));
        proxy.set_sort(Some("name"), SortDirection::Ascending);
        // Sorted: alice(1), bob(2), carol(0), dan(3).

        let changes: Rc<RefCell<Vec<DataChange>>> = Rc::new(RefCell::new(Vec::new()));
        let c = changes.clone();
        let _h = proxy.observe_changes(move |change| c.borrow_mut().push(change.clone()));

        // Rename dan → "aaa": jumps ahead of alice, forcing a renumber.
        model.set(
            3,
            Row {
                id: 4,
                name: "aaa".into(),
            },
        );

        assert_eq!(
            changes.borrow().as_slice(),
            &[DataChange::Reset],
            "a position-changing edit must fall back to the full rebuild"
        );
        assert_eq!(collect_names(&proxy), vec!["aaa", "alice", "bob", "carol"]);
    }

    #[test]
    fn update_transitions_filtered_out_to_in_and_back_rebuild() {
        let model = sample();
        let proxy = SortFilterListModel::new(model.clone()).with_predicate("name", |t| {
            let needle = t.to_string();
            Box::new(move |r: &Row| r.name.contains(&needle))
        });
        proxy.set_filter("name", "z");
        assert_eq!(proxy.len(), 0);

        let changes: Rc<RefCell<Vec<DataChange>>> = Rc::new(RefCell::new(Vec::new()));
        let c = changes.clone();
        let _h = proxy.observe_changes(move |change| c.borrow_mut().push(change.clone()));

        // Filtered out → in: "bob" doesn't match "z", "bobz" does.
        model.set(
            2,
            Row {
                id: 2,
                name: "bobz".into(),
            },
        );
        assert_eq!(changes.borrow().as_slice(), &[DataChange::Reset]);
        assert_eq!(proxy.len(), 1);
        assert_eq!(collect_names(&proxy), vec!["bobz"]);
        changes.borrow_mut().clear();

        // Filtered in → out: rename it away from "z" again.
        model.set(
            2,
            Row {
                id: 2,
                name: "bob".into(),
            },
        );
        assert_eq!(changes.borrow().as_slice(), &[DataChange::Reset]);
        assert_eq!(proxy.len(), 0);
    }

    #[test]
    fn hidden_item_update_stays_hidden_is_a_silent_fast_path() {
        let model = sample();
        let proxy = SortFilterListModel::new(model.clone()).with_predicate("name", |t| {
            let needle = t.to_string();
            Box::new(move |r: &Row| r.name.contains(&needle))
        });
        proxy.set_filter("name", "a"); // alice, carol, dan visible; bob hidden
        assert_eq!(proxy.len(), 3);

        let changes: Rc<RefCell<Vec<DataChange>>> = Rc::new(RefCell::new(Vec::new()));
        let c = changes.clone();
        let _h = proxy.observe_changes(move |change| c.borrow_mut().push(change.clone()));

        // bob stays filtered out both before and after.
        model.set(
            2,
            Row {
                id: 2,
                name: "bobby".into(),
            },
        );
        assert!(
            changes.borrow().is_empty(),
            "no visible row changed, so no notification should fire"
        );
        assert_eq!(proxy.first_changed_index(), Some(proxy.len()));
        assert_eq!(proxy.len(), 3);
    }
}
