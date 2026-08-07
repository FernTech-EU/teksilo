<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ToastSurface

`ToastSurface` — the rendered chrome of one toast.

Built by `ToastHost` for each live entry. Owns the severity
glyph, title + body column, action row, close button, and the
`Role::Alert` / `Role::Status` AccessKit node mapping. The visual
chrome (background, padding, layout) is delegated to the active
`ToastStyle` via `make_body`.

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/toast/index.html)

## `pub struct ToastSurfaceData`

Snapshot data passed to a `ToastSurface` for one live entry. Owned
by the host's `LiveEntry` and cloned into the surface at build
time. `Rc<...>` fields keep callbacks cheap to copy.

```rust
pub struct ToastSurfaceData { /* fields */ }
```

## `pub struct ToastSurface`

One rendered toast — chrome owned by `ToastStyle::make_body`,
functional pieces (glyph, body, action row, close button) owned
by this widget. Built fresh for each entry — there is no internal
`Signal<Option<…>>` slot binding (the host rebuilds on changes).

```rust
pub struct ToastSurface { /* fields */ }
```

### Methods

#### `pub fn new( data: ToastSurfaceData, leading_widget: Option<Box<dyn Widget>>, registry: ToastRegistry, closable_on_escape: bool, ) -> Self`

Build a surface for a single live toast entry. Called by `ToastHost`
once per live registry entry during each rebuild pass. `leading_widget` is `Some` for
`Toast::loading` (a spinner) and `None` for severity-glyph entries (a `SeverityBadge`
is synthesised in `build`). `closable_on_escape` mirrors the matching `Toast` field.

## `pub fn _default_dismiss(...)`  *(hidden)*

Convert milliseconds to a Duration — used for default tests.

```rust
pub fn _default_dismiss() -> std::time::Duration;
```
