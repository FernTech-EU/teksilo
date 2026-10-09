// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A window's wake target: the [`RedrawWaker`] any thread uses to have one
//! window run a frame.
//!
//! Two routes, chosen by what the wake keeps current (see
//! [`teksilo_canvas::wake`]):
//!
//! - **[`WakeKind::Draw`]** asks winit for a redraw. It is dropped while the
//!   window is hidden, and one redraw is requested when it is shown again. A
//!   request winit is already holding (a Wayland window waiting for a frame
//!   callback) merges into it, which is right for a wake that only changes
//!   pixels: the redraw winit delivers draws the latest state.
//! - **[`WakeKind::Layout`]** keeps state current, so it must reach a window
//!   that draws nothing. A redraw request cannot promise that: on Wayland,
//!   while winit holds a redraw back, a further request does not even wake
//!   the event loop (it sets a flag that is already set). It goes through the
//!   *state route* teksilo-app installs, which posts one event per burst to
//!   the event loop; without a route it falls back to a redraw request.
//!
//! On macOS a redraw requested from another thread is dispatched
//! synchronously onto the main thread, so a wake made off it would wait for
//! the main thread, holding the lock that makes disconnecting exact, and a
//! main thread taking that lock meanwhile would deadlock. A draw wake made
//! off the main thread there never asks winit itself: it posts through the
//! *off-main route* teksilo-app installs, or, with none installed, hands the
//! request to the main dispatch queue and returns.
//!
//! [`disconnect`](WindowWakeTarget::disconnect) ends it: once it has returned,
//! the target never asks winit for a redraw again, whoever still holds a
//! waker. A wake already past its check may still post through a route,
//! which an event loop that has ended refuses. The window's `Drop` runs it,
//! and teksilo-app runs it for every window when the event loop ends, since
//! winit panics on X11 when a window is asked for a redraw after that.

use std::sync::{Arc, OnceLock};
use std::thread::ThreadId;

use teksilo_canvas::sync::{AtomicBool, AtomicU64, Mutex, Ordering};
use teksilo_canvas::wake::{RedrawWaker, WakeFlag, WakeGate, WakeKind};

/// The one call a wake target makes into winit, a seam so tests can count it.
pub(crate) trait RequestRedraw: Send + Sync {
    fn request_redraw(&self);
}

impl RequestRedraw for winit::window::Window {
    fn request_redraw(&self) {
        winit::window::Window::request_redraw(self);
    }
}

/// A route that posts to the event loop. Set once by teksilo-app.
pub(crate) type WakeRoute = Arc<dyn Fn() + Send + Sync>;

/// One window's wake counters. Monotonic.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct LiveWakeStats {
    /// Wakes of either kind that reached the window, each one counted:
    /// asked winit for a redraw, or posted to the event loop, where a burst
    /// is one post.
    pub wakes: u64,
    /// Draw wakes dropped because the window was hidden. One redraw is
    /// requested when it is shown again.
    pub dropped_hidden: u64,
    /// Whether the window is hidden, as teksilo-app last said.
    pub hidden: bool,
}

/// See the [module docs](self).
pub(crate) struct WindowWakeTarget {
    /// A fast path for the check `slot` makes authoritative.
    connected: AtomicBool,
    /// The window to ask for a redraw; emptied only by `disconnect`. Held
    /// across the request, which is what makes "no call after disconnect"
    /// exact. A leaf: nothing else is locked while it is held.
    slot: Mutex<Option<Arc<dyn RequestRedraw>>>,
    /// The thread that created the window, which winit requires to be the
    /// event loop's.
    main_thread: ThreadId,
    state_route: OnceLock<WakeRoute>,
    off_main_route: OnceLock<WakeRoute>,
    gate: WakeGate,
    /// A state wake was posted and the event loop has not taken it yet.
    state_posted: WakeFlag,
    /// An off-main draw wake was posted and the event loop has not taken it.
    draw_posted: WakeFlag,
    wakes: AtomicU64,
    dropped_hidden: AtomicU64,
    /// Itself, for the request a macOS draw wake made off the main thread
    /// hands to the main queue when no off-main route is installed.
    #[cfg(target_os = "macos")]
    this: std::sync::Weak<Self>,
}

impl WindowWakeTarget {
    /// A connected target for `window`, created on the window's thread.
    pub(crate) fn new(window: Arc<dyn RequestRedraw>) -> Arc<Self> {
        Arc::new_cyclic(|_this| Self {
            connected: AtomicBool::new(true),
            slot: Mutex::new(Some(window)),
            main_thread: std::thread::current().id(),
            state_route: OnceLock::new(),
            off_main_route: OnceLock::new(),
            gate: WakeGate::new(),
            state_posted: WakeFlag::new(),
            draw_posted: WakeFlag::new(),
            wakes: AtomicU64::new(0),
            dropped_hidden: AtomicU64::new(0),
            #[cfg(target_os = "macos")]
            this: _this.clone(),
        })
    }

    /// Install the route state wakes post through. The first call wins.
    pub(crate) fn set_state_route(&self, route: WakeRoute) {
        let _ = self.state_route.set(route);
    }

    /// Install the route draw wakes made off the main thread post through.
    /// The first call wins.
    pub(crate) fn set_off_main_route(&self, route: WakeRoute) {
        let _ = self.off_main_route.set(route);
    }

    /// A draw wake: dropped while hidden, merged into a pending redraw.
    pub(crate) fn wake_draw(&self) {
        if !self.connected.load(Ordering::Acquire) {
            return;
        }
        if !self.gate.pass() {
            self.dropped_hidden.fetch_add(1, Ordering::Relaxed);
            return;
        }
        self.wakes.fetch_add(1, Ordering::Relaxed);
        if std::thread::current().id() != self.main_thread {
            if let Some(route) = self.off_main_route.get() {
                if self.draw_posted.raise() {
                    route();
                }
                return;
            }
            #[cfg(target_os = "macos")]
            {
                self.request_on_main_queue();
                return;
            }
        }
        self.request_on_slot();
    }

    /// macOS, off the main thread, with no off-main route: hand the request
    /// to the main dispatch queue and return, once per burst. It runs on the
    /// main thread, where asking winit waits for nobody, and after a
    /// `disconnect` (also made there) it finds the slot empty.
    #[cfg(target_os = "macos")]
    fn request_on_main_queue(&self) {
        if !self.draw_posted.raise() {
            return;
        }
        let this = self.this.clone();
        dispatch2::DispatchQueue::main().exec_async(move || {
            if let Some(target) = this.upgrade() {
                target.draw_posted.take();
                target.request_on_slot();
            }
        });
    }

    /// A state wake: never dropped, posted once per burst when a route is
    /// installed, a redraw request otherwise.
    pub(crate) fn wake_state(&self) {
        if !self.connected.load(Ordering::Acquire) {
            return;
        }
        self.wakes.fetch_add(1, Ordering::Relaxed);
        match self.state_route.get() {
            Some(route) => {
                if self.state_posted.raise() {
                    route();
                }
            }
            None => self.request_on_slot(),
        }
    }

    /// Event loop, on receipt of a posted wake of `kind`, *before* acting on
    /// it: the next wake of that kind posts again. Returns whether one was
    /// pending.
    pub(crate) fn take_posted(&self, kind: WakeKind) -> bool {
        match kind {
            WakeKind::Draw => self.draw_posted.take(),
            _ => self.state_posted.take(),
        }
    }

    /// Event loop: `true` while hidden, in which case the redraw the caller
    /// wanted is owed when the window is shown again.
    pub(crate) fn defer_until_shown(&self) -> bool {
        if self.gate.pass() {
            false
        } else {
            self.dropped_hidden.fetch_add(1, Ordering::Relaxed);
            true
        }
    }

    /// Event loop: record whether the window is hidden. Showing it requests
    /// the one redraw owed for the draw wakes dropped meanwhile, if any.
    pub(crate) fn set_hidden(&self, hidden: bool) {
        if hidden {
            self.gate.hide();
        } else if self.gate.show() && self.connected.load(Ordering::Acquire) {
            self.wakes.fetch_add(1, Ordering::Relaxed);
            self.request_on_slot();
        }
    }

    /// Event loop: the window draws again and a frame is about to run, which
    /// serves the draw wakes dropped while it was hidden. Record it shown,
    /// without the redraw [`set_hidden`](Self::set_hidden) would deliver for
    /// them.
    pub(crate) fn show_for_drawing(&self) {
        let _ = self.gate.show();
    }

    pub(crate) fn is_hidden(&self) -> bool {
        self.gate.is_hidden()
    }

    /// Never call into winit again. Waits for a request already in progress;
    /// idempotent. The window reference is dropped on the calling thread,
    /// after the lock is released.
    pub(crate) fn disconnect(&self) {
        self.connected.store(false, Ordering::Release);
        let window = self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        drop(window);
    }

    /// Ask winit for a redraw, unless disconnected.
    pub(crate) fn request_on_slot(&self) {
        let slot = self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(window) = slot.as_ref() {
            window.request_redraw();
        }
    }

    pub(crate) fn stats(&self) -> LiveWakeStats {
        LiveWakeStats {
            wakes: self.wakes.load(Ordering::Relaxed),
            dropped_hidden: self.dropped_hidden.load(Ordering::Relaxed),
            hidden: self.gate.is_hidden(),
        }
    }

    /// Hold the slot's lock, as an in-progress request would.
    #[cfg(all(test, not(teksilo_loom)))]
    fn hold_slot(&self) -> impl Drop + '_ {
        self.slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl RedrawWaker for WindowWakeTarget {
    fn wake(&self, kind: WakeKind) {
        match kind {
            WakeKind::Draw => self.wake_draw(),
            // Every other kind keeps state current, and is never dropped.
            _ => self.wake_state(),
        }
    }

    fn is_hidden(&self) -> bool {
        self.gate.is_hidden()
    }
}

impl std::fmt::Debug for WindowWakeTarget {
    /// Atomics only: never takes the slot's lock.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowWakeTarget")
            .field("connected", &self.connected.load(Ordering::Relaxed))
            .field("gate", &self.gate)
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}

#[cfg(all(test, not(teksilo_loom)))]
mod tests;

#[cfg(all(test, teksilo_loom))]
mod loom_tests;
