// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A 1,000,000-row windowed `ListDataSource` driven entirely through the public
//! trait — the proof that an external/huge source plugs into Teksilo's data
//! views without mirroring its rows into a `ListModel` and without holding more
//! than the visible window in memory.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use teksilo_core::ObserverHandle;
use teksilo_data::{DataChange, ListDataSource, RowState};

/// A change observer registered against the source.
type ChangeObserver = Rc<dyn Fn(&DataChange)>;

/// A source over a virtual 1M-row table that keeps only the rows it has been
/// asked to load resident.
struct WindowedSource {
    total: usize,
    resident: RefCell<HashMap<usize, String>>,
    fetches: Rc<Cell<usize>>,
    observers: RefCell<Vec<ChangeObserver>>,
    next_id: Cell<u64>,
}

impl WindowedSource {
    fn new(total: usize, fetches: Rc<Cell<usize>>) -> Self {
        Self {
            total,
            resident: RefCell::new(HashMap::new()),
            fetches,
            observers: RefCell::new(Vec::new()),
            next_id: Cell::new(1),
        }
    }

    /// Simulate an async page load completing on the main thread.
    fn deliver_window(&self, range: Range<usize>) {
        for i in range.clone() {
            self.resident.borrow_mut().insert(i, format!("row {i}"));
        }
        let change = DataChange::WindowLoaded { range };
        let observers = self.observers.borrow().clone();
        for o in observers {
            o(&change);
        }
    }
}

impl ListDataSource for WindowedSource {
    type Item = String;
    type Key = usize;

    fn len(&self) -> usize {
        self.total
    }

    fn with_item<R>(&self, index: usize, f: impl FnOnce(&String) -> R) -> Option<R> {
        self.resident.borrow().get(&index).map(f)
    }

    fn key_at(&self, index: usize) -> Option<usize> {
        (index < self.total).then_some(index)
    }

    fn row_state(&self, index: usize) -> RowState {
        if self.resident.borrow().contains_key(&index) {
            RowState::Ready
        } else {
            RowState::Loading
        }
    }

    fn request_window(&self, range: Range<usize>) {
        // Count a fetch only when the window has at least one unloaded row.
        let missing = range
            .clone()
            .any(|i| !self.resident.borrow().contains_key(&i));
        if missing {
            self.fetches.set(self.fetches.get() + 1);
        }
    }

    fn observe_changes(&self, f: impl Fn(&DataChange) + 'static) -> ObserverHandle {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        self.observers.borrow_mut().push(Rc::new(f));
        let inner: Rc<dyn std::any::Any> = Rc::new(());
        ObserverHandle::new(inner, id, Rc::new(|_| {}))
    }
}

#[test]
fn million_row_window_only_fetches_visible_range() {
    let fetches = Rc::new(Cell::new(0));
    let src = WindowedSource::new(1_000_000, fetches.clone());

    // The total is fully known (scrollbar sizes correctly) even though nothing
    // is loaded.
    assert_eq!(src.len(), 1_000_000);
    assert_eq!(src.row_state(0), RowState::Loading);
    assert_eq!(src.with_item(0, |s| s.clone()), None);
    assert_eq!(src.with_item(500_000, |s| s.clone()), None);

    // The view realizes a ~25-row window and nudges the source to load it.
    let window = 0..25;
    src.request_window(window.clone());
    assert_eq!(
        fetches.get(),
        1,
        "exactly one page fetch for the visible window"
    );

    // Page load completes; those rows are now Ready, the rest stay Loading.
    src.deliver_window(window.clone());
    for i in window.clone() {
        assert_eq!(src.row_state(i), RowState::Ready);
        let expected = format!("row {i}");
        assert_eq!(src.with_item(i, |s| s.clone()), Some(expected));
    }
    assert_eq!(src.row_state(999_999), RowState::Loading);
    assert_eq!(
        src.resident.borrow().len(),
        25,
        "only the window is materialized, never the full million"
    );

    // Re-requesting an already-resident window triggers no further fetch.
    src.request_window(window);
    assert_eq!(fetches.get(), 1);
}

#[test]
fn window_loaded_is_a_distinct_change_from_inserted() {
    let fetches = Rc::new(Cell::new(0));
    let src = WindowedSource::new(100, fetches);
    let seen: Rc<RefCell<Vec<DataChange>>> = Rc::new(RefCell::new(Vec::new()));
    let s = seen.clone();
    let _h = src.observe_changes(move |c| s.borrow_mut().push(c.clone()));

    src.deliver_window(10..20);
    // WindowLoaded — NOT ItemsInserted — so a SelectionModel won't index-shift.
    assert_eq!(
        seen.borrow().as_slice(),
        &[DataChange::WindowLoaded { range: 10..20 }]
    );
}
