//! Concrete reactive list. Owns items as `Vec<T>` behind `Rc<RefCell<>>`.
//! Mutations emit `DataChange` automatically. Cloneable for shared access.

use std::cell::RefCell;
use std::rc::Rc;

use fern_core::ObserverHandle;

use crate::data_change::DataChange;

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
}
