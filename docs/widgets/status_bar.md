<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# StatusBar

StatusBar — a horizontal chrome bar at the bottom of a window for status
information.

The bar publishes `Role::Status` so assistive technology can discover it as
a status landmark. It is **not** a live region by default — use
`announce_changes(true)` only for bars that
surface transient messages worth reading aloud (e.g. "Saved"), not for bars
showing continuous data like cursor position or zoom level that would flood
the screen reader. Visual chrome (background, border, corner radius) is
delegated to an inner [`Panel`].

```rust
# use bastyde_widgets::StatusBar;
# use bastyde_widgets::primitives::TextWidget;
# use bastyde_i18n::lit;
let _bar = StatusBar::new()
    .child(TextWidget::new(lit!("Ln 1, Col 1")))
    .announce_changes(false);
```

## Builder methods at a glance

`child`, `add_child`, `background`, `corner_radius`, `border_color`, `border_width`, `name`, `announce_changes`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/status_bar/index.html)

## `pub const STATUS_BAR_HEIGHT`

StatusBar design tokens.

```rust
pub const STATUS_BAR_HEIGHT: f32 = 22.0;
```

## `pub const STATUS_BAR_PADDING_HORIZONTAL`

```rust
pub const STATUS_BAR_PADDING_HORIZONTAL: f32 = 8.0;
```

## `pub const STATUS_BAR_ITEM_GAP`

```rust
pub const STATUS_BAR_ITEM_GAP: f32 = 2.0;
```

## `pub struct StatusBar`

A status bar for displaying information at the bottom of a window.

Visual chrome is delegated to an inner [`Panel`]. By default the bar
uses the `SurfaceRole::Sunken` surface with **square corners** (a bar
spanning the window edge shouldn't be rounded); override the surface
with `background`, the corners with
`corner_radius`, or add a frame with
`border_color` / `border_width`.

Accessibility: the bar publishes `Role::Status` (→ AT-SPI `StatusBar`,
macOS `AXApplicationStatus`, Windows `UIA_StatusBarControlTypeId`) so
it is discoverable as a status landmark. It is **not** a live region
by default — a status bar showing continuously-changing data (cursor
position, zoom level, word count) would otherwise flood the screen
reader. Call `announce_changes(true)` for a
bar that surfaces transient messages worth reading aloud ("Saved").

```rust
pub struct StatusBar { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create an empty status bar with default styling (`SurfaceRole::Sunken`,
square corners, no live region).

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Add an inline child widget (deferred insertion).

#### `pub fn add_child(mut self, id: WidgetId) -> Self`

Add a pre-registered child widget by ID.

#### `pub fn background(mut self, color: impl Into<ColorProp>) -> Self`

Override the background surface. Accepts `Color`, a
[`SurfaceRole`], or a `Signal<Color>`.
Default (unset) is `SurfaceRole::Sunken`.

#### `pub fn corner_radius(mut self, radius: impl Into<Prop<f32>>) -> Self`

Override the corner radius. Accepts a static `f32` or a reactive
`Signal<f32>`. Default (unset) is `0.0` — square corners.

#### `pub fn border_color(mut self, color: impl Into<ColorProp>) -> Self`

Override the border color. Accepts `Color`, a
`BorderRole`, or a `Signal<Color>`.
Only painted when `border_width` > 0.

#### `pub fn border_width(mut self, width: impl Into<Prop<f32>>) -> Self`

Override the border width. Accepts a static `f32` or a reactive
`Signal<f32>`. Default (unset) is `0.0` — no border.

#### `pub fn name(mut self, name: impl Into<Prop<String>>) -> Self`

Override the accessible name announced for the bar. Accepts a
static string, a `Signal<String>`, or a `tr!(...)`
`LocalizedString` (locale-reactive).
Default (unset) is the localized "Status".

#### `pub fn announce_changes(mut self, announce: bool) -> Self`

Control whether content changes are announced by assistive tech.

Default `false`: the `Role::Status` landmark is published (still
navigable) but the bar is not a live region, so continuously-changing
data (cursor position, zoom, word count) doesn't flood the screen
reader. Set `true` to make it a `Live::Polite` region for bars that
surface transient messages worth reading aloud ("Saved").
