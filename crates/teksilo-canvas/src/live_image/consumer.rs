// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`LiveImageConsumer`]: one widget in one window, attached to a source.

use std::sync::Arc;
use std::time::Duration;

use super::LiveImageSource;
use super::held::{self, Rank, Ranked};
use super::internal::{LiveImageRead, ReadAttempt};
use super::meta::LiveMeta;
use super::stats::{LiveImageAttachmentStats, LiveImageStats};
use crate::sync::{AtomicBool, AtomicU32, AtomicU64, Mutex, Ordering};
use crate::wake::{RedrawWaker, WakeFlag, WakeKind};

/// The `Send` half of one attachment of a [`LiveImageSource`]: one widget in
/// one window. It carries the attachment's wake flags, its window's waker,
/// the size and status its window last laid out, and its counters.
///
/// A [`LiveImageQuad`](super::LiveImageQuad) carries it, never pixels:
/// `Clone` is an `Arc` clone, and equality is identity.
#[derive(Clone)]
pub struct LiveImageConsumer {
    pub(crate) shared: Arc<ConsumerShared>,
}

pub(crate) struct ConsumerShared {
    pub(crate) source: LiveImageSource,
    /// Raised by a change of the source's status or size; taken by the
    /// layout pre-pass before it reads them.
    pub(crate) geometry: WakeFlag,
    /// Raised by a commit; taken by the live pass before it reads the
    /// generation, parked by a paused draw.
    pub(crate) pixels: WakeFlag,
    /// Raised when this consumer found the source lock busy: the unlock that
    /// follows wakes it.
    pub(crate) handoff: WakeFlag,
    /// The window's waker. A leaf lock.
    waker: Mutex<Option<Arc<dyn RedrawWaker>>>,
    /// The meta the window's last layout pre-pass recorded, packed.
    layout_meta: AtomicU32,
    detached: AtomicBool,
    observed: AtomicBool,
    paused: AtomicBool,
    counters: AttachmentCounters,
}

#[derive(Default)]
struct AttachmentCounters {
    window_generation: AtomicU64,
    frames_drawn: AtomicU64,
    captures: AtomicU64,
    uploads: AtomicU64,
    deferred_frames: AtomicU64,
    paused_frames: AtomicU64,
    paints: AtomicU64,
}

/// What one render did for one attachment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RenderRecord {
    /// The quad was in the render.
    pub(crate) drawn: bool,
    /// The render wrote at least one rect of the source.
    pub(crate) uploaded: bool,
    /// The render drew an older texture or only the background: the lock was
    /// busy, or the size changed after layout.
    pub(crate) deferred: bool,
    /// Every quad of the source in the render was paused.
    pub(crate) paused: bool,
    /// The texture was kept because of it.
    pub(crate) kept: bool,
    /// A capture for a screenshot, not a presented frame.
    pub(crate) capture: bool,
    /// The generation the window's texture holds afterwards.
    pub(crate) window_generation: u64,
}

impl ConsumerShared {
    pub(crate) fn new(source: LiveImageSource, waker: Option<Arc<dyn RedrawWaker>>) -> Self {
        Self {
            source,
            geometry: WakeFlag::new(),
            pixels: WakeFlag::new(),
            handoff: WakeFlag::new(),
            waker: Mutex::new(waker),
            layout_meta: AtomicU32::new(LiveMeta::default().pack()),
            detached: AtomicBool::new(false),
            observed: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            counters: AttachmentCounters::default(),
        }
    }

    pub(crate) fn waker(&self) -> Option<Arc<dyn RedrawWaker>> {
        let _rank = Ranked::acquire(Rank::Waker);
        self.waker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn is_detached(&self) -> bool {
        self.detached.load(Ordering::Acquire)
    }

    pub(crate) fn record_layout_meta(&self, meta: LiveMeta) {
        self.layout_meta.store(meta.pack(), Ordering::Relaxed);
    }

    /// Drawn unpaused in its window's last render, a window not known to be
    /// hidden. The waker is read after the list lock is released.
    pub(crate) fn is_displayed(&self) -> bool {
        self.observed.load(Ordering::Relaxed)
            && !self.paused.load(Ordering::Relaxed)
            && !self.waker().is_some_and(|w| w.is_hidden())
    }

    /// Flip `flag` to `value`, keeping the source's count of consumers with
    /// it set in step, and never counting a detached consumer.
    fn set_counted(&self, flag: &AtomicBool, value: bool, count: &AtomicU32) {
        if value && self.is_detached() {
            return;
        }
        if flag.swap(value, Ordering::AcqRel) != value {
            if value {
                count.fetch_add(1, Ordering::Relaxed);
            } else {
                count.fetch_sub(1, Ordering::Relaxed);
            }
        }
        // A detach that ran meanwhile cleared the flag before this set it:
        // take it back out of the count.
        if value && self.is_detached() && flag.swap(false, Ordering::AcqRel) {
            count.fetch_sub(1, Ordering::Relaxed);
        }
    }

    fn detach(&self) {
        if self.detached.swap(true, Ordering::AcqRel) {
            return;
        }
        let counters = &self.source.shared.counters;
        counters.attachments.fetch_sub(1, Ordering::Relaxed);
        if self.observed.swap(false, Ordering::AcqRel) {
            counters.observed.fetch_sub(1, Ordering::Relaxed);
        }
        if self.paused.swap(false, Ordering::AcqRel) {
            counters.paused.fetch_sub(1, Ordering::Relaxed);
        }
        let waker = {
            let _rank = Ranked::acquire(Rank::Waker);
            self.waker
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
        };
        drop(waker);
    }
}

impl Drop for ConsumerShared {
    fn drop(&mut self) {
        self.detach();
    }
}

impl LiveImageConsumer {
    pub(crate) fn from_shared(shared: Arc<ConsumerShared>) -> Self {
        Self { shared }
    }

    /// The source it is attached to.
    pub fn source(&self) -> &LiveImageSource {
        &self.shared.source
    }

    /// The source's counters and this attachment's. Atomics only.
    pub fn stats(&self) -> LiveImageStats {
        let c = &self.shared.counters;
        LiveImageStats {
            source: self.shared.source.stats(),
            attachment: LiveImageAttachmentStats {
                window_generation: c.window_generation.load(Ordering::Relaxed),
                frames_drawn: c.frames_drawn.load(Ordering::Relaxed),
                captures: c.captures.load(Ordering::Relaxed),
                uploads: c.uploads.load(Ordering::Relaxed),
                deferred_frames: c.deferred_frames.load(Ordering::Relaxed),
                observed: self.shared.observed.load(Ordering::Relaxed),
                paused: self.shared.paused.load(Ordering::Relaxed),
                paused_frames: c.paused_frames.load(Ordering::Relaxed),
                paints: c.paints.load(Ordering::Relaxed),
            },
        }
    }

    /// Whether `self` and `other` are the same attachment.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }

    /// The layout pre-pass, before it reads the source's meta: take the
    /// change that woke it.
    #[doc(hidden)]
    pub fn take_geometry(&self) -> bool {
        self.shared.geometry.take()
    }

    /// Whether a status or size change waits for the pre-pass. Advisory: the
    /// UI thread deciding whether a wake needs a frame drawn, which takes it
    /// in either way.
    #[doc(hidden)]
    pub fn geometry_pending(&self) -> bool {
        self.shared.geometry.is_raised()
    }

    /// The layout pre-pass records the meta it laid out with: what
    /// `Canvas::draw_live_image` stamps a quad with.
    #[doc(hidden)]
    pub fn record_layout_meta(&self, meta: LiveMeta) {
        self.shared.record_layout_meta(meta);
    }

    #[doc(hidden)]
    pub fn layout_meta(&self) -> LiveMeta {
        LiveMeta::unpack(self.shared.layout_meta.load(Ordering::Relaxed))
    }

    /// Install the window's waker, then wake it for what was raised while
    /// there was none: such a raise found no waker and woke nobody.
    #[doc(hidden)]
    pub fn set_waker(&self, waker: Option<Arc<dyn RedrawWaker>>) {
        {
            let _rank = Ranked::acquire(Rank::Waker);
            *self
                .shared
                .waker
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = waker.clone();
        }
        let Some(waker) = waker else {
            return;
        };
        if self.shared.geometry.is_raised() {
            waker.wake(WakeKind::Layout);
        } else if self.shared.pixels.is_raised() || self.shared.handoff.is_raised() {
            waker.wake(WakeKind::Draw);
        }
    }

    /// Release the attachment: its widget is gone. No change reaches it
    /// again, the source drops it at its next commit or attach, and its
    /// window's waker is let go here. Idempotent.
    #[doc(hidden)]
    pub fn detach(&self) {
        self.shared.detach();
    }

    #[doc(hidden)]
    pub fn is_detached(&self) -> bool {
        self.shared.is_detached()
    }

    /// The live pass, for a quad drawn unpaused, before it reads the
    /// generation.
    pub(crate) fn take_pixels(&self) -> bool {
        self.shared.pixels.take()
    }

    /// The live pass, for a paused quad: commits merge into the raised flag
    /// instead of waking the window.
    pub(crate) fn park_pixels(&self) {
        self.shared.pixels.park();
    }

    pub(crate) fn set_observed(&self, observed: bool) {
        let count = &self.shared.source.shared.counters.observed;
        self.shared
            .set_counted(&self.shared.observed, observed, count);
    }

    pub(crate) fn set_paused(&self, paused: bool) {
        let count = &self.shared.source.shared.counters.paused;
        self.shared.set_counted(&self.shared.paused, paused, count);
    }

    /// `Canvas::draw_live_image`: one paint of the widget.
    pub(crate) fn count_paint(&self) {
        self.shared.counters.paints.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_render(&self, record: RenderRecord) {
        let c = &self.shared.counters;
        if record.capture {
            c.captures.fetch_add(1, Ordering::Relaxed);
        } else if record.drawn {
            c.frames_drawn.fetch_add(1, Ordering::Relaxed);
        }
        if record.uploaded {
            c.uploads.fetch_add(1, Ordering::Relaxed);
        }
        if record.deferred {
            c.deferred_frames.fetch_add(1, Ordering::Relaxed);
        }
        if record.kept && !record.capture {
            c.paused_frames.fetch_add(1, Ordering::Relaxed);
        }
        c.window_generation
            .store(record.window_generation, Ordering::Relaxed);
        // Presented frames only: a screenshot of a window nobody sees must not
        // tell a producer its frame was displayed.
        if !record.capture && record.drawn {
            self.shared
                .source
                .shared
                .raise_displayed(record.window_generation);
        }
    }

    /// Lock the source for the live pass without waiting. On contention it
    /// registers for the handoff, then tries once more: if that fails too,
    /// the unlock that ends the holder's hold wakes this consumer's window,
    /// whoever held it. A source this thread holds a transaction on is busy.
    ///
    /// The retry is what makes the handoff exact. Registering is two
    /// acquire-release swaps, this consumer's flag then the source's; every
    /// unlock swaps the source's flag back after releasing the lock. If that
    /// swap comes first in the flag's order, the registration reads it and
    /// so happens after the unlock, and the retry sees the lock released or
    /// taken by a later holder whose own unlock follows the registration. If
    /// the registration comes first, the unlock reads it, then this
    /// consumer's flag, and wakes it. No `SeqCst` is needed, and a failed
    /// `try_lock` being a `Relaxed` load is enough: coherence orders it after
    /// an unlock that happens before it.
    pub(crate) fn try_read(&self) -> ReadAttempt<'_> {
        let shared = &*self.shared.source.shared;
        if held::holds(shared.id) {
            shared.request_handoff(&self.shared);
            return ReadAttempt::Busy;
        }
        if let Some(guard) = shared.try_lock_state() {
            return ReadAttempt::Locked(LiveImageRead::new(guard));
        }
        shared.request_handoff(&self.shared);
        match shared.try_lock_state() {
            Some(guard) => {
                // Not needed after all: clear it so the next unlock does not
                // wake this window for nothing.
                self.shared.handoff.take();
                ReadAttempt::Locked(LiveImageRead::new(guard))
            }
            None => ReadAttempt::Busy,
        }
    }

    /// Lock the source, waiting at most `timeout` for the open transaction,
    /// parked so that its unlock hands the lock over. On timeout, as
    /// [`try_read`](Self::try_read).
    pub(crate) fn read_for(&self, timeout: Duration) -> ReadAttempt<'_> {
        let shared = &*self.shared.source.shared;
        if held::holds(shared.id) {
            shared.request_handoff(&self.shared);
            return ReadAttempt::Busy;
        }
        match shared.lock_state_for(timeout) {
            Some(guard) => ReadAttempt::Locked(LiveImageRead::new(guard)),
            None => self.try_read(),
        }
    }
}

impl PartialEq for LiveImageConsumer {
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other)
    }
}

impl Eq for LiveImageConsumer {}

impl std::fmt::Debug for LiveImageConsumer {
    /// Atomics only, like the source's.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.stats();
        write!(
            f,
            "LiveImageConsumer({:?}, window gen {}, paints {}{})",
            self.shared.source,
            s.attachment.window_generation,
            s.attachment.paints,
            if self.is_detached() { ", detached" } else { "" }
        )
    }
}
