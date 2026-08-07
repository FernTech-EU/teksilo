<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Settings

Every public type in `teksilo-settings`, grouped by category. Each page links to its full rustdoc API reference.

## Collection

- [Keyed](list.md) — `PersistedListModel<T>` — bridge between a reactive

## Stores & services

- [AppPaths](path.md) — OS-correct path resolution for application config and data directories
- [FlushError](flush.md) — Debounced, **cross-process-safe** atomic file writer
- [MruEntry](mru.md) — Most-recently-used list — a generic, persisted reactive collection
- [PerWindowState](window_state.md) — Per-window geometry persistence via `WindowStateService`
- [Reloadable](reload.md) — `Reloadable` — the contract a (separately-built) file watcher uses to
- [SettingsBundleError](bundle.md) — `SettingsBundle` — declarative configuration for the teksilo-app
- [SettingsExt](ext.md) — Extension traits exposing settings services on `BuildContext` and
- [SettingsFileError](file.md) — `SettingsFile<T>` — typed single-struct persistence
- [SettingsReloadSink](watch.md) — Live cross-process settings sync: a `notify`-based directory watcher
- [SettingsStoreError](store.md) — Dynamic, dotted-key K/V store backed by TOML
- [Versioned](migration.md) — Schema migrations for persisted files
