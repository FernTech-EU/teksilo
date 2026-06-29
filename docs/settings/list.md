<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ListFile

`PersistedListModel<T>` — bridge between a reactive
`ListModel<T>` and a `SettingsFile`.

Construction loads the file at `path`, seeds the in-memory model from
its `items` field, and installs an `observe_changes` callback that
re-serializes the whole list on every mutation. The observer captures
the file handle via `Rc` clone — both ends are `Rc<RefCell<>>`-shaped
so cloning is cheap and share-by-handle semantics apply.

## When to use

Use this bridge for flat ordered collections (pinned items, palette
entries, saved searches) whose total size stays well below ~1 k items.
Each mutation re-serializes the full list; the debounce window
coalesces rapid bursts so this work is paid at most once per window.
For larger or rapidly-mutating lists prefer SQLite.

## Example

```ignore
use bastyde_settings::collection::list::PersistedListModel;
use bastyde_settings::migration::Migrator;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Serialize, Deserialize, Clone)]
struct Tag { name: String }

let path = std::env::temp_dir().join("tags.toml");
let plm: PersistedListModel<Tag> =
    PersistedListModel::open(path, Duration::ZERO, Migrator::new())
        .expect("open failed");
plm.model().push(Tag { name: "rust".into() });
plm.flush_now().expect("flush");
```

## Builder methods at a glance

`open`, `model`, `flush_now`, `path`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_settings/index.html)

## `pub struct ListFile`

On-disk shape for a persisted list: a versioned wrapper around
`Vec<T>`. Apps write migrations against this type, not the bare `Vec`.

```rust
pub struct ListFile<T> { /* fields */ }
```

## `pub struct PersistedListModel`

A reactive list whose mutations persist to a single TOML file.

```rust
pub struct PersistedListModel<T>
where
    T: Clone + Serialize + DeserializeOwned + 'static, { /* fields */ }
```

### Methods

#### `pub fn open( path: PathBuf, delay: Duration, migrator: Migrator<ListFile<T>>, ) -> Result<Self, SettingsFileError>`

Open the file at `path` (running `migrator`), seed the model
from its contents, and wire up automatic persistence on every
mutation.

#### `pub fn model(&self) -> &ListModel<T>`

The underlying reactive list handle. Clone it to share with
`Repeater` / `ListView` widgets; mutations flow back through the
observer and schedule a debounced disk flush automatically.

#### `pub fn flush_now(&self) -> Result<(), SettingsFileError>`

Flush any pending serialized payload to disk immediately,
bypassing the debounce window.

#### `pub fn path(&self) -> &std::path::Path`

The absolute path of the TOML file being written to.
