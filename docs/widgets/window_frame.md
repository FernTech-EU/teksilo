<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# WindowFrame

A borderless-window frame: an invisible overlay of resize strips and
corner cells along the four edges of a single content widget.

`WindowFrame` is the canonical way to wrap a `TitleBar` + body for an
undecorated Wayland window. The content child fills the entire window
bounds — there is *no* visible padding — and the resize strips +
corners sit on top of the content along the edges. teksilo-core's
`hit_test_recursive` walks children in reverse insertion order, so
the strips and corners (added after content) get first crack at any
click that lands within `thickness` pixels of an edge; clicks
anywhere else fall through to the content.

Layout (with `thickness = t`):

```text
┌─top─edge───────────────────────┐  ← top strip overlays content (0, 0, w, t)
│TL│                          │TR│  ← corners overlay the strip ends
│──│                          │──│
│L │       content (full)     │R │  ← content fills (0, 0, w, h)
│──│                          │──│
│BL│                          │BR│
└─bottom─edge────────────────────┘
```

`t` defaults to 6 logical pixels but is configurable via
`WindowFrame::thickness`. With a small thickness the frame is
visually undetectable; the cursor only changes (and the resize
gesture only triggers) when the pointer is within `t` pixels of the
window boundary.

## Builder methods at a glance

`thickness`, `content`, `content_boxed`, `content_id`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/title_bar/window_frame/index.html)

## `pub struct WindowFrame`

Invisible overlay of resize strips and corner cells that gives a borderless window
draggable edges. The content child fills the full client area with no visible inset;
the strips are hit-test-only overlays along the outer `thickness` pixels.

```rust
pub struct WindowFrame { /* fields */ }
```

### Methods

#### `pub fn new(host: Rc<dyn PlatformTitleBarHost>) -> Self`

Create a frame bound to the given platform host. Use `thickness`
and `content` to configure it before adding to the tree.

#### `pub fn thickness(mut self, t: f32) -> Self`

Logical-pixel thickness of each resize strip. Default: 6.

#### `pub fn content(mut self, w: impl Widget + 'static) -> Self`

Set the inner content widget — typically a `VStack` containing a
`TitleBar` and the application body.

#### `pub fn content_boxed(mut self, w: Box<dyn Widget>) -> Self`

Set the inner content widget from an already-boxed value. Prefer `content`
for unboxed widgets; use this variant when the concrete type is not known at the call site.

#### `pub fn content_id(mut self, id: WidgetId) -> Self`

Set the inner content widget by its already-registered `WidgetId`. Use when the content
was added to the tree before the frame was constructed and you need to retain its id.
