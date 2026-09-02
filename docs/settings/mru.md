<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# MruEntry

Most-recently-used list — a generic, persisted reactive collection
with dedupe, pinning, and LRU-style cap eviction.

Apps define their own item type by implementing `Keyed` (a stable
identity) and `MruEntry` (pin / touch semantics). The framework
handles dedupe-on-add, pin-aware cap eviction, and cross-process-safe
persistence via `PersistedListModel`; the app owns the item schema.

## When to use

Use `MruList` for any "recently opened / recently used" feature:
recent files, recent projects, recently visited locations, recently used
palette entries, etc. The backing `ListModel<T>` is the same reactive
handle you bind to a `ListView` or iterate in
a menu — no separate notification plumbing is required.

## Persistence

`MruList::open` reads `<config_dir>/<name>.toml` on first access
(cross-process safe: the read is lock-protected, and every subsequent
mutation merges by key against the document on disk, never overwriting
the whole thing). Pass `Duration::ZERO` in tests to flush
synchronously, or call `MruList::flush_now` explicitly.

```ignore
use std::path::PathBuf;
use std::time::Duration;
use teksilo_settings::{AppPaths, Keyed, MruEntry, MruList};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Clone)]
struct RecentProject {
    path: PathBuf,
    display_name: String,
    last_opened: u64,
    pinned: bool,
}

impl Keyed for RecentProject {
    type Key = PathBuf;
    fn key(&self) -> PathBuf { self.path.clone() }
}

impl MruEntry for RecentProject {
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

`open`, `open_with_delay`, `open_at`, `model`, `max_items`, `add`, `remove`, `touch`, `set_pinned`, `is_pinned`, `clear`, `flush_now`, `path`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-settings/latest/teksilo_settings/index.html)

## `pub struct MruList`

A persisted MRU list backed by `PersistedListModel<T>`.

Cheap to clone (`Rc`-shared internally). The reactive
`ListModel<T>` returned by `model()` is the same
handle the persistence bridge observes.

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

**Read-only for mutation purposes.** Use `add`,
`remove`, `touch`,
`set_pinned`, `clear` to mutate —
those are what enqueue the matching persisted op. Mutating the
returned `ListModel` directly updates what's on screen but is
never written to disk.

#### `pub fn max_items(&self) -> usize`

Returns the maximum number of unpinned entries kept in the list.

Pinned entries do not count toward this cap and are never evicted
automatically.

#### `pub fn add(&self, mut entry: T)`

Insert `entry` at the front, deduping by `entry.key()`.
`T::touch` is invoked before insertion, so the freshly-added
entry reflects "now". If a previously-pinned entry is re-added
without `pinned`, the pin state is preserved.

#### `pub fn remove<Q>(&self, key: &Q) where T::Key: Borrow<Q>, Q: Eq + ?Sized,`

Remove the entry whose key matches, then schedule a debounced flush.

No-op when no entry with that key is present. Generic over `Q` so
callers can pass a borrowed form of the key (e.g. `&Path` when
`T::Key = PathBuf`, `&str` when `T::Key = String`) without having
to allocate an owned key just to look one up.

#### `pub fn touch<Q>(&self, key: &Q) where T::Key: Borrow<Q>, Q: Eq + ?Sized,`

Mark the entry whose key matches as freshly used by calling
`MruEntry::touch` on a clone of it, then write it back and
schedule a debounced flush. No-op when no entry matches.

#### `pub fn set_pinned<Q>(&self, key: &Q, pinned: bool) where T::Key: Borrow<Q>, Q: Eq + ?Sized,`

Set the pin flag of the entry whose key matches to exactly
`pinned` (idempotent — unlike a toggle, replaying this against an
already-applied peer change does not flip it back). No-op when no
entry matches.

#### `pub fn is_pinned<Q>(&self, key: &Q) -> bool where T::Key: Borrow<Q>, Q: Eq + ?Sized,`

Is the entry with this key currently pinned? `false` when no entry
matches.

The counterpart `set_pinned` deliberately takes the
*desired* value rather than toggling, because a toggle is not idempotent:
replayed against a peer process's already-applied toggle it would flip
the value straight back, inverting their change. A pin **button** still
needs to toggle, though — so read the current value here and pass its
negation:

```ignore
let pinned = mru.is_pinned(path);
mru.set_pinned(path, !pinned);
```

#### `pub fn clear(&self)`

Drop every entry (pinned or not) and schedule a debounced flush.

#### `pub fn flush_now(&self) -> Result<(), SettingsFileError>`

Write the list to disk synchronously, bypassing the debounce window.

Useful at app shutdown or at the end of a test to guarantee the
file reflects the in-memory state before the process exits.

#### `pub fn path(&self) -> &Path`

The TOML file path this list reads from and writes to.
