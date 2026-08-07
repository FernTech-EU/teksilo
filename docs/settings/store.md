<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SettingsStoreError

Dynamic, dotted-key K/V store backed by TOML.

`SettingsStore` is the QSettings analogue: callers ask for any
dotted key with a type; the store returns a cached `Signal<T>` whose
mutations write back into an in-memory `toml::Value` and schedule a
debounced flush to disk.

Keys carry static names via `SettingsKey<T>`, or are passed as
ad-hoc strings via `SettingsStore::signal`. Same key, same type,
across any number of call sites returns clones of the same `Signal`.

## When to use

Use `SettingsStore` for **scalar and array-of-scalar** preferences
(numbers, strings, booleans, `Vec<String>`). It is the right choice
for the majority of user-facing prefs that have a flat, well-known key
name. For rich structs with migrations, use
`SettingsFile<T>` instead — struct values
serialize as TOML tables and collide with the dotted-key model.

## Invariants enforced at registration

* **Type stability** — once a key has been registered with type
  `T`, calling `signal::<U>` on the same key panics. Settings are
  programmer-named; type drift is a code bug, surfaced immediately.
* **No path-shape collisions** — `"editor.font_size"` cannot coexist
  with `"editor"` as a leaf value, in either order. Both directions
  panic at the call site that creates the conflict.

## Merging by dirty key, not by whole-document overwrite

Every `Signal<T>::set` schedules a `crate::flush::Patch` that carries
only the keys dirtied since the last schedule — never a full render of
`raw`. The patch, applied at flush time against the document read fresh
off disk under a lock, `write_nested`s just those keys onto it — so a
peer process's change to some *other* key survives. This is the fix for
Skribisto's `general.toml`: today, changing any one of its 26 keys
reverts every other key a peer process changed, because the whole
document gets re-serialized from an increasingly stale in-memory copy.

## Reload and the re-entrancy guard

`Reloadable::reload_from_disk`
pushes a peer's on-disk change straight into the already-handed-out
`Signal<T>` for that key — see `SignalCell::apply_external`'s doc
comment for why that requires capturing the concrete `T` at
registration time. Setting a signal from a reload would otherwise
re-trigger this same write-back observer and bounce the value straight
back out to disk as if it were a local edit; `StoreInner::applying_external`
is the flag the observer checks to short-circuit that.

## Cycle-free observer wiring

The cell each key owns includes an `ObserverHandle` returned by
`signal.observe(|new_val| …)`. The observer's closure captures a
`Weak<RefCell<StoreInner>>` — never a strong `Rc` — and bails when
the store has already been dropped. This avoids a reference cycle: a
strong capture would trap the entire store inside its own observer,
leaking for the life of the process.

## Example

```ignore
use teksilo_settings::{SettingsKey, SettingsStore};
use std::time::Duration;

// Declare a typed, statically-named key once — typically at the module level.
const FONT_SIZE: SettingsKey<f32> = SettingsKey::new("editor.font_size", || 14.0);

// Open the store (uses `tempfile` in tests, a real path in production).
let store = SettingsStore::open_with_delay(
    "settings.toml".into(),
    Duration::from_millis(500),
)?;

// Each call for the same key returns a clone of the same Signal<T>.
let font_size = store.signal_for(&FONT_SIZE); // Signal<f32>, seeded from disk
font_size.set(18.0);                          // writes back to TOML on next flush
store.flush_now()?;                           // force sync (useful in tests)
# Ok::<(), teksilo_settings::SettingsStoreError>(())
```

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_settings/index.html)

## `pub const DEFAULT_DEBOUNCE`

Default debounce window for store flushes.

```rust
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(500);
```

## `pub enum SettingsStoreError`

Errors surfaced by `SettingsStore::open`.

```rust
pub enum SettingsStoreError { /* variants */ }
```

### Variants

- **`Io`** — The settings file could not be read or written (missing directory, permission denied, etc.).
- **`Parse`** — The settings file exists but its contents are not valid TOML.
- **`Flush`** — An attempt to flush the in-memory state to disk failed.

## `pub struct SettingsKey`

A statically-named setting. Centralizes the dotted key, the value
type, and the default factory. Construct as a `const`:

```
use teksilo_settings::SettingsKey;

const FONT_SIZE: SettingsKey<f32> =
    SettingsKey::new("editor.font_size", || 14.0);
```

```rust
pub struct SettingsKey<T: 'static> { /* fields */ }
```

### Methods

#### `pub const fn new(key: &'static str, default: fn() -> T) -> Self`

Create a new key descriptor; intended for use in `const` declarations.

## `pub const TEXT_SCALE_KEY`

Persisted user-controlled global text-scale factor (`1.0` = 100 %).

Read at startup by `teksilo-app` to seed every window's text scale, and
bound by the `TextScaleControl` widget so edits persist. The key accepts
any `f32`; the UI control restricts the user-facing range to 80 %–200 %.
The effective rendered scale is this value multiplied by the OS
accessibility text-scale preference.

```rust
pub const TEXT_SCALE_KEY: SettingsKey<f32> =
    SettingsKey::new("accessibility.text_scale", || 1.0_f32);
```

## `pub struct SettingsStore`

A dynamic dotted-key reactive settings store.

`Clone` is cheap (an `Rc` bump). All clones share one cache and one
I/O thread.

```rust
pub struct SettingsStore { /* fields */ }
```

### Methods

#### `pub fn open(path: PathBuf) -> Result<Self, SettingsStoreError>`

Open a store at `path` with the default debounce window.

#### `pub fn open_with_delay(path: PathBuf, delay: Duration) -> Result<Self, SettingsStoreError>`

Open a store at `path` with a custom debounce window. `delay =
Duration::ZERO` is useful for tests — every set writes through
on the next worker iteration, and `flush_now()` is fully
deterministic.

#### `pub fn path(&self) -> &Path`

Path of the underlying file.

#### `pub fn flush_now(&self) -> Result<(), SettingsStoreError>`

Force any pending payload to disk synchronously.

#### `pub fn has(&self, key: &str) -> bool`

Whether the given key has already been registered.

#### `pub fn registered_keys(&self) -> Vec<String>`

All keys registered so far. Order is unspecified.

#### `pub fn signal<T>(&self, key: &str, default: T) -> Signal<T> where T: Clone + Serialize + DeserializeOwned + 'static,`

Get-or-create a `Signal<T>` for `key`, seeded from disk or
`default` if absent. Subsequent calls for the same key return
clones of the same signal.

# Panics

* If the key was previously registered with a different type.
* If the key's path conflicts with an existing leaf-value /
  table shape (e.g. `"editor"` is a string and now you ask for
  `"editor.font_size"`).

#### `pub fn signal_for<T>(&self, key: &SettingsKey<T>) -> Signal<T> where T: Clone + Serialize + DeserializeOwned + 'static,`

Like `signal`, but driven by a strongly-named
`SettingsKey<T>` constant.

#### `pub fn open_path(path: &Path) -> Result<Self, SettingsStoreError>`

Convenience constructor accepting `&Path`.
