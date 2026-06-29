<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# PersistedTreeNode

`PersistedTreeModel<T>` — bridge between a reactive
`TreeModel<T>` and a `SettingsFile`.

On-disk shape is a recursive `PersistedTreeNode<T>` structure.
Loading walks the file building `NodeId`s; saving walks the model's
roots → leaves and reconstructs the nested form.

## Cost model

Every mutation re-walks the entire tree to produce the on-disk
representation, irrespective of which subtree changed. The
debounced write coalesces rapid bursts so this work is paid at
most once per debounce window, not per mutation, but it scales
with the *total* node count, not the change.

Targets like saved queries, custom menu hierarchies, and
project-side panels live well below 1k nodes — the bridge is fine
for those at any plausible mutation rate. For trees that grow
larger or mutate at high frequency, prefer SQLite (`rusqlite`)
with a per-node row, and treat this bridge as the wrong tool.
A future revision could exploit `TreeChange` payloads for
incremental serialization, but the bookkeeping is non-trivial and
the simple form is correct.

## Example

```ignore
use bastyde_settings::collection::tree::PersistedTreeModel;
use bastyde_settings::migration::Migrator;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Serialize, Deserialize, Clone)]
struct Category { name: String }

let path = std::env::temp_dir().join("categories.toml");
let ptm: PersistedTreeModel<Category> =
    PersistedTreeModel::open(path, Duration::ZERO, Migrator::new())
        .expect("open failed");
let root = ptm.model().insert_root(0, Category { name: "Rust".into() });
ptm.model().insert_child(root, 0, Category { name: "async".into() });
ptm.flush_now().expect("flush");
```

## Builder methods at a glance

`open`, `model`, `flush_now`, `path`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_settings/index.html)

## `pub struct PersistedTreeNode`

Recursive on-disk representation of a single `TreeModel` node and
its subtree.

```rust
pub struct PersistedTreeNode<T> { /* fields */ }
```

## `pub struct TreeFile`

On-disk shape for a persisted tree: a versioned wrapper around the
list of root nodes. Apps write migrations against this type.

```rust
pub struct TreeFile<T> { /* fields */ }
```

## `pub struct PersistedTreeModel`

A reactive tree whose mutations persist to a single TOML file.

```rust
pub struct PersistedTreeModel<T>
where
    T: Clone + Serialize + DeserializeOwned + 'static, { /* fields */ }
```

### Methods

#### `pub fn open( path: PathBuf, delay: Duration, migrator: Migrator<TreeFile<T>>, ) -> Result<Self, SettingsFileError>`

Open the file at `path` (running `migrator`), seed the model
from its contents, and wire up persistence on every mutation.

#### `pub fn model(&self) -> &TreeModel<T>`

The underlying reactive tree handle. Clone it to share with
`TreeView` widgets; mutations flow through the observer and schedule
a debounced disk flush automatically.

#### `pub fn flush_now(&self) -> Result<(), SettingsFileError>`

Flush any pending serialized payload to disk immediately,
bypassing the debounce window.

#### `pub fn path(&self) -> &std::path::Path`

The absolute path of the TOML file being written to.
