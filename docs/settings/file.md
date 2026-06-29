<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SettingsFileError

`SettingsFile<T>` — typed single-struct persistence.

Used when the persisted shape is a known struct (recents, window
state) rather than a dynamic K/V map. The current value lives in a
`RefCell<T>` inside an `Rc<>`-shared inner so that multiple handles
can observe and mutate the same projection.

The file is loaded once at construction (running registered
migrations); after that, all reads come from `current` in memory and
all writes flush via `DebouncedWriter`. Corrupt or unmigratable
files are renamed to `<path>.broken-<unix_ts>` and the in-memory
value falls back to `T::default()` so the app keeps running.

## When to use

Prefer `SettingsFile<T>` over `crate::SettingsStore` when the
settings form a known typed struct (e.g. a window-layout blob or a
recents list). Use `SettingsStore` for open-ended scalar K/V pairs
that arrive at different call sites.

```ignore
use bastyde_settings::{SettingsFile, Migrator, Versioned};
use serde::{Serialize, Deserialize};
use std::time::Duration;

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
struct AppPrefs { version: u32, font_size: f32 }
impl Versioned for AppPrefs {
    const CURRENT_VERSION: u32 = 1;
    fn version(&self) -> u32 { self.version }
    fn set_version(&mut self, v: u32) { self.version = v; }
}

let path = dirs::config_dir().unwrap().join("myapp/prefs.toml");
let file: SettingsFile<AppPrefs> =
    SettingsFile::load(path, Duration::from_millis(500), &Migrator::new()).unwrap();

file.mutate(|p| p.font_size = 16.0).unwrap();
file.flush_now().unwrap();
```

## Builder methods at a glance

`load`, `load_strict`, `borrow`, `snapshot`, `replace`, `mutate`, `flush_now`, `path`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_settings/index.html)

## `pub enum SettingsFileError`

Errors surfaced by `SettingsFile` operations.

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
pub struct SettingsFile<T> { /* fields */ }
```

### Methods

#### `pub fn load( path: PathBuf, delay: Duration, migrator: &Migrator<T>, ) -> Result<Self, SettingsFileError>`

Load the file from disk (running migrations) or initialize with
`T::default()` if the file does not exist.

`delay` is the debounce window for subsequent writes.

On parse / migration failure the offending file is renamed to
`<path>.broken-<ts>` and the returned `SettingsFile` starts from
`T::default()`. The caller can detect this by passing a
pre-known default; the broken-file path is returned in the
error variants only when `load_strict` is used.

#### `pub fn load_strict( path: PathBuf, delay: Duration, migrator: &Migrator<T>, ) -> Result<Self, SettingsFileError>`

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

#### `pub fn replace(&self, mut new: T) -> Result<(), SettingsFileError>`

Replace the current value and schedule a debounced write.
`T::set_version(T::CURRENT_VERSION)` is called so the version
stamp is always coherent, even if the caller forgot.

#### `pub fn mutate<F: FnOnce(&mut T)>(&self, f: F) -> Result<(), SettingsFileError>`

Mutate the current value in place and schedule a debounced write.

#### `pub fn flush_now(&self) -> Result<(), SettingsFileError>`

Synchronously write any pending payload to disk.

#### `pub fn path(&self) -> &Path`

The path being written to.
