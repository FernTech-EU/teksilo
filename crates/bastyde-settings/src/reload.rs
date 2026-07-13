// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`Reloadable`] — the contract a (separately-built) file watcher uses to
//! push a peer process's write into live state.
//!
//! Every persisted type in this crate is cross-process safe on the *write*
//! side (see `flush.rs`'s `Patch` design): a write always merges against
//! whatever is on disk, under a lock. That alone is not enough — a process
//! that loaded its state once and never looks again will not notice a peer's
//! write until it happens to mutate something itself. [`Reloadable`] is the
//! *read* side of the same story: a way for an external watcher (inotify /
//! FSEvents / ReadDirectoryChangesW, wired up outside this crate) to say
//! "the file changed, go look," without needing to know anything about the
//! concrete type it's reloading.
//!
//! ## The self-write-suppression contract
//!
//! A naive implementation would feed back into itself: this process writes
//! `general.toml`, the watcher notices *that very write* a few milliseconds
//! later, and calls `reload_from_disk()` — which had better be a cheap no-op,
//! not a full re-parse-and-notify cycle (and, worse, must never re-apply our
//! own value as if it were a peer's newer one, which could bounce a
//! just-superseded value back into a live `Signal` between the user's edit
//! and the debounced write landing).
//!
//! Every implementation therefore layers two checks, cheapest first:
//!
//! 1. **Stamp check.** Each implementor records the `(mtime, len)` of the
//!    file as of the last time it either wrote to it or read it. If the
//!    file's current stamp matches, `reload_from_disk` returns `Ok(false)`
//!    immediately — no read, no parse, nothing touched. This is the common
//!    case for a self-write notification.
//! 2. **Content backstop.** If the stamp *did* change (a real write happened,
//!    by us or a peer, since a filesystem's mtime resolution can coincide,
//!    or the write path didn't get a chance to update the stamp), the file
//!    is read and parsed, then compared *by value* against what's already
//!    live. Only a genuine difference is pushed into signals / models;
//!    `Ok(false)` is returned — again touching nothing — when the content
//!    is unchanged. This is the actual correctness guarantee; the stamp
//!    check above is purely an optimization to skip the common case cheaply.
//!
//! Implementors: [`crate::SettingsFile`], [`crate::SettingsStore`],
//! [`crate::PersistedListModel`], [`crate::WindowStateService`].

use std::path::Path;

use crate::file::SettingsFileError;

/// A persisted type that can be told "the file may have changed on disk —
/// go look," and will push any genuinely new content into its live
/// signals/models.
///
/// This is the hook a file-system watcher calls when it observes a write to
/// one of this crate's managed files. It is deliberately decoupled from any
/// particular watcher implementation (inotify, kqueue, ReadDirectoryChangesW)
/// — this crate only defines the contract; wiring an actual watcher onto it
/// is a separate concern (a file-watcher module built on top of this trait).
pub trait Reloadable {
    /// The file this instance reads from and writes to. A watcher uses this
    /// to know which path to associate with which `Reloadable` handle.
    fn path(&self) -> &Path;

    /// Re-read the file from disk and push any genuinely new content into
    /// live signals/models.
    ///
    /// Returns `Ok(true)` if the in-memory state changed as a result,
    /// `Ok(false)` if nothing needed to change (including the common
    /// self-write-notification case — see the module docs' "self-write
    /// suppression contract"). `Ok(false)` is a hard guarantee that nothing
    /// was touched: no signal fired, no model mutated.
    fn reload_from_disk(&self) -> Result<bool, SettingsFileError>;
}
