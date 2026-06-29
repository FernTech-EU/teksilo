<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Versioned

Schema migrations for persisted files.

Every persisted struct carries a `version: u32` (via `Versioned`).
`Migrator<T>` holds an ordered set of `from_version → from_version + 1`
transformations expressed on raw `toml::Value` — pre-deserialization,
so a v1 file that no longer matches the v2 type can still be upgraded.

```
use bastyde_settings::{Versioned, Migrator};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Default)]
struct Recents {
    version: u32,
    items: Vec<Entry>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Entry { path: String, pinned: bool }

impl Versioned for Recents {
    const CURRENT_VERSION: u32 = 2;
    fn version(&self) -> u32 { self.version }
    fn set_version(&mut self, v: u32) { self.version = v; }
}

// v1 didn't have `pinned`; supply false.
let migrator: Migrator<Recents> = Migrator::new()
    .step(1, |mut v| {
        if let Some(items) = v.get_mut("items").and_then(|i| i.as_array_mut()) {
            for item in items {
                if let Some(t) = item.as_table_mut() {
                    t.insert("pinned".into(), toml::Value::Boolean(false));
                }
            }
        }
        Ok(v)
    });
```

## Builder methods at a glance

`step`, `run`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_settings/index.html)

## `pub enum MigrationError`

Errors surfaced by `Migrator::run`.

```rust
pub enum MigrationError { /* variants */ }
```

### Variants

- **`NewerThanCurrent`** — The on-disk version number exceeds `T::CURRENT_VERSION`; the file was written by a newer build and cannot be read safely.
- **`NoStepFor`** — The chain is missing a step for the encountered version, making it impossible to reach `T::CURRENT_VERSION`.
- **`Step`** — A migration step closure returned `Err(message)`.
- **`Deserialize`** — The migrated `toml::Value` did not deserialize as `T`.

## `pub struct Migrator`

Schema migration pipeline for a `Versioned` type.

Add `from → from + 1` steps with `Migrator::step`; the order in which
they're added does not matter — `Migrator::run` walks them in
version order.

```rust
pub struct Migrator<T: Versioned + DeserializeOwned> { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create an empty migrator with no steps registered.

If `T::CURRENT_VERSION` is 1 (the initial schema) or the file
is already at the current version, no steps are needed and
`run` will succeed immediately.

#### `pub fn step<F>(mut self, from: u32, func: F) -> Self where F: Fn(toml::Value) -> Result<toml::Value, String> + Send + Sync + 'static,`

Register a step that promotes a value from `from` to `from + 1`.
Steps may be registered in any order; `run` finds
the right one for the current version on demand.

#### `pub fn run(&self, mut raw: toml::Value) -> Result<T, MigrationError>`

Migrate `raw` from its on-disk version up to
`T::CURRENT_VERSION`, then deserialize.

Reads the version directly from the `version` field of the raw
`toml::Value` — never deserializes-then-checks, because a v1
payload typically fails to deserialize as the v2 type.

Files missing the `version` field are treated as v1 (legacy).
