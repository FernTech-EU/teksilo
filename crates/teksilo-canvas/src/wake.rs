// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Waking a window from another thread.
//!
//! Some content changes off the UI thread: a terminal's child prints, a video
//! decoder finishes a frame, a background task updates a status. The thread
//! that changed it cannot touch the widget tree, so it stores the change where
//! the UI thread will read it and then *wakes* the window, which runs a frame
//! that reads it. [`RedrawWaker`] is that wake. A window gives one to the
//! framework, which hands it to the off-thread sources attached to the
//! window's widgets.
//!
//! # The protocol
//!
//! A wake carries no data, and a burst of changes must cost one frame, not one
//! per change. A [`WakeFlag`] per consumer does both:
//!
//! - **Producer:** store the change, release any lock it holds, then
//!   `if flag.raise() { waker.wake(kind) }`. Only the clear-to-set edge wakes;
//!   every later raise until the consumer takes the flag merges into it.
//! - **Consumer (UI thread):** `flag.take()`, *then* read the change. Or
//!   `flag.park()` to leave it for later on purpose, without being woken
//!   again meanwhile.
//!
//! No wake is lost. `raise` and `take` are read-modify-writes of one atomic,
//! so they are totally ordered. If the raise comes first, the take reads it
//! (or a later raise in its release sequence) and synchronises with it, so
//! the change is visible to the read that follows. If the take comes first,
//! the raise reads `false` and wakes. No `SeqCst` is needed, which matters
//! twice: loom models `SeqCst` as acquire-release, so a proof that leaned on
//! it would be one loom cannot check, and the argument holds on weakly
//! ordered hardware as written.
//!
//! `take` is `AcqRel` rather than `Acquire` for the second case: there it
//! makes the take synchronise with the raise that follows, which orders the
//! platform's own redraw flag (Wayland's is `Relaxed`) behind it, so a wake
//! the platform merged into a redraw it had already queued is served by a
//! frame that sees the change. `park` is an RMW, not a store, for the same
//! reason as `raise`: a plain store would end the release sequence, and a
//! take after it would not synchronise with a producer whose raise it
//! follows.
//!
//! # Deciding before taking
//!
//! A consumer may look at a flag to decide whether to serve the change at all
//! in this frame, and take it only if it does (a widget whose paint is cached
//! is marked for paint only when it has a repaint pending). That look is
//! [`WakeFlag::peek`], an RMW like the rest, not a load. Suppose it reads the
//! flag clear, and a producer raises it right after. The raise must see what
//! the consumer did before looking, the platform clearing its own redraw flag
//! as it delivered the frame, so that the producer's wake queues another
//! frame instead of merging into the one being delivered. With an RMW the
//! raise reads what the peek wrote and synchronises with it; a load releases
//! nothing, and on weakly ordered hardware the frame would see no request
//! while the wake merged into a redraw already delivered.
//!
//! # Hidden windows
//!
//! A window nobody can see may drop a [`WakeKind::Draw`] wake, provided it
//! owes one when it is shown again: drawing it then shows the latest state,
//! and drawing it while hidden would show nothing. A [`WakeGate`] carries the
//! window's hidden state and whether a wake was dropped in one atomic, so that
//! exactly one of "the producer routes its wake" and "showing the window
//! re-issues one" happens. A [`WakeKind::Layout`] wake is never dropped: what
//! layout and accessibility read must stay current on a window that draws
//! nothing.
//!
//! # Rules for a waker
//!
//! [`RedrawWaker::wake`] is cheap, never waits for the UI thread, is
//! idempotent (repeated wakes merge), must not panic (it can run in a `Drop`
//! while unwinding), and is never called with one of the framework's locks
//! held. [`RedrawWaker::is_hidden`] is a lock-free load: it is read on any
//! thread, possibly while a producer holds a lock of its own.
//!
//! The windowed implementation lives in teksilo-platform and is installed by
//! teksilo-app; a headless tree takes a [`CountingWaker`] or none.

use crate::sync::{AtomicBool, AtomicU8, Ordering};

/// Wakes one window from any thread. See the [module docs](self) for the
/// protocol around it and the rules an implementation follows.
pub trait RedrawWaker: Send + Sync + 'static {
    /// Ask the window for a frame that reads what changed. `kind` says what
    /// changed, and so whether a window that draws nothing may drop it.
    fn wake(&self, kind: WakeKind);

    /// `true` while the window is known to be hidden. Advisory, for a producer
    /// that would rather not produce for nobody. A lock-free load.
    fn is_hidden(&self) -> bool {
        false
    }
}

/// What a wake asks of a window, ordered by how much of a frame it needs: a
/// change that asks for both wakes once, with the larger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum WakeKind {
    /// Only what the window draws changed: a new frame of a live image, state
    /// a widget's `paint()` reads. A window that draws nothing may drop it,
    /// and owes one when it is shown again.
    Draw,
    /// State that layout or accessibility reads changed: a size, a status, a
    /// relayout request. Never dropped: a window that draws nothing still runs
    /// the layout half of a frame for it.
    Layout,
}

impl WakeKind {
    const fn index(self) -> usize {
        match self {
            Self::Draw => 0,
            Self::Layout => 1,
        }
    }
}

/// A clear-to-set wake flag, one per consumer. Every write is an
/// acquire-release read-modify-write, so none can break a release sequence.
/// See the [module docs](self).
#[doc(hidden)]
pub struct WakeFlag {
    raised: AtomicBool,
}

impl WakeFlag {
    /// A clear flag.
    pub fn new() -> Self {
        Self {
            raised: AtomicBool::new(false),
        }
    }

    /// Producer, after storing the change and releasing any lock. Returns
    /// `true` when the flag was clear: the caller must then wake.
    #[must_use = "a `true` return obliges the caller to wake"]
    pub fn raise(&self) -> bool {
        !self.raised.swap(true, Ordering::AcqRel)
    }

    /// Consumer, before reading the change. Returns whether the flag was
    /// raised; it is clear afterwards, so the next raise wakes.
    pub fn take(&self) -> bool {
        self.raised.swap(false, Ordering::AcqRel)
    }

    /// Consumer that leaves a change for later on purpose: raises merge into
    /// the flag instead of waking, until a [`take`](Self::take).
    pub fn park(&self) {
        self.raised.swap(true, Ordering::AcqRel);
    }

    /// Consumer that decides from the flag whether to serve the change now,
    /// before taking it: whether it is raised, consuming nothing. See
    /// [Deciding before taking](self#deciding-before-taking).
    pub fn peek(&self) -> bool {
        self.raised.fetch_or(false, Ordering::AcqRel)
    }

    /// Whether the flag is raised. Advisory: it consumes nothing and orders
    /// nothing a read of the change could rely on, nor a decision a later
    /// raise must see; [`peek`](Self::peek) does the latter.
    pub fn is_raised(&self) -> bool {
        self.raised.load(Ordering::Acquire)
    }
}

impl Default for WakeFlag {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for WakeFlag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = if self.is_raised() { "raised" } else { "clear" };
        write!(f, "WakeFlag({state})")
    }
}

const HIDDEN: u8 = 1;
const MISSED: u8 = 2;

/// A window's hidden state and whether a droppable wake was dropped while it
/// was hidden, in one atomic: one modification order, so exactly one of "the
/// producer routes its wake" ([`pass`](Self::pass) returns `true`) and "showing
/// the window re-issues one" ([`show`](Self::show) returns `true`) happens.
#[doc(hidden)]
pub struct WakeGate {
    state: AtomicU8,
}

impl WakeGate {
    /// A gate for a shown window.
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(0),
        }
    }

    /// Any thread, for a wake the window may drop while hidden. `true`: route
    /// it now. `false`: the window is hidden, the drop is recorded, and the
    /// next [`show`](Self::show) returns `true`.
    #[must_use = "a `true` return obliges the caller to route the wake"]
    pub fn pass(&self) -> bool {
        let mut state = self.state.load(Ordering::Acquire);
        while state & HIDDEN != 0 {
            // A strong exchange, never a weak one: a spurious failure would
            // only loop, but loom would explore it as a real interleaving.
            match self.state.compare_exchange(
                state,
                state | MISSED,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return false,
                Err(now) => state = now,
            }
        }
        true
    }

    /// UI thread: the window is hidden from now on.
    pub fn hide(&self) {
        self.state.fetch_or(HIDDEN, Ordering::AcqRel);
    }

    /// UI thread: the window is shown again. `true` when a wake was dropped
    /// while it was hidden: the caller re-issues one now.
    #[must_use = "a dropped wake must be re-issued"]
    pub fn show(&self) -> bool {
        self.state.fetch_and(!(HIDDEN | MISSED), Ordering::AcqRel) & MISSED != 0
    }

    /// Whether the window is hidden. Advisory: what a
    /// [`RedrawWaker::is_hidden`] built on this gate returns.
    pub fn is_hidden(&self) -> bool {
        self.state.load(Ordering::Relaxed) & HIDDEN != 0
    }
}

impl Default for WakeGate {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for WakeGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.load(Ordering::Relaxed);
        f.debug_struct("WakeGate")
            .field("hidden", &(state & HIDDEN != 0))
            .field("missed", &(state & MISSED != 0))
            .finish()
    }
}

/// A [`RedrawWaker`] that counts the wakes it receives, for tests: a headless
/// tree has no window to wake. A test can wait for a wake made on another
/// thread with [`wait_for`](Self::wait_for).
///
/// It uses the standard library's lock whatever the build, so it is not for
/// use inside a loom model.
///
/// ```
/// use std::sync::Arc;
/// use std::time::Duration;
/// use teksilo_canvas::wake::{CountingWaker, RedrawWaker, WakeKind};
///
/// let waker = Arc::new(CountingWaker::new());
/// let remote = waker.clone();
/// std::thread::spawn(move || remote.wake(WakeKind::Draw));
/// assert!(waker.wait_for(1, Duration::from_secs(5)));
/// assert_eq!(waker.count_of(WakeKind::Draw), 1);
/// ```
#[derive(Debug, Default)]
pub struct CountingWaker {
    counts: std::sync::Mutex<[u64; 2]>,
    changed: std::sync::Condvar,
}

impl CountingWaker {
    /// A waker that has counted nothing yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every wake received, whatever its kind.
    pub fn count(&self) -> u64 {
        self.counts().iter().sum()
    }

    /// The wakes received of `kind`.
    pub fn count_of(&self, kind: WakeKind) -> u64 {
        self.counts()[kind.index()]
    }

    /// Block until [`count`](Self::count) reaches `at_least` or `timeout`
    /// passes. `true` when it was reached.
    pub fn wait_for(&self, at_least: u64, timeout: std::time::Duration) -> bool {
        let counts = self
            .counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (counts, _) = self
            .changed
            .wait_timeout_while(counts, timeout, |counts| {
                counts.iter().sum::<u64>() < at_least
            })
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        counts.iter().sum::<u64>() >= at_least
    }

    fn counts(&self) -> [u64; 2] {
        *self
            .counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl RedrawWaker for CountingWaker {
    fn wake(&self, kind: WakeKind) {
        let mut counts = self
            .counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        counts[kind.index()] += 1;
        drop(counts);
        self.changed.notify_all();
    }
}

#[cfg(all(test, not(teksilo_loom)))]
mod tests;

#[cfg(all(test, teksilo_loom))]
mod loom_tests;
