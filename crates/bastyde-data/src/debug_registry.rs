// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Debug-only registry of named data models for the Bastyde inspector.
//!
//! The registry is a thread-local `Vec<(String, Weak<dyn ModelDebug>)>`.
//! Models opt in by calling `.debug_named("name")` (a debug-only
//! builder method on `ListModel<T>`, `TreeModel<T>`, and
//! `SelectionModel`). The model itself owns the strong adapter `Rc`, so
//! the registration drops automatically when the last model handle is
//! freed — `snapshot()` prunes dead `Weak` entries on every call.
//!
//! This entire module compiles to nothing in release builds.

#![cfg(debug_assertions)]

use std::cell::RefCell;
use std::rc::{Rc, Weak};

/// Type-erased debug view of a data model. Implementors live in
/// `bastyde-data` itself (`ListModel`, `TreeModel`, etc.); the inspector
/// uses these methods to render the Data Models tab without needing
/// to know `T`.
#[allow(clippy::len_without_is_empty)]
pub trait ModelDebug: 'static {
    /// Discriminator string — `"ListModel"`, `"TreeModel"`,
    /// `"SelectionModel"`, … Shown verbatim in the inspector.
    fn kind(&self) -> &'static str;

    /// Number of items / nodes in the model. Cheap.
    fn len(&self) -> usize;

    /// Write a human-readable dump of the model's items into `out`.
    /// Each item should land on its own line. Best-effort; long
    /// outputs may be truncated by callers.
    fn debug_dump(&self, out: &mut dyn std::fmt::Write);
}

thread_local! {
    static REGISTRY: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
}

struct Entry {
    name: String,
    weak: Weak<dyn ModelDebug>,
}

/// Register a model under a human-readable name. The name does not
/// need to be unique across registrations — the inspector lists every
/// entry — but uniqueness aids tracking.
///
/// Stores a `Weak` to the adapter so the registry never keeps models
/// alive. The caller is responsible for retaining a strong `Rc` to
/// the adapter (typically by stashing it inside the model's own
/// `Rc<RefCell<Inner>>`, see `ListModel::debug_named`).
pub fn register(name: impl Into<String>, adapter: Weak<dyn ModelDebug>) {
    REGISTRY.with(|cell| {
        cell.borrow_mut().push(Entry {
            name: name.into(),
            weak: adapter,
        });
    });
}

/// Snapshot every live registration. Drops dead `Weak`s during the
/// walk so the registry doesn't grow unbounded as models churn.
pub fn snapshot() -> Vec<(String, Rc<dyn ModelDebug>)> {
    REGISTRY.with(|cell| {
        let mut out: Vec<(String, Rc<dyn ModelDebug>)> = Vec::new();
        cell.borrow_mut().retain(|entry| {
            if let Some(strong) = entry.weak.upgrade() {
                out.push((entry.name.clone(), strong));
                true
            } else {
                false
            }
        });
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct DummyModel {
        len: Cell<usize>,
    }

    impl ModelDebug for DummyModel {
        fn kind(&self) -> &'static str {
            "DummyModel"
        }
        fn len(&self) -> usize {
            self.len.get()
        }
        fn debug_dump(&self, out: &mut dyn std::fmt::Write) {
            let _ = write!(out, "len={}", self.len.get());
        }
    }

    #[test]
    fn register_and_snapshot_round_trip() {
        // Use a fresh thread-local to avoid pollution from other tests.
        // Tests are isolated by thread; this thread's REGISTRY starts empty.
        let snap_before = snapshot();
        let initial = snap_before.len();

        let m: Rc<dyn ModelDebug> = Rc::new(DummyModel { len: Cell::new(3) });
        register("alpha", Rc::downgrade(&m));

        let snap = snapshot();
        assert_eq!(snap.len(), initial + 1);
        let added = snap.iter().find(|(n, _)| n == "alpha").unwrap();
        assert_eq!(added.1.len(), 3);
    }

    #[test]
    fn dead_weaks_are_pruned() {
        let snap_before = snapshot();
        let initial = snap_before.len();

        {
            let m: Rc<dyn ModelDebug> = Rc::new(DummyModel { len: Cell::new(0) });
            register("ephemeral", Rc::downgrade(&m));
            // m drops here, weak becomes dead
        }

        let snap = snapshot();
        // The dead entry has been pruned — count is back to initial.
        assert_eq!(snap.len(), initial);
        assert!(snap.iter().all(|(n, _)| n != "ephemeral"));
    }
}
