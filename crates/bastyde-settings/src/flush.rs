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
//! * coalesces rapid `Schedule` bursts by **appending** to the queue and
//!   resetting the deadline — so debouncing collapses *writes*, never
//!   *mutations*. (The old design could overwrite the pending payload precisely
//!   because each payload was a complete, self-superseding rendering.)
//!
//! A failed write **retains** the queue and retries with backoff, up to
//! [`MAX_WRITE_ATTEMPTS`] — the patches replay cleanly against whatever is on
//! disk then, which is the correct merge rather than a stale overwrite.
//!
//! Application logic stays single-threaded — `SettingsStore` and friends never
//! block on I/O. `Drop` sends an `Unregister` that synchronously flushes the
//! queue before returning, so end-of-process state is never lost.
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
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

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

fn worker_loop(rx: Receiver<PoolMsg>) {
    let mut state = PoolState {
        delays: HashMap::new(),
        paths: HashMap::new(),
        pending: HashMap::new(),
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
            Ok(PoolMsg::Schedule { id, patch }) => {
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
                // Reset the deadline so debouncing still means "wait
                // until the burst stops, then write" — we coalesce the
                // *write*, never a mutation.
                slot.deadline = Instant::now() + delay;
            }
            Ok(PoolMsg::FlushNow { id, ack }) => {
                let _ = ack.send(flush_writer(&mut state, id));
            }
            Ok(PoolMsg::Unregister { id, ack }) => {
                if let Err(e) = flush_writer(&mut state, id) {
                    let path = state.paths.get(&id).map(|p| p.display().to_string());
                    eprintln!(
                        "bastyde-settings: final flush of {} failed: {e}",
                        path.as_deref().unwrap_or("<unknown>"),
                    );
                }
                state.pending.remove(&id);
                state.delays.remove(&id);
                state.paths.remove(&id);
                let _ = ack.send(());
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
                eprintln!(
                    "bastyde-settings: giving up on {} after {} failed attempts; \
                     {} queued change(s) discarded: {e}",
                    path.display(),
                    slot.attempts,
                    slot.patches.len(),
                );
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
    let _guard = crate::lock::FileLock::acquire_exclusive(path)?;

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
    use tempfile::tempdir;

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
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
}
