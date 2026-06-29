<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SceneTapEvent

Per-item event handlers, cursor and tooltip overrides.

`SceneItemHandlerSet` is the lightweight-tier counterpart to
widget-level `HandlerSet`.
It carries optional closures the `SceneView`
invokes when pointer / hover / context-menu events land on the
item, plus per-item cursor and tooltip overrides.

Apps attach handlers via `Scene::set_item_handlers` /
`Scene::handlers_mut` after `add_item`:

```ignore
let id = scene.add_item(rect, Point::ZERO);
scene.handlers_mut(id).unwrap()
    .on_tap(|_pt, ctx| ctx.send_intent(AppIntent::OpenCard))
    .cursor(CursorIcon::Pointer)
    .tooltip("Open card");
```

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_scene/index.html)

## `pub struct SceneTapEvent`

Click-style gesture envelope for scene items. Mirrors the
widget-tier `bastyde_core::gesture::TapEvent` but with the
position in **scene** coordinates instead of widget-local. Used
by the tap / double-tap / triple-tap / long-press / context-menu
handlers on `SceneItemHandlerSet`.

`#[non_exhaustive]` so future additions (e.g. tap count,
stylus pressure) can land without breaking match patterns.

```rust
pub struct SceneTapEvent { /* fields */ }
```

### Methods

#### `pub fn new(position_scene: Point, button: PointerButton, modifiers: Modifiers) -> Self`

Construct one by hand. Useful for tests; dispatch builds
these from the live pointer event in `SceneView`.

## `pub enum DragMode`

What a `SceneView`'s on-canvas pointer drag
does in empty space.

* `DragMode::NoDrag` — nothing happens. Useful for embedded
  read-only diagrams.
* `DragMode::ScrollHandDrag` — left-click-drag pans the view.
  Item-level on-drag handlers are bypassed; the canvas grabs
  the gesture unconditionally.
* `DragMode::RubberBand` (default) — drag-on-empty-space
  creates a marquee that selects items inside on release.
  Drag-on-an-item dispatches to that item's drag handler if
  wired (the drag pipeline honours `IS_DRAGGABLE` for drag-to-move).

```rust
pub enum DragMode { /* variants */ }
```

### Variants

- **`NoDrag`** — Empty-space drag is a no-op; useful for read-only embedded diagrams.
- **`ScrollHandDrag`** — Left-click-drag pans the viewport; item-level drag handlers are bypassed.
- **`RubberBand`** — Empty-space drag draws a selection marquee; item drag dispatches to the item's drag handler (respecting `IS_DRAGGABLE`). This is the default.

## `pub struct SceneItemHandlerSet`

Per-item event closures + cursor + tooltip + drop acceptance.

Closures are stored as `Rc<dyn Fn>` so cloning the handler set
is cheap; the SceneView clones into its dispatch path.

```rust
pub struct SceneItemHandlerSet { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

An empty handler set — every closure unset, no cursor or
tooltip.

#### `pub fn on_tap<F>(&mut self, f: F) -> &mut Self where F: Fn(Point, &mut EventContext) + 'static,`

Register a tap callback. Simpler `Fn(Point, &mut ctx)`
signature for callers that only need the click position;
internally wraps in a shim that extracts
`event.position_scene`. For modifier-aware handlers (Shift-
click selection, Ctrl-click toggle, etc.) use
`Self::on_tap_event` which exposes the full
`SceneTapEvent`.

#### `pub fn on_tap_event<F>(&mut self, f: F) -> &mut Self where F: Fn(&SceneTapEvent, &mut EventContext) + 'static,`

Register a tap callback that receives the full
`SceneTapEvent` — scene-coord position, button, modifiers.
Use for modifier-aware patterns (`Shift+click extends
selection`, `Ctrl+click toggles`, middle-click handlers
once paired with `accept_tap_buttons`).

#### `pub fn on_double_tap<F>(&mut self, f: F) -> &mut Self where F: Fn(Point, &mut EventContext) + 'static,`

Register a double-tap callback (Point-only shim — see
`Self::on_tap`). **Not wired yet:** the SceneView's
dispatch doesn't recognise double-tap; the field is stored
but never fired. A future unit wires the recognizer.

#### `pub fn on_double_tap_event<F>(&mut self, f: F) -> &mut Self where F: Fn(&SceneTapEvent, &mut EventContext) + 'static,`

Rich-event variant of `Self::on_double_tap`.

#### `pub fn on_hover<F>(&mut self, f: F) -> &mut Self where F: Fn(bool, &mut EventContext) + 'static,`

Register a hover callback. Receives `true` on enter,
`false` on leave.

#### `pub fn on_context_menu<F>(&mut self, f: F) -> &mut Self where F: Fn(Point, &mut EventContext) + 'static,`

Register a context-menu callback (right-click). Point-only
shim; see `Self::on_context_menu_event` for the rich
variant.

#### `pub fn on_context_menu_event<F>(&mut self, f: F) -> &mut Self where F: Fn(&SceneTapEvent, &mut EventContext) + 'static,`

Rich-event variant of `Self::on_context_menu`.

#### `pub fn accept_tap_buttons(&mut self, mask: ButtonMask) -> &mut Self`

Mask of pointer buttons that should be treated as a tap
for this item. Default `ButtonMask::PRIMARY`. Right-click
(`SECONDARY`) always routes through `on_context_menu`
regardless of this mask.

#### `pub fn cursor(&mut self, c: CursorIcon) -> &mut Self`

Override the cursor icon shown over this item.

#### `pub fn tooltip(&mut self, t: impl Into<LocalizedString>) -> &mut Self`

Set a tooltip. Accepts anything convertible into
`LocalizedString` — most commonly
`tr!(...)` for translated copy or `lit!(...)` for fixed text.
Stored unresolved; the SceneView resolves it against the active
locale when the tooltip is shown, so a `tr!(...)` source tracks
locale changes.

#### `pub fn accepts_drops(&mut self, accepts: bool) -> &mut Self`

Mark whether the item accepts dropped payloads.
