<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Settings and persistence

Use `teksilo::settings` for preferences, recent-item lists, and window geometry.
Changes can notify widgets through signals and data models.

## Minimal example

<!-- compile-check -->
```rust
use teksilo::prelude::*;
use teksilo::settings::SettingsBundle;
use teksilo::widgets::TextWidget;

fn main() {
    TeksiloAppBuilder::new()
        .theme(intui::light())
        .application("eu", "Example", "SettingsDemo")
        .settings(SettingsBundle::new().with_window_state(true))
        .initial_window(
            WindowConfig::new()
                .id("main")
                .title("Settings demo")
                .size(640, 400)
                .root(|tree, _| tree.add(TextWidget::new(lit!("Window geometry is saved")))),
        )
        .run();
}
```

`application` selects the application directories. A stable window `id` enables
geometry restoration when the window-state service is installed.

## Choose a storage type

| Type | Use | Write behavior |
| --- | --- | --- |
| `SettingsStore` | Named scalar preferences exposed as signals | Debounced patches for changed keys |
| `SettingsFile<T>` | A typed struct with schema migrations | Synchronous write from `mutate` or `replace` |
| `PersistedListModel<T>` | Persistent collections with stable keys | Debounced keyed operations |
| `MruList<T>` | Recent items with deduplication, pinning, and a limit | Uses `PersistedListModel<T>` |

Cloned handles share state. Register additional handles with `.app_state(...)`;
`SettingsBundle` opens the preference store and optional window-state service.

## Common operations

- Read or update a registered preference through `SettingsStore::signal_for`.
- Use `SettingsFile::mutate` to change fields in the latest on-disk value.
- Use `SettingsFile::replace` only when replacing the entire value is intended.
- Implement `Versioned` and supply a `Migrator` for a typed file's schema changes.
- Use `MruList::open` for recent documents or projects. See the
  [recent-projects example](../examples/recent_projects/src/main.rs).
- Keep a window's `id` stable across launches. Restored geometry is checked
  against the available monitors; Wayland controls final window placement.

## Concurrent writes and reloads

Writes use a file lock and atomic replacement. This prevents interleaved file
writes, but it does not resolve every application-level conflict.

| Operation | Concurrent behavior |
| --- | --- |
| Store updates to different keys | Changed-key patches preserve unrelated keys |
| Store updates to the same key | The later applied write wins |
| `SettingsFile::mutate` | The closure receives a fresh value read under the lock |
| `SettingsFile::replace` | Replaces the whole value, including a peer's fields |
| Persistent list operations | Replay keyed insert, update, remove, or clear operations |

A closure that assigns a stale whole-object snapshot can still discard a peer's
changes. Prefer field updates based on the value passed to `mutate`.

The running application installs a watcher when `.settings(...)` is configured.
Custom persistent handles must be registered with `SettingsRegistry` to receive
watcher updates. Outside the app event loop, call `Reloadable::reload_from_disk`
or `SettingsFile::reload_if_stale` explicitly.

## Constraints

- `SettingsFile` writes synchronously. Avoid writing on every pointer move or frame.
- Persistent-list row reordering does not produce a persistence operation.
  Store ordering explicitly if the application needs it.
- A write to one settings file is not a transaction across several files.
- Use `SettingsFile::load_strict` when invalid data must be reported instead of
  falling back to defaults. Handle write errors and shutdown flushing explicitly.
- Handles are intended for the UI thread. Do not share them across threads.

## Reference

- [Settings API](settings/index.md)
- [Settings source](../crates/teksilo-settings/src/lib.rs)
- [Multi-window guide](multi-window.md)


## Engineering reference

[Implementation details and review history](https://github.com/ferntech-eu/teksilo/blob/main/engineering/docs/settings.md)
are retained in the repository.
