// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`PersistedListModel<T>`] — bridge between a reactive
//! [`ListModel<T>`](bastyde_data::ListModel) and a single TOML file,
//! merging by **op**, not by whole-document snapshot.
//!
//! ## Why ops, not snapshots
//!
//! The previous design re-derived the *entire* `Vec<T>` from the live model
//! on every mutation and scheduled a debounced write of that whole
//! snapshot. That is last-write-wins by construction: if a peer process
//! added an entry to the same file in the meantime, this process's next
//! flush would overwrite the peer's row right off the disk — the exact
//! "a newly-opened project vanishes from Recents" bug this crate exists to
//! fix.
//!
//! Instead, every mutation records a small, **replayable** [`ListOp<T>`]
//! and hands it to the shared debounced writer as a [`crate::flush::Patch`]:
//! "given the file's current text, apply this one op to it." The patch is
//! applied against the document read **fresh off disk, under a lock**, at
//! flush time — so it replays cleanly on top of whatever a peer wrote in
//! the meantime, key by key, instead of overwriting the whole thing.
//!
//! ## Identity
//!
//! Every item needs a stable identity to merge by — see [`Keyed`]. Ops are
//! keyed, not indexed: `Remove` only needs to carry a key, never a value,
//! which is exactly what a diff of "what's gone" can always produce even
//! though the value itself is no longer available once removed.
//!
//! ## Mutating through this type, not through `.model()`
//!
//! `.model()` is for **reading** and for reactive binding (`ListView` /
//! `Repeater`) — every UI observer wants live updates regardless of who
//! mutates. Writing must go through [`upsert_front`](PersistedListModel::upsert_front),
//! [`update_in_place`](PersistedListModel::update_in_place),
//! [`remove`](PersistedListModel::remove) and
//! [`clear`](PersistedListModel::clear): those are the only places that both
//! mutate the live model *and* enqueue the matching op. Mutating the
//! `ListModel` returned by `.model()` directly updates what's on screen but
//! is never persisted — there is no observer bridging arbitrary model
//! mutations to disk any more (that observer *was* the whole-snapshot
//! overwrite bug).
//!
//! ## Example
//!
//! ```
//! use bastyde_settings::{Keyed, Migrator, PersistedListModel};
//! use serde::{Deserialize, Serialize};
//! use std::time::Duration;
//!
//! #[derive(Serialize, Deserialize, Clone)]
//! struct Tag { name: String }
//!
//! impl Keyed for Tag {
//!     type Key = String;
//!     fn key(&self) -> String { self.name.clone() }
//! }
//!
//! let path = std::env::temp_dir().join("tags-list-doctest.toml");
//! let plm: PersistedListModel<Tag> =
//!     PersistedListModel::open(path, Duration::ZERO, Migrator::new())
//!         .expect("open failed");
//! plm.upsert_front(Tag { name: "rust".into() });
//! plm.flush_now().expect("flush");
//! ```

use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bastyde_data::ListModel;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::file::{SettingsFileError, disk_stamp, quarantine, read_toml_with_retry};
use crate::flush::{DebouncedWriter, FlushError};
use crate::lock::FileLock;
use crate::migration::{Migrator, Versioned};
use crate::reload::Reloadable;

/// An item with a stable, owned identity — the merge key
/// [`PersistedListModel`] dedupes and diffs by.
///
/// `Key` is owned (not borrowed, unlike the old `MruEntry::Key: ?Sized`
/// shape) because it must be captured into a `Patch` (`crate::flush::Patch`)
/// closure that crosses to the shared I/O worker thread — a borrow into
/// `T` cannot outlive the mutation call that produced it.
pub trait Keyed {
    /// The key type. Typically `String` / `PathBuf` / a small `Copy` id.
    type Key: Eq + Hash + Clone + Send + 'static;

    /// This item's identity. Returned by value: cheap for the small key
    /// types this is meant for (clone a `String`/`PathBuf`/id), and it
    /// sidesteps borrow-lifetime issues entirely.
    fn key(&self) -> Self::Key;
}

/// A replayable mutation of a [`PersistedListModel`]'s backing list,
/// expressed **by key** so it can be applied to *any* starting `Vec<T>` —
/// in particular, the fresh one read off disk at flush time, which may
/// already include a peer process's concurrent changes.
#[derive(Debug, Clone)]
pub enum ListOp<T: Keyed> {
    /// Remove any existing entry with this item's key, then insert `T` at
    /// the front. This is the "most recently used" operation: re-running
    /// it against any starting vector — including one a peer has already
    /// mutated — reproduces the same dedupe-and-promote-to-front
    /// invariant `MruList::add` relies on.
    UpsertFront(T),
    /// Replace the entry with this item's key **in place** (no
    /// reordering). A no-op if the key is no longer present — e.g. a peer
    /// concurrently removed it, in which case that removal wins.
    UpdateInPlace(T),
    /// Remove the entry with this key, if present. No-op otherwise.
    Remove(T::Key),
    /// Drop every entry.
    Clear,
}

/// On-disk shape for a persisted list: a versioned wrapper around
/// `Vec<T>`. Apps write migrations against this type, not the bare `Vec`.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ListFile<T> {
    /// Schema version, matched against [`Versioned::CURRENT_VERSION`] on
    /// load to run any registered migrations before deserialization.
    #[serde(default = "default_version")]
    pub version: u32,
    /// The ordered list of items as stored on disk.
    #[serde(default = "Vec::new")]
    pub items: Vec<T>,
}

fn default_version() -> u32 {
    1
}

impl<T> Default for ListFile<T> {
    fn default() -> Self {
        Self {
            version: 1,
            items: Vec::new(),
        }
    }
}

/// `T` doesn't carry a version itself; the wrapper does. This impl is
/// parameterized on the version a particular app uses by way of the
/// `Versioned for ListFile<T>` instance the app produces. We provide a
/// default `CURRENT_VERSION = 1`; apps that bump the schema replace
/// the impl via newtype.
impl<T: 'static> Versioned for ListFile<T> {
    const CURRENT_VERSION: u32 = 1;
    fn version(&self) -> u32 {
        self.version
    }
    fn set_version(&mut self, v: u32) {
        self.version = v;
    }
}

/// A reactive, [`Keyed`]-item list whose mutations persist to a single
/// TOML file by merging **ops**, not by overwriting a whole-document
/// snapshot.
pub struct PersistedListModel<T>
where
    T: Keyed + Clone + Serialize + DeserializeOwned + Send + 'static,
{
    model: ListModel<T>,
    writer: DebouncedWriter,
    /// Retained for the handle's whole lifetime: every op-patch re-reads
    /// and re-migrates the on-disk document fresh (a peer might still be
    /// on an older schema), not just the one read at construction.
    migrator: Migrator<ListFile<T>>,
    /// `(mtime, len)` as of the last time this handle read or wrote the
    /// file — the cheap staleness / self-write-suppression stamp behind
    /// [`Reloadable::reload_from_disk`].
    last_known_stamp: std::cell::Cell<(Option<SystemTime>, Option<u64>)>,
}

impl<T> PersistedListModel<T>
where
    T: Keyed + Clone + Serialize + DeserializeOwned + Send + 'static,
{
    /// Open the file at `path` (running `migrator`, under an exclusive
    /// lock so a peer mid-write can't hand us a torn read), seed the model
    /// from its contents, and retain everything needed to enqueue op
    /// patches on every mutation.
    ///
    /// `delay` is the debounce window for writes — unlike
    /// [`crate::SettingsFile`], this type's writes are expected to be
    /// frequent (every `add`/`touch`/`remove` on a live MRU list), so the
    /// debounce is real and load-bearing here, not vestigial.
    pub fn open(
        path: PathBuf,
        delay: Duration,
        migrator: Migrator<ListFile<T>>,
    ) -> Result<Self, SettingsFileError> {
        let lock = FileLock::acquire_exclusive(&path).map_err(SettingsFileError::Io)?;
        let file = match read_list_or_default(&path, &migrator) {
            Ok(f) => f,
            Err(other) => {
                quarantine(&path);
                eprintln!(
                    "bastyde-settings: load failed for {}: {}; falling back to an empty list",
                    path.display(),
                    other,
                );
                ListFile::default()
            }
        };
        let stamp = disk_stamp(&path);
        drop(lock);

        let model = ListModel::from_vec(file.items);
        let writer = DebouncedWriter::new(path, delay);

        Ok(Self {
            model,
            writer,
            migrator,
            last_known_stamp: std::cell::Cell::new(stamp),
        })
    }

    /// The underlying reactive list handle. Clone it to share with
    /// `Repeater` / `ListView` widgets for **reading**. See the module
    /// docs: mutating the returned handle directly does not persist —
    /// use this type's own mutation methods instead.
    pub fn model(&self) -> &ListModel<T> {
        &self.model
    }

    /// Insert `item` at the front, deduping by `item.key()` (removing any
    /// existing entry with the same key first). Updates the live model
    /// immediately and enqueues the matching [`ListOp::UpsertFront`].
    pub fn upsert_front(&self, item: T) {
        if let Some(idx) = self.find_index(&item.key()) {
            self.model.remove(idx);
        }
        self.model.insert(0, item.clone());
        self.schedule_op(ListOp::UpsertFront(item));
    }

    /// Replace the entry with `item.key()` **in place** (no reordering).
    /// Returns `false` (and does nothing) if no entry with that key
    /// exists locally. Enqueues [`ListOp::UpdateInPlace`] on success.
    pub fn update_in_place(&self, item: T) -> bool {
        let Some(idx) = self.find_index(&item.key()) else {
            return false;
        };
        self.model.set(idx, item.clone());
        self.schedule_op(ListOp::UpdateInPlace(item));
        true
    }

    /// Remove the entry with this key, if present locally. Returns
    /// whether anything was removed. Enqueues [`ListOp::Remove`] on
    /// success.
    pub fn remove(&self, key: &T::Key) -> bool {
        let Some(idx) = self.find_index(key) else {
            return false;
        };
        self.model.remove(idx);
        self.schedule_op(ListOp::Remove(key.clone()));
        true
    }

    /// Drop every entry, locally and on disk.
    pub fn clear(&self) {
        self.model.clear();
        self.schedule_op(ListOp::Clear);
    }

    /// Flush any pending op(s) to disk immediately, bypassing the
    /// debounce window. Flushes the **op queue** — never a re-derived
    /// snapshot of the in-memory list, which is exactly the mechanism
    /// that used to let a cleanly-exiting process erase a peer's
    /// newly-added entry.
    pub fn flush_now(&self) -> Result<(), SettingsFileError> {
        self.writer.flush_now().map_err(SettingsFileError::Flush)?;
        self.last_known_stamp.set(disk_stamp(self.writer.path()));
        Ok(())
    }

    /// The absolute path of the TOML file being written to.
    pub fn path(&self) -> &Path {
        self.writer.path()
    }

    fn find_index(&self, key: &T::Key) -> Option<usize> {
        let model = &self.model;
        (0..model.len()).find(|&i| model.with_item(i, |t| t.key() == *key).unwrap_or(false))
    }

    fn schedule_op(&self, op: ListOp<T>) {
        let migrator = self.migrator.clone();
        let patch: crate::flush::Patch = Box::new(move |current: Option<String>| {
            let file = parse_list_file_text(current.as_deref(), &migrator)
                .map_err(|e| FlushError::Merge(e.to_string()))?;
            let mut items = file.items;
            apply_list_op(&mut items, &op);
            let new_file = ListFile {
                version: <ListFile<T> as Versioned>::CURRENT_VERSION,
                items,
            };
            toml::to_string_pretty(&new_file).map_err(|e| FlushError::Merge(e.to_string()))
        });
        self.writer.schedule(patch);
    }
}

/// Apply a single [`ListOp`] to `items` (the freshly-read-from-disk
/// vector), by key. This is the actual merge: it never looks at what this
/// process's in-memory list looked like, only at `items` as given.
fn apply_list_op<T: Keyed + Clone>(items: &mut Vec<T>, op: &ListOp<T>) {
    match op {
        ListOp::UpsertFront(item) => {
            let key = item.key();
            items.retain(|t| t.key() != key);
            items.insert(0, item.clone());
        }
        ListOp::UpdateInPlace(item) => {
            let key = item.key();
            if let Some(slot) = items.iter_mut().find(|t| t.key() == key) {
                *slot = item.clone();
            }
            // No-op if the key is gone — a peer's concurrent removal wins.
        }
        ListOp::Remove(key) => {
            items.retain(|t| t.key() != *key);
        }
        ListOp::Clear => {
            items.clear();
        }
    }
}

/// Read `path`'s TOML (retrying on transient parse failure) and run
/// `migrator`, falling back to an empty [`ListFile`] if the file is
/// absent.
fn read_list_or_default<T>(
    path: &Path,
    migrator: &Migrator<ListFile<T>>,
) -> Result<ListFile<T>, SettingsFileError>
where
    T: Clone + Serialize + DeserializeOwned + 'static,
{
    match read_toml_with_retry(path)? {
        Some(raw) => {
            let mut file = migrator.run(raw).map_err(SettingsFileError::Migrate)?;
            file.version = <ListFile<T> as Versioned>::CURRENT_VERSION;
            Ok(file)
        }
        None => Ok(ListFile::default()),
    }
}

/// Like [`read_list_or_default`], but parses already-in-hand text (used
/// from inside a [`crate::flush::Patch`] closure, which receives the
/// current text directly rather than a path to re-read) instead of a
/// missing file falling back on `NotFound` — `None` (no file yet) is
/// handled the same way either way.
fn parse_list_file_text<T>(
    text: Option<&str>,
    migrator: &Migrator<ListFile<T>>,
) -> Result<ListFile<T>, SettingsFileError>
where
    T: Clone + Serialize + DeserializeOwned + 'static,
{
    match text {
        Some(text) => {
            let raw: toml::Value = toml::from_str(text).map_err(SettingsFileError::Parse)?;
            let mut file = migrator.run(raw).map_err(SettingsFileError::Migrate)?;
            file.version = <ListFile<T> as Versioned>::CURRENT_VERSION;
            Ok(file)
        }
        None => Ok(ListFile::default()),
    }
}

impl<T> Reloadable for PersistedListModel<T>
where
    T: Keyed + Clone + Serialize + DeserializeOwned + Send + PartialEq + 'static,
{
    fn path(&self) -> &Path {
        PersistedListModel::path(self)
    }

    fn reload_from_disk(&self) -> Result<bool, SettingsFileError> {
        // Flush OUR OWN pending queue before reading, so a peer's write
        // landing mid-debounce can never make the model transiently drop
        // a local, not-yet-flushed change (F14): without this, a fresh
        // read here would reflect the peer's write but NOT our own
        // still-queued op, and reconciling down to that snapshot would
        // visibly revert the user's just-performed action until our own
        // debounced write landed moments later on its own.
        //
        // Deliberately bypasses the public `flush_now()` wrapper: that
        // wrapper also unconditionally restamps `last_known_stamp` to
        // "disk state right now", which here would make the staleness
        // check just below always pass and skip the read this function
        // exists to perform — silently discarding the very peer write
        // that triggered the reload. Calling `self.writer.flush_now()`
        // directly flushes our queue without touching the stamp, so the
        // existing comparison below runs against the OLD (pre-flush)
        // stamp, correctly detects the change, and proceeds into a real
        // read+reconcile that sees peer and ours already merged (the
        // locked read-merge-write inside the op patch guarantees that).
        if let Err(e) = self.writer.flush_now() {
            eprintln!(
                "bastyde-settings: pre-reload flush of {} failed: {e}; reloading anyway",
                self.writer.path().display(),
            );
        }

        let path = self.writer.path();
        let current_stamp = disk_stamp(path);
        if current_stamp == self.last_known_stamp.get() {
            return Ok(false);
        }

        let file = read_list_or_default(path, &self.migrator)?;
        self.last_known_stamp.set(current_stamp);

        let current: Vec<T> = (0..self.model.len())
            .filter_map(|i| self.model.with_item(i, |t| t.clone()))
            .collect();
        if current == file.items {
            return Ok(false);
        }

        self.model.reconcile_by_key(file.items, |t| t.key());
        Ok(true)
    }
}

impl<T> std::fmt::Debug for PersistedListModel<T>
where
    T: Keyed + Clone + Serialize + DeserializeOwned + Send + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersistedListModel")
            .field("path", &self.writer.path())
            .field("len", &self.model.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::collections::HashSet;
    use std::fs;
    use tempfile::tempdir;

    #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
    struct Item {
        name: String,
        count: i32,
    }

    impl Keyed for Item {
        type Key = String;
        fn key(&self) -> String {
            self.name.clone()
        }
    }

    fn item(name: &str, count: i32) -> Item {
        Item {
            name: name.into(),
            count,
        }
    }

    #[test]
    fn fresh_file_starts_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("list.toml");
        let plm: PersistedListModel<Item> =
            PersistedListModel::open(path, Duration::ZERO, Migrator::new()).unwrap();
        assert_eq!(plm.model().len(), 0);
    }

    #[test]
    fn upsert_front_persists_and_reopens() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("list.toml");

        {
            let plm: PersistedListModel<Item> =
                PersistedListModel::open(path.clone(), Duration::ZERO, Migrator::new()).unwrap();
            plm.upsert_front(item("a", 1));
            plm.upsert_front(item("b", 2));
            plm.flush_now().unwrap();
        }

        let plm: PersistedListModel<Item> =
            PersistedListModel::open(path, Duration::ZERO, Migrator::new()).unwrap();
        assert_eq!(plm.model().len(), 2);
        assert_eq!(
            plm.model().with_item(0, |x| x.clone()).unwrap(),
            item("b", 2)
        );
        assert_eq!(
            plm.model().with_item(1, |x| x.clone()).unwrap(),
            item("a", 1)
        );
    }

    #[test]
    fn upsert_front_dedupes_by_key() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("list.toml");
        let plm: PersistedListModel<Item> =
            PersistedListModel::open(path, Duration::ZERO, Migrator::new()).unwrap();

        plm.upsert_front(item("a", 1));
        plm.upsert_front(item("b", 2));
        plm.upsert_front(item("a", 99));

        assert_eq!(plm.model().len(), 2);
        assert_eq!(
            plm.model().with_item(0, |x| x.clone()).unwrap(),
            item("a", 99)
        );
        assert_eq!(
            plm.model().with_item(1, |x| x.clone()).unwrap(),
            item("b", 2)
        );
    }

    #[test]
    fn update_in_place_does_not_reorder() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("list.toml");
        let plm: PersistedListModel<Item> =
            PersistedListModel::open(path, Duration::ZERO, Migrator::new()).unwrap();

        plm.upsert_front(item("a", 1));
        plm.upsert_front(item("b", 2));
        assert!(plm.update_in_place(item("a", 42)));

        assert_eq!(
            plm.model().with_item(0, |x| x.clone()).unwrap(),
            item("b", 2)
        );
        assert_eq!(
            plm.model().with_item(1, |x| x.clone()).unwrap(),
            item("a", 42)
        );
    }

    #[test]
    fn update_in_place_returns_false_for_missing_key() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("list.toml");
        let plm: PersistedListModel<Item> =
            PersistedListModel::open(path, Duration::ZERO, Migrator::new()).unwrap();
        assert!(!plm.update_in_place(item("ghost", 0)));
    }

    #[test]
    fn remove_drops_entry_and_persists() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("list.toml");
        let plm: PersistedListModel<Item> =
            PersistedListModel::open(path.clone(), Duration::ZERO, Migrator::new()).unwrap();

        plm.upsert_front(item("a", 1));
        plm.upsert_front(item("b", 2));
        assert!(plm.remove(&"a".to_string()));
        plm.flush_now().unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        let parsed: ListFile<Item> = toml::from_str(&raw).unwrap();
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].name, "b");
    }

    #[test]
    fn clear_empties_and_persists() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("list.toml");
        let plm: PersistedListModel<Item> =
            PersistedListModel::open(path.clone(), Duration::ZERO, Migrator::new()).unwrap();
        plm.upsert_front(item("a", 1));
        plm.clear();
        plm.flush_now().unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        let parsed: ListFile<Item> = toml::from_str(&raw).unwrap();
        assert!(parsed.items.is_empty());
    }

    // -----------------------------------------------------------------
    // THE HEADLINE TEST — the merge, not the snapshot.
    // -----------------------------------------------------------------

    /// Two independent handles over the *same* file — standing in for two
    /// Skribisto processes sharing `recents.toml` — each `upsert_front` a
    /// *different* entry with no coordination between them. Because every
    /// op merges against the document read fresh under the lock at flush
    /// time, both entries must survive: neither handle's op can see, let
    /// alone erase, the other's addition.
    #[test]
    fn two_concurrent_handles_each_adding_a_different_entry_both_survive() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("shared_list.toml");

        let a: PersistedListModel<Item> =
            PersistedListModel::open(path.clone(), Duration::ZERO, Migrator::new()).unwrap();
        let b: PersistedListModel<Item> =
            PersistedListModel::open(path.clone(), Duration::ZERO, Migrator::new()).unwrap();

        a.upsert_front(item("alpha", 1));
        a.flush_now().unwrap();
        b.upsert_front(item("beta", 2));
        b.flush_now().unwrap();

        // A third, fresh handle proves both are actually on disk together.
        let c: PersistedListModel<Item> =
            PersistedListModel::open(path, Duration::ZERO, Migrator::new()).unwrap();
        let mut names: Vec<String> = (0..c.model().len())
            .map(|i| c.model().with_item(i, |x| x.name.clone()).unwrap())
            .collect();
        names.sort();
        assert_eq!(names, vec!["alpha".to_string(), "beta".to_string()]);
    }

    /// The bug this replaces: a whole-snapshot design would have `b`'s
    /// flush re-serialize *its own* in-memory list (which never saw `a`'s
    /// addition) and overwrite it on disk. With ops, `b`'s patch only ever
    /// touches `beta`'s key.
    #[test]
    fn a_peers_addition_is_not_erased_by_a_later_unrelated_flush() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("no_clobber.toml");

        let a: PersistedListModel<Item> =
            PersistedListModel::open(path.clone(), Duration::ZERO, Migrator::new()).unwrap();
        let b: PersistedListModel<Item> =
            PersistedListModel::open(path.clone(), Duration::ZERO, Migrator::new()).unwrap();

        a.upsert_front(item("from-a", 1));
        a.flush_now().unwrap();

        // b never saw a's write (no reload) — its own mutation must
        // still not clobber a's entry on disk.
        b.upsert_front(item("from-b", 2));
        b.flush_now().unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        let parsed: ListFile<Item> = toml::from_str(&raw).unwrap();
        let names: HashSet<String> = parsed.items.iter().map(|i| i.name.clone()).collect();
        assert!(names.contains("from-a"), "a's entry must survive");
        assert!(names.contains("from-b"), "b's entry must be present too");
    }

    #[test]
    fn multiple_ops_in_one_debounce_window_all_land() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("burst.toml");
        let plm: PersistedListModel<Item> =
            PersistedListModel::open(path, Duration::from_millis(200), Migrator::new()).unwrap();

        for i in 0..5 {
            plm.upsert_front(item(&format!("item{i}"), i));
        }
        plm.flush_now().unwrap();
        assert_eq!(plm.model().len(), 5);
    }

    // -----------------------------------------------------------------
    // Reloadable
    // -----------------------------------------------------------------

    #[test]
    fn reload_from_disk_picks_up_a_peers_addition() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reload_list.toml");

        let a: PersistedListModel<Item> =
            PersistedListModel::open(path.clone(), Duration::ZERO, Migrator::new()).unwrap();
        let b: PersistedListModel<Item> =
            PersistedListModel::open(path, Duration::ZERO, Migrator::new()).unwrap();

        a.upsert_front(item("peer-item", 1));
        a.flush_now().unwrap();

        assert!(Reloadable::reload_from_disk(&b).unwrap());
        assert_eq!(b.model().len(), 1);
        assert_eq!(
            b.model().with_item(0, |x| x.name.clone()).unwrap(),
            "peer-item"
        );
    }

    #[test]
    fn reload_from_disk_returns_false_when_unchanged() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reload_unchanged.toml");
        let a: PersistedListModel<Item> =
            PersistedListModel::open(path, Duration::ZERO, Migrator::new()).unwrap();
        assert!(!Reloadable::reload_from_disk(&a).unwrap());
    }

    #[test]
    fn reload_from_disk_preserves_positions_of_unrelated_items() {
        // A peer adds a new item; this handle's existing items must not
        // be reshuffled by the reconciliation (only the addition should
        // cause a change).
        let dir = tempdir().unwrap();
        let path = dir.path().join("reload_stable.toml");

        let a: PersistedListModel<Item> =
            PersistedListModel::open(path.clone(), Duration::ZERO, Migrator::new()).unwrap();
        a.upsert_front(item("first", 1));
        a.upsert_front(item("second", 2));
        a.flush_now().unwrap();

        let b: PersistedListModel<Item> =
            PersistedListModel::open(path.clone(), Duration::ZERO, Migrator::new()).unwrap();
        assert_eq!(
            b.model().with_item(0, |x| x.name.clone()).unwrap(),
            "second"
        );
        assert_eq!(b.model().with_item(1, |x| x.name.clone()).unwrap(), "first");

        a.upsert_front(item("third", 3));
        a.flush_now().unwrap();

        assert!(Reloadable::reload_from_disk(&b).unwrap());
        assert_eq!(b.model().len(), 3);
        assert_eq!(b.model().with_item(0, |x| x.name.clone()).unwrap(), "third");
        assert_eq!(
            b.model().with_item(1, |x| x.name.clone()).unwrap(),
            "second"
        );
        assert_eq!(b.model().with_item(2, |x| x.name.clone()).unwrap(), "first");
    }

    /// F14 repro: a local, not-yet-flushed `upsert_front` must survive a
    /// `reload_from_disk` triggered by a peer's concurrent write, instead
    /// of being transiently reverted by reconciling down to a disk
    /// snapshot that doesn't yet contain our own queued op. A non-zero
    /// debounce window is required so the op is still pending (not
    /// already auto-flushed) when `reload_from_disk` runs.
    #[test]
    fn reload_from_disk_does_not_revert_a_local_not_yet_flushed_change() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("f14.toml");

        // Seed the file with a peer's baseline entry and let both handles
        // observe it, so `a`'s later reload has a real stamp to compare
        // against.
        let seed: PersistedListModel<Item> =
            PersistedListModel::open(path.clone(), Duration::ZERO, Migrator::new()).unwrap();
        seed.upsert_front(item("peer-baseline", 0));
        seed.flush_now().unwrap();
        drop(seed);

        let a: PersistedListModel<Item> =
            PersistedListModel::open(path.clone(), Duration::from_secs(3600), Migrator::new())
                .unwrap();
        assert_eq!(a.model().len(), 1);

        // Local action: lands in memory immediately, but with an hour-long
        // debounce the write to disk is still pending.
        a.upsert_front(item("X", 1));
        assert_eq!(
            a.model().with_item(0, |x| x.name.clone()).unwrap(),
            "X",
            "X must be at the front in memory right away"
        );

        // A peer writes a DIFFERENT valid ListFile directly to the same
        // path, bypassing `a` entirely — it does NOT contain X.
        let peer_file = ListFile {
            version: 1,
            items: vec![item("peer-baseline", 0), item("peer-new", 2)],
        };
        fs::write(&path, toml::to_string_pretty(&peer_file).unwrap()).unwrap();

        // Old (pre-F14) behavior: this reload would read the peer's
        // snapshot (no X in it) and reconcile the live model down to
        // exactly that, erasing X from the front of the list until a's own
        // debounced write eventually landed on its own.
        let changed = Reloadable::reload_from_disk(&a).unwrap();
        assert!(changed, "the peer's write must be observed as a change");

        let names_after: Vec<String> = (0..a.model().len())
            .map(|i| a.model().with_item(i, |x| x.name.clone()).unwrap())
            .collect();
        assert!(
            names_after.contains(&"X".to_string()),
            "a's own not-yet-flushed change must survive reload_from_disk, got {names_after:?}"
        );
        assert!(
            names_after.contains(&"peer-new".to_string()),
            "the peer's concurrent addition must also be present, got {names_after:?}"
        );

        // And the merge must have actually reached disk too: both the
        // peer's entry and our own pending op were flushed by the
        // pre-reload flush inside `reload_from_disk`.
        let raw = fs::read_to_string(&path).unwrap();
        let parsed: ListFile<Item> = toml::from_str(&raw).unwrap();
        let on_disk: HashSet<String> = parsed.items.iter().map(|i| i.name.clone()).collect();
        assert!(on_disk.contains("X"), "X must have reached disk too");
        assert!(
            on_disk.contains("peer-new"),
            "the peer's entry must still be on disk too"
        );
    }
}
