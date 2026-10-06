// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The synchronisation primitives Teksilo's off-thread protocols are written
//! against: the standard library's in every ordinary build, loom's under
//! `RUSTFLAGS="--cfg teksilo_loom"`, so that loom can run every interleaving
//! of a protocol through the very code that ships.
//!
//! One facade for the whole workspace. Every crate whose protocol loom models
//! (canvas, core, platform, terminal) imports from here, rather than from
//! `std`, the atomics, `Mutex` and `Condvar` the protocol's correctness rests
//! on, and the `Arc` of what a model shares between its threads; a model
//! reaches loom's `model` and `thread` through it too, so no other crate
//! depends on loom. What a protocol only holds, and loom need not see, stays
//! `std`'s: a `Weak` (loom has none), a `OnceLock` set once before any
//! thread reads it.
//!
//! Under the cfg, every type here is loom's, and loom's types panic outside a
//! `model`. A loom run therefore always filters on the `loom_` test-name
//! prefix that every model carries:
//!
//! ```sh
//! RUSTFLAGS="--cfg teksilo_loom" LOOM_MAX_PREEMPTIONS=3 \
//!     cargo test --lib -p teksilo-canvas loom_
//! ```
//!
//! A lock loom cannot model, such as `parking_lot`'s, is not here: a protocol
//! whose correctness rests on a lock takes that lock from here.
//!
//! Not part of Teksilo's API: it moves with the protocols.

#[cfg(not(teksilo_loom))]
pub use std::sync::atomic::{
    AtomicBool, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering, fence,
};
#[cfg(not(teksilo_loom))]
pub use std::sync::{Arc, Condvar, Mutex, MutexGuard};

#[cfg(teksilo_loom)]
pub use loom::sync::atomic::{
    AtomicBool, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering, fence,
};
#[cfg(teksilo_loom)]
pub use loom::sync::{Arc, Condvar, Mutex, MutexGuard};
/// Thread-locals a protocol keeps per thread: loom's model threads all run
/// on one OS thread, so a protocol's own must be loom's under the cfg.
#[cfg(teksilo_loom)]
pub use loom::thread_local;

/// Run `f` under every interleaving loom explores. Only under the cfg.
#[cfg(teksilo_loom)]
pub use loom::model;

/// Threads a `model` spawns. Only under the cfg.
#[cfg(teksilo_loom)]
pub use loom::thread;

#[cfg(all(test, teksilo_loom))]
mod loom_tests {
    use super::{Arc, AtomicBool, Ordering, model, thread};
    use std::sync::atomic::AtomicUsize as StdCounter;

    /// The cfg reaches the facade and loom really explores: a two-thread model
    /// runs more than once. Without `--cfg teksilo_loom` this test is not
    /// compiled at all, and a CI step that finds no `loom_` test fails.
    #[test]
    fn loom_explores_more_than_one_interleaving() {
        static RUNS: StdCounter = StdCounter::new(0);
        model(|| {
            RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let flag = Arc::new(AtomicBool::new(false));
            let writer = {
                let flag = flag.clone();
                thread::spawn(move || flag.store(true, Ordering::Release))
            };
            let _ = flag.load(Ordering::Acquire);
            writer.join().unwrap();
        });
        assert!(
            RUNS.load(std::sync::atomic::Ordering::Relaxed) > 1,
            "loom ran the model once: it explored no interleaving"
        );
    }
}
