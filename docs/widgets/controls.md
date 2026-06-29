<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# WindowControls

The minimize / maximize / close button cluster on the trailing edge of
a `TitleBar`. Rendered only when
`PlatformTitleBarHost::renders_custom_controls` is `true`
(Windows + Wayland; never on macOS).

These are deliberately NOT built on top of the regular `Button` widget:
`Button` carries a 72 dp minimum width, themed padding, focus ring and
border, none of which are appropriate for a flush-fitting Win11-style
window control. Instead, each control is a small composing widget
`ControlButton` built from primitives (FixedSize + ZStack +
RectWidget + Center + TextWidget) so we inherit centering, theming and
reactive hover for free.

For M2 the maximize/restore swap is *not* implemented — the maximize
button always shows the `□` glyph. M3+ will add a `Signal<bool>`-driven
glyph swap once the host can update it from `WindowEvent::Resized`.

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/title_bar/index.html)

## `pub struct WindowControlsLayout`

Layout snapshot that `WindowControls` exports to its parent `TitleBar`
so the `after_paint` aggregator can read the per-button `WidgetId`s.
Populated during `WindowControls::build`.

The maximize slot is the **Switcher** that wraps the two glyph
buttons (`□` / `❐`), not either child directly: the inactive
Switcher child is dormant and has `Rect::ZERO` bounds, but the
Switcher container itself is always laid out by the parent
HStack and has valid bounds. A synthetic tap dispatched at the
Switcher's bounds-center routes through hit-testing to whichever
child is currently visible.

```rust
pub struct WindowControlsLayout { /* fields */ }
```

## `pub type ControlAction`

Action invoked when a `ControlButton` is tapped.

```rust
pub type ControlAction = Rc<dyn Fn(&mut EventContext)>;
```

## `pub struct ControlButton`

A compact, flush-fitting window-control button.

Composes existing primitives — a `FixedSize` cell wrapping a `ZStack`
of (hover background, centred glyph). Hover state is tracked in a
`Signal<bool>` that drives a derived `Signal<SurfaceRole>` background,
so a hover change repaints with no relayout. Both the glyph color
(`fg`) and the hover surface are stored as *roles* (`ColorProp` /
`SurfaceRole`) that resolve against the current theme at paint time —
so the cluster retints live across `ctx.set_theme(...)` without a
rebuild.

```rust
pub struct ControlButton { /* fields */ }
```

### Methods

#### `pub fn new(glyph: &'static str, width: f32, height: f32, fg: impl Into<ColorProp>) -> Self`

Create a control button with the given Unicode glyph, fixed cell dimensions, and
foreground color role. The hover background defaults to transparent until overridden
via `hover_background`.

#### `pub fn hover_background(mut self, role: SurfaceRole) -> Self`

Set the surface role painted over the title bar background while the pointer is inside
the button cell. The default is `SurfaceRole::Transparent` (flat).

#### `pub fn on_tap(mut self, action: impl Fn(&mut EventContext) + 'static) -> Self`

Register the callback invoked when the user taps this button.

## `pub struct WindowControls`

The minimize / maximize / close cluster, laid out as an HStack of
`ControlButton`s. Each cell forwards taps to the supplied host.

```rust
pub struct WindowControls { /* fields */ }
```

### Methods

#### `pub fn new( host: Rc<dyn PlatformTitleBarHost>, is_maximized: Signal<bool>, close_action: Option<CloseAction>, ) -> Self`

Build the minimize / maximize / close cluster for the given platform host. `is_maximized`
drives the maximize ↔ restore glyph swap; `close_action` overrides the default
`ctx.close_window()` behaviour (e.g. to show a "save before closing?" dialog).
