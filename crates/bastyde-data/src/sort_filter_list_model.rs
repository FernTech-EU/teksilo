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
//! - The upstream source emits any [`DataChange`]. The proxy collapses every
//!   upstream change into a single [`DataChange::Reset`] for its own
//!   observers — translating fine-grained inserts / updates through a sort
//!   projection is correctness-fragile (an updated item's sort key can move
//!   it to a different visible row), so a Reset is the safe contract.
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

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;

use bastyde_core::ObserverHandle;
use bastyde_core::signal::Signal;

use crate::data_change::DataChange;
use crate::list_data_source::ListDataSource;
use crate::list_model::ListModel;

/// Sort direction emitted by `TableView` / `TreeTable` headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
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
    fn rebuild(&mut self) {
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
        let upstream_handle = (observe_fn)(Box::new(move |_change| {
            if let Some(strong) = weak.upgrade() {
                rebuild_and_notify(&strong);
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
    pub fn bind_sort_signal(&self, signal: Signal<Option<(String, SortDirection)>>) {
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
    pub fn bind_filters_signal(&self, signal: Signal<HashMap<String, String>>) {
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
        // observer registered by `bind_sort_signal` will call back into
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
    // Drop the borrow before invoking observer callbacks so they may freely
    // call back into the proxy (`with_item`, `len`, etc.).
    let callbacks = {
        let mut guard = inner.borrow_mut();
        guard.rebuild();
        guard.snapshot_callbacks()
    };
    let change = DataChange::Reset;
    for cb in &callbacks {
        cb(&change);
    }
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
        proxy.bind_sort_signal(sig.clone());

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
        proxy.bind_filters_signal(sig.clone());

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
        proxy.bind_sort_signal(sig.clone());
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
}
