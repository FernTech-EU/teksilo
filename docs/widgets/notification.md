<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# NotificationEntry

Persistent notification archive — the storage and data-model layer
backing `NotificationLog`, `NotificationCenterButton`, and
`NotificationLogDialog`.

Every toast presented through the toast registry is mirrored into a
`NotificationArchiveModel` when archiving is enabled via
`ToastInstallOptions::archive`. The model is a
`ListModel<NotificationEntry>` plus an
unread-count signal — shaped for one-line binding to the notification
UI family. Two storage variants are available: an in-memory session-only
ring buffer (`NotificationArchive::InMemory`) and a file-backed
persistent store (`NotificationArchive::Persistent`) that survives app
restarts. Action callbacks attached via raw closures are lost on
archival; actions that should remain re-invokable from the log carry an
`intent_name` that the log replays through `ctx.send_intent(...)`.

## When to use

- Pair with `TeksiloAppBuilder::install_toast_default()` to get the full
  bell-button + log + persistence stack for free.
- Construct `NotificationArchiveModel::in_memory` directly in tests or
  custom toast setups.

```ignore
// In app boot, after install_toast:
let archive = ctx.app_state::<Rc<RefCell<NotificationArchiveModel>>>().unwrap();
let log = NotificationLog::new(archive.clone());
```

## Builder methods at a glance

`in_memory`, `in_memory_with_limit`, `persistent`, `persistent_with_limit`, `limit`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/notification/index.html)

## `pub struct NotificationEntry`

A single archived notification entry rendered by `NotificationLog` and
persisted under `NotificationArchive::Persistent`. Carries plain owned
fields only — no closures, no `Rc<dyn Fn>` — so it is `Serialize`-friendly.

```rust
pub struct NotificationEntry { /* fields */ }
```

## `pub struct NotificationUpdate`

One in-place mutation applied when a `Toast` with the same `id` as an
existing entry is presented again. The archive merges these onto the
existing row — the "Uploading 3 of 7 → Upload complete" pattern.

```rust
pub struct NotificationUpdate { /* fields */ }
```

## `pub enum ArchivedActionStyle`

Visual presentation of an archived action button. Maps one-to-one to
`ToastActionStyle`; re-declared as a self-contained `Serialize`-friendly
enum so the archive type does not depend on `ButtonVariant`.

```rust
pub enum ArchivedActionStyle { /* variants */ }
```

### Variants

- **`Link`** — JetBrains-style hyperlink in the body row.
- **`PrimaryButton`** — Filled (primary CTA).
- **`SecondaryButton`** — Plain (secondary).
- **`Destructive`** — Destructive (red-tinted).

## `pub struct ArchivedAction`

A single action stored alongside an archived notification entry. Only
re-invokable from `NotificationLog` when `intent_name` is set — actions
whose live closure has torn down render as inert descriptive labels.

```rust
pub struct ArchivedAction { /* fields */ }
```

## `pub const DEFAULT_ARCHIVE_LIMIT`

Default per-archive entry cap. IntelliJ's notification log keeps
hundreds of entries with no cap visible to the user; we pick a
pragmatic limit so persistent files don't grow unbounded.

```rust
pub const DEFAULT_ARCHIVE_LIMIT: usize = 200;
```

## `pub const ARCHIVE_FILE_NAME`

File-name (without extension) used for the persistent archive.
Resolved through `AppPaths::config_file` into
`<config_dir>/<app>/notifications.toml`.

```rust
pub const ARCHIVE_FILE_NAME: &str = "notifications";
```

## `pub enum NotificationArchive`

Storage mode for the notification archive. Passed inside
`ToastInstallOptions::archive` to the install helper.

```rust
pub enum NotificationArchive { /* variants */ }
```

### Variants

- **`InMemory`** — Session-only — entries live in a `ListModel` for the running session. Cheap, no disk I/O. Default for apps that don't install a `SettingsBundle`.
- **`Persistent`** — File-backed via `PersistedListModel`. The path is built at install time from `AppPaths::config_file` using the configured `file_name`.

### Methods

#### `pub fn in_memory() -> Self`

In-memory archive with the default 200-entry cap.

#### `pub fn in_memory_with_limit(limit: usize) -> Self`

In-memory archive with a custom cap.

#### `pub fn persistent(file_name: impl Into<String>) -> Self`

File-backed archive resolved through `AppPaths::config_file`
at install time. The default file name (`"notifications"`)
yields `<config_dir>/<app>/notifications.toml`. Apps that
want a different name pass it here; tests pass an arbitrary
name and use `AppPaths::for_testing(tmpdir)`.

#### `pub fn persistent_with_limit(file_name: impl Into<String>, limit: usize) -> Self`

#### `pub fn limit(&self) -> usize`

## `pub struct NotificationArchiveModel`

Shared model — clones share state. Constructed by the install
helper from `NotificationArchive` + `AppPaths`; apps reach it
via `ctx.app_state::<Rc<RefCell<NotificationArchiveModel>>>()`.

`NotificationLog` and `NotificationCenterButton`
consume this model directly.

```rust
pub struct NotificationArchiveModel { /* fields */ }
```

### Methods

#### `pub fn open( archive: &NotificationArchive, paths: &AppPaths, debounce: Duration, ) -> Result<Self, NotificationArchiveError>`

Construct from a `NotificationArchive` config. For
`Persistent` mode, resolves the path through `AppPaths`.
Tests use `AppPaths::for_testing(tmpdir)` + `Duration::ZERO`
debounce.

#### `pub fn in_memory() -> Self`

Convenience: construct an `NotificationArchive::InMemory`
archive with the default cap, without going through paths.
Mostly useful for tests and apps that explicitly want no
persistence.

#### `pub fn entries(&self) -> &ListModel<NotificationEntry>`

Reactive handle on the entries. Bind to a `ListView` /
`Repeater` for live UI.

#### `pub fn unread_count(&self) -> &Signal<usize>`

Signal of the unread count. Drives the bell-button badge.

#### `pub fn version_signal(&self) -> &Signal<u64>`

Reactive handle on the archive's mutation version. Widgets
that render the archive (`NotificationLog`,
`NotificationCenterButton`) bind to this at
`BindingLevel::Rebuild`, in every window — one signal is enough
for N of them, see
`ToastRegistry::version_signal`
for the history of why that had to be said out loud.

#### `pub fn limit(&self) -> usize`

#### `pub fn flush_now(&self) -> Result<(), SettingsFileError>`

Force the persistent backing file to disk synchronously.
No-op for `InMemory`. Tests call this between mutations and
re-opening the file to verify persistence.

#### `pub fn push(&self, mut entry: NotificationEntry)`

Push a new entry. Inserts at index 0 (newest first), evicts
the oldest if the resulting length exceeds `limit`. Stamps
the entry's `id` field from `next_id`. Bumps `unread_count`
when the entry is unread (which is the typical case from a
toast push).

If `entry.dedup_id` matches an existing entry, the existing
entry is updated in place (title / body / progress collapsed
into a `NotificationUpdate` appended to `updates`) and no
new row is inserted. Unread count increments either way (an
in-place update IS new information for the user).

#### `pub fn mark_read_where(&self, mut predicate: impl FnMut(&NotificationEntry) -> bool)`

Mark every UNREAD entry matching `predicate` as read,
decrementing `unread_count` by exactly how many were flipped.
This is the scoped counterpart of `mark_all_read`:
a bell scoped to one window/audience must only mark ITS
entries read on close — calling the unscoped `mark_all_read`
from a scoped bell would incorrectly clear every OTHER
window's/audience's unread state too.

#### `pub fn mark_all_read(&self)`

Mark every archived entry as read; reset `unread_count` to 0.
Called by `NotificationCenterButton` when its popover opens.

#### `pub fn clear(&self)`

Clear the entire archive (resets `unread_count` to 0).

#### `pub fn clear_where(&self, mut predicate: impl FnMut(&NotificationEntry) -> bool)`

Remove every entry matching `predicate`, decrementing
`unread_count` for each removed entry that was unread. The
scoped counterpart of `clear`: a bell scoped to
one window/audience must only clear ITS entries — the unscoped
`clear()` wipes the ENTIRE shared archive (every window's
history), which would be wrong for a scoped "Clear" button.

#### `pub fn remove_by_id(&self, id: u64)`

Remove the entry with the given **stable** id (see
`NotificationEntry::id` — "assigned by the archive on first
push; never reused"). Updates `unread_count` if the removed entry
was unread. No-op (no version bump) when no entry has that id.

Deliberately id-based rather than index-based: an index is a
snapshot of the list's shape at the moment it was read, and is
meaningless once anything else — a concurrent peer-process reload
merged in via the live archive, another `push`, another `remove` —
has shifted rows out from under it. A caller that captured "the row
I want to dismiss" as an index earlier and replays it later against
a since-mutated list can silently remove the *wrong* entry; keying
off `id` instead re-resolves the row's current position at the
moment of removal, so it always removes the entry the caller meant.

## `pub struct NotificationLogDialog`

One-liner modal preset around `NotificationLog`. Apps usually
wire this to a menu item or shortcut (e.g. "Window → Notification
Log…").

```rust
pub struct NotificationLogDialog;
```

### Methods

#### `pub fn show(archive: Rc<NotificationArchiveModel>, ctx: &mut EventContext)`

Present the dialog with the standard chrome (title +
720x520 default size, escape-or-click-outside dismissal).

#### `pub fn show_with( archive: Rc<NotificationArchiveModel>, ctx: &mut EventContext, configure: impl FnOnce(NotificationLog) -> NotificationLog + 'static, )`

Same as `show`, but lets the caller configure the embedded
`NotificationLog` (e.g. attach an `on_action_invoked` hook
for archive replay).
