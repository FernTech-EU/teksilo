<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Keyed

`PersistedListModel<T>` — bridge between a reactive
`ListModel<T>` and a single TOML file,
merging by **op**, not by whole-document snapshot.

## Why ops, not snapshots

The previous design re-derived the *entire* `Vec<T>` from the live model
on every mutation and scheduled a debounced write of that whole
snapshot. That is last-write-wins by construction: if a peer process
added an entry to the same file in the meantime, this process's next
flush would overwrite the peer's row right off the disk — the exact
"a newly-opened project vanishes from Recents" bug this crate exists to
fix.

Instead, every mutation records a small, **replayable** `ListOp<T>`
and hands it to the shared debounced writer as a [`crate::flush::Patch`]:
"given the file's current text, apply this one op to it." The patch is
applied against the document read **fresh off disk, under a lock**, at
flush time — so it replays cleanly on top of whatever a peer wrote in
the meantime, key by key, instead of overwriting the whole thing.

## Identity

Every item needs a stable identity to merge by — see `Keyed`. Ops are
keyed, not indexed: `Remove` only needs to carry a key, never a value,
which is exactly what a diff of "what's gone" can always produce even
though the value itself is no longer available once removed.

## Mutating through this type, not through `.model()`

`.model()` is for **reading** and for reactive binding (`ListView` /
`Repeater`) — every UI observer wants live updates regardless of who
mutates. Writing must go through `upsert_front`,
`update_in_place`,
`remove` and
`clear`: those are the only places that both
mutate the live model *and* enqueue the matching op. Mutating the
`ListModel` returned by `.model()` directly updates what's on screen but
is never persisted — there is no observer bridging arbitrary model
mutations to disk any more (that observer *was* the whole-snapshot
overwrite bug).

## Example

```
use bastyde_settings::{Keyed, Migrator, PersistedListModel};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Serialize, Deserialize, Clone)]
struct Tag { name: String }

impl Keyed for Tag {
    type Key = String;
    fn key(&self) -> String { self.name.clone() }
}

let path = std::env::temp_dir().join("tags-list-doctest.toml");
let plm: PersistedListModel<Tag> =
    PersistedListModel::open(path, Duration::ZERO, Migrator::new())
        .expect("open failed");
plm.upsert_front(Tag { name: "rust".into() });
plm.flush_now().expect("flush");
```

## Builder methods at a glance

`open`, `model`, `upsert_front`, `update_in_place`, `remove`, `clear`, `flush_now`, `path`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_settings/index.html)

## `pub enum ListOp`

A replayable mutation of a `PersistedListModel`'s backing list,
expressed **by key** so it can be applied to *any* starting `Vec<T>` —
in particular, the fresh one read off disk at flush time, which may
already include a peer process's concurrent changes.

```rust
pub enum ListOp<T: Keyed> { /* variants */ }
```

### Variants

- **`UpsertFront`** — Remove any existing entry with this item's key, then insert `T` at the front. This is the "most recently used" operation: re-running it against any starting vector — including one a peer has already mutated — reproduces the same dedupe-and-promote-to-front invariant `MruList::add` relies on.
- **`UpdateInPlace`** — Replace the entry with this item's key **in place** (no reordering). A no-op if the key is no longer present — e.g. a peer concurrently removed it, in which case that removal wins.
- **`Remove`** — Remove the entry with this key, if present. No-op otherwise.
- **`Clear`** — Drop every entry.

## `pub struct ListFile`

On-disk shape for a persisted list: a versioned wrapper around
`Vec<T>`. Apps write migrations against this type, not the bare `Vec`.

```rust
pub struct ListFile<T> { /* fields */ }
```

## `pub struct PersistedListModel`

A reactive, `Keyed`-item list whose mutations persist to a single
TOML file by merging **ops**, not by overwriting a whole-document
snapshot.

```rust
pub struct PersistedListModel<T>
where
    T: Keyed + Clone + Serialize + DeserializeOwned + Send + 'static, { /* fields */ }
```

### Methods

#### `pub fn open( path: PathBuf, delay: Duration, migrator: Migrator<ListFile<T>>, ) -> Result<Self, SettingsFileError>`

Open the file at `path` (running `migrator`, under an exclusive
lock so a peer mid-write can't hand us a torn read), seed the model
from its contents, and retain everything needed to enqueue op
patches on every mutation.

`delay` is the debounce window for writes — unlike
`crate::SettingsFile`, this type's writes are expected to be
frequent (every `add`/`touch`/`remove` on a live MRU list), so the
debounce is real and load-bearing here, not vestigial.

#### `pub fn model(&self) -> &ListModel<T>`

The underlying reactive list handle. Clone it to share with
`Repeater` / `ListView` widgets for **reading**. See the module
docs: mutating the returned handle directly does not persist —
use this type's own mutation methods instead.

#### `pub fn upsert_front(&self, item: T)`

Insert `item` at the front, deduping by `item.key()` (removing any
existing entry with the same key first). Updates the live model
immediately and enqueues the matching `ListOp::UpsertFront`.

#### `pub fn update_in_place(&self, item: T) -> bool`

Replace the entry with `item.key()` **in place** (no reordering).
Returns `false` (and does nothing) if no entry with that key
exists locally. Enqueues `ListOp::UpdateInPlace` on success.

#### `pub fn remove(&self, key: &T::Key) -> bool`

Remove the entry with this key, if present locally. Returns
whether anything was removed. Enqueues `ListOp::Remove` on
success.

#### `pub fn clear(&self)`

Drop every entry, locally and on disk.

#### `pub fn flush_now(&self) -> Result<(), SettingsFileError>`

Flush any pending op(s) to disk immediately, bypassing the
debounce window. Flushes the **op queue** — never a re-derived
snapshot of the in-memory list, which is exactly the mechanism
that used to let a cleanly-exiting process erase a peer's
newly-added entry.

#### `pub fn path(&self) -> &Path`

The absolute path of the TOML file being written to.
