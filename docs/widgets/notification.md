<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# NotificationEntry

Persistent notification archive — the storage and data-model layer
backing [`NotificationLog`], [`NotificationCenterButton`], and
[`NotificationLogDialog`].

Every toast presented through the toast registry is mirrored into a
[`NotificationArchiveModel`] when archiving is enabled via
`ToastInstallOptions::archive`. The model is a
`ListModel<NotificationEntry>` plus an
unread-count signal — shaped for one-line binding to the notification
UI family. Two storage variants are available: an in-memory session-only
ring buffer ([`NotificationArchive::InMemory`]) and a file-backed
persistent store ([`NotificationArchive::Persistent`]) that survives app
restarts. Action callbacks attached via raw closures are lost on
archival; actions that should remain re-invokable from the log carry an
`intent_name` that the log replays through `ctx.send_intent(...)`.

## When to use

- Pair with `BastydeAppBuilder::install_toast_default()` to get the full
  bell-button + log + persistence stack for free.
- Construct [`NotificationArchiveModel::in_memory`] directly in tests or
  custom toast setups.

```ignore
// In app boot, after install_toast:
let archive = ctx.app_state::<Rc<RefCell<NotificationArchiveModel>>>().unwrap();
let log = NotificationLog::new(archive.clone());
```

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/notification/index.html)

## `pub struct NotificationEntry`

A single archived notification entry rendered by [`NotificationLog`] and
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
re-invokable from [`NotificationLog`] when `intent_name` is set — actions
whose live closure has torn down render as inert descriptive labels.

```rust
pub struct ArchivedAction { /* fields */ }
```
