//! Per-row checkbox state for collection widgets.
//!
//! Parallels [`crate::SelectionModel`]. Selection (which row is the
//! cursor on) and checked-ness (which rows are *marked*) are
//! orthogonal axes — Outlook / Files-app convention.
//!
//! Issues a writable `Signal<bool>` per index. Repeated calls with
//! the same index return the same signal handle. The Checkbox widget
//! writes to the signal on click; the model observes each per-index
//! signal and keeps a central `Signal<BTreeSet<usize>>` in sync for
//! consumers that want "all checked indices, reactively."

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use fern_core::signal::{ObserverHandle, Signal};

struct Inner {
    per_index: HashMap<usize, Signal<bool>>,
    /// Observer handles keep the per-index → central propagation
    /// alive for the lifetime of the model.
    observers: HashMap<usize, ObserverHandle>,
}

/// Tracks which indices in a list are checked.
pub struct CheckedModel {
    /// Aggregate set, derived from per-index signals via observers.
    /// Read-only externally; the model updates it whenever a per-index
    /// signal flips.
    checked: Signal<BTreeSet<usize>>,
    inner: Rc<RefCell<Inner>>,
}

impl CheckedModel {
    pub fn new() -> Self {
        Self {
            checked: Signal::new(BTreeSet::new()),
            inner: Rc::new(RefCell::new(Inner {
                per_index: HashMap::new(),
                observers: HashMap::new(),
            })),
        }
    }

    /// Reactive view of the full checked-set.
    pub fn checked_signal(&self) -> Signal<BTreeSet<usize>> {
        self.checked.clone()
    }

    /// Writable per-index signal. Repeat calls cache the same handle —
    /// any consumer (the model itself, the Checkbox widget, an external
    /// observer) writing through it propagates to the central
    /// `checked_signal()`.
    pub fn signal_for(&self, index: usize) -> Signal<bool> {
        // Fast path — already cached.
        if let Some(sig) = self.inner.borrow().per_index.get(&index) {
            return sig.clone();
        }
        // Slow path — create + observe.
        let sig = Signal::new(false);
        let central = self.checked.clone();
        let handle = sig.observe(move |checked| {
            let mut set = central.get();
            let changed = if *checked {
                set.insert(index)
            } else {
                set.remove(&index)
            };
            if changed {
                central.set(set);
            }
        });
        let mut inner = self.inner.borrow_mut();
        inner.per_index.insert(index, sig.clone());
        inner.observers.insert(index, handle);
        sig
    }

    pub fn is_checked(&self, index: usize) -> bool {
        self.inner
            .borrow()
            .per_index
            .get(&index)
            .map(|s| s.get())
            .unwrap_or(false)
    }

    pub fn checked_indices(&self) -> Vec<usize> {
        self.checked.get().into_iter().collect()
    }

    pub fn checked_count(&self) -> usize {
        self.checked.get().len()
    }

    pub fn check(&self, index: usize) {
        let sig = self.signal_for(index);
        if !sig.get() {
            sig.set(true);
        }
    }

    pub fn uncheck(&self, index: usize) {
        let sig = self.signal_for(index);
        if sig.get() {
            sig.set(false);
        }
    }

    pub fn toggle(&self, index: usize) {
        let sig = self.signal_for(index);
        sig.set(!sig.get());
    }

    pub fn check_all(&self, count: usize) {
        for i in 0..count {
            self.check(i);
        }
    }

    pub fn clear(&self) {
        // Snapshot keys to avoid borrow-during-iteration when set()
        // recurses into the observer.
        let keys: Vec<usize> = self.inner.borrow().per_index.keys().copied().collect();
        for i in keys {
            self.uncheck(i);
        }
    }
}

impl Default for CheckedModel {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for CheckedModel {
    fn clone(&self) -> Self {
        Self {
            checked: self.checked.clone(),
            inner: self.inner.clone(),
        }
    }
}

impl std::fmt::Debug for CheckedModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CheckedModel")
            .field("checked_count", &self.checked.get().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_for_returns_same_root_per_index() {
        let m = CheckedModel::new();
        let a = m.signal_for(2);
        let b = m.signal_for(2);
        assert_eq!(a.get(), b.get());
        a.set(true);
        assert!(b.get(), "cached signal handles share the same root");
    }

    #[test]
    fn writing_per_index_signal_updates_central() {
        let m = CheckedModel::new();
        let s = m.signal_for(7);
        s.set(true);
        assert_eq!(m.checked_indices(), vec![7]);
        s.set(false);
        assert_eq!(m.checked_indices(), Vec::<usize>::new());
    }

    #[test]
    fn check_uncheck_toggle_round_trip() {
        let m = CheckedModel::new();
        assert!(!m.is_checked(3));
        m.check(3);
        assert!(m.is_checked(3));
        m.toggle(3);
        assert!(!m.is_checked(3));
        m.toggle(3);
        assert!(m.is_checked(3));
        m.uncheck(3);
        assert!(!m.is_checked(3));
    }

    #[test]
    fn check_all_then_clear() {
        let m = CheckedModel::new();
        m.check_all(5);
        assert_eq!(m.checked_indices(), vec![0, 1, 2, 3, 4]);
        m.clear();
        assert_eq!(m.checked_count(), 0);
    }

    #[test]
    fn signal_updates_propagate() {
        let m = CheckedModel::new();
        let s = m.signal_for(7);
        assert!(!s.get());
        m.check(7);
        assert!(s.get());
        m.uncheck(7);
        assert!(!s.get());
    }

    #[test]
    fn unrelated_index_does_not_flip_signal() {
        let m = CheckedModel::new();
        let s = m.signal_for(1);
        m.check(2);
        assert!(!s.get());
    }
}
