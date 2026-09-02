<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ToastHost

`ToastHost` — invisible sibling widget that owns the toast queue.

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
is bumped, the host rebuilds, the surface widgets are destroyed.

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

## Builder methods at a glance

`wrapping`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/toast/host/index.html)

## `pub struct ToastInstallOptions`

Configuration for the installed `ToastHost`. Passed to
`install_toast` in the `teksilo` umbrella crate.

```rust
pub struct ToastInstallOptions { /* fields */ }
```

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

#### `pub fn new(registry: ToastRegistry, options: ToastInstallOptions) -> Self`

Construct a host bound to the given registry. Add to the tree
alongside the user root inside a `ZStack`.

#### `pub fn wrapping( _user_root: WidgetId, registry: ToastRegistry, options: ToastInstallOptions, ) -> Self`

Backwards-compatibility alias for ergonomic post-root
installation: an app that already has a wrapping ZStack can
construct a host via the standalone `new(...)`. This helper
returns a fresh wrapper that uses `ZStack` internally — but
since the wrapping is owned by `install_toast` itself, this is
rarely called by user code.
