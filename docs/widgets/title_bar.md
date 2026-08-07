<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TitleBar

Custom window title bar widget.

`TitleBar` replaces a window's native chrome with a horizontal bar that
can host menus, tools, and the standard window controls (minimize /
maximize / close). The platform plumbing — beginning a window drag,
returning the right `WM_NCHITTEST` codes on Windows, repositioning the
macOS traffic lights — lives behind the
`PlatformTitleBarHost` trait in
`teksilo-platform`. The widget itself is platform-agnostic.

Construct a `TitleBar` from inside the root-builder closure, fetching
the host from the widget tree:

```ignore
.root(|tree| {
    let host = tree.title_bar_host().expect("custom_chrome enabled");
    tree.add(
        VStack::new()
            .child(TitleBar::new(host)
                .background(theme.colors.surface_raised)
                .border(theme.colors.border, 1.0)
                .leading(TextWidget::new(lit!("My App"))))
            .child(Expand::new().child(/* body */)))
})
```

## Builder methods at a glance

`controls_visible`, `height`, `background`, `border`, `leading`, `leading_id`, `center`, `center_id`, `trailing`, `trailing_id`, `close_action`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/title_bar/index.html)

## `pub type CloseAction`

Type alias for the user-supplied close action that overrides
`host.close()` (which on Wayland is currently a no-op due to winit 0.30
lacking `Window::request_close`). Set via `TitleBar::close_action`.

```rust
pub type CloseAction = Rc<dyn Fn(&mut EventContext)>;
```

## `pub struct TitleBar`

A custom window title bar.

Layout (left to right):

```text
[leading inset] [leading slot] [drag region (flexible)] [trailing slot] [trailing inset] [window controls]
```

The leading inset reserves space for the OS-drawn traffic lights on
macOS. The drag region is a `Spacer`-style flex
child that absorbs all leftover horizontal space and forwards
pointer / drag / double-tap gestures to the platform host. The window
controls (minimize / maximize / close) are rendered only when the host
advertises `PlatformTitleBarHost::renders_custom_controls` — i.e. on
Windows and Wayland but not on macOS.

## This widget builds exactly once

`build` consumes the leading / center / trailing slots with `take()`, so a
second pass finds them all `None` and produces a bar containing nothing but
window controls — no menu, no title, no tools. Nothing here may therefore
carry a `BindingLevel::Rebuild`
binding. Reactive state on this widget is expressed either as a
`RepaintOnly` colour prop or, for structure, as dormancy via
`teksilo_core::BuildContext::visible_when` on an always-built child — which is how
`controls_visible` works. Memoising the
resolved slot ids is *not* a workaround: a rebuild replaces the inner row
and prunes its subtree, so the cached ids dangle and re-adding them yields
an empty bar just the same.

```rust
pub struct TitleBar { /* fields */ }
```

### Methods

#### `pub fn new(host: Rc<dyn PlatformTitleBarHost>) -> Self`

Construct a `TitleBar` bound to the given platform host.

The maximize/restore glyph follows `WindowState::placement` via
`ctx.window()` at build time — the host no longer owns the
maximize signal.

#### `pub fn controls_visible(mut self, visible: impl Into<Prop<bool>>) -> Self`

Show or hide the minimize / maximize / close cluster. Default `true`.

Accepts a plain `bool` or a `Signal<bool>`. Applied through the
framework's own dormancy (`teksilo_core::BuildContext::visible_when`), so a flip
costs a relayout and **never a rebuild** of the bar: a dormant node is
skipped by layout, hit-test, focus and paint, so a hidden cluster takes
no space and receives no input. A derived (`.map`) signal is fine —
binding resolves through to the mutable roots and never calls `observe`.

The case this exists for is **fullscreen**.
`WindowPlacement::Fullscreen`
is documented as "covers the entire display, title bar and all chrome
hidden", and every desktop convention agrees: macOS hides the traffic
lights, Windows fullscreen has no caption buttons, browsers and editors
hide their chrome outright. Minimize and maximize are meaningless for a
window with no frame. An app drawing custom chrome
(`DecorationsMode::CustomChrome`) owns
that decision itself, because the framework cannot hide a title bar the
app composed — so it gates it here.

An app that hides these **must** keep some other visible way out of
fullscreen: a menu item, an on-screen button, or a documented shortcut.

#### `pub fn height(mut self, height: f32) -> Self`

Set the title bar's logical-pixel height. Default: 40.

#### `pub fn background(mut self, color: impl Into<ColorProp>) -> Self`

Fill the title bar with a solid background color. Default:
transparent (the window's clear color shows through).

Accepts a `Color`, a `Signal<Color>`, or any of the role types
(`SurfaceRole`, `TextRole`, `BorderRole`, or their `Signal<…>`
variants). Role values resolve at paint time, so the title bar
retints live across `ctx.set_theme(...)` switches.

#### `pub fn border(mut self, color: impl Into<ColorProp>, width: f32) -> Self`

Draw a 1px-or-thicker bottom border separating the title bar from
the body.

Color accepts the same range as `Self::background`; pair with
`BorderRole::Default` for a theme-tracking divider.

#### `pub fn leading(mut self, widget: impl Widget + 'static) -> Self`

Set the leading-edge content (e.g. app icon, menus). Rendered to the
right of the macOS traffic-light inset.

#### `pub fn leading_id(mut self, id: WidgetId) -> Self`

Set the leading-edge content by pre-registered ID.

#### `pub fn center(mut self, widget: impl Widget + 'static) -> Self`

Set the center content (e.g. search box, breadcrumbs). Wrapped in a
flexible drag region: clicks that are not consumed by the child
initiate a window drag.

#### `pub fn center_id(mut self, id: WidgetId) -> Self`

Set the center content by pre-registered ID.

#### `pub fn trailing(mut self, widget: impl Widget + 'static) -> Self`

Set the trailing-edge content (e.g. user avatar, notification bell).
Rendered before the window controls.

#### `pub fn trailing_id(mut self, id: WidgetId) -> Self`

Set the trailing-edge content by pre-registered ID.

#### `pub fn close_action(mut self, action: impl Fn(&mut EventContext) + 'static) -> Self`

Override the close-button action. When set, the close button calls
this closure instead of `host.close()`. Required on Wayland where
the host's `close()` is a no-op (winit 0.30 has no
`Window::request_close`); the application typically wires this to
call `EventContext::close_window` directly, or to send an
`Intent` whose root-level `Action` handler calls it.
