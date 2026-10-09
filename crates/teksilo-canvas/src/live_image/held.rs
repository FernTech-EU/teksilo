// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What this thread holds: the sources it has a write transaction open on,
//! and, in debug builds, which of the live-image locks it holds right now.
//!
//! The source lock is not reentrant. A thread that holds a transaction and
//! then takes the lock again, through another writer of the same source,
//! would wait for itself forever. Every producer call that locks checks here
//! first and panics instead, naming the source; `try_lock` reports
//! `WouldBlock`; a renderer read treats the source as busy, which is what an
//! `async` task suspended inside a transaction on the UI thread looks like
//! to it. A writer session's last token dropped while its thread holds a
//! transaction cannot lock either, so it leaves its release here for the
//! transaction to apply as it ends.
//!
//! The lock order is the state lock, then a source's consumer list, then a
//! consumer's waker slot. Debug builds check it on every acquisition.

use std::cell::RefCell;

use super::LiveImageId;

struct Entry {
    id: LiveImageId,
    /// Writer sessions whose last token dropped while the transaction was
    /// open, in the order they dropped.
    deferred_releases: Vec<u64>,
}

#[cfg(not(teksilo_loom))]
std::thread_local! {
    static HELD: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
}

// loom's own, since its model threads share one OS thread; its macro takes
// no `const` initialiser.
#[cfg(teksilo_loom)]
crate::sync::thread_local! {
    static HELD: RefCell<Vec<Entry>> = RefCell::new(Vec::new());
}

/// Whether this thread holds a write transaction on source `id`.
pub(super) fn holds(id: LiveImageId) -> bool {
    HELD.with(|held| held.borrow().iter().any(|e| e.id == id))
}

/// This thread opened a transaction on `id`.
pub(super) fn enter(id: LiveImageId) {
    HELD.with(|held| {
        held.borrow_mut().push(Entry {
            id,
            deferred_releases: Vec::new(),
        });
    });
}

/// This thread's transaction on `id` ends: the releases left for it.
pub(super) fn leave(id: LiveImageId) -> Vec<u64> {
    HELD.with(|held| {
        let mut held = held.borrow_mut();
        match held.iter().position(|e| e.id == id) {
            Some(at) => held.swap_remove(at).deferred_releases,
            None => Vec::new(),
        }
    })
}

/// Leave the release of writer session `session` to this thread's open
/// transaction on `id`. `false` when there is none: the caller releases
/// itself.
pub(super) fn defer_release(id: LiveImageId, session: u64) -> bool {
    HELD.with(|held| {
        let mut held = held.borrow_mut();
        match held.iter_mut().find(|e| e.id == id) {
            Some(entry) => {
                entry.deferred_releases.push(session);
                true
            }
            None => false,
        }
    })
}

/// The live-image locks, by rank: a thread takes them in this order only.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Rank {
    State = 0,
    List = 1,
    Waker = 2,
}

#[cfg(all(debug_assertions, not(teksilo_loom)))]
mod order {
    use std::cell::Cell;

    use super::Rank;

    std::thread_local! {
        /// How many locks of each rank this thread holds right now: two
        /// state locks, of two sources, are allowed.
        static HELD: Cell<[u8; 3]> = const { Cell::new([0; 3]) };
    }

    pub(in super::super) fn acquire(rank: Rank) {
        HELD.with(|held| {
            let mut counts = held.get();
            if let Some(later) = counts[rank as usize + 1..].iter().position(|&n| n > 0) {
                panic!(
                    "live image lock order: the {} lock taken while the {} lock is held \
                     (the order is: state, consumer list, waker)",
                    name(rank as usize),
                    name(rank as usize + 1 + later),
                );
            }
            counts[rank as usize] += 1;
            held.set(counts);
        });
    }

    pub(in super::super) fn release(rank: Rank) {
        HELD.with(|held| {
            let mut counts = held.get();
            counts[rank as usize] = counts[rank as usize].saturating_sub(1);
            held.set(counts);
        });
    }

    fn name(rank: usize) -> &'static str {
        ["state", "consumer list", "waker"][rank]
    }
}

#[cfg(not(all(debug_assertions, not(teksilo_loom))))]
mod order {
    use super::Rank;

    pub(in super::super) fn acquire(_rank: Rank) {}

    pub(in super::super) fn release(_rank: Rank) {}
}

/// Held for as long as a live-image lock of `rank` is held; checks the order
/// in debug builds.
pub(super) struct Ranked(Rank);

impl Ranked {
    pub(super) fn acquire(rank: Rank) -> Self {
        order::acquire(rank);
        Self(rank)
    }
}

impl Drop for Ranked {
    fn drop(&mut self) {
        order::release(self.0);
    }
}
