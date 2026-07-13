// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A small RAII advisory file-lock guard, used by [`crate::SettingsFile`]'s
//! locked read-modify-write (the *only* mode it has — see `file.rs`'s
//! module docs) to serialize that read-modify-write around a settings
//! file that more than one process may write to concurrently.
//!
//! ## Why a sidecar lock file, not locking the settings file itself
//!
//! `bastyde-settings` writes atomically via temp-file + rename (see
//! [`crate::flush::write_atomic`]). A lock held on the settings path
//! itself would be awkward to reconcile with that dance (the inode a
//! lock is held on can change out from under a lock holder across a
//! rename). Locking a stable sidecar path (`<path>.lock`) sidesteps
//! that entirely: the lock's identity never depends on what the
//! protected file's inode happens to be at any given moment.
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
//!   `bastyde-settings` needs since it targets desktop apps on all three
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
/// true of any non-Bastyde process that touches the same file by hand.
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
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // `unlock` failing here is not actionable by the caller (we are
        // already tearing down); log and move on rather than panic —
        // panicking in `Drop` during unwind would abort the process.
        if let Err(e) = FileExt::unlock(&self.file) {
            eprintln!("bastyde-settings: failed to release advisory lock: {e}");
        }
    }
}

fn sidecar_path(settings_path: &Path) -> PathBuf {
    let mut lock_path = settings_path.to_path_buf();
    let new_name = match settings_path.file_name() {
        Some(name) => format!("{}.lock", name.to_string_lossy()),
        None => "settings.lock".to_string(),
    };
    lock_path.set_file_name(new_name);
    lock_path
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
    fn creates_sidecar_lock_file_next_to_settings_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.toml");

        let _guard = FileLock::acquire_exclusive(&path).unwrap();
        assert!(dir.path().join("s.toml.lock").exists());
    }

    #[test]
    fn creates_parent_directories_for_the_sidecar() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested/deeper/s.toml");

        let _guard = FileLock::acquire_exclusive(&path).unwrap();
        assert!(dir.path().join("nested/deeper/s.toml.lock").exists());
    }
}
