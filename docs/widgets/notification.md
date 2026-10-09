<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# NotificationEntry

Persistent notification archive — the storage and data-model layer
backing `NotificationLog`, `NotificationCenterButton`, and
`NotificationLogDialog`.

## Public types

| Kind | Name |
| ---: | :--- |
| `struct` | [`NotificationEntry`](#notificationentry) — A single archived notification entry rendered by `NotificationLog` and persisted under `NotificationArchive::Persistent` |
| `struct` | [`NotificationUpdate`](#notificationupdate) — One in-place mutation applied when a `Toast` with the same `id` as an existing entry is presented again |
| `enum` | [`ArchivedActionStyle`](#archivedactionstyle) — Visual presentation of an archived action button |
| `struct` | [`ArchivedAction`](#archivedaction) — A single action stored alongside an archived notification entry |
| `const` | [`DEFAULT_ARCHIVE_LIMIT`](#default_archive_limit) — Default per-archive entry cap |
| `const` | [`ARCHIVE_FILE_NAME`](#archive_file_name) — File-name (without extension) used for the persistent archive |
| `const` | [`UPDATE_HISTORY_LIMIT`](#update_history_limit) — How many `NotificationUpdate` records one row keeps |
| `enum` | [`NotificationArchive`](#notificationarchive) — Storage mode for the notification archive |
| `struct` | [`NotificationArchiveModel`](#notificationarchivemodel) — Shared model — clones share state |
| `struct` | [`NotificationLogDialog`](#notificationlogdialog) — One-liner modal preset around `NotificationLog` |

## Public functions

### `NotificationArchive`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`in_memory()`](#notificationarchive-in_memory) |
| `Self` | [`in_memory_with_limit(limit: usize)`](#notificationarchive-in_memory_with_limit) |
| `Self` | [`persistent(file_name: impl Into<String>)`](#notificationarchive-persistent) |
| `Self` | [`persistent_with_limit(file_name: impl Into<String>, limit: usize)`](#notificationarchive-persistent_with_limit) |
| | **Methods** |
| `usize` | [`limit()`](#notificationarchive-limit) |

### `NotificationArchiveModel`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Result<Self, NotificationArchiveError>` | [`open(archive: &NotificationArchive, paths: &AppPaths, debounce: Duration)`](#notificationarchivemodel-open) |
| `Self` | [`in_memory()`](#notificationarchivemodel-in_memory) |
| | **Methods** |
| `&ListModel<NotificationEntry>` | [`entries()`](#notificationarchivemodel-entries) |
| `&Signal<usize>` | [`unread_count()`](#notificationarchivemodel-unread_count) |
| `&Signal<u64>` | [`version_signal()`](#notificationarchivemodel-version_signal) |
| `usize` | [`limit()`](#notificationarchivemodel-limit) |
| `Result<(), SettingsFileError>` | [`flush_now()`](#notificationarchivemodel-flush_now) |
|  | [`push(entry: NotificationEntry)`](#notificationarchivemodel-push) |
|  | [`push_update(entry: NotificationEntry)`](#notificationarchivemodel-push_update) |
|  | [`mark_read_where(mut predicate: impl FnMut(&NotificationEntry) -> bool)`](#notificationarchivemodel-mark_read_where) |
|  | [`mark_all_read()`](#notificationarchivemodel-mark_all_read) |
|  | [`clear()`](#notificationarchivemodel-clear) |
|  | [`clear_where(mut predicate: impl FnMut(&NotificationEntry) -> bool)`](#notificationarchivemodel-clear_where) |
|  | [`remove_by_id(id: u64)`](#notificationarchivemodel-remove_by_id) |

### `NotificationLogDialog`

| Returns | Function |
| ---: | :--- |
| | **Associated functions** |
|  | [`show(archive: Rc<NotificationArchiveModel>, ctx: &mut EventContext)`](#notificationlogdialog-show) |
|  | [`show_with(archive: Rc<NotificationArchiveModel>, ctx: &mut EventContext, configure: impl FnOnce(NotificationLog) -> NotificationLog + 'static)`](#notificationlogdialog-show_with) |

## Detailed description

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

#### When to use

- Pair with `TeksiloAppBuilder::install_toast_default()` to get the full
  bell-button + log + persistence stack for free.
- Construct `NotificationArchiveModel::in_memory` directly in tests or
  custom toast setups.

```ignore
// In app boot, after install_toast:
let archive = ctx.app_state::<Rc<RefCell<NotificationArchiveModel>>>().unwrap();
let log = NotificationLog::new(archive.clone());
```

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/notification/index.html)

<a id="notificationentry"></a>

## `pub struct NotificationEntry`

A single archived notification entry rendered by `NotificationLog` and
persisted under `NotificationArchive::Persistent`. Carries plain owned
fields only — no closures, no `Rc<dyn Fn>` — so it is `Serialize`-friendly.

```rust
pub struct NotificationEntry { /* fields */ }
```

<a id="notificationupdate"></a>

## `pub struct NotificationUpdate`

One in-place mutation applied when a `Toast` with the same `id` as an
existing entry is presented again. The archive merges these onto the
existing row — the "Uploading 3 of 7 → Upload complete" pattern.

```rust
pub struct NotificationUpdate { /* fields */ }
```

<a id="archivedactionstyle"></a>

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

<a id="archivedaction"></a>

## `pub struct ArchivedAction`

A single action stored alongside an archived notification entry. Only
re-invokable from `NotificationLog` when `intent_name` is set — actions
whose live closure has torn down render as inert descriptive labels.

```rust
pub struct ArchivedAction { /* fields */ }
```

<a id="default_archive_limit"></a>

## `pub const DEFAULT_ARCHIVE_LIMIT`

Default per-archive entry cap. IntelliJ's notification log keeps
hundreds of entries with no cap visible to the user; we pick a
pragmatic limit so persistent files don't grow unbounded.

```rust
pub const DEFAULT_ARCHIVE_LIMIT: usize = 200;
```

<a id="archive_file_name"></a>

## `pub const ARCHIVE_FILE_NAME`

File-name (without extension) used for the persistent archive.
Resolved through `AppPaths::config_file` into
`<config_dir>/<app>/notifications.toml`.

```rust
pub const ARCHIVE_FILE_NAME: &str = "notifications";
```

<a id="update_history_limit"></a>

## `pub const UPDATE_HISTORY_LIMIT`

How many `NotificationUpdate`
records one row keeps. A progress notice that reports each step in its
wording records one update per step, and an id an app reuses for every
run of an operation gathers them from every run, so a row keeps the most
recent ones and drops the oldest. The row itself always shows the notice
as it now stands, whatever the history kept.

```rust
pub const UPDATE_HISTORY_LIMIT: usize = 20;
```

<a id="notificationarchive"></a>

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

<a id="notificationarchive-in_memory"></a>

#### `pub fn in_memory() -> Self`

In-memory archive with the default 200-entry cap.

<a id="notificationarchive-in_memory_with_limit"></a>

#### `pub fn in_memory_with_limit(limit: usize) -> Self`

In-memory archive with a custom cap.

<a id="notificationarchive-persistent"></a>

#### `pub fn persistent(file_name: impl Into<String>) -> Self`

File-backed archive resolved through `AppPaths::config_file`
at install time. The default file name (`"notifications"`)
yields `<config_dir>/<app>/notifications.toml`. Apps that
want a different name pass it here; tests pass an arbitrary
name and use `AppPaths::for_testing(tmpdir)`.

<a id="notificationarchive-persistent_with_limit"></a>

#### `pub fn persistent_with_limit(file_name: impl Into<String>, limit: usize) -> Self`

<a id="notificationarchive-limit"></a>

#### `pub fn limit(&self) -> usize`

<a id="notificationarchivemodel"></a>

## `pub struct NotificationArchiveModel`

Shared model — clones share state. Constructed by the install
helper from `NotificationArchive` + `AppPaths`; apps reach it
via `ctx.app_state::<Rc<NotificationArchiveModel>>()`.

`NotificationLog` and `NotificationCenterButton`
consume this model directly.

```rust
pub struct NotificationArchiveModel { /* fields */ }
```

### Methods

<a id="notificationarchivemodel-open"></a>

#### `pub fn open( archive: &NotificationArchive, paths: &AppPaths, debounce: Duration, ) -> Result<Self, NotificationArchiveError>`

Construct from a `NotificationArchive` config. For
`Persistent` mode, resolves the path through `AppPaths`.
Tests use `AppPaths::for_testing(tmpdir)` + `Duration::ZERO`
debounce.

<a id="notificationarchivemodel-in_memory"></a>

#### `pub fn in_memory() -> Self`

Convenience: construct an `NotificationArchive::InMemory`
archive with the default cap, without going through paths.
Mostly useful for tests and apps that explicitly want no
persistence.

<a id="notificationarchivemodel-entries"></a>

#### `pub fn entries(&self) -> &ListModel<NotificationEntry>`

Reactive handle on the entries. Bind to a `ListView` /
`Repeater` for live UI.

<a id="notificationarchivemodel-unread_count"></a>

#### `pub fn unread_count(&self) -> &Signal<usize>`

Signal of the unread count. Drives the bell-button badge.

<a id="notificationarchivemodel-version_signal"></a>

#### `pub fn version_signal(&self) -> &Signal<u64>`

Reactive handle on the archive's mutation version. Widgets
that render the archive (`NotificationLog`,
`NotificationCenterButton`) bind to this at
`BindingLevel::Rebuild`, in every window — one signal is enough
for N of them, see
`ToastRegistry::version_signal`
for the history of why that had to be said out loud.

<a id="notificationarchivemodel-limit"></a>

#### `pub fn limit(&self) -> usize`

<a id="notificationarchivemodel-flush_now"></a>

#### `pub fn flush_now(&self) -> Result<(), SettingsFileError>`

Force the persistent backing file to disk synchronously.
No-op for `InMemory`. Tests call this between mutations and
re-opening the file to verify persistence.

<a id="notificationarchivemodel-push"></a>

#### `pub fn push(&self, entry: NotificationEntry)`

Push a notice that has just been raised. Inserts at index 0
(newest first), evicts the oldest if the resulting length exceeds
`limit`. Stamps the entry's `id` field from `next_id`. Bumps
`unread_count` when the entry is unread (which is the typical case
from a toast push).

If `entry.dedup_id` matches an existing row, the notice is that row
raised again: an operation that failed this morning and fails again
now, under the same `Toast::id`. No new row
is inserted. The row moves to the front, takes the entry's
`timestamp`, records a
`NotificationUpdate` and
takes the entry's read state, which makes it unread again, even when
the notice says exactly what it said before: the log and the bell show
that it happened again. Every other field is merged as
`push_update` describes.

<a id="notificationarchivemodel-push_update"></a>

#### `pub fn push_update(&self, entry: NotificationEntry)`

Push an in-place update of a notice that is still on screen: a
progress notice's next step, or its result. This is what the toast
registry calls when a `Toast::id` matches a live toast.

If `entry.dedup_id` matches an existing row, that row is updated in
place: same position, and it keeps the time the notice was raised.
The row then shows the notice **as it now stands**, the way the live
toast it mirrors does:

- `title`, `body`, `severity`, `priority`, `actions` and `route`
  are the update's. An update offering no actions leaves the row
  offering none: a progress notice's Cancel must not outlive the
  notice that said the work was cancelled, nor stand in for the
  Open and See report a finished one offers.
- `group` and `source` are the update's when it names them, and
  stay as they were when it does not.
- `id`, `timestamp` (when the notice was raised, which places the row
  in the log) and `dedup_id` stay.
- A `NotificationUpdate`
  is appended when the update changes what the row shows (its
  wording, severity or actions), carrying the update's timestamp and
  whichever of title and body changed. A row keeps the most recent
  `UPDATE_HISTORY_LIMIT` of them. An update that repeats the row
  exactly records nothing and leaves its read state alone.
- A change gives the row the update's read state, which for a toast
  is unread. `unread_count` counts rows, so it grows only when the
  row had been read.

With no matching row (the user cleared it, or it was evicted), the
update is pushed as a new row, like `push`.

<a id="notificationarchivemodel-mark_read_where"></a>

#### `pub fn mark_read_where(&self, mut predicate: impl FnMut(&NotificationEntry) -> bool)`

Mark every UNREAD entry matching `predicate` as read,
decrementing `unread_count` by exactly how many were flipped.
This is the scoped counterpart of `mark_all_read`:
a bell scoped to one window/audience must only mark ITS
entries read on close — calling the unscoped `mark_all_read`
from a scoped bell would incorrectly clear every OTHER
window's/audience's unread state too.

<a id="notificationarchivemodel-mark_all_read"></a>

#### `pub fn mark_all_read(&self)`

Mark every archived entry as read; reset `unread_count` to 0.
Called by `NotificationCenterButton` when its popover closes.

<a id="notificationarchivemodel-clear"></a>

#### `pub fn clear(&self)`

Clear the entire archive (resets `unread_count` to 0).

<a id="notificationarchivemodel-clear_where"></a>

#### `pub fn clear_where(&self, mut predicate: impl FnMut(&NotificationEntry) -> bool)`

Remove every entry matching `predicate`, decrementing
`unread_count` for each removed entry that was unread. The
scoped counterpart of `clear`: a bell scoped to
one window/audience must only clear ITS entries — the unscoped
`clear()` wipes the ENTIRE shared archive (every window's
history), which would be wrong for a scoped "Clear" button.

<a id="notificationarchivemodel-remove_by_id"></a>

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

<a id="notificationlogdialog"></a>

## `pub struct NotificationLogDialog`

One-liner modal preset around `NotificationLog`. Apps usually
wire this to a menu item or shortcut (e.g. "Window → Notification
Log…").

```rust
pub struct NotificationLogDialog;
```

### Methods

<a id="notificationlogdialog-show"></a>

#### `pub fn show(archive: Rc<NotificationArchiveModel>, ctx: &mut EventContext)`

Present the dialog with the standard chrome (title +
720x520 default size, escape-or-click-outside dismissal).

<a id="notificationlogdialog-show_with"></a>

#### `pub fn show_with( archive: Rc<NotificationArchiveModel>, ctx: &mut EventContext, configure: impl FnOnce(NotificationLog) -> NotificationLog + 'static, )`

Same as `show`, but lets the caller configure the embedded
`NotificationLog` (e.g. attach an `on_action_invoked` hook
for archive replay).
