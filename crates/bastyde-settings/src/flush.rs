// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Debounced, **cross-process-safe** atomic file writer.
//!
//! `DebouncedWriter` accepts [`Patch`]es — *replayable mutations* — via
//! [`schedule`](DebouncedWriter::schedule), batches rapid bursts inside a
//! debounce window, and then applies the whole batch to the document **read from
//! disk under an exclusive advisory lock**, writing the result atomically
//! (write-temp + fsync + rename).
//!
//! ## Why a patch and not a rendered string
//!
//! This writer used to carry a pre-rendered `String`: the caller serialised its
//! entire in-memory document and the worker blindly wrote those bytes. That is
//! **last-write-wins by construction** — the worker had nothing to merge *with*,
//! so any concurrent change a peer process made to another part of the file was
//! silently destroyed. (A lock alone does not fix this: it serialises the two
//! writes but does nothing about the stale snapshot one of them was rendered
//! from.)
//!
//! A `Patch` instead says "given the file's current text, produce its new text",
//! so the merge happens against reality:
//!
//! ```text
//!   lock  ->  read current  ->  apply queued patches  ->  write atomically  ->  unlock
//! ```
//!
//! Patches are built inside this crate from *owned* snapshots of each type's
//! pending mutations (a `Vec<(key, value)>`, a list of ops), so they capture no
//! `Rc` and can cross to the worker thread. Callers never see one: they keep
//! writing `signal.set(v)`, `mru.add(e)`, `file.mutate(|s| ..)`.
//!
//! ## Single shared I/O thread
//!
//! All `DebouncedWriter`s in a process share **one** background I/O thread
//! (lazily started on first use). Each writer registers under a unique
//! [`WriterId`]. The shared thread:
//!
//! * keeps a per-id `(deadline, patch queue)`,
//! * blocks on the next-due deadline (or waits for a message if nothing is
//!   pending),
//! * coalesces rapid `Schedule` bursts by **appending** to the queue,
//!   resetting the failure streak (a just-queued patch has never itself
//!   failed to write) and moving the deadline **forward, never backward** —
//!   so debouncing collapses *writes*, never *mutations*, and a live
//!   `RETRY_BACKOFF` deadline installed after a failed attempt can't be
//!   clobbered back to "now" by an unrelated new patch on a zero-delay
//!   writer. (The old design could overwrite the pending payload precisely
//!   because each payload was a complete, self-superseding rendering.)
//!
//! A failed write **retains** the queue and retries with backoff, up to
//! [`MAX_WRITE_ATTEMPTS`] — the patches replay cleanly against whatever is on
//! disk then, which is the correct merge rather than a stale overwrite. Once
//! the cap is reached (or a writer is dropped mid-failure at process
//! teardown), the queue is discarded for good and reported through the
//! process-wide [`WriteFailureSink`] (registered via
//! [`set_write_failure_sink`]) in addition to the existing log — the write
//! side's analogue of [`crate::reload::Reloadable`]'s read-side contract.
//! Conversely, every writer may also register a [`WriteLandedSink`] (via
//! [`DebouncedWriter::set_landed_sink`]) to learn the *real* on-disk stamp
//! the instant its queued patches land successfully — useful to a caller
//! whose own `apply()`-style API schedules a write and returns before it's
//! actually on disk.
//!
//! The locked read-merge-write ([`apply_and_write`]) acquires its advisory
//! lock **non-blocking**: because every writer in the process shares this
//! one thread, a lock held by a peer process must never stall it — a
//! contended lock is just another transient [`FlushError::Io`], retried
//! with the same backoff as any other write failure.
//!
//! Application logic stays single-threaded — `SettingsStore` and friends never
//! block on I/O. `Drop` sends an `Unregister` that synchronously flushes the
//! queue before returning, so end-of-process state is never lost (unless the
//! flush itself is still failing, in which case the discard is reported
//! through `WriteFailureSink` exactly as above).
//!
//! ## Why one thread, not one-per-writer
//!
//! An app that opens the K/V store + recents + window state already has 3
//! writers; a richer app might have 5–10, each idle ~99% of the time. One shared
//! worker is leaner and has identical semantics from the caller's point of view.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use tempfile::NamedTempFile;

/// Errors surfaced by [`DebouncedWriter::flush_now`].
#[derive(Debug, thiserror::Error)]
pub enum FlushError {
    /// The shared I/O worker thread has panicked or shut down; writes
    /// can no longer be delivered.
    #[error("settings I/O thread disconnected")]
    Disconnected,
    /// The atomic write (temp-file + rename) failed at the OS level.
    #[error("settings flush failed: {0}")]
    Io(#[from] io::Error),
    /// A `Patch` could not be applied to the document currently on
    /// disk — e.g. a peer wrote something this process cannot parse or
    /// migrate.
    #[error("settings merge failed: {0}")]
    Merge(String),
}

/// A **replayable** mutation of a settings file.
///
/// "Given the file's current raw text on disk (`None` if it does not
/// exist yet), produce its new raw text."
///
/// This is the heart of cross-process correctness. The worker applies a
/// patch to the document it *just read, under the lock* — never to a
/// snapshot this process cached minutes ago — so a peer's concurrent
/// changes to other parts of the document survive. Contrast the previous
/// design, where the payload was a pre-rendered `String`: the worker had
/// nothing to merge *with*, so every write was a blind whole-document
/// overwrite, i.e. last-write-wins.
///
/// ## Why `Fn`, not `FnOnce`
///
/// A patch may need to be applied more than once: if the write fails
/// (disk full, a transient network mount), the queue is **retained** and
/// replayed on the next tick against whatever is on disk *then*. That
/// re-application is exactly the right merge, and it is only possible if
/// a patch can be called again. `FnOnce` would force us to either drop
/// the mutation (data loss) or cache a pre-rendered string (defeating
/// the merge).
///
/// ## Why this never leaks into the public API
///
/// Patches are built **inside** this crate by each persisted type, from
/// *owned snapshots* of its own pending mutations (a `Vec<(key, value)>`,
/// a list of ops). They therefore capture no `Rc`, which is what lets
/// them cross to the writer thread. Callers keep writing
/// `signal.set(v)` / `mru.add(e)` / `file.mutate(|s| ...)` and never see
/// a `Patch`.
pub(crate) type Patch = Box<dyn Fn(Option<String>) -> Result<String, FlushError> + Send + 'static>;

/// How many times a failing write is retried before the queued patches
/// are dropped (with a loud log). Without a cap, a permanently
/// unwritable file (read-only mount, revoked permissions) would spin the
/// worker forever.
const MAX_WRITE_ATTEMPTS: u32 = 5;

/// Backoff floor between write retries, so a failing disk does not spin
/// the worker at the debounce interval.
const RETRY_BACKOFF: Duration = Duration::from_millis(250);

/// Invoked (off the caller's thread — on the shared worker thread) when a
/// `DebouncedWriter`'s queued patches are **permanently** discarded: either
/// `flush_writer` gave up after `MAX_WRITE_ATTEMPTS`, or the writer was
/// dropped (`Unregister`) while its final flush was still failing. This is
/// the write-side analogue of [`crate::reload::Reloadable`]'s read-side
/// contract — the previous behaviour was a bare `eprintln!` that never left
/// the worker thread, so a permanently unwritable settings file (read-only
/// mount, revoked permissions, disk full) silently ate every change for the
/// rest of the session with zero signal to the application. Registered
/// process-wide via [`set_write_failure_sink`].
pub type WriteFailureSink = Arc<dyn Fn(PathBuf, u32, usize, String) + Send + Sync + 'static>;

/// Register a process-wide sink invoked whenever any `DebouncedWriter`
/// permanently discards a queued write (see [`WriteFailureSink`]). There is
/// only one slot: a later call replaces an earlier one. `bastyde-app` uses
/// this to forward the failure to the UI thread as a typed `AppEvent`.
pub fn set_write_failure_sink(sink: WriteFailureSink) {
    let _ = pool().send(PoolMsg::SetFailureSink(sink));
}

/// The `(mtime, len)` stamp `disk_stamp` computes for a settings file —
/// named so every `Arc<Mutex<...>>` wrapping it (here and in
/// `WindowStateService`) reads as one term instead of clippy's
/// `type_complexity`-tripping nested-generics spelling.
pub type LandedStamp = (Option<SystemTime>, Option<u64>);

/// Invoked on the shared worker thread the instant a `DebouncedWriter`'s
/// queued patches land successfully, with the fresh on-disk `(mtime, len)`
/// stamp (one extra `fs::metadata`, computed once, right after the write —
/// negligible cost). The write-side analogue of `WriteFailureSink`. `Send +
/// Sync` because it runs off the caller's thread — a consumer that needs to
/// update `!Send` state (an `Rc<Cell<_>>`) must copy the value out on its
/// own thread the next time it looks (see
/// `WindowStateService::reload_from_disk`).
pub type WriteLandedSink = Arc<dyn Fn(LandedStamp) + Send + Sync + 'static>;

// ---------------------------------------------------------------------------
// Shared worker pool
// ---------------------------------------------------------------------------

/// Opaque per-writer identifier minted by [`next_writer_id`]. Used by
/// the shared worker thread to key pending payloads.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct WriterId(u64);

fn next_writer_id() -> WriterId {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    WriterId(COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Messages exchanged with the shared worker thread.
enum PoolMsg {
    Register {
        id: WriterId,
        path: PathBuf,
        delay: Duration,
    },
    Schedule {
        id: WriterId,
        patch: Patch,
    },
    FlushNow {
        id: WriterId,
        ack: SyncSender<Result<(), FlushError>>,
    },
    Unregister {
        id: WriterId,
        ack: SyncSender<()>,
    },
    /// Register the process-wide sink invoked when a write is permanently
    /// discarded (see [`WriteFailureSink`]). Replaces any previous sink.
    SetFailureSink(WriteFailureSink),
    /// Register `id`'s sink invoked with the fresh on-disk stamp whenever
    /// its queued patches land successfully (see [`WriteLandedSink`]).
    /// Replaces any previous sink for the same `id`.
    SetLandedSink {
        id: WriterId,
        sink: WriteLandedSink,
    },
}

/// A writer's queued-but-not-yet-written mutations.
struct Pending {
    deadline: Instant,
    /// **A queue, not a slot.** Patches are deltas, so a later one does
    /// not supersede an earlier one — dropping any of them silently
    /// loses a mutation. (The previous design could overwrite the
    /// pending payload precisely because each payload was a complete
    /// whole-document rendering.) Debouncing still coalesces *writes*:
    /// the whole queue is applied in one locked read-merge-write.
    patches: Vec<Patch>,
    /// Consecutive failed write attempts, for backoff + the drop cap.
    attempts: u32,
}

struct PoolState {
    delays: HashMap<WriterId, Duration>,
    paths: HashMap<WriterId, PathBuf>,
    pending: HashMap<WriterId, Pending>,
    /// Process-wide sink for permanently-discarded writes (F3). At most
    /// one at a time — a later `SetFailureSink` replaces an earlier one.
    failure_sink: Option<WriteFailureSink>,
    /// Per-writer sinks for successful-flush stamps (F11 /
    /// `WriteLandedSink`), keyed by the same `WriterId` the writer was
    /// registered under.
    landed_sinks: HashMap<WriterId, WriteLandedSink>,
}

/// Lazily-started shared I/O worker. Returns the sender side of the
/// pool channel. The receiver lives inside the worker thread and the
/// thread itself is detached: the process exit teardown closes any
/// in-flight resources, but every `DebouncedWriter::Drop` synchronously
/// flushes its own payload first via the `Unregister` ack, so no data
/// is lost in normal teardown.
fn pool() -> &'static Sender<PoolMsg> {
    static POOL: OnceLock<Sender<PoolMsg>> = OnceLock::new();
    POOL.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("bastyde-settings-writer".into())
            .spawn(move || worker_loop(rx))
            .expect("bastyde-settings: failed to spawn writer thread");
        tx
    })
}

/// Handle a `Schedule` message: append `patch` to `id`'s queue, resetting
/// the failure streak and (monotonically) extending the deadline forward.
/// Factored out of `worker_loop`'s match arm so tests can drive exactly
/// this logic without going through the channel/thread machinery.
fn apply_schedule(state: &mut PoolState, id: WriterId, patch: Patch) {
    let delay = state.delays.get(&id).copied().unwrap_or(Duration::ZERO);
    let slot = state.pending.entry(id).or_insert_with(|| Pending {
        deadline: Instant::now() + delay,
        patches: Vec::new(),
        attempts: 0,
    });
    // APPEND. A patch is a delta; replacing the slot (as the
    // old whole-document design did) would silently drop the
    // earlier mutation.
    slot.patches.push(patch);
    // New work resets the failure streak: the patch just
    // appended has never itself failed to write, and
    // MAX_WRITE_ATTEMPTS is meant to police a *persistent*
    // failure with no new work arriving — the plain
    // per-tick retry path (the `Err(RecvTimeoutError::Timeout)`
    // arm in `worker_loop`) still enforces that cap unaffected by
    // this reset, since it only re-flushes ids already in
    // `pending` without going through `Schedule` again.
    slot.attempts = 0;
    // The deadline only ever moves *forward*, never back.
    // Ordinary debounce coalescing during an active burst is
    // unaffected (the slot's deadline was already `<= now +
    // delay` in that case, since it was armed by an earlier
    // `Schedule` in the same burst), but a zero-delay writer
    // that just had `flush_writer` install a future
    // `RETRY_BACKOFF` deadline after a failed attempt can no
    // longer have that backoff clobbered back to `now` by an
    // unrelated new `Schedule` arriving before the backoff
    // elapses.
    slot.deadline = slot.deadline.max(Instant::now() + delay);
}

fn worker_loop(rx: Receiver<PoolMsg>) {
    let mut state = PoolState {
        delays: HashMap::new(),
        paths: HashMap::new(),
        pending: HashMap::new(),
        failure_sink: None,
        landed_sinks: HashMap::new(),
    };

    loop {
        // Compute how long to wait. With no pending payload we block
        // indefinitely on the next message; otherwise we wait until
        // the next-due deadline (or for a new message, whichever
        // comes first).
        let next_deadline = state.pending.values().map(|p| p.deadline).min();
        let recv_result = match next_deadline {
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            Some(d) => {
                let wait = d.saturating_duration_since(Instant::now());
                rx.recv_timeout(wait)
            }
        };

        match recv_result {
            Ok(PoolMsg::Register { id, path, delay }) => {
                state.delays.insert(id, delay);
                state.paths.insert(id, path);
            }
            Ok(PoolMsg::Schedule { id, patch }) => apply_schedule(&mut state, id, patch),
            Ok(PoolMsg::FlushNow { id, ack }) => {
                let _ = ack.send(flush_writer(&mut state, id));
            }
            Ok(PoolMsg::Unregister { id, ack }) => {
                if let Err(e) = flush_writer(&mut state, id) {
                    let path = state.paths.get(&id).cloned();
                    let path_str = path.as_ref().map(|p| p.display().to_string());
                    eprintln!(
                        "bastyde-settings: final flush of {} failed: {e}",
                        path_str.as_deref().unwrap_or("<unknown>"),
                    );
                    // If `flush_writer`'s own give-up branch already fired
                    // (MAX_WRITE_ATTEMPTS reached), it already removed
                    // `pending` and invoked `failure_sink` itself — no
                    // entry remains here, so nothing more to report.
                    // Otherwise this `Unregister` is the *second* discard
                    // site F3 calls out: process teardown can't wait for
                    // further retries, so it forces the drop here, below
                    // the attempt cap, and must report it itself.
                    if let Some(pending) = state.pending.get(&id) {
                        let attempts = pending.attempts;
                        let dropped = pending.patches.len();
                        if let Some(sink) = &state.failure_sink {
                            sink(path.unwrap_or_default(), attempts, dropped, e.to_string());
                        }
                    }
                }
                state.pending.remove(&id);
                state.delays.remove(&id);
                state.paths.remove(&id);
                state.landed_sinks.remove(&id);
                let _ = ack.send(());
            }
            Ok(PoolMsg::SetFailureSink(sink)) => {
                state.failure_sink = Some(sink);
            }
            Ok(PoolMsg::SetLandedSink { id, sink }) => {
                state.landed_sinks.insert(id, sink);
            }
            Err(RecvTimeoutError::Timeout) => {
                let now = Instant::now();
                // Collect every id whose deadline has expired. We can't
                // mutate the map while iterating it, so collect first,
                // drain second.
                let due: Vec<WriterId> = state
                    .pending
                    .iter()
                    .filter_map(|(id, p)| if p.deadline <= now { Some(*id) } else { None })
                    .collect();
                for id in due {
                    if let Err(e) = flush_writer(&mut state, id) {
                        let path = state.paths.get(&id).map(|p| p.display().to_string());
                        eprintln!(
                            "bastyde-settings: write to {} failed: {e}",
                            path.as_deref().unwrap_or("<unknown>"),
                        );
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                // Channel closed (only happens at process tear-down,
                // since the sender is held in a static OnceLock).
                return;
            }
        }
    }
}

/// Apply `id`'s queued patches to the document **currently on disk**, under an
/// exclusive advisory lock, and write the result.
///
/// This is the whole point of the design: the read, the merge and the write are
/// one critical section, so a peer process cannot interleave a write between our
/// read and our write and have it silently overwritten.
///
/// On success the queue is cleared. On failure it is **retained** and retried
/// (with backoff) on a later tick — patches are `Fn`, so replaying them against
/// whatever is on disk *then* is a correct merge, not a stale overwrite. After
/// [`MAX_WRITE_ATTEMPTS`] the queue is dropped with a loud log, so a permanently
/// unwritable file cannot spin the worker forever.
fn flush_writer(state: &mut PoolState, id: WriterId) -> Result<(), FlushError> {
    let Some(pending) = state.pending.get(&id) else {
        return Ok(()); // nothing queued
    };
    if pending.patches.is_empty() {
        state.pending.remove(&id);
        return Ok(());
    }
    let Some(path) = state.paths.get(&id).cloned() else {
        // Unregistered mid-flight; nothing sensible to write to.
        state.pending.remove(&id);
        return Ok(());
    };

    let result = apply_and_write(&path, &pending.patches);

    match result {
        Ok(()) => {
            state.pending.remove(&id);
            // F11: report the fresh post-write stamp to whoever
            // registered a `WriteLandedSink` for this writer (one extra
            // `fs::metadata`, computed once, right after the write —
            // negligible next to the write itself). Consumers that need
            // to mutate `!Send` state off this thread (e.g.
            // `WindowStateService`'s `Rc<Cell<_>>`) stash the value and
            // pick it up next time they run on their own thread.
            if let Some(sink) = state.landed_sinks.get(&id) {
                sink(crate::file::disk_stamp(&path));
            }
            Ok(())
        }
        Err(e) => {
            let delay = state.delays.get(&id).copied().unwrap_or(Duration::ZERO);
            let slot = state
                .pending
                .get_mut(&id)
                .expect("pending entry checked above");
            slot.attempts += 1;
            if slot.attempts >= MAX_WRITE_ATTEMPTS {
                let attempts = slot.attempts;
                let dropped = slot.patches.len();
                eprintln!(
                    "bastyde-settings: giving up on {} after {} failed attempts; \
                     {} queued change(s) discarded: {e}",
                    path.display(),
                    attempts,
                    dropped,
                );
                // F3, discard site 1: the queue is about to be dropped
                // for good — report it through the process-wide sink (if
                // one is registered) in addition to the log above.
                if let Some(sink) = &state.failure_sink {
                    sink(path.clone(), attempts, dropped, e.to_string());
                }
                state.pending.remove(&id);
            } else {
                slot.deadline = Instant::now() + delay.max(RETRY_BACKOFF);
            }
            Err(e)
        }
    }
}

/// The locked read-merge-write itself.
fn apply_and_write(path: &Path, patches: &[Patch]) -> Result<(), FlushError> {
    // Hold the lock across read + merge + write, so this is atomic with
    // respect to every other `FileLock` holder (this process's other
    // handles, and any peer process using the same primitive).
    //
    // Non-blocking (F10): ALL `DebouncedWriter`s in the process share this
    // one worker thread, so a *blocking* acquire here would stall every
    // other writer's flush (and `flush_now`'s synchronous ack, called on
    // the UI thread at shutdown) for as long as some peer holds the lock —
    // including hanging process exit. A contended lock is just another
    // transient `FlushError::Io`: `flush_writer`'s existing retry+backoff
    // loop already handles any `Err` from this function uniformly, so
    // losing this one immediate attempt to a peer costs nothing but a
    // retry on the next scheduled tick.
    let _guard = crate::lock::FileLock::try_acquire_exclusive(path)?;

    let current = match fs::read_to_string(path) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };

    let mut text = current;
    for patch in patches {
        text = Some(patch(text)?);
    }

    match text {
        Some(t) => write_atomic(path, &t).map_err(FlushError::from),
        // Unreachable: `patches` is non-empty and every patch yields a
        // `String`. Guard rather than unwrap.
        None => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Atomic write
// ---------------------------------------------------------------------------

/// Write `contents` to `path` atomically: create a temp file in the
/// same directory, write + `sync_all`, then rename over the target.
/// Same-directory rename is atomic on every supported POSIX filesystem
/// and on NTFS (Windows).  Parent directories are created if absent.
pub(crate) fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    let dir = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "DebouncedWriter path has no parent directory",
        )
    })?;
    fs::create_dir_all(dir)?;
    let mut tmp = NamedTempFile::new_in(dir)?;
    tmp.as_file_mut().write_all(contents.as_bytes())?;
    tmp.as_file_mut().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// DebouncedWriter — public handle
// ---------------------------------------------------------------------------

/// Atomic, debounced single-file writer.
///
/// All writers in a process share one background I/O thread (see
/// module docs). Each writer is identified by an opaque `WriterId`;
/// dropping a writer synchronously flushes its pending payload before
/// returning.
pub struct DebouncedWriter {
    id: WriterId,
    path: PathBuf,
    delay: Duration,
}

impl DebouncedWriter {
    /// Create a writer that will atomically write to `path`, coalescing
    /// rapid `schedule` bursts inside `delay`.
    ///
    /// `delay = Duration::ZERO` makes every `schedule` flush on the
    /// worker's very next iteration — useful for tests.
    pub fn new(path: PathBuf, delay: Duration) -> Self {
        let id = next_writer_id();
        let _ = pool().send(PoolMsg::Register {
            id,
            path: path.clone(),
            delay,
        });
        Self { id, path, delay }
    }

    /// Queue a [`Patch`] — a replayable mutation of the file.
    ///
    /// The patch is **appended** to this writer's queue, and the deadline is
    /// reset to `now + delay` (the debounce window restarts on activity). At
    /// the deadline the whole queue is applied, in order, to the document read
    /// from disk **under an exclusive lock**, and the result written atomically.
    ///
    /// Debouncing therefore coalesces *writes*, never *mutations*: ten `set`s
    /// inside one window still produce one write, but all ten are applied — and
    /// applied on top of whatever a peer process wrote in the meantime.
    pub(crate) fn schedule(&self, patch: Patch) {
        let _ = pool().send(PoolMsg::Schedule { id: self.id, patch });
    }

    /// Force any queued patches to disk synchronously. Returns `Ok(())` if
    /// there was nothing queued.
    pub fn flush_now(&self) -> Result<(), FlushError> {
        let (ack_tx, ack_rx) = mpsc::sync_channel(0);
        pool()
            .send(PoolMsg::FlushNow {
                id: self.id,
                ack: ack_tx,
            })
            .map_err(|_| FlushError::Disconnected)?;
        match ack_rx.recv() {
            Ok(result) => result,
            Err(_) => Err(FlushError::Disconnected),
        }
    }

    /// Register a sink for this writer's successful-flush stamp (opt-in; a
    /// writer with none behaves exactly as today). May be called any time
    /// after construction — including after the writer has already flushed
    /// once, since the sink is only ever consulted on a *future* successful
    /// flush.
    ///
    /// This is how a caller learns the *real* on-disk stamp resulting from
    /// its own debounced write, without guessing: `apply()` schedules a
    /// patch and returns before it lands, so only the worker thread — right
    /// after the write actually succeeds — knows the resulting `(mtime,
    /// len)`. See `WindowStateService::reload_from_disk` for the consumer
    /// side (F11).
    pub fn set_landed_sink(&self, sink: WriteLandedSink) {
        let _ = pool().send(PoolMsg::SetLandedSink { id: self.id, sink });
    }

    /// The destination path this writer flushes to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The debounce window configured at construction; `Duration::ZERO`
    /// means every scheduled payload is written on the worker's next
    /// iteration (useful in tests).
    pub fn delay(&self) -> Duration {
        self.delay
    }
}

impl Drop for DebouncedWriter {
    fn drop(&mut self) {
        // Send an Unregister with a sync ack so we don't return from
        // Drop until the pending payload (if any) has been flushed.
        // Without this, exit-time data would race with process
        // teardown.
        let (ack_tx, ack_rx) = mpsc::sync_channel(0);
        if pool()
            .send(PoolMsg::Unregister {
                id: self.id,
                ack: ack_tx,
            })
            .is_ok()
        {
            let _ = ack_rx.recv();
        }
    }
}

impl std::fmt::Debug for DebouncedWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DebouncedWriter")
            .field("path", &self.path)
            .field("delay", &self.delay)
            .field("id", &self.id.0)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::tempdir;

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    /// A fresh, empty `PoolState`, for tests that drive `flush_writer` /
    /// `apply_schedule` directly rather than through the process-global
    /// `pool()` singleton — avoids serializing tests around shared global
    /// state (per the review's stated preference).
    fn empty_state() -> PoolState {
        PoolState {
            delays: HashMap::new(),
            paths: HashMap::new(),
            pending: HashMap::new(),
            failure_sink: None,
            landed_sinks: HashMap::new(),
        }
    }

    /// A patch that ignores whatever is currently on disk and unconditionally
    /// writes `s` — the moral equivalent of the old "pre-rendered whole
    /// document" payload. Useful for tests that only care about which
    /// payload wins a race, not about merging.
    fn const_patch(s: impl Into<String>) -> Patch {
        let s = s.into();
        Box::new(move |_current: Option<String>| Ok(s.clone()))
    }

    /// A patch that **appends** `line` to whatever text is already there
    /// (or starts fresh if the file doesn't exist yet) — a minimal stand-in
    /// for a real merge patch, used to prove that multiple queued patches
    /// are *all* applied, in order, rather than the queue being a
    /// single overwritable slot.
    fn append_line_patch(line: &'static str) -> Patch {
        Box::new(move |current: Option<String>| {
            let mut text = current.unwrap_or_default();
            text.push_str(line);
            Ok(text)
        })
    }

    #[test]
    fn flush_now_writes_pending_payload() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("out.toml");
        let writer = DebouncedWriter::new(path.clone(), Duration::from_millis(500));

        writer.schedule(const_patch("alpha = 1\n"));
        writer.flush_now().unwrap();
        assert_eq!(read(&path), "alpha = 1\n");
    }

    #[test]
    fn schedule_coalesces_rapid_bursts_into_one_write() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("burst.toml");
        let writer = DebouncedWriter::new(path.clone(), Duration::from_millis(50));

        for i in 0..10 {
            writer.schedule(const_patch(format!("v = {i}\n")));
        }
        // Drop -> graceful flush. Each of these patches ignores whatever
        // came before it (that's the point of `const_patch`), so folding
        // all 10 in order still nets out to the last one's payload.
        drop(writer);

        assert_eq!(read(&path), "v = 9\n");
    }

    /// THE HEADLINE flush.rs TEST. Two patches scheduled inside one
    /// debounce window must **both** land. The old design's `Schedule`
    /// overwrote a single pending-payload slot — the second `schedule`
    /// call would have silently discarded the first payload entirely, a
    /// real mutation lost, not merely a redundant write coalesced away.
    #[test]
    fn two_mutations_in_one_debounce_window_both_land() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("both_land.toml");
        let writer = DebouncedWriter::new(path.clone(), Duration::from_millis(200));

        writer.schedule(append_line_patch("alpha = 1\n"));
        writer.schedule(append_line_patch("beta = 2\n"));
        writer.flush_now().unwrap();

        let contents = read(&path);
        assert!(
            contents.contains("alpha = 1"),
            "the first queued patch must not be dropped by the second: {contents:?}"
        );
        assert!(
            contents.contains("beta = 2"),
            "the second queued patch must also land: {contents:?}"
        );
    }

    #[test]
    fn debounce_window_actually_waits() {
        // The worker thread is shared with every other writer in the
        // process, so under `cargo test` parallelism it can be heavily
        // contended. Generous windows keep the test robust:
        //   - 300 ms debounce window,
        //   - up to 3 seconds of polling for the post-window write.
        // Polling beats a single long sleep because most invocations
        // finish well within ~400 ms; we only pay the long tail when
        // the worker is actually backlogged.
        let dir = tempdir().unwrap();
        let path = dir.path().join("debounced.toml");
        let writer = DebouncedWriter::new(path.clone(), Duration::from_millis(300));

        writer.schedule(const_patch("first = 1\n"));
        // Sleep well under 1/3 of the window — file must not exist.
        thread::sleep(Duration::from_millis(50));
        assert!(
            !path.exists(),
            "file should not exist before debounce window expires"
        );

        // Queue a second patch — deadline resets, and since `const_patch`
        // ignores `current`, the net result is just its own payload.
        writer.schedule(const_patch("second = 2\n"));

        // Poll up to 3 s for the file to materialize with the second
        // payload. (We can't read the file mid-write — atomic rename
        // means it either has the old or new contents, never partial.)
        let deadline = std::time::Instant::now() + Duration::from_millis(3000);
        loop {
            if path.exists() && read(&path) == "second = 2\n" {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "debounced flush did not complete within 3 s",
            );
            thread::sleep(Duration::from_millis(25));
        }
    }

    #[test]
    fn drop_flushes_pending_data() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("drop.toml");

        {
            let writer = DebouncedWriter::new(path.clone(), Duration::from_secs(60));
            writer.schedule(const_patch("survived = true\n"));
        }
        assert_eq!(read(&path), "survived = true\n");
    }

    #[test]
    fn zero_delay_writes_immediately_after_flush_now() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("zero.toml");
        let writer = DebouncedWriter::new(path.clone(), Duration::ZERO);

        writer.schedule(const_patch("v = 1\n"));
        writer.flush_now().unwrap();
        assert_eq!(read(&path), "v = 1\n");
    }

    #[test]
    fn write_atomic_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested/deeper/out.toml");
        write_atomic(&path, "ok = true\n").unwrap();
        assert_eq!(read(&path), "ok = true\n");
    }

    #[test]
    fn many_writers_coexist_on_the_shared_thread() {
        // Stress test: 20 writers, 20 different paths, all served by
        // the single shared I/O worker. Each gets its own payload and
        // independent flush; nothing collides.
        let dir = tempdir().unwrap();
        let mut writers = Vec::new();
        for i in 0..20 {
            let path = dir.path().join(format!("w{i}.toml"));
            let w = DebouncedWriter::new(path, Duration::ZERO);
            w.schedule(const_patch(format!("id = {i}\n")));
            w.flush_now().unwrap();
            writers.push(w);
        }
        for i in 0..20 {
            let path = dir.path().join(format!("w{i}.toml"));
            assert_eq!(read(&path), format!("id = {i}\n"));
        }
    }

    /// A failing write must **retain** the queued patch(es) and retry —
    /// never silently drop a mutation just because one attempt hit a
    /// transient disk error. We force a failure deterministically by
    /// making the target path an existing *directory*, so the atomic
    /// rename onto it fails at the OS level; clearing the obstruction
    /// must let a later retry succeed with the originally-queued payload
    /// intact.
    #[test]
    fn failed_write_retains_the_queue_and_lands_once_unblocked() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("obstructed.toml");
        fs::create_dir_all(&path).unwrap();

        let writer = DebouncedWriter::new(path.clone(), Duration::ZERO);
        writer.schedule(const_patch("v = 1\n"));

        // Give the worker a moment to hit (and fail) its first attempt.
        thread::sleep(Duration::from_millis(150));
        assert!(
            path.is_dir(),
            "the write must have failed while the obstruction stood"
        );

        // Clear the obstruction. The retained patch must land on a later
        // retry rather than having been dropped after the first failure.
        fs::remove_dir(&path).unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if path.is_file() && fs::read_to_string(&path).ok().as_deref() == Some("v = 1\n") {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the queued patch was not retried/retained after the obstruction cleared",
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    // -- F9: Schedule must reset attempts and never pull the deadline
    // backward -----------------------------------------------------------

    /// THE HEADLINE F9 test. A zero-delay writer that just failed a write
    /// has a future `RETRY_BACKOFF` deadline installed by `flush_writer`.
    /// The OLD `Schedule` arm (`slot.deadline = Instant::now() + delay`)
    /// unconditionally overwrote that with `now` — pulling the backoff
    /// back to immediate and spinning the worker at full speed on a
    /// persistently-failing writer — and never reset `attempts`, so a
    /// brand-new, unrelated patch inherited the existing failure streak
    /// and could be discarded on its very first failure. This drives
    /// `flush_writer` and `apply_schedule` directly against a real
    /// `PoolState`, without the worker thread, so the assertions are
    /// exact rather than racing wall-clock polling.
    #[test]
    fn schedule_after_a_failure_resets_attempts_and_does_not_rewind_the_backoff_deadline() {
        let dir = tempdir().unwrap();
        // An existing directory at the write target makes every write
        // fail deterministically (the atomic rename onto it fails at the
        // OS level).
        let path = dir.path().join("obstructed.toml");
        fs::create_dir_all(&path).unwrap();

        let id = next_writer_id();
        let mut state = empty_state();
        state.delays.insert(id, Duration::ZERO);
        state.paths.insert(id, path.clone());
        state.pending.insert(
            id,
            Pending {
                deadline: Instant::now(),
                patches: vec![const_patch("v = 1\n")],
                attempts: 0,
            },
        );

        // First attempt: fails, `attempts` becomes 1, and a future
        // RETRY_BACKOFF deadline is installed.
        let before_backoff = Instant::now();
        assert!(flush_writer(&mut state, id).is_err());
        let slot = state
            .pending
            .get(&id)
            .expect("still pending after 1/5 failures");
        assert_eq!(slot.attempts, 1);
        assert!(
            slot.deadline >= before_backoff + RETRY_BACKOFF,
            "a failed attempt must install a future backoff deadline, got {:?} (now was {:?})",
            slot.deadline,
            before_backoff,
        );
        let backoff_deadline = slot.deadline;

        // A second, unrelated patch arrives (a genuine new mutation, not
        // a retry) *before* the backoff elapses — exactly the scenario
        // `apply_schedule` (the extracted `Schedule` handler) must not
        // regress.
        apply_schedule(&mut state, id, const_patch("v = 2\n"));

        let slot = state.pending.get(&id).expect("still pending");
        assert_eq!(
            slot.attempts, 0,
            "new work must reset the failure streak, since the newly queued \
             patch has never itself failed to write"
        );
        assert!(
            slot.deadline >= backoff_deadline,
            "an unrelated Schedule must never rewind an already-armed backoff \
             deadline back toward `now`: backoff was {backoff_deadline:?}, \
             deadline after Schedule was {:?}",
            slot.deadline,
        );
        assert_eq!(slot.patches.len(), 2, "both patches must still be queued");
    }

    /// Regression guard: ordinary rapid debounce coalescing (no failures
    /// involved) must still work exactly as before — each new `Schedule`
    /// during a healthy burst still pushes the deadline forward to `now +
    /// delay`, so `.max()` must never *shorten* the debounce window
    /// relative to the old unconditional-overwrite behaviour.
    #[test]
    fn schedule_still_coalesces_a_healthy_burst_into_one_forward_moving_deadline() {
        let id = next_writer_id();
        let mut state = empty_state();
        let delay = Duration::from_millis(50);
        state.delays.insert(id, delay);

        let mut last_deadline = Instant::now();
        for i in 0..5 {
            let before = Instant::now();
            apply_schedule(&mut state, id, const_patch(format!("v = {i}\n")));
            let slot = state.pending.get(&id).unwrap();
            assert!(
                slot.deadline >= before + delay,
                "each Schedule in a healthy burst must push the deadline to \
                 at least `now + delay`, got {:?} (now + delay was {:?})",
                slot.deadline,
                before + delay,
            );
            assert!(
                slot.deadline >= last_deadline,
                "the deadline must never move backward across a healthy burst"
            );
            last_deadline = slot.deadline;
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(state.pending.get(&id).unwrap().patches.len(), 5);
    }

    // -- F10: a contended lock on one writer must not stall the shared
    // worker thread's service of every other writer -----------------------

    /// THE HEADLINE F10 test. Two real `DebouncedWriter`s share the one
    /// process-global worker thread. One target's sidecar lock is held
    /// externally (as a peer process holding it would). Before the fix,
    /// `apply_and_write`'s blocking `FileLock::acquire_exclusive` would
    /// stall the *entire* shared worker thread on that single contended
    /// lock, so the unrelated, perfectly healthy writer's flush would
    /// never land either — this asserts it lands promptly regardless.
    #[test]
    fn contended_lock_on_one_writer_does_not_stall_others_on_the_shared_thread() {
        let dir = tempdir().unwrap();
        let locked_path = dir.path().join("locked.toml");
        let healthy_path = dir.path().join("healthy.toml");

        // Hold the "locked" writer's sidecar lock externally, exactly as
        // a peer process would.
        let external_lock = crate::lock::FileLock::acquire_exclusive(&locked_path).unwrap();

        let locked_writer = DebouncedWriter::new(locked_path.clone(), Duration::ZERO);
        let healthy_writer = DebouncedWriter::new(healthy_path.clone(), Duration::ZERO);

        locked_writer.schedule(const_patch("v = locked\n"));
        healthy_writer.schedule(const_patch("v = healthy\n"));

        // The healthy writer must land well within a couple of seconds —
        // it shares the worker thread with the contended writer, but must
        // not be stuck behind it.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if healthy_path.exists() && read(&healthy_path) == "v = healthy\n" {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the healthy writer's write did not land promptly — the shared \
                 worker thread appears stalled behind the other writer's \
                 contended lock",
            );
            thread::sleep(Duration::from_millis(20));
        }

        // And the locked writer correctly has NOT succeeded yet — it is
        // still contended, not silently skipped.
        assert!(
            !locked_path.exists(),
            "the locked writer should not have been able to write while its \
             lock is still externally held"
        );

        drop(external_lock);
    }

    // -- F3: a permanently-discarded write must reach the failure sink ----

    /// THE HEADLINE F3 test. Drives `flush_writer` directly (bypassing the
    /// process-global `pool()`/`set_write_failure_sink` indirection, since
    /// that is process-wide singleton state best kept out of parallel
    /// tests) against a target that can never be written (an existing
    /// directory), asserting the registered sink fires exactly once, at
    /// the give-up point, with the correct attempt count and dropped-patch
    /// count. Before F3 this information never left the worker thread at
    /// all — only an `eprintln!` recorded it.
    #[test]
    fn giving_up_after_max_attempts_reports_through_the_failure_sink() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("obstructed_sink.toml");
        fs::create_dir_all(&path).unwrap();

        let id = next_writer_id();
        let mut state = empty_state();
        state.delays.insert(id, Duration::ZERO);
        state.paths.insert(id, path.clone());
        state.pending.insert(
            id,
            Pending {
                deadline: Instant::now(),
                patches: vec![const_patch("v = 1\n")],
                attempts: 0,
            },
        );

        // (path, attempts, dropped_patches, message) — one call to the sink.
        type FailureCall = (PathBuf, u32, usize, String);
        let calls: Arc<Mutex<Vec<FailureCall>>> = Arc::new(Mutex::new(Vec::new()));
        let calls_for_sink = calls.clone();
        state.failure_sink = Some(Arc::new(move |path, attempts, dropped, message| {
            calls_for_sink
                .lock()
                .unwrap()
                .push((path, attempts, dropped, message));
        }));

        for _ in 0..MAX_WRITE_ATTEMPTS {
            let _ = flush_writer(&mut state, id);
        }

        let recorded = calls.lock().unwrap();
        assert_eq!(
            recorded.len(),
            1,
            "the sink must fire exactly once, precisely at the give-up point: {recorded:?}"
        );
        let (sunk_path, attempts, dropped, message) = &recorded[0];
        assert_eq!(sunk_path, &path);
        assert_eq!(*attempts, MAX_WRITE_ATTEMPTS);
        assert_eq!(*dropped, 1);
        assert!(!message.is_empty());
        assert!(
            !state.pending.contains_key(&id),
            "the queue must be gone once the sink has been told about the discard"
        );
    }

    /// A `Schedule`d writer that never fails must never invoke the
    /// failure sink at all — it exists only for the give-up path.
    #[test]
    fn a_healthy_writer_never_invokes_the_failure_sink() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("healthy_no_sink.toml");
        let writer = DebouncedWriter::new(path.clone(), Duration::ZERO);

        let fired = Arc::new(Mutex::new(false));
        let fired_for_sink = fired.clone();
        set_write_failure_sink(Arc::new(move |_, _, _, _| {
            *fired_for_sink.lock().unwrap() = true;
        }));

        writer.schedule(const_patch("v = 1\n"));
        writer.flush_now().unwrap();

        assert!(
            !*fired.lock().unwrap(),
            "a successful write must never invoke the failure sink"
        );

        // Reset the process-global sink so later tests in this binary
        // that rely on `set_write_failure_sink`'s default (unset) state
        // aren't affected by this test's registration. `pool()`/the sink
        // slot are process-global, so the last writer wins; explicitly
        // installing a no-op keeps this test's side effect from leaking
        // into whichever test happens to run after it.
        set_write_failure_sink(Arc::new(|_, _, _, _| {}));
    }

    // -- F11: WriteLandedSink fires with the real post-write stamp --------

    /// THE HEADLINE F11 test. Registers a `WriteLandedSink` on a real
    /// `DebouncedWriter`, schedules a patch, forces it to land via
    /// `flush_now`, and asserts the sink received a stamp equal to one
    /// taken independently (via `crate::file::disk_stamp`) right after —
    /// proving the sink's value is the *real*, authoritative post-write
    /// stamp, not a guess.
    #[test]
    fn landed_sink_fires_with_the_real_post_write_stamp() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("landed.toml");
        let writer = DebouncedWriter::new(path.clone(), Duration::ZERO);

        let received: Arc<Mutex<Option<LandedStamp>>> = Arc::new(Mutex::new(None));
        let received_for_sink = received.clone();
        writer.set_landed_sink(Arc::new(move |stamp| {
            *received_for_sink.lock().unwrap() = Some(stamp);
        }));

        writer.schedule(const_patch("v = 1\n"));
        writer.flush_now().unwrap();

        let expected = crate::file::disk_stamp(&path);
        let got = received
            .lock()
            .unwrap()
            .expect("the landed sink must have fired after a successful flush");
        assert_eq!(
            got, expected,
            "the sink's stamp must match a stamp taken independently right \
             after the write landed"
        );
        // Sanity: the stamp is not the "file doesn't exist" placeholder —
        // the write really happened.
        assert!(expected.0.is_some() || expected.1.is_some());
    }

    /// A writer with no registered sink must behave exactly as before —
    /// no panic, no special-casing.
    #[test]
    fn writer_without_a_landed_sink_flushes_normally() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("no_landed_sink.toml");
        let writer = DebouncedWriter::new(path.clone(), Duration::ZERO);

        writer.schedule(const_patch("v = 1\n"));
        writer.flush_now().unwrap();

        assert_eq!(read(&path), "v = 1\n");
    }
}
