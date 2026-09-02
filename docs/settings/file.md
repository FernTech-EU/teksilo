<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SettingsFileError

`SettingsFile<T>` — typed single-struct persistence.

Used when the persisted shape is a known struct (recents, window
state) rather than a dynamic K/V map. The current value lives in a
`RefCell<T>` inside an `Rc<>`-shared inner so that multiple handles
can observe and mutate the same projection.

## Cross-process safety is the only mode

Every read and every write goes through the exclusive advisory lock on
`<path>.lock` (see `crate::lock`):

* `load` acquires the lock, reads + migrates the file
  fresh, and retains the `Migrator` for the handle's whole lifetime —
  not just for this one read — because `mutate`,
  `replace`,
  `reload_if_stale`
  and `reload_from_disk` all need
  to re-migrate a peer's still-older on-disk schema on demand, not just
  once at construction.
* `mutate` and `replace` perform a
  **locked read-modify-write**: acquire the lock, re-read + re-migrate
  the file from disk *under the lock*, apply the caller's change to that
  fresh value, write it back atomically, refresh the in-memory snapshot,
  then release the lock. A lock alone would only stop the two writes
  from interleaving on disk — it does nothing to stop a stale in-memory
  snapshot from clobbering a peer's newer data, so the re-read has to
  happen *under* the same lock that guards the write. These writes are
  synchronous, on the calling thread, bypassing the shared debounced I/O
  worker entirely — deliberately: `SettingsFile<T>` is for **rare**
  writes (a settings change, one record per backup), so there is no
  burst to coalesce. Contrast `crate::SettingsStore` and
  `crate::PersistedListModel`, which write far more often and keep
  the debounce.
* `reload_if_stale` and
  `reload_from_disk` are how
  *reads* pick up a peer's change — a cheap mtime/len check, escalating
  to a full re-read only when something actually moved.

```ignore
use teksilo_settings::{SettingsFile, Migrator, Versioned};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
struct AppPrefs { version: u32, font_size: f32 }
impl Versioned for AppPrefs {
    const CURRENT_VERSION: u32 = 1;
    fn version(&self) -> u32 { self.version }
    fn set_version(&mut self, v: u32) { self.version = v; }
}

let path = dirs::config_dir().unwrap().join("myapp/prefs.toml");
let file: SettingsFile<AppPrefs> =
    SettingsFile::load(path, Migrator::new()).unwrap();

file.mutate(|p| p.font_size = 16.0).unwrap();
```

## Builder methods at a glance

`load`, `load_strict`, `borrow`, `snapshot`, `replace`, `mutate`, `reload_if_stale`, `flush_now`, `path`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-settings/latest/teksilo_settings/index.html)

## `pub enum SettingsFileError`

Errors surfaced by `SettingsFile` operations (and, by extension, every
other persisted type in this crate — they all share this error type).

```rust
pub enum SettingsFileError { /* variants */ }
```

### Variants

- **`Io`** — An OS-level file I/O error (read, write, or rename).
- **`Parse`** — The file's TOML could not be parsed.
- **`Migrate`** — A migration step failed; the file version could not be brought up to `T::CURRENT_VERSION`.
- **`Serialize`** — The in-memory value could not be serialized to TOML before writing.
- **`Flush`** — The debounced background write failed.

## `pub struct SettingsFile`

A reactive handle to a single typed file on disk.

`Clone` is cheap (an `Rc` bump). All clones share one in-memory
projection and one I/O thread.

```rust
pub struct SettingsFile<T: Versioned + DeserializeOwned> { /* fields */ }
```

### Methods

#### `pub fn load(path: PathBuf, migrator: Migrator<T>) -> Result<Self, SettingsFileError>`

Load the file from disk (running migrations) or initialize with
`T::default()` if the file does not exist.

The initial read is lock-protected, exactly like every subsequent
`mutate` / `replace`: a peer that is mid-write when this process
starts up cannot hand us a torn read.

`migrator` is taken **by value** and retained for the lifetime of
the handle: every later locked read re-runs it, since a peer might
still be on an older on-disk schema at any point, not just at
startup.

On a genuine parse failure (the bytes are not valid TOML at all,
surviving `MAX_READ_ATTEMPTS` retries) the offending file is
renamed to `<path>.broken-<ts>` and the returned `SettingsFile`
starts from `T::default()` — the file really is corrupt, and the
quarantine lets the next launch start clean instead of repeatedly
failing to load it.

A `SettingsFileError::Migrate` or `SettingsFileError::Io`
failure, by contrast, is **not** quarantined:

* `Migrate` means the TOML parsed fine, but this build's own
  `Migrator` chain doesn't know how to bring it up to
  `T::CURRENT_VERSION` — the classic symptom of an *older* build
  opening a file a *newer* peer process already wrote in a newer
  schema. The file is not corrupt; renaming it would destroy that
  peer's live, legitimate, still-in-use data.
* `Io` means we couldn't even read the file (permissions, a
  transient failure) — we never saw its content, so there is no
  basis at all for deciding it's corrupt, and renaming (itself
  another I/O operation, on a path we just failed to read) would
  be reckless.

In both of those cases the handle falls back to `T::default()` for
this session only, but the file on disk is left completely
untouched. Use `load_strict` in tests that
want to assert on the specific failure instead.

#### `pub fn load_strict(path: PathBuf, migrator: Migrator<T>) -> Result<Self, SettingsFileError>`

Like `load`, but returns parse / migration errors
instead of quarantining the file. Intended for tests that want
to assert on a specific failure mode.

#### `pub fn borrow(&self) -> Ref<'_, T>`

Borrow the current value. The returned `Ref` holds a `RefCell`
guard; do not call any mutating method on this `SettingsFile`
while a `Ref` is alive.

#### `pub fn snapshot(&self) -> T`

Clone the current value out. Convenient when you don't want to
juggle a borrow.

#### `pub fn replace(&self, new: T) -> Result<(), SettingsFileError>`

Replace the current value and persist it via a locked
read-modify-write. The disk read is discarded — `replace` always
wins over whatever was on disk — but the lock still serializes it
against a concurrent peer write, and the fresh disk stamp is
recorded so a subsequent reload doesn't re-read our own write back
in as if it were new. `T::set_version(T::CURRENT_VERSION)` is called
so the version stamp is always coherent, even if the caller forgot.

#### `pub fn mutate<F: FnOnce(&mut T)>(&self, f: F) -> Result<(), SettingsFileError>`

Mutate the current value in place and persist it via a locked
read-modify-write: the file is re-read and re-migrated from disk
*under an exclusive lock* before `f` is applied, so `f` always sees
a fresh value — not this handle's possibly-stale in-memory snapshot
— and the result is written back atomically before the lock is
released.

Takes `f` as `FnOnce` (not `Fn`) and imposes no `Send` bound on `T`:
this write is synchronous on the calling thread, never replayed on a
background worker, so there is no reason to tax every call site with
a `Send`/`Fn` requirement it doesn't need.

#### `pub fn reload_if_stale(&self) -> Result<bool, SettingsFileError>`

Pick up a peer's change: if the on-disk `(mtime, len)` differs from
the last one this handle observed, re-read and re-migrate the file
and refresh `current`. Returns whether a reload happened.

This is the cheap public probe — a `stat`, safe to call
speculatively (e.g. on every focus-in, or on a timer). It does not
perform the content-equality backstop that
`Reloadable::reload_from_disk` adds on top (which additionally
requires `T: PartialEq`); use that when a value-level "did anything
actually change" guarantee is needed (e.g. driven by a file
watcher, where a coincident stamp match must never be relied on
alone).

#### `pub fn flush_now(&self) -> Result<(), SettingsFileError>`

Synchronously write any pending payload to disk. A genuine no-op:
`mutate` / `replace` already write synchronously on the calling
thread, so nothing is ever pending — this type never registers
with the shared debounced-write worker pool at all, so there is
nothing to flush and nothing that can fail. Kept so callers that
hold a `SettingsFile` alongside debounced types (`SettingsStore`,
`PersistedListModel`) can flush everything uniformly without
special-casing this type.

#### `pub fn path(&self) -> &Path`

The path being written to.
