//! Debounced atomic file writer.
//!
//! `DebouncedWriter` accepts new payloads via [`schedule`](DebouncedWriter::schedule),
//! batches rapid bursts inside a debounce window, and writes the most-recent
//! payload to disk atomically (write-temp + rename via [`tempfile`]).
//!
//! ## Single shared I/O thread
//!
//! All `DebouncedWriter`s in a process share **one** background I/O
//! thread (lazily started on first use). Each writer registers under
//! a unique [`WriterId`] and routes its payloads through a single
//! `mpsc::Sender<PoolMsg>`. The shared thread:
//!
//! * keeps a per-id `(deadline, payload)` pending slot,
//! * blocks on the next-due deadline (or waits for a message if no
//!   payload is pending),
//! * coalesces rapid `Schedule` bursts: a new payload during the
//!   debounce window replaces the prior one, and the deadline is
//!   reset on each push so debouncing matches the user's expectation
//!   for "wait until the burst stops, then write."
//!
//! Application logic stays single-threaded — `SettingsStore` and
//! friends never block on I/O. `Drop` on a `DebouncedWriter` sends
//! an `Unregister` message that synchronously flushes the writer's
//! pending payload before returning, so end-of-process state is
//! never lost.
//!
//! ## Why one thread, not one-per-writer
//!
//! An app that opens the K/V store + recents + window state already
//! has 3 writers; a richer app might have 5–10. Each idle ~99% of the
//! time. One shared worker is leaner and has identical semantics from
//! the caller's point of view.

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
    /// The shared I/O thread panicked or has already shut down. The
    /// writer cannot make progress.
    #[error("settings I/O thread disconnected")]
    Disconnected,
    /// The atomic write failed.
    #[error("settings flush failed: {0}")]
    Io(#[from] io::Error),
}

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
        payload: String,
    },
    FlushNow {
        id: WriterId,
        ack: SyncSender<io::Result<()>>,
    },
    Unregister {
        id: WriterId,
        ack: SyncSender<()>,
    },
}

struct PoolState {
    delays: HashMap<WriterId, Duration>,
    paths: HashMap<WriterId, PathBuf>,
    /// Per-writer pending payload + deadline.
    pending: HashMap<WriterId, (Instant, String)>,
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
            .name("fern-settings-writer".into())
            .spawn(move || worker_loop(rx))
            .expect("fern-settings: failed to spawn writer thread");
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
        let next_deadline = state.pending.values().map(|(d, _)| *d).min();
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
            Ok(PoolMsg::Schedule { id, payload }) => {
                let delay = state.delays.get(&id).copied().unwrap_or(Duration::ZERO);
                state.pending.insert(id, (Instant::now() + delay, payload));
            }
            Ok(PoolMsg::FlushNow { id, ack }) => {
                let result = match state.pending.remove(&id) {
                    Some((_, payload)) => match state.paths.get(&id) {
                        Some(path) => write_atomic(path, &payload),
                        None => Ok(()),
                    },
                    None => Ok(()),
                };
                let _ = ack.send(result);
            }
            Ok(PoolMsg::Unregister { id, ack }) => {
                if let Some((_, payload)) = state.pending.remove(&id)
                    && let Some(path) = state.paths.get(&id)
                    && let Err(e) = write_atomic(path, &payload)
                {
                    eprintln!(
                        "fern-settings: final flush of {} failed: {e}",
                        path.display(),
                    );
                }
                state.delays.remove(&id);
                state.paths.remove(&id);
                let _ = ack.send(());
            }
            Err(RecvTimeoutError::Timeout) => {
                let now = Instant::now();
                // Collect every id whose deadline has expired. We
                // can't mutate the map while iterating it, so collect
                // first, drain second.
                let due: Vec<WriterId> = state
                    .pending
                    .iter()
                    .filter_map(|(id, (d, _))| if *d <= now { Some(*id) } else { None })
                    .collect();
                for id in due {
                    if let Some((_, payload)) = state.pending.remove(&id)
                        && let Some(path) = state.paths.get(&id)
                        && let Err(e) = write_atomic(path, &payload)
                    {
                        eprintln!("fern-settings: write to {} failed: {e}", path.display(),);
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

// ---------------------------------------------------------------------------
// Atomic write
// ---------------------------------------------------------------------------

/// Atomic write: write to a temp file in the same directory, fsync,
/// rename. Same-directory rename is atomic on every supported
/// filesystem we target.
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

    /// Replace the pending payload. If a flush is already scheduled,
    /// the new payload overwrites the prior one and the deadline is
    /// reset to `now + delay` (debounce window restarts on activity).
    pub fn schedule(&self, serialized: String) {
        let _ = pool().send(PoolMsg::Schedule {
            id: self.id,
            payload: serialized,
        });
    }

    /// Force any pending payload to disk synchronously. Returns
    /// `Ok(())` if there was nothing to write.
    pub fn flush_now(&self) -> Result<(), FlushError> {
        let (ack_tx, ack_rx) = mpsc::sync_channel(0);
        pool()
            .send(PoolMsg::FlushNow {
                id: self.id,
                ack: ack_tx,
            })
            .map_err(|_| FlushError::Disconnected)?;
        match ack_rx.recv() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(FlushError::Io(e)),
            Err(_) => Err(FlushError::Disconnected),
        }
    }

    /// The destination path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The configured debounce delay.
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

    #[test]
    fn flush_now_writes_pending_payload() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("out.toml");
        let writer = DebouncedWriter::new(path.clone(), Duration::from_millis(500));

        writer.schedule("alpha = 1\n".into());
        writer.flush_now().unwrap();
        assert_eq!(read(&path), "alpha = 1\n");
    }

    #[test]
    fn schedule_coalesces_rapid_bursts_into_one_write() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("burst.toml");
        let writer = DebouncedWriter::new(path.clone(), Duration::from_millis(50));

        for i in 0..10 {
            writer.schedule(format!("v = {i}\n"));
        }
        // Drop -> graceful flush of last payload only.
        drop(writer);

        assert_eq!(read(&path), "v = 9\n");
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

        writer.schedule("first = 1\n".into());
        // Sleep well under 1/3 of the window — file must not exist.
        thread::sleep(Duration::from_millis(50));
        assert!(
            !path.exists(),
            "file should not exist before debounce window expires"
        );

        // Replace the payload — deadline resets.
        writer.schedule("second = 2\n".into());

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
            writer.schedule("survived = true\n".into());
        }
        assert_eq!(read(&path), "survived = true\n");
    }

    #[test]
    fn zero_delay_writes_immediately_after_flush_now() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("zero.toml");
        let writer = DebouncedWriter::new(path.clone(), Duration::ZERO);

        writer.schedule("v = 1\n".into());
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
            w.schedule(format!("id = {i}\n"));
            w.flush_now().unwrap();
            writers.push(w);
        }
        for i in 0..20 {
            let path = dir.path().join(format!("w{i}.toml"));
            assert_eq!(read(&path), format!("id = {i}\n"));
        }
    }
}
