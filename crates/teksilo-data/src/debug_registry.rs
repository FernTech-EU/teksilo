// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `debug_registry` — opt-in registry of named data models for the Teksilo inspector.
//!
//! Provides the infrastructure for the inspector's *Models* tab: models register
//! themselves under a human-readable name, and the inspector calls [`snapshot`] to
//! obtain a live list of every registered model without keeping them alive past
//! their natural lifetime.
//!
//! The registry is a thread-local `Vec` of `Weak<dyn ModelDebug>` entries.
//! Models opt in via the debug-only `.debug_named("name")` builder method on
//! [`crate::ListModel`], [`crate::TreeModel`], and [`crate::SelectionModel`];
//! that method creates a strong `Rc<dyn ModelDebug>` adapter and stores it
//! inside the model's own `Rc<RefCell<Inner>>`, while registering a `Weak`
//! clone here. When the last model handle is dropped the `Weak` becomes dead,
//! and the next call to [`snapshot`] prunes it automatically.
//!
//! This entire module is compiled only when `debug_assertions` are enabled
//! and contributes zero overhead to release builds.

#![cfg(debug_assertions)]

use std::cell::RefCell;
use std::rc::{Rc, Weak};

/// Type-erased debug view of a data model. Implementors live in
/// `teksilo-data` itself (`ListModel`, `TreeModel`, etc.); the inspector
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
///
/// Prunes dead entries first — [`snapshot`] also prunes, but a session that
/// churns many short-lived models (e.g. lazily realized rows) without ever
/// calling `snapshot` would otherwise grow the registry unbounded.
pub fn register(name: impl Into<String>, adapter: Weak<dyn ModelDebug>) {
    REGISTRY.with(|cell| {
        let mut registry = cell.borrow_mut();
        registry.retain(|entry| entry.weak.strong_count() > 0);
        registry.push(Entry {
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

    #[test]
    fn register_prunes_dead_entries_without_a_snapshot_call() {
        // Regression: `register` must prune dead weaks itself rather than
        // relying on the caller to eventually call `snapshot` — a session
        // that churns many short-lived models (lazily realized rows, say)
        // without ever opening the inspector would otherwise grow the
        // registry unbounded. Fresh thread-local (tests run on their own
        // thread, see `dead_weaks_are_pruned`), so it starts empty.
        {
            let m: Rc<dyn ModelDebug> = Rc::new(DummyModel { len: Cell::new(0) });
            register("ephemeral", Rc::downgrade(&m));
            // m drops here, weak becomes dead — never snapshotted.
        }

        let m2: Rc<dyn ModelDebug> = Rc::new(DummyModel { len: Cell::new(1) });
        register("still-alive", Rc::downgrade(&m2));

        // `register` pruned the dead "ephemeral" entry before pushing the
        // new one, so exactly one raw entry remains — not two.
        let raw_len = REGISTRY.with(|cell| cell.borrow().len());
        assert_eq!(raw_len, 1);
    }
}
