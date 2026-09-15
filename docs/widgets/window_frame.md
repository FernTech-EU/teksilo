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
window boundary — with a *mouse*. A finger reaches the same edge
through each strip's `Widget::hit_outset`, which widens the band to
the density's target size without moving a pixel of layout; see
`resize_strip`.

## Telling the OS the same number

On a platform where the window manager answers the resize hit test itself
— Windows, through `WM_NCHITTEST` — the frame's own strips never see the
press, so the two layers have to agree about how wide the band is or a
finger lands in the gap between them. The frame therefore publishes its
**coarse** band (the widened one, in logical pixels) as
`HitRegions::resize_borders` every frame, and the backend applies it to
coarse messages only, never shrinking the band a mouse gets.

That channel has one publisher per window — `TitleBar`
aggregates the drag region and the control buttons into one snapshot from
its own `after_paint`. The frame does not compete with it: the snapshot it
publishes carries a non-zero `resize_borders` and nothing else, which is the
documented shape a backend reads as a *band update* rather than a
replacement. `after_paint` is post-order, so wrapping the title bar (the
canonical shape — `WindowFrame::content(VStack { TitleBar, body })`) puts
the band update after the aggregate snapshot every frame.

Wrapping the frame in a `WidgetBuilder` method is safe:
`WidgetWithHandlers` forwards `wants_after_paint` / `after_paint` along with
the rest of the trait, so `WindowFrame::new(host).content(..).on_tap(..)`
still publishes. It did not always — the wrapper's forwarding list was
incomplete, and an unforwarded hook silences a publish with no diagnostic —
so the list is now exhaustive and lint-guarded at its own impl.

## Builder methods at a glance

`thickness`, `content`, `content_boxed`, `content_id`, `coarse_resize_borders`

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

Logical-pixel thickness of each resize strip. Default:
`WINDOW_FRAME_RESIZE_THICKNESS`.

#### `pub fn content(mut self, w: impl Widget + 'static) -> Self`

Set the inner content widget — typically a `VStack` containing a
`TitleBar` and the application body.

#### `pub fn content_boxed(mut self, w: Box<dyn Widget>) -> Self`

Set the inner content widget from an already-boxed value. Prefer `content`
for unboxed widgets; use this variant when the concrete type is not known at the call site.

#### `pub fn content_id(mut self, id: WidgetId) -> Self`

Set the inner content widget by its already-registered `WidgetId`. Use when the content
was added to the tree before the frame was constructed and you need to retain its id.

#### `pub fn coarse_resize_borders(&self, tokens: &InputTokens) -> ResizeBorders`

The band a **coarse** pointer actually catches on each edge, in logical
pixels: the strip's painted thickness widened to the density's target
size by `Widget::hit_outset` (24 dp Compact, 44 dp Touch).

This is what the frame publishes as `HitRegions::resize_borders` so a
backend that answers the resize hit test itself can use the same number.
It is deliberately *not* the mouse band: a mouse keeps the painted
thickness, and a backend must never shrink its own metric to this.

Uniform across the four edges — every strip is built at the same
thickness — so a caller reading one field reads them all.

## `pub const WINDOW_FRAME_RESIZE_THICKNESS`

Logical-pixel thickness of each resize strip, and the frame's default.

The same 6 dp gutter the `Splitter` and the dock resize handle use. It is
**fixed at every density**: the strip is a hit-test-only overlay drawn over
the window's own edge, so widening it would eat into the content rather
than into empty space. A coarse pointer reaches it through
`Widget::hit_outset` (24 dp, 44 at Touch) over an unchanged 6 dp visual —
see `docs/density-inventory.md` and the touch design's Constants table.

```rust
pub const WINDOW_FRAME_RESIZE_THICKNESS: f32 = 6.0;
```
