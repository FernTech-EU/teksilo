<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Reloadable

`Reloadable` — the contract a (separately-built) file watcher uses to
push a peer process's write into live state.

Every persisted type in this crate is cross-process safe on the *write*
side (see `flush.rs`'s `Patch` design): a write always merges against
whatever is on disk, under a lock. That alone is not enough — a process
that loaded its state once and never looks again will not notice a peer's
write until it happens to mutate something itself. `Reloadable` is the
*read* side of the same story: a way for an external watcher (inotify /
FSEvents / ReadDirectoryChangesW, wired up outside this crate) to say
"the file changed, go look," without needing to know anything about the
concrete type it's reloading.

## The self-write-suppression contract

A naive implementation would feed back into itself: this process writes
`general.toml`, the watcher notices *that very write* a few milliseconds
later, and calls `reload_from_disk()` — which had better be a cheap no-op,
not a full re-parse-and-notify cycle (and, worse, must never re-apply our
own value as if it were a peer's newer one, which could bounce a
just-superseded value back into a live `Signal` between the user's edit
and the debounced write landing).

Every implementation therefore layers two checks, cheapest first:

1. **Stamp check.** Each implementor records the `(mtime, len)` of the
   file as of the last time it either wrote to it or read it. If the
   file's current stamp matches, `reload_from_disk` returns `Ok(false)`
   immediately — no read, no parse, nothing touched. This is the common
   case for a self-write notification.
2. **Content backstop.** If the stamp *did* change (a real write happened,
   by us or a peer, since a filesystem's mtime resolution can coincide,
   or the write path didn't get a chance to update the stamp), the file
   is read and parsed, then compared *by value* against what's already
   live. Only a genuine difference is pushed into signals / models;
   `Ok(false)` is returned — again touching nothing — when the content
   is unchanged. This is the actual correctness guarantee; the stamp
   check above is purely an optimization to skip the common case cheaply.

Implementors: `crate::SettingsFile`, `crate::SettingsStore`,
`crate::PersistedListModel`, `crate::WindowStateService`.

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-settings/latest/teksilo_settings/index.html)
