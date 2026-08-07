// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A small RAII advisory file-lock guard, used by [`crate::SettingsFile`]'s
//! locked read-modify-write (the *only* mode it has — see `file.rs`'s
//! module docs) to serialize that read-modify-write around a settings
//! file that more than one process may write to concurrently.
//!
//! ## Why a sidecar lock file, not locking the settings file itself
//!
//! `teksilo-settings` writes atomically via temp-file + rename (see
//! [`crate::flush::write_atomic`]). A lock held on the settings path
//! itself would be awkward to reconcile with that dance (the inode a
//! lock is held on can change out from under a lock holder across a
//! rename). Locking a stable sidecar path sidesteps that entirely: the
//! lock's identity never depends on what the protected file's inode
//! happens to be at any given moment.
//!
//! ## Where the sidecars live, and why they are never deleted
//!
//! Each settings file's lock lives at `<dir>/locks/<file_name>.lock` — a
//! dedicated subdirectory, so the sidecars don't litter the
//! `~/.config/<app>/` directory that users open and hand-edit.
//!
//! They are **permanent**. A lock file can never be safely unlinked: doing
//! so lets one process hold the lock on an unlinked inode while another
//! creates a fresh one and locks *that*, so both proceed at once and the
//! mutual exclusion is silently gone. Nor can the location be keyed off an
//! ambient path like `$XDG_RUNTIME_DIR`, since a process launched without
//! that variable would resolve a different lock path and share no lock with
//! its peers. [`sidecar_path`] carries the full argument.
//!
//! ## Why `fs2`
//!
//! There is no file-locking crate anywhere else in this workspace, so
//! this module introduces the first one. `fs2` was picked over rolling
//! raw `libc`/`windows-sys` `flock`/`LockFileEx` calls by hand because:
//!
//! * it is a tiny, dependency-light wrapper — exactly the
//!   `lock_exclusive` / `unlock` surface this module needs, nothing more;
//! * it is cross-platform (POSIX `flock`, Windows `LockFileEx`), which
//!   `teksilo-settings` needs since it targets desktop apps on all three
//!   major OSes;
//! * it has been the de-facto standard for this exact "advisory lock
//!   around a config/lock file" use case in the Rust ecosystem for
//!   years (Cargo itself has shipped with it for package-cache
//!   locking), so its stability is well-exercised in production; and
//!   its tiny, unchanging surface means its low maintenance churn is a
//!   feature here, not a risk — there is nothing left to add to
//!   `lock_exclusive`/`unlock`.
//!
//! `docs/settings.md`'s "Threading and source-of-truth" section
//! previously named `fcntl` directly as "the path forward" for this
//! exact feature; `fs2` is that path, made portable.
//!
//! ## Why the sidecar `.lock` file is never removed
//!
//! This is a deliberate, permanent design decision, not an oversight: the
//! sidecar accumulates one file per distinct settings path this process
//! (or any peer) has ever opened, and it is **never** unlinked, even once
//! nothing holds it.
//!
//! * **Correctness first.** A lock file must never be unlinked while it
//!   might still be held. `flock`/`LockFileEx` lock an *inode*, not a
//!   *path* — if this process removed `<path>.lock` the instant it
//!   released its own hold, a peer process that had already `open()`'d
//!   the (now-unlinked-but-still-referenced) old inode and is blocked in
//!   `lock_exclusive` on it would be locking something no longer
//!   reachable by that path. A third process opening the path fresh
//!   would create a *new* inode and could acquire an exclusive lock on
//!   *that* one at the same time — two processes each correctly holding
//!   "the" exclusive lock, on what has quietly become two different
//!   files. Never removing the path sidesteps this class of bug
//!   entirely.
//! * **No registry needed.** Colocating the lock beside its settings file
//!   lets every process derive the sidecar path independently, from just
//!   the settings path (see [`sidecar_path`]), with no shared registry or
//!   IPC. Moving these files to a platform runtime dir (`XDG_RUNTIME_DIR`
//!   and equivalents) was considered and rejected: it would require a
//!   *stable* mapping from an arbitrary settings path to a runtime-dir
//!   filename — exactly the same hash-stability hazard flagged elsewhere
//!   for `window_state.toml`'s per-window keys (switching hashers there
//!   silently orphans every existing row) — and is not worth entangling
//!   with what is otherwise a purely cosmetic concern.
//! * **Bounded, not growing.** Unlike `window_state.toml`'s rows (one per
//!   window ever opened, growing over the life of the install), the
//!   number of sidecar lock files is bounded by the number of *distinct
//!   settings files* the app opens — a handful (the K/V store, recents,
//!   window state, …), not one per record and not growing per run.
//!
//! Net effect: a handful of zero-byte `*.lock` files persist alongside
//! the user's settings files for the life of the install. This is
//! accepted, not a bug to fix.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use fs2::FileExt;

/// RAII guard around an exclusive advisory lock on `<settings_path>.lock`.
///
/// The lock is released on `Drop` — including during unwind, since
/// `Drop::drop` never panics here; a failure to unlock is only logged.
/// Holding a `FileLock` across a settings read + mutate + write is what
/// makes [`crate::SettingsFile`]'s locked read-modify-write atomic *with
/// respect to other `FileLock` holders* (this process's other handles,
/// or a peer process using the same primitive). It is advisory: it does
/// nothing to stop a process that never asks for the lock from writing
/// to the file anyway — which cannot happen from inside this crate,
/// since every persisted type routes through [`crate::SettingsFile`] (or
/// the same lock primitive directly, as
/// [`crate::collection::list::PersistedListModel`] does), but remains
/// true of any non-Teksilo process that touches the same file by hand.
pub(crate) struct FileLock {
    // Kept alive for the duration of the lock; the OS releases the lock
    // implicitly when the file descriptor closes, but we still release
    // explicitly in `Drop` so failures are observable (logged) rather
    // than silent.
    file: File,
}

impl FileLock {
    /// Block until an exclusive lock on `<settings_path>.lock` is
    /// acquired, creating the sidecar file (and its parent directory)
    /// if necessary.
    ///
    /// The sidecar is never written to; it exists purely as a lock
    /// handle so the lock's lifetime is independent of the settings
    /// file's own atomic write-temp-then-rename dance.
    pub(crate) fn acquire_exclusive(settings_path: &Path) -> io::Result<Self> {
        let lock_path = sidecar_path(settings_path);
        if let Some(dir) = lock_path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)?;
        file.lock_exclusive()?;
        Ok(Self { file })
    }

    /// Make a single, immediate attempt to acquire an exclusive lock on
    /// `<settings_path>.lock`, creating the sidecar file (and its parent
    /// directory) if necessary. Returns `Err` (kind
    /// [`io::ErrorKind::WouldBlock`], via `fs2::lock_contended_error()`)
    /// at once, instead of waiting, if another holder already has the
    /// lock — it never blocks.
    ///
    /// Used by the shared debounced-writer worker thread (`flush.rs`),
    /// which must never block on one writer's lock at the expense of
    /// every other writer sharing that same thread (F10): a contended
    /// lock there is just another transient `FlushError::Io`, retried on
    /// the next scheduled tick via the existing `MAX_WRITE_ATTEMPTS` /
    /// `RETRY_BACKOFF` machinery. `file.rs` and
    /// `crate::collection::list` still legitimately use the *blocking*
    /// [`Self::acquire_exclusive`] for their once-per-open synchronous
    /// loads, where waiting briefly for a peer is the correct behaviour.
    pub(crate) fn try_acquire_exclusive(settings_path: &Path) -> io::Result<Self> {
        let lock_path = sidecar_path(settings_path);
        if let Some(dir) = lock_path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)?;
        file.try_lock_exclusive()?;
        Ok(Self { file })
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // `unlock` failing here is not actionable by the caller (we are
        // already tearing down); log and move on rather than panic —
        // panicking in `Drop` during unwind would abort the process.
        if let Err(e) = FileExt::unlock(&self.file) {
            eprintln!("teksilo-settings: failed to release advisory lock: {e}");
        }
    }
}

/// Name of the subdirectory, alongside the settings files, that holds every
/// lock sidecar. See [`sidecar_path`].
pub(crate) const LOCK_DIR: &str = "locks";

/// The lock sidecar for `settings_path`: `<dir>/locks/<file_name>.lock`.
///
/// ## Why a subdirectory, and why the file is never deleted
///
/// Two constraints pull in opposite directions here, and both are hard.
///
/// **The sidecar can never be unlinked.** Not on `Drop`, not on a "clean"
/// shutdown, not by a sweeper. Deleting a file that an advisory lock is taken on
/// silently destroys mutual exclusion, via a race that is easy to miss:
///
/// ```text
///   P1  open(lock) -> inode X,  flock(X)          [holds the lock]
///   P2  open(lock) -> inode X,  flock(X) ...      [blocked, correctly]
///   P1  unlock(X), unlink(lock), close(X)         [tidies up on exit]
///   P2      ...flock(X) succeeds                  [holds the lock, on an
///                                                  unlinked inode]
///   P3  open(lock) -> MISSING -> creates inode Y,
///       flock(Y) succeeds immediately             [also "holds the lock"]
/// ```
///
/// P2 and P3 now both believe they hold the lock, on two different inodes, and
/// proceed to do a concurrent read-modify-write on the same settings file —
/// precisely the corruption this whole module exists to prevent. There is no
/// safe moment to delete: any window between another process's `open` and its
/// `flock` is enough. So we don't, ever.
///
/// **But they must not clutter the user's config directory either.** These files
/// sit in `~/.config/<app>/`, a directory users open and hand-edit, and one
/// `.lock` twin per `.toml` doubles its apparent contents with files that are
/// pure implementation detail.
///
/// The resolution is a dedicated `locks/` subdirectory: the sidecars are
/// out of sight, still permanent, and — critically — the path is still derived
/// *purely from the settings path*, with no dependency on the environment. That
/// last property is not negotiable. Keying the lock off `$XDG_RUNTIME_DIR` (or
/// any other ambient location) would be tidier still, but a process launched
/// without that variable set — from a systemd unit, a cron job, a different
/// session — would fall back to a *different* lock path and share no lock with
/// its peers at all. That is the same broken-mutual-exclusion failure as
/// deleting the file, only quieter: everything would appear to work until two
/// processes wrote at once.
fn sidecar_path(settings_path: &Path) -> PathBuf {
    let file_name = match settings_path.file_name() {
        Some(name) => format!("{}.lock", name.to_string_lossy()),
        None => "settings.lock".to_string(),
    };
    match settings_path.parent() {
        Some(dir) => dir.join(LOCK_DIR).join(file_name),
        // A bare relative file name (`"settings.toml"`) has an empty parent, not
        // `None`, so this is the degenerate `/` or `""` case. Keep the lock
        // beside it rather than inventing a path.
        None => PathBuf::from(file_name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
    fn acquire_then_drop_releases_the_lock() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.toml");

        let guard = FileLock::acquire_exclusive(&path).unwrap();
        drop(guard);

        // Should be immediately re-acquirable — the previous guard's
        // Drop released it.
        let _guard2 = FileLock::acquire_exclusive(&path).unwrap();
    }

    #[test]
    fn second_acquire_blocks_until_first_is_dropped() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.toml");

        let guard = FileLock::acquire_exclusive(&path).unwrap();
        let (tx, rx) = mpsc::channel();
        let path2 = path.clone();
        let handle = thread::spawn(move || {
            let _g = FileLock::acquire_exclusive(&path2).unwrap();
            tx.send(()).unwrap();
        });

        // The background thread should still be blocked on the lock.
        assert!(
            rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "second acquire should not succeed while the first guard is held"
        );

        drop(guard);

        rx.recv_timeout(Duration::from_secs(5))
            .expect("second lock should be acquired promptly after release");
        handle.join().unwrap();
    }

    #[test]
    fn sidecars_go_in_a_locks_subdir_not_beside_the_settings_file() {
        // F12: the sidecars used to sit directly beside the settings files, so
        // `~/.config/<app>/` showed a `.lock` twin for every `.toml` the user
        // actually edits. They now live in one `locks/` subdirectory.
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.toml");

        let _guard = FileLock::acquire_exclusive(&path).unwrap();

        assert!(
            dir.path().join(LOCK_DIR).join("s.toml.lock").exists(),
            "the lock belongs in the locks/ subdirectory"
        );
        assert!(
            !dir.path().join("s.toml.lock").exists(),
            "and must no longer clutter the settings directory itself"
        );
    }

    #[test]
    fn creates_parent_directories_for_the_sidecar() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested/deeper/s.toml");

        let _guard = FileLock::acquire_exclusive(&path).unwrap();
        assert!(
            dir.path()
                .join("nested/deeper")
                .join(LOCK_DIR)
                .join("s.toml.lock")
                .exists()
        );
    }

    /// Two settings files in one directory get two distinct locks — the
    /// `locks/` subdirectory must not collapse them onto one another, which
    /// would serialize every write in the app behind a single global lock.
    #[test]
    fn distinct_settings_files_get_distinct_locks() {
        let dir = tempdir().unwrap();
        let a = dir.path().join("general.toml");
        let b = dir.path().join("recents.toml");

        let _guard_a = FileLock::acquire_exclusive(&a).unwrap();
        // Must NOT block: a different file, therefore a different lock.
        let _guard_b = FileLock::try_acquire_exclusive(&b)
            .expect("a lock on one settings file must not block another");

        assert!(dir.path().join(LOCK_DIR).join("general.toml.lock").exists());
        assert!(dir.path().join(LOCK_DIR).join("recents.toml.lock").exists());
    }

    /// **The sidecar must survive `Drop`.** Deleting a locked file is the
    /// classic `flock` footgun: a process blocked on the old inode and a
    /// process that creates a fresh one would both end up "holding" the lock,
    /// and the concurrent read-modify-write this module exists to prevent
    /// happens anyway. This pins the never-delete invariant so a future
    /// "let's tidy up on exit" change fails loudly here.
    #[test]
    fn dropping_the_guard_releases_the_lock_but_never_removes_the_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.toml");
        let lock_file = dir.path().join(LOCK_DIR).join("s.toml.lock");

        let guard = FileLock::acquire_exclusive(&path).unwrap();
        assert!(lock_file.exists());
        drop(guard);

        assert!(
            lock_file.exists(),
            "the sidecar must outlive the guard — unlinking it breaks mutual exclusion"
        );
        // Released, though: immediately re-acquirable without blocking.
        let _reacquired = FileLock::try_acquire_exclusive(&path)
            .expect("Drop must have released the lock, even though the file remains");
    }

    /// F10: `try_acquire_exclusive` must never block. Before this method
    /// existed, the only way to attempt the lock was the blocking
    /// `acquire_exclusive`, which would have hung this test (and, in
    /// production, the shared settings-writer thread) for as long as the
    /// first guard is held. This asserts the *non-blocking* contract
    /// directly: contended -> immediate `WouldBlock`, released -> immediate
    /// success — both bounded by a tight wall-clock budget, not merely
    /// "eventually returns".
    #[test]
    fn try_acquire_exclusive_never_blocks() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.toml");

        let guard = FileLock::acquire_exclusive(&path).unwrap();

        let start = std::time::Instant::now();
        let err = match FileLock::try_acquire_exclusive(&path) {
            Ok(_) => panic!("a contended lock must fail, not succeed"),
            Err(e) => e,
        };
        let elapsed = start.elapsed();
        assert_eq!(
            err.kind(),
            io::ErrorKind::WouldBlock,
            "contended try_acquire_exclusive must fail with WouldBlock, got: {err:?}"
        );
        assert!(
            elapsed < Duration::from_millis(50),
            "try_acquire_exclusive must return immediately on contention, took {elapsed:?}"
        );

        drop(guard);

        let start = std::time::Instant::now();
        let _guard2 = FileLock::try_acquire_exclusive(&path)
            .expect("lock must be re-acquirable once the holder drops");
        assert!(
            start.elapsed() < Duration::from_millis(50),
            "an uncontended try_acquire_exclusive must also return immediately"
        );
    }
}
