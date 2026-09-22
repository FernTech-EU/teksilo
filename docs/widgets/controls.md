<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# WindowControls

The minimize / maximize / close button cluster on the trailing edge of
a `TitleBar`. Rendered only when
`PlatformTitleBarHost::renders_custom_controls` is `true`
(Windows, Wayland, and X11 with a capable window manager; never on macOS).

These are deliberately NOT built on top of the regular `Button` widget:
`Button` carries a 72 dp minimum width, themed padding, focus ring and
border, none of which are appropriate for a flush-fitting Win11-style
window control. Instead, each control is a small composing widget
`ControlButton` built from primitives (FixedSize + ZStack +
RectWidget + Center + TextWidget) so we inherit centering, theming and
reactive hover for free.

The maximize/restore swap is driven by a `Signal<bool>` (`show_restore`,
sourced from `WindowState::placement`): the a11y name and action toggle
between Maximize and Restore. The glyph itself does not swap — both
states render `□`, since text-typeset's font fallback has no reliable
"two stacked squares" glyph (see `WindowControls::build`).

## Touch and pen

A control cell clears the conformance floor on both axes at every density, so
nothing here needs widening, and each button activates on the release. The cell
is **not** density-projected: its height is the title bar's, which the platform
chrome sizes, and raising it at Touch would overflow the bar.

The hover tint is decoration. On Windows the OS owns hover over the non-client
area, which is what the external hover signal is for; a contact produces no
hover on any platform and loses nothing by it.

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/title_bar/index.html)

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
`ControlButton`s. Each cell drives the window directly through
`WindowState::placement` / `ctx.close_window()`; the host is used only
to register each button's external hover signal.

```rust
pub struct WindowControls { /* fields */ }
```

### Methods

#### `pub fn new( host: Rc<dyn PlatformTitleBarHost>, show_restore: Signal<bool>, close_action: Option<CloseAction>, ) -> Self`

Build the minimize / maximize / close cluster for the given platform host.

`show_restore` drives the maximize ↔ restore swap: `true` renders the
**Restore** affordance (a11y name and action), `false` the **Maximize**
one. It is deliberately not called `is_maximized`: a window is also
restorable — and must not offer "maximize" — while it is
`WindowPlacement::Fullscreen`,
which `WindowPlacement::is_maximized` reports as `false`. See
`crate::title_bar::TitleBar`'s own derivation.

`close_action` overrides the default `ctx.close_window()` behaviour (e.g.
to show a "save before closing?" dialog).
