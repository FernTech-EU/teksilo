<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Changelog

All notable changes to Bastyde are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/) —
though pre-1.0, so breaking changes can land in a minor bump. `release.toml`
keeps every workspace crate on one shared version; entries below are grouped
by crate for clarity, not because crates version independently.

## [Unreleased]

### Changed — `bastyde-settings` (breaking)

**Cross-process safety is now the default and only behaviour for every
persisted type in the crate**, not an opt-in mode. Previously,
`docs/settings.md` said — twice, deliberately — "multi-process is out of
scope; two app instances writing to the same file is last-write-wins," then
later grew an opt-in `SettingsFile::load_shared` escape hatch for callers
that needed better. Both of those are gone: `SettingsStore`,
`SettingsFile<T>`, and `PersistedListModel<T>` (and everything built on
them — `MruList<T>`, `WindowStateService`) now always merge a write against
the document read fresh under an exclusive lock, and a peer process's write
now arrives live, through the same `Signal`/`ListModel` a caller is already
bound to, via a new `notify`-based file watcher — with nothing for the
caller to remember to call, unlike Qt's `QSettings::sync()`. See
`docs/settings.md` for the full architecture (the patch/merge model, why a
lock alone isn't sufficient, the live-reload watcher, and the honest
performance trade-offs).

- **Removed:** `SettingsFile::load_shared`. `SettingsFile::load` /
  `load_strict` are unconditionally correct now — there is no longer a
  "which mode did I open this with" question to get wrong. `load` /
  `load_strict` also dropped their now-meaningless `delay: Duration`
  parameter (`SettingsFile<T>`'s writes are always synchronous).
- **Removed:** `MruList::toggle_pin`. Replaced by
  `MruList::set_pinned(key, bool)`. A toggle's effect depends on the
  *current* state at the moment it runs, which is exactly the kind of
  transient, position-dependent meaning a replayable, possibly-delayed
  write can no longer assume — `set_pinned` states the desired end state
  directly, so replaying it against a document a peer has already changed
  always lands on the same answer.
- **Removed:** `PersistedTreeModel<T>` (and its `collection::tree` module).
  Zero consumers anywhere in this workspace or in Skribisto, and it carried
  the exact whole-snapshot-clobber defect the rest of this crate was just
  hardened against. Reintroduce it ops-based, from scratch, if a real
  consumer needs a persisted tree — do not resurrect the deleted version.
- **Changed (source-compatible):** `Keyed` is a new, separate trait
  (`type Key` + `fn key(&self) -> Self::Key`, owned key) that `MruEntry`
  now requires alongside its existing pin/touch methods; existing
  `impl MruEntry` blocks need a companion `impl Keyed` (previously `Key`
  and `key()` lived directly on `MruEntry`).
- **Source-compatible, despite the headline:** `SettingsFile::mutate`,
  every `SettingsStore` signal (`signal`/`signal_for`/`.set()`), and
  `MruList::add`/`touch`/`remove`/`clear` keep their existing call-site
  shape. "Cross-process safe by default" sounds like it should cost
  callers something; it doesn't — the correctness moved into the write
  path and the new live-reload watcher, not into new ceremony at the
  call site.

### Changed — `bastyde-widgets` (breaking)

- `NotificationArchiveModel::remove(index: usize)` → `remove_by_id(id: u64)`.
  Same reasoning as `set_pinned` above: an index names a position in this
  process's *current* view of the list, which a peer's concurrent insert
  (or this crate's own debounce delay) can invalidate before the removal
  actually runs. An id is stable identity regardless of how many
  neighboring rows moved in the meantime.

### Added — `bastyde-data`

- `ListModel::reconcile_by_key(new_items, key_fn)`: diffs the live model
  against a new authoritative `Vec<T>` by key and emits the minimal
  granular `DataChange`s (coalesced removes/inserts, single-row moves only
  where something is actually out of place, updates where content
  differs) — and **never** a blanket `Reset`, which would otherwise clear
  a user's positional selection every time a peer's settings write landed.
  This is the primitive `PersistedListModel<T>`'s live-reload path is
  built on.
- `adjust_single_index_for_change` (in `data_change.rs`) and its use in
  `ListView`'s focused-index tracking, so a single-selection widget keeps
  its focus pointed at the right row across a reload-driven reconciliation,
  not just across ordinary user-driven inserts/removes.

## Earlier history

Entries before this file was introduced are not backfilled; see `git log`
for the full history.
