<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# MruEntry

Most-recently-used list — a generic, persisted reactive collection
with dedupe, pinning, and LRU-style cap eviction.

Apps define their own item type by implementing [`MruEntry`]: a small
trait that exposes a dedupe key, an optional pin flag, and an optional
touch hook. The framework handles dedupe-on-add, pin-aware cap eviction,
and debounced disk persistence; the app owns the item schema and
semantics (e.g. updating a `last_opened` timestamp in `touch`).

## When to use

Use `MruList` for any "recently opened / recently used" feature:
recent files, recent projects, recently visited locations, recently used
palette entries, etc. The backing `ListModel<T>` is the same reactive
handle you bind to a `ListView` or iterate in
a menu — no separate notification plumbing is required.

## Persistence

`MruList::open` reads `<config_dir>/<name>.toml` on first access and
writes it back (atomically, via a temp-and-rename) after every mutation,
subject to the debounce window. Pass `Duration::ZERO` in tests to flush
synchronously, or call `MruList::flush_now` explicitly.

```ignore
use std::path::{Path, PathBuf};
use std::time::Duration;
use bastyde_settings::{AppPaths, MruEntry, MruList};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Clone)]
struct RecentProject {
    path: PathBuf,
    display_name: String,
    last_opened: u64,
    pinned: bool,
}

impl MruEntry for RecentProject {
    type Key = Path;
    fn key(&self) -> &Path { &self.path }
    fn is_pinned(&self) -> bool { self.pinned }
    fn set_pinned(&mut self, p: bool) { self.pinned = p; }
    fn touch(&mut self) { self.last_opened += 1; }
}

// In tests: AppPaths::for_testing(tmp.path()) + Duration::ZERO.
// In production: AppPaths::new(qualifier, org, app).
let tmp = tempfile::tempdir().unwrap();
let paths = AppPaths::for_testing(tmp.path());
let recents: MruList<RecentProject> =
    MruList::open_with_delay(&paths, "recents", 10, Duration::ZERO).unwrap();

recents.add(RecentProject {
    path: "/projects/foo".into(),
    display_name: "Foo".into(),
    last_opened: 0,
    pinned: false,
});
assert_eq!(recents.model().len(), 1);
```

## Builder methods at a glance

`open`, `open_with_delay`, `open_at`, `model`, `max_items`, `add`, `remove`, `touch`, `toggle_pin`, `clear`, `flush_now`, `path`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_settings/index.html)

## `pub struct MruList`

A persisted MRU list backed by `PersistedListModel<T>`.

Cheap to clone (`Rc`-shared internally). The reactive
`ListModel<T>` returned by `model()` is the same
handle the persistence bridge observes; mutating it through any
clone fires both the on-screen UI updates and the debounced
disk flush.

```rust
pub struct MruList<T: MruEntry> { /* fields */ }
```

### Methods

#### `pub fn open(paths: &AppPaths, name: &str, max_items: usize) -> Result<Self, SettingsFileError>`

Open at `<paths.config_dir()>/<name>.toml` with the default debounce window.

Creates the file (and any missing parent directories) if it does not
yet exist. Use `open_with_delay` to override
the debounce in tests.

#### `pub fn open_with_delay( paths: &AppPaths, name: &str, max_items: usize, delay: Duration, ) -> Result<Self, SettingsFileError>`

Open at `<paths.config_dir()>/<name>.toml` with a custom debounce window.

Pass `Duration::ZERO` in tests to flush
every mutation synchronously.

#### `pub fn open_at( path: PathBuf, max_items: usize, delay: Duration, ) -> Result<Self, SettingsFileError>`

Open at an explicit path with the given debounce window.

Lower-level alternative to `open` when the caller
already has a resolved `PathBuf` (e.g. from a custom directory layout).

#### `pub fn model(&self) -> &ListModel<T>`

The underlying reactive list; bind to UI widgets via clones of this handle.

This is the same `ListModel<T>` the
persistence bridge observes — any mutation schedules a debounced
disk flush automatically.

#### `pub fn max_items(&self) -> usize`

Returns the maximum number of unpinned entries kept in the list.

Pinned entries do not count toward this cap and are never evicted
automatically.

#### `pub fn add(&self, mut entry: T)`

Insert `entry` at the front, deduping by `entry.key()`.
`T::touch` is invoked before insertion, so the freshly-added
entry reflects "now". If a previously-pinned entry is re-added
without `pinned`, the pin state is preserved.

#### `pub fn remove(&self, key: &T::Key)`

Remove the entry whose key matches, then schedule a debounced flush.

No-op when no entry with that key is present.

#### `pub fn touch(&self, key: &T::Key)`

Mark the entry whose key matches as freshly used by calling
`MruEntry::touch` on a clone of it, then write it back and
schedule a debounced flush. No-op when no entry matches.

#### `pub fn toggle_pin(&self, key: &T::Key)`

Flip the pin flag of the entry whose key matches, then schedule a
debounced flush. No-op when no entry matches.

#### `pub fn clear(&self)`

Drop every entry (pinned or not) and schedule a debounced flush.

#### `pub fn flush_now(&self) -> Result<(), SettingsFileError>`

Write the list to disk synchronously, bypassing the debounce window.

Useful at app shutdown or at the end of a test to guarantee the
file reflects the in-memory state before the process exits.

#### `pub fn path(&self) -> &Path`

The TOML file path this list reads from and writes to.
