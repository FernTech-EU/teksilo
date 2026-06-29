// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Most-recently-used list — a generic, persisted reactive collection
//! with dedupe, pinning, and LRU-style cap eviction.
//!
//! Apps define their own item type by implementing [`MruEntry`]: a small
//! trait that exposes a dedupe key, an optional pin flag, and an optional
//! touch hook. The framework handles dedupe-on-add, pin-aware cap eviction,
//! and debounced disk persistence; the app owns the item schema and
//! semantics (e.g. updating a `last_opened` timestamp in `touch`).
//!
//! ## When to use
//!
//! Use [`MruList`] for any "recently opened / recently used" feature:
//! recent files, recent projects, recently visited locations, recently used
//! palette entries, etc. The backing [`ListModel<T>`] is the same reactive
//! handle you bind to a [`ListView`](bastyde_data::ListModel) or iterate in
//! a menu — no separate notification plumbing is required.
//!
//! ## Persistence
//!
//! [`MruList::open`] reads `<config_dir>/<name>.toml` on first access and
//! writes it back (atomically, via a temp-and-rename) after every mutation,
//! subject to the debounce window. Pass [`Duration::ZERO`] in tests to flush
//! synchronously, or call [`MruList::flush_now`] explicitly.
//!
//! ```ignore
//! use std::path::{Path, PathBuf};
//! use std::time::Duration;
//! use bastyde_settings::{AppPaths, MruEntry, MruList};
//! use serde::{Serialize, Deserialize};
//!
//! #[derive(Serialize, Deserialize, Clone)]
//! struct RecentProject {
//!     path: PathBuf,
//!     display_name: String,
//!     last_opened: u64,
//!     pinned: bool,
//! }
//!
//! impl MruEntry for RecentProject {
//!     type Key = Path;
//!     fn key(&self) -> &Path { &self.path }
//!     fn is_pinned(&self) -> bool { self.pinned }
//!     fn set_pinned(&mut self, p: bool) { self.pinned = p; }
//!     fn touch(&mut self) { self.last_opened += 1; }
//! }
//!
//! // In tests: AppPaths::for_testing(tmp.path()) + Duration::ZERO.
//! // In production: AppPaths::new(qualifier, org, app).
//! let tmp = tempfile::tempdir().unwrap();
//! let paths = AppPaths::for_testing(tmp.path());
//! let recents: MruList<RecentProject> =
//!     MruList::open_with_delay(&paths, "recents", 10, Duration::ZERO).unwrap();
//!
//! recents.add(RecentProject {
//!     path: "/projects/foo".into(),
//!     display_name: "Foo".into(),
//!     last_opened: 0,
//!     pinned: false,
//! });
//! assert_eq!(recents.model().len(), 1);
//! ```

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use bastyde_data::ListModel;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::collection::list::PersistedListModel;
use crate::file::SettingsFileError;
use crate::migration::Migrator;
use crate::path::AppPaths;
use crate::store::DEFAULT_DEBOUNCE;

/// An item that can live in an [`MruList`].
///
/// `Key` may be unsized (e.g. `Path`, `str`) — `key()` returns a
/// reference, and equality is performed on the dereferenced value.
pub trait MruEntry: Clone + Serialize + DeserializeOwned + 'static {
    /// The dedupe key. Two entries whose keys compare equal are
    /// considered "the same item" — adding the second replaces the
    /// first (preserving the prior pin state by default).
    type Key: PartialEq + ?Sized + 'static;

    /// Borrow the key for this entry.
    fn key(&self) -> &Self::Key;

    /// Whether this entry should resist eviction by `cap_to_max`.
    /// Default: never pinned.
    fn is_pinned(&self) -> bool {
        false
    }

    /// Set the pinned flag. Default: ignore.
    fn set_pinned(&mut self, _pinned: bool) {}

    /// Hook called by [`MruList::add`] and [`MruList::touch`] to mark
    /// this entry as freshly used. Apps that track a `last_opened`
    /// timestamp update it here. Default: no-op.
    fn touch(&mut self) {}
}

/// A persisted MRU list backed by [`PersistedListModel<T>`].
///
/// Cheap to clone (`Rc`-shared internally). The reactive
/// [`ListModel<T>`](bastyde_data::ListModel) returned by [`model()`](Self::model) is the same
/// handle the persistence bridge observes; mutating it through any
/// clone fires both the on-screen UI updates and the debounced
/// disk flush.
pub struct MruList<T: MruEntry> {
    persisted: Rc<PersistedListModel<T>>,
    max_items: usize,
}

impl<T: MruEntry> Clone for MruList<T> {
    fn clone(&self) -> Self {
        Self {
            persisted: Rc::clone(&self.persisted),
            max_items: self.max_items,
        }
    }
}

impl<T: MruEntry> MruList<T> {
    /// Open at `<paths.config_dir()>/<name>.toml` with the default debounce window.
    ///
    /// Creates the file (and any missing parent directories) if it does not
    /// yet exist. Use [`open_with_delay`](Self::open_with_delay) to override
    /// the debounce in tests.
    pub fn open(paths: &AppPaths, name: &str, max_items: usize) -> Result<Self, SettingsFileError> {
        Self::open_with_delay(paths, name, max_items, DEFAULT_DEBOUNCE)
    }

    /// Open at `<paths.config_dir()>/<name>.toml` with a custom debounce window.
    ///
    /// Pass [`Duration::ZERO`](std::time::Duration::ZERO) in tests to flush
    /// every mutation synchronously.
    pub fn open_with_delay(
        paths: &AppPaths,
        name: &str,
        max_items: usize,
        delay: Duration,
    ) -> Result<Self, SettingsFileError> {
        Self::open_at(paths.config_file(name), max_items, delay)
    }

    /// Open at an explicit path with the given debounce window.
    ///
    /// Lower-level alternative to [`open`](Self::open) when the caller
    /// already has a resolved [`PathBuf`] (e.g. from a custom directory layout).
    pub fn open_at(
        path: PathBuf,
        max_items: usize,
        delay: Duration,
    ) -> Result<Self, SettingsFileError> {
        let persisted = PersistedListModel::open(path, delay, Migrator::new())?;
        Ok(Self {
            persisted: Rc::new(persisted),
            max_items,
        })
    }

    /// The underlying reactive list; bind to UI widgets via clones of this handle.
    ///
    /// This is the same [`ListModel<T>`](bastyde_data::ListModel) the
    /// persistence bridge observes — any mutation schedules a debounced
    /// disk flush automatically.
    pub fn model(&self) -> &ListModel<T> {
        self.persisted.model()
    }

    /// Returns the maximum number of unpinned entries kept in the list.
    ///
    /// Pinned entries do not count toward this cap and are never evicted
    /// automatically.
    pub fn max_items(&self) -> usize {
        self.max_items
    }

    /// Insert `entry` at the front, deduping by `entry.key()`.
    /// `T::touch` is invoked before insertion, so the freshly-added
    /// entry reflects "now". If a previously-pinned entry is re-added
    /// without `pinned`, the pin state is preserved.
    pub fn add(&self, mut entry: T) {
        let model = self.persisted.model();

        // Dedupe by key. We have to look up by `entry.key()` while
        // entry is still owned, so capture the existing pin state and
        // index in one pass before touching/inserting.
        let existing = (0..model.len()).find_map(|i| {
            model
                .with_item(i, |t| {
                    if t.key() == entry.key() {
                        Some((i, t.is_pinned()))
                    } else {
                        None
                    }
                })
                .flatten()
        });
        if let Some((i, was_pinned)) = existing {
            if was_pinned && !entry.is_pinned() {
                entry.set_pinned(true);
            }
            model.remove(i);
        }

        entry.touch();
        model.insert(0, entry);
        self.cap_to_max();
    }

    /// Remove the entry whose key matches, then schedule a debounced flush.
    ///
    /// No-op when no entry with that key is present.
    pub fn remove(&self, key: &T::Key) {
        if let Some(idx) = self.find_index(key) {
            self.persisted.model().remove(idx);
        }
    }

    /// Mark the entry whose key matches as freshly used by calling
    /// [`MruEntry::touch`] on a clone of it, then write it back and
    /// schedule a debounced flush. No-op when no entry matches.
    pub fn touch(&self, key: &T::Key) {
        if let Some(idx) = self.find_index(key) {
            let model = self.persisted.model();
            let mut updated = match model.with_item(idx, |t| t.clone()) {
                Some(v) => v,
                None => return,
            };
            updated.touch();
            model.set(idx, updated);
        }
    }

    /// Flip the pin flag of the entry whose key matches, then schedule a
    /// debounced flush. No-op when no entry matches.
    pub fn toggle_pin(&self, key: &T::Key) {
        if let Some(idx) = self.find_index(key) {
            let model = self.persisted.model();
            let mut updated = match model.with_item(idx, |t| t.clone()) {
                Some(v) => v,
                None => return,
            };
            updated.set_pinned(!updated.is_pinned());
            model.set(idx, updated);
        }
    }

    /// Drop every entry (pinned or not) and schedule a debounced flush.
    pub fn clear(&self) {
        self.persisted.model().clear();
    }

    /// Write the list to disk synchronously, bypassing the debounce window.
    ///
    /// Useful at app shutdown or at the end of a test to guarantee the
    /// file reflects the in-memory state before the process exits.
    pub fn flush_now(&self) -> Result<(), SettingsFileError> {
        self.persisted.flush_now()
    }

    /// The TOML file path this list reads from and writes to.
    pub fn path(&self) -> &Path {
        self.persisted.path()
    }

    fn find_index(&self, key: &T::Key) -> Option<usize> {
        let model = self.persisted.model();
        (0..model.len()).find(|&i| model.with_item(i, |t| t.key() == key).unwrap_or(false))
    }

    fn cap_to_max(&self) {
        let model = self.persisted.model();
        let len = model.len();

        let mut unpinned = 0usize;
        for i in 0..len {
            if model.with_item(i, |t| !t.is_pinned()).unwrap_or(false) {
                unpinned += 1;
            }
        }
        if unpinned <= self.max_items {
            return;
        }
        let mut to_drop = unpinned - self.max_items;

        let mut i = len;
        while i > 0 && to_drop > 0 {
            i -= 1;
            let is_unpinned = model.with_item(i, |t| !t.is_pinned()).unwrap_or(false);
            if is_unpinned {
                model.remove(i);
                to_drop -= 1;
            }
        }
    }
}

impl<T: MruEntry> std::fmt::Debug for MruList<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MruList")
            .field("len", &self.persisted.model().len())
            .field("max_items", &self.max_items)
            .field("path", &self.persisted.path())
            .field("entry_type", &std::any::type_name::<T>())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;

    #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
    struct DemoItem {
        path: PathBuf,
        name: String,
        opened_at: u64,
        pinned: bool,
    }

    impl DemoItem {
        fn new(path: &str, name: &str) -> Self {
            Self {
                path: path.into(),
                name: name.into(),
                opened_at: 0,
                pinned: false,
            }
        }
        fn pinned(mut self) -> Self {
            self.pinned = true;
            self
        }
    }

    impl MruEntry for DemoItem {
        type Key = Path;
        fn key(&self) -> &Path {
            &self.path
        }
        fn is_pinned(&self) -> bool {
            self.pinned
        }
        fn set_pinned(&mut self, p: bool) {
            self.pinned = p;
        }
        fn touch(&mut self) {
            self.opened_at += 1;
        }
    }

    fn open(dir: &Path, max: usize) -> MruList<DemoItem> {
        let paths = AppPaths::for_testing(dir);
        MruList::open_with_delay(&paths, "mru", max, Duration::ZERO).unwrap()
    }

    #[test]
    fn add_pushes_to_front() {
        let dir = tempdir().unwrap();
        let mru = open(dir.path(), 5);
        mru.add(DemoItem::new("/a", "A"));
        mru.add(DemoItem::new("/b", "B"));
        assert_eq!(mru.model().len(), 2);
        assert_eq!(mru.model().with_item(0, |i| i.name.clone()).unwrap(), "B");
    }

    #[test]
    fn add_dedupes_by_key() {
        let dir = tempdir().unwrap();
        let mru = open(dir.path(), 5);
        mru.add(DemoItem::new("/a", "A"));
        mru.add(DemoItem::new("/b", "B"));
        mru.add(DemoItem::new("/a", "A again"));
        // Two entries: A-renamed at front, B after.
        assert_eq!(mru.model().len(), 2);
        assert_eq!(
            mru.model().with_item(0, |i| i.name.clone()).unwrap(),
            "A again"
        );
        assert_eq!(mru.model().with_item(1, |i| i.name.clone()).unwrap(), "B");
    }

    #[test]
    fn add_preserves_pin_on_dedupe() {
        let dir = tempdir().unwrap();
        let mru = open(dir.path(), 5);
        mru.add(DemoItem::new("/a", "A").pinned());
        mru.add(DemoItem::new("/a", "A renamed")); // not pinned in arg
        assert!(mru.model().with_item(0, |i| i.pinned).unwrap());
    }

    #[test]
    fn touch_invokes_entry_hook() {
        let dir = tempdir().unwrap();
        let mru = open(dir.path(), 5);
        mru.add(DemoItem::new("/a", "A")); // touch -> opened_at = 1
        let before = mru.model().with_item(0, |i| i.opened_at).unwrap();
        mru.touch(Path::new("/a"));
        let after = mru.model().with_item(0, |i| i.opened_at).unwrap();
        assert!(after > before);
    }

    #[test]
    fn cap_drops_oldest_unpinned() {
        let dir = tempdir().unwrap();
        let mru = open(dir.path(), 2);
        mru.add(DemoItem::new("/a", "A"));
        mru.add(DemoItem::new("/b", "B"));
        mru.add(DemoItem::new("/c", "C"));
        // Should keep the 2 most recent: C, B.
        let names: Vec<String> = (0..mru.model().len())
            .map(|i| mru.model().with_item(i, |x| x.name.clone()).unwrap())
            .collect();
        assert_eq!(names, vec!["C", "B"]);
    }

    #[test]
    fn pinned_survives_cap() {
        let dir = tempdir().unwrap();
        let mru = open(dir.path(), 2);
        mru.add(DemoItem::new("/a", "A").pinned());
        mru.add(DemoItem::new("/b", "B"));
        mru.add(DemoItem::new("/c", "C"));
        mru.add(DemoItem::new("/d", "D"));
        let mut names: Vec<String> = (0..mru.model().len())
            .map(|i| mru.model().with_item(i, |x| x.name.clone()).unwrap())
            .collect();
        names.sort();
        assert_eq!(names, vec!["A", "C", "D"]);
    }

    #[test]
    fn toggle_pin_flips_state() {
        let dir = tempdir().unwrap();
        let mru = open(dir.path(), 5);
        mru.add(DemoItem::new("/a", "A"));
        assert!(!mru.model().with_item(0, |i| i.pinned).unwrap());
        mru.toggle_pin(Path::new("/a"));
        assert!(mru.model().with_item(0, |i| i.pinned).unwrap());
        mru.toggle_pin(Path::new("/a"));
        assert!(!mru.model().with_item(0, |i| i.pinned).unwrap());
    }

    #[test]
    fn remove_drops_entry() {
        let dir = tempdir().unwrap();
        let mru = open(dir.path(), 5);
        mru.add(DemoItem::new("/a", "A"));
        mru.add(DemoItem::new("/b", "B"));
        mru.remove(Path::new("/a"));
        assert_eq!(mru.model().len(), 1);
    }

    #[test]
    fn persists_across_reopen() {
        let dir = tempdir().unwrap();
        {
            let mru = open(dir.path(), 5);
            mru.add(DemoItem::new("/foo", "Foo"));
            mru.add(DemoItem::new("/bar", "Bar"));
            mru.flush_now().unwrap();
        }
        let mru = open(dir.path(), 5);
        assert_eq!(mru.model().len(), 2);
        assert_eq!(mru.model().with_item(0, |i| i.name.clone()).unwrap(), "Bar");
    }

    /// A second item type with a non-Path key — proves the trait
    /// works for arbitrary `Key: PartialEq + ?Sized`.
    #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
    struct StringItem {
        token: String,
        count: u32,
    }
    impl MruEntry for StringItem {
        type Key = str;
        fn key(&self) -> &str {
            &self.token
        }
    }

    #[test]
    fn works_with_str_keyed_entries() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());
        let mru: MruList<StringItem> =
            MruList::open_with_delay(&paths, "tokens", 3, Duration::ZERO).unwrap();
        mru.add(StringItem {
            token: "alpha".into(),
            count: 1,
        });
        mru.add(StringItem {
            token: "beta".into(),
            count: 2,
        });
        mru.add(StringItem {
            token: "alpha".into(),
            count: 99,
        });
        assert_eq!(mru.model().len(), 2);
        mru.remove("beta");
        assert_eq!(mru.model().len(), 1);
    }
}
