// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`LiveImageSource`]: the shared state, the lock every access goes
//! through, writer sessions and the wakes a change sends.

use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use super::consumer::{ConsumerShared, LiveImageConsumer};
use super::damage::{DamageRing, Pending};
use super::held::{self, Rank, Ranked};
use super::lock::{Guard, Lock};
use super::meta::LiveMeta;
use super::stats::LiveImageSourceStats;
use super::writer::{LiveImageWriter, WriterToken};
use super::{LiveImageError, LiveImageId, LiveImageStatus, LivePixelFormat};
use crate::sync::{AtomicU32, AtomicU64, Mutex, Ordering};
use crate::wake::{RedrawWaker, WakeFlag, WakeKind};

/// A raster that producer threads rewrite and any number of windows show.
///
/// The first `Send` type in Teksilo that feeds the renderer: `Send + Sync`,
/// `Clone` is an `Arc` clone, and equality is identity. Producers write
/// through a [`LiveImageWriter`]; widgets show it with `LiveImage`, or with
/// [`Canvas::draw_live_image`](crate::Canvas::draw_live_image) in a widget of
/// one's own.
///
/// It holds one frame, latest wins. A commit that lands before a window has
/// shown the one before is merged into what that window uploads next, so a
/// producer faster than the display costs one upload per displayed frame.
/// Pixels never pass through `paint()` or an event: each window's renderer
/// pulls what it lacks from the source, under its lock.
#[derive(Clone)]
pub struct LiveImageSource {
    pub(crate) shared: Arc<Shared>,
}

/// Sets what is fixed at construction.
#[derive(Debug)]
#[must_use]
pub struct LiveImageSourceBuilder {
    format: LivePixelFormat,
    label: Option<String>,
    hint: Option<(u32, u32)>,
}

impl LiveImageSourceBuilder {
    /// A name for `Debug` output, stats, texture labels and automation.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// The size layout uses before any frame exists: a VM screen shows its
    /// portrait glass before the guest's first frame. A size
    /// [`set_size_hint`](LiveImageSource::set_size_hint) would refuse (a
    /// zero side, or one above the maximum) sets no hint.
    pub fn size_hint(mut self, width: u32, height: u32) -> Self {
        self.hint = validate_size(width, height).ok().map(|()| (width, height));
        self
    }

    pub fn build(self) -> LiveImageSource {
        LiveImageSource::with(self.format, self.label, self.hint)
    }
}

pub(crate) struct Shared {
    pub(crate) id: LiveImageId,
    pub(crate) label: Option<Box<str>>,
    pub(crate) format: LivePixelFormat,
    state: Lock<State>,
    /// [`LiveMeta`] packed: written under the state lock, read without it.
    meta: AtomicU32,
    /// The last published generation: written under the state lock.
    generation: AtomicU64,
    displayed_generation: AtomicU64,
    /// The attached consumers. Behind the state lock in the lock order.
    consumers: Mutex<Vec<Weak<ConsumerShared>>>,
    /// A consumer found the state lock busy and waits for an unlock to wake
    /// it: every unlock takes this, then the consumers' own flags.
    handoff: WakeFlag,
    /// Renderer reads waiting in a timed lock: an unlock then hands the lock
    /// to them instead of letting its producer take it straight back.
    parked: AtomicU32,
    pub(crate) counters: SourceCounters,
}

#[derive(Default)]
pub(crate) struct SourceCounters {
    pub(crate) commits: AtomicU64,
    pub(crate) abandoned: AtomicU64,
    pub(crate) bytes_written: AtomicU64,
    pub(crate) wakes: AtomicU64,
    pub(crate) wakes_coalesced: AtomicU64,
    /// Mirrors `State::writers`, for lock-free stats.
    pub(crate) writers: AtomicU32,
    pub(crate) attachments: AtomicU32,
    pub(crate) observed: AtomicU32,
    pub(crate) paused: AtomicU32,
}

/// Everything behind the state lock.
pub(crate) struct State {
    /// The frame, `stride` bytes per row. Empty without a buffer.
    pub(crate) pixels: Vec<u8>,
    pub(crate) size: Option<(u32, u32)>,
    pub(crate) stride: usize,
    /// The size layout uses while there is no buffer.
    pub(crate) hint: Option<(u32, u32)>,
    pub(crate) status: LiveImageStatus,
    /// Live writer tokens of the current session.
    pub(crate) writers: u32,
    pub(crate) session: u64,
    pub(crate) generation: u64,
    pub(crate) ring: DamageRing,
    pub(crate) pending: Pending,
}

impl State {
    fn meta(&self) -> LiveMeta {
        LiveMeta {
            status: self.status,
            size: self.size.or(self.hint),
        }
    }

    /// Free the pixels and keep their size as the hint. The next commit
    /// uploads in full.
    pub(crate) fn free(&mut self) {
        if let Some(size) = self.size.take() {
            self.hint = Some(size);
        }
        self.pixels = Vec::new();
        self.stride = 0;
        self.pending = Pending::new();
        self.pending.force_full = true;
    }

    /// Zero-fill a `width × height` buffer of `bpp` bytes per pixel,
    /// reusing capacity. A no-op at the current size. On failure nothing
    /// changed.
    pub(crate) fn resize(
        &mut self,
        width: u32,
        height: u32,
        bpp: usize,
    ) -> Result<(), LiveImageError> {
        validate_size(width, height)?;
        if self.size == Some((width, height)) {
            return Ok(());
        }
        let stride = width as usize * bpp;
        let needed = stride * height as usize;
        if needed > self.pixels.capacity() {
            reserve(&mut self.pixels, needed)?;
        }
        self.pixels.clear();
        self.pixels.resize(needed, 0);
        self.stride = stride;
        self.size = Some((width, height));
        self.pending.resized = true;
        self.pending.force_full = true;
        Ok(())
    }

    /// Publish what is pending: `Some(generation)` for a commit that
    /// published, `None` for an empty one.
    pub(crate) fn publish(&mut self, now: Instant) -> Option<u64> {
        if !self.pending.publishes() {
            return None;
        }
        let damage = if self.pending.force_full || self.pending.resized {
            super::damage::Damage::Full
        } else {
            self.pending.damage
        };
        if self.status == LiveImageStatus::Waiting && self.pending.wrote {
            self.status = LiveImageStatus::Live;
        }
        self.generation += 1;
        let (w, h) = self.size.unwrap_or((0, 0));
        self.ring.record(self.generation, damage, w, h, now);
        self.pending = Pending::new();
        Some(self.generation)
    }
}

/// Grow `pixels`' capacity to at least `needed` bytes, or report it.
fn reserve(pixels: &mut Vec<u8>, needed: usize) -> Result<(), LiveImageError> {
    let out_of_memory = || LiveImageError::OutOfMemory {
        bytes: needed as u64,
    };
    #[cfg(all(test, not(teksilo_loom)))]
    if test_hooks::take_reserve_failure() {
        return Err(out_of_memory());
    }
    pixels
        .try_reserve_exact(needed - pixels.len())
        .map_err(|_| out_of_memory())
}

/// `ZeroSize` or `TooLarge` for a frame no source can hold.
pub(crate) fn validate_size(width: u32, height: u32) -> Result<(), LiveImageError> {
    if width == 0 || height == 0 {
        return Err(LiveImageError::ZeroSize { width, height });
    }
    let bytes = u64::from(width) * u64::from(height) * 4;
    if width > LiveImageSource::MAX_DIMENSION
        || height > LiveImageSource::MAX_DIMENSION
        || bytes > LiveImageSource::MAX_BYTES as u64
    {
        return Err(LiveImageError::TooLarge { width, height });
    }
    Ok(())
}

/// The state lock, held. Every acquisition of the lock is one of these, and
/// every release runs the handoff: a consumer that found the lock busy is
/// woken by the unlock that follows, whoever held it.
pub(crate) struct StateGuard<'a> {
    guard: Option<Guard<'a, State>>,
    rank: Option<Ranked>,
    shared: &'a Shared,
}

impl<'a> StateGuard<'a> {
    pub(crate) fn state(&self) -> &State {
        self.guard.as_ref().expect("held until drop").get()
    }

    pub(crate) fn state_mut(&mut self) -> &mut State {
        self.guard.as_mut().expect("held until drop").get_mut()
    }

    pub(crate) fn shared(&self) -> &'a Shared {
        self.shared
    }
}

impl Drop for StateGuard<'_> {
    fn drop(&mut self) {
        let fair = self.shared.parked.load(Ordering::Relaxed) > 0;
        if let Some(guard) = self.guard.take() {
            guard.unlock(fair);
        }
        drop(self.rank.take());
        self.shared.run_handoff();
    }
}

impl Shared {
    fn wrap<'a>(&'a self, guard: Guard<'a, State>, rank: Ranked) -> StateGuard<'a> {
        StateGuard {
            guard: Some(guard),
            rank: Some(rank),
            shared: self,
        }
    }

    /// Wait for the lock. Producer paths check [`Self::assert_not_held`] first.
    pub(crate) fn lock_state(&self) -> StateGuard<'_> {
        let rank = Ranked::acquire(Rank::State);
        let guard = self.state.lock();
        self.wrap(guard, rank)
    }

    pub(crate) fn try_lock_state(&self) -> Option<StateGuard<'_>> {
        let rank = Ranked::acquire(Rank::State);
        let guard = self.state.try_lock()?;
        Some(self.wrap(guard, rank))
    }

    /// Wait at most `timeout`, parked so that the holder's unlock hands the
    /// lock over.
    pub(crate) fn lock_state_for(&self, timeout: Duration) -> Option<StateGuard<'_>> {
        let rank = Ranked::acquire(Rank::State);
        self.parked.fetch_add(1, Ordering::Relaxed);
        let guard = self.state.try_lock_for(timeout);
        self.parked.fetch_sub(1, Ordering::Relaxed);
        Some(self.wrap(guard?, rank))
    }

    /// Panic when this thread holds a write transaction on the source: the
    /// lock is not reentrant, so `what` would wait for itself forever.
    pub(crate) fn assert_not_held(&self, what: &str) {
        if held::holds(self.id) {
            panic!(
                "{what} on {} while this thread holds a write transaction on it: \
                 commit or drop the transaction first (the lock is not reentrant)",
                self.describe()
            );
        }
    }

    pub(crate) fn describe(&self) -> String {
        match &self.label {
            Some(label) => format!("live image {} {label:?}", self.id),
            None => format!("live image {}", self.id),
        }
    }

    pub(crate) fn meta(&self) -> LiveMeta {
        LiveMeta::unpack(self.meta.load(Ordering::Acquire))
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub(crate) fn raise_displayed(&self, generation: u64) {
        self.displayed_generation
            .fetch_max(generation, Ordering::Relaxed);
    }

    /// Store `state`'s meta if it changed: whether it did. Under the lock.
    pub(crate) fn publish_meta(&self, state: &State) -> bool {
        let word = state.meta().pack();
        word != self.meta.swap(word, Ordering::AcqRel)
    }

    /// Store a published generation. Under the lock.
    pub(crate) fn publish_generation(&self, generation: u64) {
        self.generation.store(generation, Ordering::Release);
        self.counters.commits.fetch_add(1, Ordering::Relaxed);
    }

    /// The consumers not yet detached, pruning the rest. Takes the list
    /// lock briefly; nothing else is locked or called while it is held.
    pub(crate) fn live_consumers(&self) -> Vec<Arc<ConsumerShared>> {
        let _rank = Ranked::acquire(Rank::List);
        let mut list = self
            .consumers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        list.retain(|weak| weak.upgrade().is_some_and(|c| !c.is_detached()));
        list.iter().filter_map(Weak::upgrade).collect()
    }

    pub(crate) fn register(&self, consumer: &Arc<ConsumerShared>) {
        let _rank = Ranked::acquire(Rank::List);
        let mut list = self
            .consumers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Prune here too: a one-commit source attached on every rebuild and
        // never committed again would otherwise grow its list without bound.
        list.retain(|weak| weak.upgrade().is_some_and(|c| !c.is_detached()));
        list.push(Arc::downgrade(consumer));
    }

    /// After an unlock: wake every consumer that found the lock busy since
    /// the last unlock that looked. See `LiveImageConsumer::try_read`.
    fn run_handoff(&self) {
        if !self.handoff.take() {
            return;
        }
        let mut wakers = Wakers::default();
        for consumer in self.live_consumers() {
            if consumer.handoff.take()
                && let Some(waker) = consumer.waker()
            {
                wakers.add(waker, WakeKind::Draw);
            }
        }
        wakers.wake(&self.counters);
    }

    /// A consumer registers for the handoff: the next unlock wakes it.
    pub(crate) fn request_handoff(&self, consumer: &ConsumerShared) {
        let _ = consumer.handoff.raise();
        let _ = self.handoff.raise();
    }

    /// After an unlock: tell the consumers what changed. `pixels`: a commit
    /// published. `meta`: the status or the size changed.
    pub(crate) fn notify(&self, pixels: bool, meta: bool) {
        if !pixels && !meta {
            return;
        }
        let consumers = self.live_consumers();
        if consumers.is_empty() {
            return;
        }
        let mut wakers = Wakers::default();
        let mut raised = false;
        for consumer in &consumers {
            let mut kind = None;
            if pixels && consumer.pixels.raise() {
                kind = Some(WakeKind::Draw);
            }
            if meta && consumer.geometry.raise() {
                kind = Some(WakeKind::Layout);
            }
            let Some(kind) = kind else {
                continue;
            };
            raised = true;
            if let Some(waker) = consumer.waker() {
                wakers.add(waker, kind);
            }
        }
        if !raised {
            self.counters
                .wakes_coalesced
                .fetch_add(1, Ordering::Relaxed);
        }
        wakers.wake(&self.counters);
    }

    /// Writer session `session`'s last token dropped. Off a thread holding
    /// a transaction on the source, it locks; on one, the transaction
    /// applies it as it ends.
    pub(crate) fn release_session(&self, session: u64) {
        if held::defer_release(self.id, session) {
            return;
        }
        let mut guard = self.lock_state();
        self.apply_release(guard.state_mut(), session);
        let meta = self.publish_meta(guard.state());
        drop(guard);
        self.notify(false, meta);
    }

    /// Under the lock: one token of `session` is gone. The session's last
    /// frees the pixels and disconnects; a revoked session's changes
    /// nothing.
    pub(crate) fn apply_release(&self, state: &mut State, session: u64) {
        if state.session != session {
            return;
        }
        state.writers -= 1;
        self.counters
            .writers
            .store(state.writers, Ordering::Relaxed);
        if state.writers == 0 {
            state.free();
            state.status = LiveImageStatus::Disconnected;
        }
    }
}

/// The windows to wake for one change: each once, with the larger kind.
#[derive(Default)]
pub(crate) struct Wakers(Vec<(Arc<dyn RedrawWaker>, WakeKind)>);

impl Wakers {
    pub(crate) fn add(&mut self, waker: Arc<dyn RedrawWaker>, kind: WakeKind) {
        let data = Arc::as_ptr(&waker) as *const ();
        match self
            .0
            .iter_mut()
            .find(|(other, _)| Arc::as_ptr(other) as *const () == data)
        {
            Some((_, other)) => *other = (*other).max(kind),
            None => self.0.push((waker, kind)),
        }
    }

    /// Call them, with no lock held. A waker that panics while this thread
    /// is already unwinding (a writer dropped by a panic) is contained: a
    /// second panic would abort.
    pub(crate) fn wake(self, counters: &SourceCounters) {
        for (waker, kind) in self.0 {
            if std::thread::panicking() {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake(kind)));
            } else {
                waker.wake(kind);
            }
            counters.wakes.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl LiveImageSource {
    /// The largest side a source accepts. Each renderer also checks its
    /// device's own limit, and draws nothing for a larger source.
    pub const MAX_DIMENSION: u32 = 16_384;

    /// The largest frame a source accepts: 1 GiB, `MAX_DIMENSION² × 4`, so
    /// only a side can be too large. A renderer uploads a frame larger than
    /// its staging budget over several frames, showing the previous picture
    /// until the new one is complete.
    pub const MAX_BYTES: usize = 1 << 30;

    /// A source with no label, no size hint and no writer: `Disconnected`.
    pub fn new(format: LivePixelFormat) -> Self {
        Self::with(format, None, None)
    }

    pub fn builder(format: LivePixelFormat) -> LiveImageSourceBuilder {
        LiveImageSourceBuilder {
            format,
            label: None,
            hint: None,
        }
    }

    fn with(format: LivePixelFormat, label: Option<String>, hint: Option<(u32, u32)>) -> Self {
        let state = State {
            pixels: Vec::new(),
            size: None,
            stride: 0,
            hint,
            status: LiveImageStatus::Disconnected,
            writers: 0,
            session: 0,
            generation: 0,
            ring: DamageRing::new(),
            pending: Pending::new(),
        };
        let meta = state.meta().pack();
        Self {
            shared: Arc::new(Shared {
                id: LiveImageId::next(),
                label: label.map(String::into_boxed_str),
                format,
                state: Lock::new(state),
                meta: AtomicU32::new(meta),
                generation: AtomicU64::new(0),
                displayed_generation: AtomicU64::new(0),
                consumers: Mutex::new(Vec::new()),
                handoff: WakeFlag::new(),
                parked: AtomicU32::new(0),
                counters: SourceCounters::default(),
            }),
        }
    }

    /// Join the current writer session. The first writer of a session moves
    /// `Disconnected` to `Waiting`. Use it for threads that cooperate on one
    /// stream; a stream that restarts takes
    /// [`writer_exclusive`](Self::writer_exclusive).
    ///
    /// # Panics
    ///
    /// When this thread holds a write transaction on the source.
    pub fn writer(&self) -> LiveImageWriter {
        self.shared.assert_not_held("LiveImageSource::writer");
        let mut guard = self.shared.lock_state();
        let state = guard.state_mut();
        if state.writers == 0 {
            state.status = LiveImageStatus::Waiting;
            state.pending.force_full = true;
        }
        state.writers += 1;
        let session = state.session;
        self.shared
            .counters
            .writers
            .store(state.writers, Ordering::Relaxed);
        let meta = self.shared.publish_meta(guard.state());
        drop(guard);
        self.shared.notify(false, meta);
        LiveImageWriter::new(WriterToken::new(self.clone(), session))
    }

    /// Start a new writer session: revoke every existing writer, free the
    /// pixels, set `Waiting`, and return the session's first writer. A
    /// revoked writer's calls return [`LiveImageError::Revoked`] without
    /// touching anything, so a task left over from an old stream cannot
    /// write into the new one. Waits for an open transaction to end.
    ///
    /// # Panics
    ///
    /// When this thread holds a write transaction on the source.
    pub fn writer_exclusive(&self) -> LiveImageWriter {
        self.shared
            .assert_not_held("LiveImageSource::writer_exclusive");
        let mut guard = self.shared.lock_state();
        let state = guard.state_mut();
        state.session += 1;
        state.writers = 1;
        state.free();
        state.status = LiveImageStatus::Waiting;
        let session = state.session;
        self.shared.counters.writers.store(1, Ordering::Relaxed);
        let meta = self.shared.publish_meta(guard.state());
        drop(guard);
        self.shared.notify(false, meta);
        LiveImageWriter::new(WriterToken::new(self.clone(), session))
    }

    /// The size layout uses while no buffer exists, shared by every clone.
    ///
    /// # Panics
    ///
    /// When this thread holds a write transaction on the source.
    pub fn set_size_hint(&self, width: u32, height: u32) -> Result<(), LiveImageError> {
        validate_size(width, height)?;
        self.shared
            .assert_not_held("LiveImageSource::set_size_hint");
        let mut guard = self.shared.lock_state();
        guard.state_mut().hint = Some((width, height));
        let meta = self.shared.publish_meta(guard.state());
        drop(guard);
        self.shared.notify(false, meta);
        Ok(())
    }

    /// Process-unique; keys each renderer's texture cache.
    pub fn id(&self) -> LiveImageId {
        self.shared.id
    }

    /// Fixed at construction.
    pub fn label(&self) -> Option<&str> {
        self.shared.label.as_deref()
    }

    pub fn format(&self) -> LivePixelFormat {
        self.shared.format
    }

    /// Lock-free.
    pub fn status(&self) -> LiveImageStatus {
        self.shared.meta().status
    }

    /// The buffer's size, else the size hint. Lock-free, and may be ahead of
    /// what widgets laid out.
    pub fn size(&self) -> Option<(u32, u32)> {
        self.shared.meta().size
    }

    /// Lock-free and monotonic: +1 per commit that published.
    pub fn generation(&self) -> u64 {
        self.shared.generation()
    }

    /// The highest generation a window has presented. A producer that wants
    /// backpressure compares it with [`generation`](Self::generation).
    pub fn displayed_generation(&self) -> u64 {
        self.shared.displayed_generation.load(Ordering::Relaxed)
    }

    /// Whether at least one widget showing the source was in its window's
    /// last render, whatever the status: a placeholder counts.
    pub fn is_observed(&self) -> bool {
        self.shared.counters.observed.load(Ordering::Relaxed) > 0
    }

    /// Whether at least one widget drew the source unpaused in its window's
    /// last render, in a window not known to be hidden. A producer can
    /// throttle while it is `false`. Takes the consumer-list lock briefly.
    /// On Wayland a minimised window still counts: its compositor stops its
    /// frames, but no window state says so.
    pub fn is_displayed(&self) -> bool {
        self.shared
            .live_consumers()
            .iter()
            .any(|c| c.is_displayed())
    }

    /// Counters read from atomics; never takes a lock.
    pub fn stats(&self) -> LiveImageSourceStats {
        let c = &self.shared.counters;
        LiveImageSourceStats {
            generation: self.generation(),
            displayed_generation: self.displayed_generation(),
            commits: c.commits.load(Ordering::Relaxed),
            abandoned: c.abandoned.load(Ordering::Relaxed),
            bytes_written: c.bytes_written.load(Ordering::Relaxed),
            wakes: c.wakes.load(Ordering::Relaxed),
            wakes_coalesced: c.wakes_coalesced.load(Ordering::Relaxed),
            writers: c.writers.load(Ordering::Relaxed),
            attachments: c.attachments.load(Ordering::Relaxed),
            observed_attachments: c.observed.load(Ordering::Relaxed),
            paused_attachments: c.paused.load(Ordering::Relaxed),
        }
    }

    /// Whether `self` and `other` are the same source.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }

    /// What the packed meta word holds: one load, no lock.
    #[doc(hidden)]
    pub fn meta(&self) -> LiveMeta {
        self.shared.meta()
    }

    /// Attach a consumer: one widget in one window, woken through `waker`.
    /// Register-then-read: the consumer is published to the source before
    /// the meta it records is read, so a change made meanwhile either is in
    /// that meta or raises the consumer's flag. Its flags start clear.
    #[doc(hidden)]
    pub fn attach(&self, waker: Option<Arc<dyn RedrawWaker>>) -> LiveImageConsumer {
        let consumer = Arc::new(ConsumerShared::new(self.clone(), waker));
        self.shared.register(&consumer);
        self.shared
            .counters
            .attachments
            .fetch_add(1, Ordering::Relaxed);
        consumer.record_layout_meta(self.shared.meta());
        LiveImageConsumer::from_shared(consumer)
    }
}

impl PartialEq for LiveImageSource {
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other)
    }
}

impl Eq for LiveImageSource {}

impl std::hash::Hash for LiveImageSource {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.shared.id.hash(state);
    }
}

impl std::fmt::Debug for LiveImageSource {
    /// Atomics only, so it never waits on a producer:
    /// `LiveImageSource(#3 "vm-screen" Bgrx8 720x1280 Live gen 5021)`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let meta = self.shared.meta();
        write!(f, "LiveImageSource({}", self.shared.id)?;
        if let Some(label) = &self.shared.label {
            write!(f, " {label:?}")?;
        }
        write!(f, " {:?}", self.shared.format)?;
        match meta.size {
            Some((w, h)) => write!(f, " {w}x{h}")?,
            None => write!(f, " no size")?,
        }
        write!(f, " {:?} gen {})", meta.status, self.generation())
    }
}

#[cfg(all(test, not(teksilo_loom)))]
pub(crate) mod test_hooks {
    use std::cell::Cell;

    std::thread_local! {
        static FAIL_RESERVE: Cell<bool> = const { Cell::new(false) };
    }

    /// Make this thread's next buffer growth report `OutOfMemory`.
    pub(crate) fn fail_next_reserve() {
        FAIL_RESERVE.with(|f| f.set(true));
    }

    pub(super) fn take_reserve_failure() -> bool {
        FAIL_RESERVE.with(|f| f.replace(false))
    }
}
