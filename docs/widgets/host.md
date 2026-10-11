<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ToastHost

`ToastHost` — invisible sibling widget that owns the toast queue.

## Public types

| Kind | Name |
| ---: | :--- |
| `struct` | [`ToastInstallOptions`](#toastinstalloptions) — Configuration for the installed `ToastHost` |
| `struct` | [`ToastHost`](#toasthost) — Invisible sibling widget that owns the toast queue |

## Public functions

### `ToastHost`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(registry: ToastRegistry, options: ToastInstallOptions)`](#toasthost-new) |
| `Self` | [`wrapping(_user_root: WidgetId, registry: ToastRegistry, options: ToastInstallOptions)`](#toasthost-wrapping) |

## Detailed description

Installed by `install_toast(opts)` in the `teksilo` umbrella. The
umbrella's `TeksiloAppBuilderToastExt::install_toast` registers a
`DefaultPostRoot` closure that wraps
every window's root with a `ZStack` of `[user_root, ToastHost]`.
The host renders its toast surfaces as direct children, positioned
absolutely at the configured viewport corner. The wrapping ZStack
ensures toasts paint above the user content; the host itself fills
the viewport (so its children — the toasts — have absolute screen
coordinates to anchor against) and is `event_pass_through` outside
the toast bounds so the user can still interact with content below.

No overlay system involvement — toasts are regular widgets in the
arena. The host owns the per-frame timer + hover-pause; expired
entries are removed from the registry's queue, the version signal
is bumped, the host rebuilds, and the expired entry's surface is
destroyed.

The host **reconciles**: a rebuild keeps the surface of every toast
still live and builds one only for a toast that has just appeared.
A surface is a live region, and every platform announces a live
node when it enters the tree, so a host that rebuilt every surface
on every queue change read every toast still up again each time one
came or went, and destroyed the control a keyboard user was on. An
update in place (`Toast::id`) rebuilds only that toast's surface,
which keeps what did not change (see
`ToastSurface`).

Routing: each host filters `live_entry_ids()` down to entries whose
`ToastRoute` matches its own window id / assigned audience, or that
are `Broadcast`. Every host binds the SAME
`ToastRegistry::version_signal` at `BindingLevel::Rebuild` — one
signal reaches N windows, because each window's `WidgetTree` owns
its own `BindingRegistry` and that registry remembers the
generation it last reconciled (see
`teksilo_core::binding::BindingRegistry`). A host that matches
nothing in a given rebuild just produces zero new surfaces, which
is cheap and lets one shared queue serve every window without a
per-window registry.

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/toast/host/index.html)

<a id="toastinstalloptions"></a>

## `pub struct ToastInstallOptions`

Configuration for the installed `ToastHost`. Passed to
`install_toast` in the `teksilo` umbrella crate.

```rust
pub struct ToastInstallOptions { /* fields */ }
```

### Fields

- **`corner`**: `Corner` — Which viewport corner the toasts anchor to. Default `BottomTrailing` (matches JetBrains IntelliJ and Windows system tray conventions). Under RTL, Trailing flips to the physical left edge.
- **`margin`**: `Vec2` — Outer margin from the corner — (x, y) in logical pixels. Default `(24, 24)`.
- **`gap`**: `f32` — Vertical gap between stacked toasts. Default `8.0`.
- **`max_visible`**: `usize` — Maximum simultaneously visible toasts PER ROUTE BUCKET — each window, each audience and `Broadcast` get their own budget (see `ToastRegistry::enqueue`). Default `5`. Normal priority overflow drops; High / Urgent evict the oldest Normal in the same bucket.
- **`entry_width`**: `f32` — Fixed width for each toast surface. Default `380.0` (matches IntelliJ balloon width).
- **`pause_on_hover_group`**: `bool` — When true (default), hovering any toast pauses every timer. When false, only the hovered toast pauses (libadwaita behaviour).
- **`archive`**: `Option<NotificationArchive>` — Notification archive — `None` disables archival entirely (apps don't need a NotificationLog). Default: `Some(NotificationArchive::persistent(ARCHIVE_FILE_NAME))`, which writes to `<config>/notifications.toml` via `PersistedListModel`. Apps that don't have `AppPaths` configured (`SettingsBundle` not installed) should explicitly override to `Some(NotificationArchive::in_memory())` or `None` — `Persistent` will fail at install time without a `config_dir`.
- **`initial_audience`**: `Option<ToastAudience>` — Audience to seed this window with, the first time its `ToastHost` observes a real window (see `ToastHost::build`). Default `None` — the window starts unassigned (its toasts/bell show only origin-window-routed and broadcast entries) until app code calls `ToastRegistry::set_window_audience` explicitly.  Because `install_toast`'s `DefaultPostRoot` closure clones the SAME `ToastInstallOptions` for every window it wraps, this field gives every window the SAME initial audience — it is NOT a way to give distinct windows distinct starting audiences through the shared install path. Apps that need per-window initial audiences should call `set_window_audience` right after creating each window instead (typically from the window-opened handler that also knows which document/session that window is showing).

<a id="toasthost"></a>

## `pub struct ToastHost`

Invisible sibling widget that owns the toast queue. Installed once
per window by the `install_toast` extension trait via a
`DefaultPostRoot` closure (see `teksilo::toast_install`).

Renders its toast surfaces as direct children positioned at the
configured corner. Use `ZStack::new().child(user_root).child(host)`
to put the host above the user content.

```rust
pub struct ToastHost { /* fields */ }
```

### Methods

<a id="toasthost-new"></a>

#### `pub fn new(registry: ToastRegistry, options: ToastInstallOptions) -> Self`

Construct a host bound to the given registry. Add to the tree
alongside the user root inside a `ZStack`.

<a id="toasthost-wrapping"></a>

#### `pub fn wrapping( _user_root: WidgetId, registry: ToastRegistry, options: ToastInstallOptions, ) -> Self`

Backwards-compatibility alias for ergonomic post-root
installation: an app that already has a wrapping ZStack can
construct a host via the standalone `new(...)`. This helper
ignores `_user_root` and just returns that host — the ZStack
wrapping is owned by `install_toast` itself, so this is
since the wrapping is owned by `install_toast` itself, this is
rarely called by user code.
