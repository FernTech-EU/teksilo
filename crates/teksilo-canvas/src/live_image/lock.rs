// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The lock a live image's state sits behind.
//!
//! In an ordinary build it is a `parking_lot::Mutex`: `try_lock_for` bounds
//! how long the UI thread waits for a producer, and `unlock_fair` hands the
//! lock to a renderer parked on it instead of letting the producer take it
//! straight back.
//!
//! Under `--cfg teksilo_loom` it is a lock built on a loom atomic with
//! parking_lot's own orderings: a failed `try_lock` is a `Relaxed` load, a
//! successful one an `Acquire` exchange, an unlock a `Release` exchange. A
//! loom mutex would not do: it is linearisable, so a failed `try_lock` could
//! never read a stale "locked", which is the read the busy-lock handoff has
//! to survive. The timed lock is a plain `try_lock` there, since a model has
//! no clock.
//!
//! Nothing here knows about live images: `Shared` wraps every guard so that
//! every unlock runs the handoff (see `source.rs`).

#[cfg(not(teksilo_loom))]
mod imp {
    use std::time::Duration;

    pub(crate) struct Lock<T>(parking_lot::Mutex<T>);

    pub(crate) struct Guard<'a, T>(parking_lot::MutexGuard<'a, T>);

    impl<T> Lock<T> {
        pub(crate) fn new(value: T) -> Self {
            Self(parking_lot::Mutex::new(value))
        }

        pub(crate) fn lock(&self) -> Guard<'_, T> {
            Guard(self.0.lock())
        }

        pub(crate) fn try_lock(&self) -> Option<Guard<'_, T>> {
            self.0.try_lock().map(Guard)
        }

        pub(crate) fn try_lock_for(&self, timeout: Duration) -> Option<Guard<'_, T>> {
            self.0.try_lock_for(timeout).map(Guard)
        }
    }

    impl<'a, T> Guard<'a, T> {
        pub(crate) fn get(&self) -> &T {
            &self.0
        }

        pub(crate) fn get_mut(&mut self) -> &mut T {
            &mut self.0
        }

        /// Unlock, handing the lock to the longest-parked waiter when `fair`.
        pub(crate) fn unlock(self, fair: bool) {
            if fair {
                parking_lot::MutexGuard::unlock_fair(self.0);
            } else {
                drop(self.0);
            }
        }
    }
}

#[cfg(teksilo_loom)]
mod imp {
    use std::mem::ManuallyDrop;
    use std::time::Duration;

    use loom::cell::{MutPtr, UnsafeCell};

    use crate::sync::{AtomicBool, Ordering};

    pub(crate) struct Lock<T> {
        locked: AtomicBool,
        value: UnsafeCell<T>,
    }

    // SAFETY: the value is reached only through a `Guard`, and at most one
    // guard exists at a time: `try_lock`'s exchange succeeds for one thread
    // until the guard's release exchange clears the flag.
    unsafe impl<T: Send> Send for Lock<T> {}
    unsafe impl<T: Send> Sync for Lock<T> {}

    pub(crate) struct Guard<'a, T> {
        lock: &'a Lock<T>,
        // Dropped before the flag is cleared, so loom sees the access end
        // before another thread can take the lock.
        value: ManuallyDrop<MutPtr<T>>,
    }

    impl<T> Lock<T> {
        pub(crate) fn new(value: T) -> Self {
            Self {
                locked: AtomicBool::new(false),
                value: UnsafeCell::new(value),
            }
        }

        pub(crate) fn lock(&self) -> Guard<'_, T> {
            loop {
                if let Some(guard) = self.try_lock() {
                    return guard;
                }
                loom::thread::yield_now();
            }
        }

        /// parking_lot's: a `Relaxed` load first, then an `Acquire`
        /// exchange.
        pub(crate) fn try_lock(&self) -> Option<Guard<'_, T>> {
            if self.locked.load(Ordering::Relaxed) {
                return None;
            }
            self.locked
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .ok()?;
            Some(Guard {
                lock: self,
                value: ManuallyDrop::new(self.value.get_mut()),
            })
        }

        /// A model has no clock: one attempt.
        pub(crate) fn try_lock_for(&self, _timeout: Duration) -> Option<Guard<'_, T>> {
            self.try_lock()
        }
    }

    impl<'a, T> Guard<'a, T> {
        pub(crate) fn get(&self) -> &T {
            // SAFETY: this guard is the lock's only holder.
            unsafe { self.value.deref() }
        }

        pub(crate) fn get_mut(&mut self) -> &mut T {
            // SAFETY: this guard is the lock's only holder.
            unsafe { self.value.deref() }
        }

        /// No queue to be fair to in a model.
        pub(crate) fn unlock(self, _fair: bool) {
            drop(self);
        }
    }

    impl<T> Drop for Guard<'_, T> {
        fn drop(&mut self) {
            // SAFETY: dropped exactly once, here, and never used after.
            unsafe { ManuallyDrop::drop(&mut self.value) };
            // parking_lot's fast unlock: a `Release` exchange.
            let _ = self.lock.locked.compare_exchange(
                true,
                false,
                Ordering::Release,
                Ordering::Relaxed,
            );
        }
    }
}

pub(crate) use imp::{Guard, Lock};
