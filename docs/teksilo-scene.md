<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# `teksilo-scene`

A pannable / zoomable scene viewport for Teksilo. Use it for any
**scene-based** application — story corkboards, mind maps, node-graph
editors, timeline views, CAD canvases, simple maps — where content is
free-positioned at scene coordinates instead of placed by a layout
algorithm.

The crate sits at the same tier as `teksilo-widgets`: it depends on
`teksilo-core`, `teksilo-canvas`, and `teksilo-tokens`, but **not** on
`teksilo-widgets`. Apps mixing scene-based and standard-widget UI bring
both crates in.

---

## Two tiers of content

Every scene mixes two tiers under one view transform:

- **Heavyweight tier** — any [`Widget`](../crates/teksilo-core/src/widget.rs)
  (Button, TextInput, Panel, custom composites) placed at a scene
  position. Fully interactive, fully accessible — every framework
  affordance survives the embedding (focus, animation, AT, drag-and-
  drop, etc.).
- **Lightweight tier** — `SceneItem`s: paint-only objects with no
  arena overhead. Cheap to render thousands of them. Used for the
  background furniture of a scene (connector lines, grids,
  decorative tiles, status dots).

Apps freely mix the two: heavyweight cards arranged on a lightweight
connector-line backdrop, a lightweight grid under a heavyweight
toolbar overlay, etc.

```rust
use teksilo_scene::{RectItem, Scene, SceneView};
use teksilo_canvas::{Point, Rect};

let mut scene = Scene::new();
scene.add_widget(my_card_widget(), Rect::new(0.0, 0.0, 200.0, 120.0));
scene.add_item(
    RectItem::new(Rect::new(0.0, 0.0, 50.0, 50.0))
        .fill(teksilo_tokens::Color::RED),
    Point::new(220.0, 0.0),
);
let view = SceneView::new(scene);
tree.add(view);
```

---

## Coordinate model

Coordinates are **parent-relative**, mirroring Qt's
`QGraphicsItem`:

- `local_pos: Point` — origin of the item's local frame, in its
  *parent*'s coordinates (or scene coords if `parent == None`).
- `local_bounds: Rect` — AABB at origin in **local** coords.
- `transform: Transform2D` — rotation / scale / shear applied around
  the local origin before translating by `local_pos`.

The Scene composes the chain (`local → parent → … → scene`) on
demand via [`Scene::scene_transform(id)`](../crates/teksilo-scene/src/scene.rs).
Helpers project both ways:

```rust
scene.scene_pos(id)          // Point in scene coords
scene.scene_rect(id)         // AABB in scene coords (used by the spatial index)
scene.scene_transform(id)    // local → scene affine
scene.map_to_scene(id, pt)   // local → scene
scene.map_from_scene(id, pt) // scene → local

view.map_to_scene(view_pt)             // view → scene
view.map_from_scene(scene_pt)          // scene → view
view.map_rect_to_scene(view_rect)
view.map_rect_from_scene(scene_rect)
```

Per-item rotation / scale composes through ancestors, so rotating a
parent rotates every descendant visually and updates their hit-test
shapes in lockstep.

---

## `SceneItem` trait

Custom items implement [`SceneItem`](../crates/teksilo-scene/src/item.rs):

```rust
pub trait SceneItem: Debug + 'static {
    fn local_bounds(&self) -> Rect;
    fn set_local_bounds(&mut self, bounds: Rect);
    fn paint(&self, canvas: &mut Canvas, ctx: &SceneItemPaintContext<'_>);

    // Optional:
    fn set_fill(&mut self, fill: Option<ColorProp>) -> bool;                     // live colour mutation
    fn set_stroke(&mut self, stroke: Option<(ColorProp, StrokeStyle)>) -> bool;
    fn shape_contains(&self, local_pt: Point) -> bool;       // exact-shape hit-test
    fn initial_flags(&self) -> ItemFlags;                     // set on insert
    fn label(&self) -> Option<String>;                        // debug + AT default
    fn cache_mode(&self) -> CacheMode;                        // None | ItemCoordinate
    fn access_subtree_mode(&self) -> AccessSubtreeMode;
    fn register_bindings(&self, ctx: &mut BuildContext, view_id: WidgetId);
    fn accessibility(&self, b: &mut AccessNodeBuilder, ctx: &SceneItemA11yContext);
}
```

`paint` runs in **local coordinates** — the canvas already has the
item's `scene_transform` (chain × view) pushed by the SceneView paint
walk, so a `RectItem` paints with `canvas.fill_rect(self.local_bounds, ...)`.
`set_fill` / `set_stroke` default to a no-op (`false`) and back the live
[`SceneModel::set_item_fill`](../crates/teksilo-scene/src/scene.rs) /
`set_item_stroke` mutators — see **Item colours & theming** below.

Five built-ins ship out of the box:
[`RectItem`](../crates/teksilo-scene/src/items/rect.rs) (optional
`corner_radius` and styled/dashed strokes via `stroke_styled`),
[`PathItem`](../crates/teksilo-scene/src/items/path.rs) (with per-segment
hit-test for stroke-only paths, also `stroke_styled`),
[`ImageItem`](../crates/teksilo-scene/src/items/image.rs),
[`TextItem`](../crates/teksilo-scene/src/items/text.rs) (static or
signal-bound; horizontal `align(TextAlign::{Leading,Center,Trailing})`, a
free `rotation(radians)`, and a `measure(&mut dyn TextBackend) -> Size`
helper for sizing a slot around a label), and
[`GroupItem`](../crates/teksilo-scene/src/items/group.rs) (labelled box
or logical-only AT container — also has `corner_radius` and
`stroke_styled`).

---

## Item colours & theming

`SceneItem::paint` receives a
[`SceneItemPaintContext`](../crates/teksilo-scene/src/item.rs) carrying
everything a colour-bearing item needs to resolve its chrome against the
live theme:

| Field | Meaning |
|---|---|
| `theme: &Theme` | The **fully-projected** theme for this paint pass — already swapped to the inactive-window / high-contrast variant by the render walker. Read it directly; never call `Theme::for_inactive_window` yourself. |
| `window_active: bool` | `true` iff the host window is focused and unoccluded (`true` in headless tests). For behavioural blur cues the theme swap alone can't cover. |
| `enabled: bool` | The item's effective `IS_ENABLED` flag, forwarded to `ColorProp::resolve` so a role colour picks its disabled variant. |
| `text_scale: f32` | The global accessibility text-scale factor, for `TextItem::follow_text_scale` opt-ins. |
| `view_transform` / `dirty_scene_rect` | Unchanged — pan/zoom/rotation and the current repaint region. |

Built-in items' fill / stroke / foreground fields are
[`ColorProp`](../crates/teksilo-core/src/color_prop.rs)s — accepting a plain
[`Color`](../crates/teksilo-tokens/src/color.rs), a theme role (`SurfaceRole`
/ `TextRole` / `BorderRole`), a `Signal<Color>`, or a `Signal<Role>` — and are
resolved with `prop.resolve(ctx.theme, ctx.enabled)` inside `paint`. Because
`ctx.theme` is already the inactive-window projection, a role fill
auto-desaturates when the window loses focus with zero per-item code:

```rust
RectItem::new(rect)
    .fill(SurfaceRole::Sunken)       // resolves against ctx.theme at paint
    .stroke(BorderRole::Default, 1.0)
```

### Reactive colours

A colour is continuously reactive in two ways:

- **Build-time**: construct the item with a `Signal<Color>` or a
  `Signal<Role>` (e.g. `.fill(my_signal.clone())`). Every colour-bearing
  built-in (`RectItem`, `PathItem`, `GroupItem`, `TextItem`) registers its
  bound `ColorProp`s at `BindingLevel::RepaintOnly` in `register_bindings`, so
  a signal change repaints the owning `SceneView` — no relayout, no rebuild.
- **Runtime**: mutate a *mounted* item's colour live through the shared
  [`SceneModel`] — `set_item_fill` / `clear_item_fill` / `set_item_stroke` /
  `clear_item_stroke`. Each emits
  [`ItemChange::AppearanceChanged`](../crates/teksilo-scene/src/scene.rs),
  which the view treats as **repaint-only, always** (it evicts the item's paint
  cache and repaints — never a relayout, rebuild, or AccessKit re-walk). These
  install a *snapshot*, which is all a static colour ever needs. Passing a
  `Signal`/dynamic role here paints its current value immediately and starts
  tracking it continuously from the view's next rebuild (whenever some other
  structural change re-runs `register_bindings`) — a colour change is
  deliberately never allowed to cost a rebuild. **For a colour that tracks its
  signal forever, construct the item with it** (the build-time path above).

```rust
let model = view.model();
model.set_item_fill(card_id, SurfaceRole::AccentSubtle);   // repaint only
model.set_item_stroke(card_id, Color::RED, StrokeStyle::dashed(2.0, 6.0, 4.0));
model.clear_item_fill(card_id);
```

`set_fill` / `set_stroke` are also the `SceneItem` trait hooks (default
no-op) a custom item overrides to participate in the same live-mutation
path: `RectItem` / `PathItem` / `GroupItem` accept both; `TextItem` maps
`set_fill` onto its foreground colour (a `None` clear is rejected — text
always has a colour); `ImageItem` accepts neither. Note that on a `GroupItem`,
giving a previously **logical** (chrome-less, click-through) group a fill or
stroke makes it *visual* — it starts hit-testing and will absorb clicks that
used to fall through to the items it groups.

> **Minimap caveat.** `SceneItem::thumbnail_color` (what
> [`SceneMinimap`](../crates/teksilo-scene/src/minimap.rs) renders — see
> [Minimap](#minimap)) is theme-free by signature, so an item whose colour is a
> **theme role** has no theme to resolve against and falls back to a neutral
> grey on the minimap. Use a concrete `Color` or a `Signal<Color>` for items you
> want faithfully represented there.

> **Cache caveat.** A custom item that opts into `CacheMode::ItemCoordinate`
> bakes its *resolved* colours into the cached frame. The `SceneView`
> invalidates that cache on a theme swap, a window-active flip, and an
> `IS_ENABLED` change, so role colours stay correct — but the cache is still
> keyed by `(id, raster_scale)`, so a custom item whose paint depends on any
> *other* ambient state must use the default `CacheMode::None`.

---

## Item flags

Per-item behaviour is a bitset on [`ItemFlags`](../crates/teksilo-scene/src/flags.rs).
Default is `IS_VISIBLE | IS_ENABLED | IS_SELECTABLE`.

| Flag | Effect |
|---|---|
| `IS_VISIBLE` | When cleared, the item skips paint and hit-test. Composes through ancestors via `Scene::is_effectively_visible`. |
| `IS_ENABLED` | Disabled items don't dispatch pointer events and don't take focus. |
| `IS_DRAGGABLE` | The item participates in drag-to-move when the view is in `DragMode::RubberBand`. |
| `IS_SELECTABLE` | The item can be picked up by marquee-select. |
| `IS_FOCUSABLE` | The item can receive keyboard focus. |
| `ACCEPTS_HOVER` | Reserved — tracks hover entrance / exit. |
| `CLIPS_TO_SHAPE` / `CLIPS_CHILDREN_TO_SHAPE` | Reserved for clip-region paint. |
| `IGNORES_TRANSFORMATIONS` | Item paints / hit-tests at fixed pixel size regardless of view zoom. Anchor (parent-relative scene point) still follows pan/zoom — so the item tracks the data point underneath, but its size stays constant. Mirrors Qt's `ItemIgnoresTransformations`. |
| `HAS_NO_CONTENTS` | Logical-only entry, skipped by the paint walk. |

Read / mutate via `Scene::flags(id)` / `Scene::set_flag(id, flag, on)` /
`Scene::set_flags(id, flags)`. Convenience: `Scene::set_visible(id,
v)`.

---

## Per-item events

Mirrors `WidgetBuilder`'s attached-handler chain for the lightweight
tier. Install via [`SceneItemHandlerSet`](../crates/teksilo-scene/src/item_handlers.rs):

```rust
let mut handlers = SceneItemHandlerSet::new();
handlers
    .on_tap(|pt, ctx| ctx.send_intent(MyIntent::Clicked))
    .on_double_tap(|pt, ctx| ctx.send_intent(MyIntent::Open))
    .on_hover(|entered, ctx| { /* … */ })
    .on_context_menu(|pt, ctx| ctx.send_intent(MyIntent::Menu))
    .cursor(CursorIcon::Pointer)
    .tooltip(tr!(card_tooltip()));   // accepts LocalizedString
scene.set_item_handlers(item_id, Some(handlers));
```

The view's pointer-dispatch path projects the screen-space pointer to
scene coords, broad-phases via the spatial index, narrow-phases via
`SceneItem::shape_contains`, then dispatches to the topmost-z hit
item's handlers.

---

## View transform & gestures

[`SceneView`](../crates/teksilo-scene/src/view.rs) owns four animated
`Signal<f32>`s:

- `pan_x`, `pan_y`
- `zoom`
- `rotation`

The composite [`view_transform`](../crates/teksilo-scene/src/view.rs)
projects scene → screen and is bound via
`BuildContext::set_transform` so the renderer pushes it around the
entire subtree.

Pointer and gesture input plugs in directly:

- **Trackpad two-finger pan** and **mouse-wheel scroll** drive
  `pan_x` / `pan_y` (Ctrl+wheel = zoom-about-pointer).
- **Pinch** drives `zoom` and `rotation` anchored on the gesture
  center. Both producers deliver the same thing, which is what makes the one
  ingress worth having: `PinchChanged`'s `scale` is the factor **since the
  previous sample** and its `rotation` the twist since the previous sample in
  **radians**, so the handler multiplies the one in and adds the other. A
  touchscreen spread to twice the starting span leaves the zoom at exactly twice,
  whatever the sample rate, and a one-degree trackpad twist turns the scene one
  degree — winit's degrees are converted at the platform seam, not here. Both are
  pinned by tests in `view/tests/touch_camera.rs`.
- **One finger** pans the camera; see the touch model below for when it
  does and when the marquee takes the gesture instead.
- **Reduced-motion** is honoured: pan / zoom snap instead of
  animating.

Programmatic API:

```rust
view.set_pan(target);     view.pan_to(target, duration);
view.set_zoom(target);    view.zoom_to(target, duration);
view.set_rotation(rad);   view.rotate_to(rad, duration);
view.ensure_visible(scene_rect, margin);   // pan-only fit
view.fit_to_content();    view.fit_to_items(&ids);    view.fit_to_selection();
```

State persistence:

```rust
let snap = view.state();    // SceneViewState — Serde-friendly
view.restore_state(snap);
```

---

## Scene policy: pan / zoom axes

The Scene declares which navigation gestures are permitted. Apps with
a fixed-extent diagram, a horizontal-only timeline, or a
"as-large-as-the-window" embedded mini-graph set:

```rust
scene.pan_axes(PanAxes::None | PanAxes::Horizontal | PanAxes::Vertical | PanAxes::Both);
scene.zoomable(false);   // disables Ctrl+wheel, pinch, +/-
```

The View captures these as `Signal` handles when it wires its gesture
handlers and reads their **values on every event**, so a runtime change
to `pan_axes`, `zoomable`, the zoom range or the pan bounds takes effect
on the next gesture with no rebuild. Pan deltas on a restricted axis
pass through to ancestor scrollables (correct event propagation).

For inline embeddings of a scene that fills its slot exactly, the
view itself sizes to the scene:

```rust
SceneView::new(scene).adopt_scene_size(true)   // view.size = scene_rect_extent
```

In adopt mode user pan / zoom are no-ops (the entire scene is on
screen) and the view's `layout_response` returns the scene's content
extent instead of a default.

`scene_rect` clamping:

```rust
scene.set_scene_rect(Some(Rect::new(-500.0, -500.0, 2000.0, 2000.0)));
// Programmatic + animated pan now clamp to scene_rect ± viewport.
```

---

## Drag mode

```rust
SceneView::new(scene).drag_mode(DragMode::RubberBand)        // default — item drag → move; empty → marquee
SceneView::new(scene).drag_mode(DragMode::ScrollHandDrag)    // left-drag pans the view
SceneView::new(scene).drag_mode(DragMode::NoDrag)
```

Middle-click pan is unconditional. Right-click on an item with an
`on_context_menu` handler fires the handler.

**Drag-start hit-test (narrow-phase).** In `RubberBand` mode, a press
decides *item drag vs. marquee* by hitting only **draggable** lightweight
items (`IS_DRAGGABLE` — opt in via `.draggable(true)`), and it hits them with
the **exact-shape** test, not just their AABB: a per-item snapshot carries the
item's `scene_rect` (broad-phase) plus its `shape_contains` predicate
(narrow-phase) and scene transform, sorted topmost-first. A press lands on an
item only when it falls inside the *shape* (a thin diagonal `PathItem`, a
ring, a rotated rect) — a press in the AABB but off the shape, or over a
non-draggable backdrop / heavyweight card, falls through to a **marquee**.
A pointer with a slop radius of its own gets one further offer once that
test has missed, which is bounded and does not reach the middle of a large
AABB; see the touch model below. This
is why dragging from on top of a select-only (non-`IS_DRAGGABLE`) card still
rubber-bands instead of nudging the scene: the card is not in the draggable
snapshot, and the cross-widget tap/drag disambiguation (see
[events-and-gestures.md](events-and-gestures.md)) lets the view's `on_drag`
start even though the card carries an `on_tap`.

---

## The touch model

A scene is navigated the same way whatever is pointing at it, but a finger
and a mouse do not have the same reach, the same idea of "still", or the
same way of asking for a menu. What follows is what differs, and what
deliberately does not.

### One finger pans the camera

An interactive `SceneView` declares a kinetic
[`PanClaim`](../crates/teksilo-core/src/pointer/touch_action.rs) on both
axes, so a contact's pan arrives at the same `on_scroll` handler a
trackpad pan does, tagged `ScrollSource::TouchPan`. The camera is `set`
rather than tweened — a finger is already the animation — and the coast
that follows a flick arrives as further scrolls along the same claimant
chain, hard-clamped, so a bounded scene stops at its edge instead of
rubber-banding. A pan the scene's own `pan_axes` has closed is declined,
which re-offers the gesture to whatever scrolls outside the view.

**A view that registers the marquee/item-drag handler — which it does
whenever selection is on or magnetism is configured — pans under a plain
finger and marquees after a hold.** That handler puts a drag recognizer on
the same node as the pan claim, and a node can hold only one role in a
pointer sequence: it is enrolled as the pan claimant before any handler
runs, so the drag half arrives at a taken slot.
`PointerSequence::defer_own_drag` is that half's door — it attaches the
drag's `DragActivation` to the member the node already has, and `Auto` on a
direct pointer with an eligible pan resolves to `AfterLongPress`. So a
finger pans at the touch **pan** slop, a hold arms the marquee (or the item
grab, or a magnet port drag) and it then latches at the **drag** slop, and
a mouse — which enrols no pan claimant at all — latches at the 5 dp it
always did. Before that arm existed the drag latched at 18 dp through the
capture dispatch and decided the sequence before the pan was ever eligible,
so this surface could not pan under a finger at all.

Two consequences worth stating. The hold has to be a **hold**: a press that
has already travelled past `long_press_slop` when the deadline arrives is
withdrawn rather than armed, so a slow, deliberate pan stays a pan instead
of becoming a marquee by outlasting the clock. And the hold is spent **on
this node only** — a heavyweight `add_widget_item` widget inside the view
keeps its own touch long press and its own touch context menu; what the
view gives up is its *own* hold, on a press that landed on its background.
A view that wants its background hold back for a menu answers
`EventContext::set_drag_activation(DragActivation::Immediate)` from its
press handler for that press, which spends no hold. `DragMode::ScrollHandDrag`
on a toolbar toggle remains available and is read live.

See `docs/events-and-gestures.md` §4.2 and
`crates/teksilo-core/tests/dual_role_arbitration.rs`.

### The grab tolerances a finger earns

Three numbers change with the pointing device, and all three come from
[`HitSlop::for_pointer`](../crates/teksilo-core/src/pointer/hit_slop.rs)
or the pointer's own gesture profile rather than from a scene-specific
token — so the mouse's values are the ones it always had, by arithmetic
and not by a branch (the mouse profile's slop radius is `0.0`).

- **Item grab and item tap.** The exact-shape test above runs first and
  unchanged. Only when it finds nothing does a pointer whose profile
  declares a slop radius — a finger, and a pen by a much smaller amount —
  get a second offer, against each item's bounding box widened by the slop that
  item's *screen* size earns — nearest box wins rather than topmost, so a
  near stroke is preferred to a further box. An item already at least the
  density's target size on its smaller axis earns nothing, so a scene of
  large cards acquires no halo; a two-pixel connector stroke earns the
  full offer, which is what makes it grabbable at all. The offer cannot
  outrank an exact hit: an exact hit is inside its own box, so its
  distance is zero.
- **Magnet handles.** `MagnetismConfig::capture_px` is the whole radius
  for a mouse and a floor for everything else, which adds what a disc of
  that diameter earns from its own profile. The four snap-*arrival* radii are left
  alone: they decide how close a dragged thing has to come before it
  snaps, which is feel, not reach.
- **Tap versus drag.** The movement a press may carry and still be a tap
  is measured in **view pixels**, so it is the same physical distance at
  every zoom; a scene-unit comparison shrinks it as the view zooms out
  until an ordinary click cannot land. A precise pointer keeps the view's
  own `TAP_MOVEMENT_THRESHOLD` floor; a coarse one is allowed the travel
  its gesture profile already calls a tap.

### Hover, tips and menus

The hover seam — an item's `on_hover`, its tooltip, the cursor shape — is
only run for a pointer that hovers. A contact is dispatched moves like
any other pointer but has no hover to give, and running the seam for one
both schedules tips nobody asked for and strands them, because a lift
produces no pointer-leave to retract them with.

A finger reaches an item's tooltip by **holding still on it**: the press
arms a delayed show at the pointer's own long-press duration, travel past
the tap tolerance disarms it, and a lift before the deadline drops it —
so a tap leaves nothing behind. A tip the hold did show stays up after
the lift, because a finger cannot keep pointing at what it is reading;
the next press anywhere in the view retracts it.

**A hold does not yet open an item's context menu.** The obvious
implementation — an `on_long_press` handler on the view — cannot be used:
it installs a long-press recognizer in the view's own gesture arena, a
recognizer that wins resets its peers, and it outranks drag, so a *mouse*
press held past the deadline would lose the marquee its drag recognizer
was waiting to start. `a_mouse_press_held_past_the_hold_deadline_still_marquees`
guards that. The framework's tree-owned hold route is the mechanism that
does not have this problem, but it resolves against the *node* under the
press, and a scene item has no node: it carries its handlers on the
scene, not the arena. Reaching an item's menu from a hold therefore needs
a way for a widget to answer what a hold at a given point means for it.
Right-click still opens an item's menu, and a finger can reach the same
command through whatever the app puts on the item's tap.

---

## Reactive observers — `item_change_signal`

Every Scene mutation fires an [`ItemChange`](../crates/teksilo-scene/src/scene.rs)
event through `Scene::item_change_signal()`. Apps observe to wire validation,
persistence, telemetry, or mirroring into a data layer:

```rust
// `model` is a SceneModel; `writer` is a clone of it.
let writer = model.clone();
let _h = model.item_change_signal().observe(move |change| {
    if let ItemChange::LocalPosChanged { id, new, .. } = *change {
        // Reading the scene from an observer is allowed …
        if let Some(r) = writer.scene_rect(id) {
            audit_log(id, r);
        }
        // … and so is writing it back. Guard the write, or you have a cycle.
        let snapped = Point::new((new.x / 25.0).round() * 25.0,
                                 (new.y / 25.0).round() * 25.0);
        if snapped != new {
            writer.set_local_pos(id, snapped);
        }
    }
});
```

`ItemChange` variants: `Added`, `Removed`, `LocalPosChanged`,
`LocalBoundsChanged`, `TransformChanged`, `VisibilityChanged`,
`OpacityChanged`, `FlagsChanged`, `ZChanged`, `LayerChanged`, `ParentChanged`,
`PayloadChanged`, `AppearanceChanged`.

Logical-AT-structure mutations (groups, parents, relations, live regions,
landmarks, rotor categories, magnets) are not item geometry, so they go through
a second channel — `a11y_change_signal`, a monotonic counter. It behaves
identically to the one above, including for the termination policy below: each
bump names the `A11yNode` it was about — a magnet's names its owning item — and
is charged to that node. Nothing caps a single node's share, and nothing caps
the channel's: "keep AT structure in step per node" is the intended use, not a
runaway, and it only has to stay inside the one whole-drain budget both channels
share.

### Deferred fan-out — why an observer may touch the scene

A `SceneModel` mutator holds `borrow_mut()` on the scene for the whole call, and
`Scene::emit_item_change` is reached from *inside* a `&mut self` method, whose
borrow Rust only releases on return. A synchronous fan-out therefore ran every
observer under a live exclusive borrow, and the two things this section
advertises — reading the scene, writing it back — both panicked with
`RefCell already (mutably) borrowed`.

So the scene does what `teksilo-data` does, adapted: **mutate, then notify.**
`ListModel` can scope its borrow and call `notify` after it; `Scene` cannot lift
the notification out of a `&mut self` method, so the notification is **queued in
the scene** and drained by whoever opened the write scope, once that borrow has
dropped. The seven properties worth knowing:

- **Ordering is emission order.** FIFO, one queue shared by both channels — so a
  batch that moves an item and then declares a landmark on it delivers in that
  order, and `Scene::remove`'s documented leaves-then-root sequence survives.
- **An observer's own write queues behind what is already queued.** A drain
  re-entered from an observer returns immediately and lets the outermost one
  pick the new changes up, which is what keeps delivery in emission order rather
  than depth-first. The drain runs until the queue is empty, so an observer's
  writes are never dropped — the price is that an observer whose write never
  converges never settles. The two channels differ here, and the difference
  decides what the fix is. The geometry mutators already suppress a write that
  changes nothing (`set_local_pos` returns without emitting when the position is
  unchanged), so an observer that keeps writing the *same* value settles on its
  own; a geometry cascade that does not settle is one computing a *different*
  value each time — an accumulating offset, a rounding drift, a spring with no
  rest state. The `set_a11y_*` mutators do not self-suppress: they bump on every
  call, so comparing before calling one is a real guard there.
- **A runaway is stopped by a flat cap on total deliveries, not by a cap on
  depth, width, or batch size.** The distinction matters, because the shapes
  look identical from far enough away and only two of them are bugs:

  | shape | what it does to the queue | verdict |
  | --- | --- | --- |
  | caller's own batch — a bulk load, a subtree teardown, a scene-wide a11y re-tag | one round, however many changes | free, at any size |
  | settling cascade — "when a node moves, drag the one chained to it", guarded | one round *per link*, one delivery per link | passes, at any chain length |
  | guarded aggregate over a hub — every incident edge recomputes it | one subject re-notified once per edge that changes it | passes, at any fan-in the budget can pay for |
  | runaway write-back — one subject rewritten to a *new* value on every change | the same subject re-notified for ever | panics, naming that subject |
  | runaway spawner — adds or removes an item on every change | a *new* subject every round, so no subject repeats | panics, with every subject count near 1 |

  So one number trips: **observer-generated deliveries in a single drain**,
  across both channels and every subject. The caller's own batch — everything
  queued before the first observer ran — is not counted; nor are cascade depth,
  fan-in, or the number of distinct subjects a cascade touches.

  Per-subject counts are still kept, and the panic reports the most-charged
  subject with its count — that is what turns "something did not settle" into
  "*this* did not settle", and it is what separates row 4 from row 5 for a
  reader. They are **diagnostics**, not a second trip condition.

  The cap is enforced in **every** build profile. That is deliberately unlike
  `Signal::try_set`'s guard, which can be debug-only because `try_set` recurses
  and an unchecked loop there blows the stack and aborts loudly by itself; this
  drain is an iterative loop, so unchecked it would be a frozen UI thread with
  no diagnostic and no core dump.

- **The cap is flat, and it is a knob.** Two earlier shapes of this budget each
  fixed the previous one's false positive and bought a worse problem:

  - a cap on **rounds** aborted a legitimate 300-link settling chain, because a
    round is what was queued when it began, so an N-link chain costs N rounds;
  - a cap **per subject** moved the limit onto fan-in — a guarded aggregate over
    a hub passed at 4000 incident edges and aborted at 4200 against a flat 4096;
  - **scaling** the per-subject cap with the scene's entry count fixed that and
    destroyed the guard, because the budget it resolved grew with the model.

  A flat total is the only one of the three that bounds a runaway to the **same
  amount of work whatever the scene's size**. Measured in release, changing only
  the budget (`crates/teksilo-scene`, one `set_cascade_budget` call apart):

  | runaway | 10 000 entries | 50 000 entries |
  | --- | --- | --- |
  | unguarded write-back, flat 100 000 | 520 ms | 3.95 s |
  | unguarded write-back, what the scaled budget allowed (640 000 / 3 200 000) | 3.10 s | 131 s |
  | spawner, flat 100 000 | 43 ms, **+100 002 items** | 52 ms, **+100 002 items** |
  | spawner, what the scaled budget allowed | 435 ms, **+640 002 items** | 2.85 s, **+3 200 002 items** |

  Both flat rows do the same *work* at both sizes — the same 100 001 deliveries,
  the same ~100 000 items allocated. The write-back row's wall clock still grows,
  and that is worth knowing: it is the **app's own** write that is O(entries),
  not the drain. `Scene::set_local_pos` re-buckets the moved subtree, which
  rebuilds a parent→children map over every entry, so 100 001 of them cost
  100 001 × O(entries). The same runaway on the logical-AT channel, whose mutator
  does no re-bucketing, aborts in **4.8 ms at both sizes** — that is the drain's
  own cost, and it is flat. The budget bounds how many times an observer's write
  runs; how expensive each one is remains the scene's business.

  ```text
  budget = CascadeBudget::total    # flat, default 100 000 deliveries per drain
  ```

  Every legitimate shape this tier runs clears it by more than an order of
  magnitude: a 4200-edge guarded aggregate is ~4 200 deliveries, and a 2000-link
  chain that re-places eight port magnets and refreshes three AT properties per
  link is ~24 000.

  **What it cannot decide:** "guarded cascade that is genuinely enormous" and
  "unguarded write-back" are the same picture from the queue's side. The drain
  does not claim otherwise — the panic states the bound it enforced, the total
  delivered, and the most-charged subject, then offers a cause as the *likeliest*
  one rather than the proven one.

  **How to raise it**, for a graph that genuinely settles past the default —
  mechanism in the framework, policy in the consumer:

  ```rust
  model.set_cascade_budget(CascadeBudget::new(1_000_000));
  ```

  Raising it to silence a cycle that does *not* settle only postpones the freeze
  it exists to prevent — the drain is still unbounded in time, just later. Check
  the write is guarded first. A budget of zero is clamped to the smallest usable
  one at the setter rather than stored, so a panic's numbers always describe a
  cascade that happened.

- **A panicking observer costs one delivery, not the rest of the batch.** Each
  notification leaves the queue immediately before it is handed to the signal and
  is never parked in a local buffer, so whatever the panic did not reach is still
  queued, in order, and the next `flush_changes()` delivers it. (Anything else
  would let the scene advance with changes nothing can ever report — the
  corruption the termination policy above exists to avoid.)
- **No coalescing.** Two moves of one item are two notifications. Persistence
  and audit observers need every event, and merging two changes would mean
  merging one's `old` with the other's `new` — a transaction semantic this crate
  does not define.
- **A batch, not a change, is the unit.** `SceneModel::write_guard()` holds the
  write scope open across several edits, which then fan out together:

  ```rust
  {
      let mut scene = model.write_guard();
      scene.set_local_pos(card, Point::new(120.0, 80.0));
      scene.set_z(card, 10.0);
  } // both changes fan out here, borrow already released
  ```

Three doors are deliberately **not** covered, and each will still trap an
observer that re-enters the model:

- A bare `&mut Scene` fans out synchronously, because nothing is holding a
  `RefCell` open for it to escape from.

  For a `Scene` you own outright that is safe: no second handle to it can exist.
  It is **not** safe for a `&mut Scene` reborrowed out of a `SceneModel` —
  `SceneView::scene_mut()` today hands out a raw `RefMut` from the shared cell,
  which leaves the write scope closed, so observers still run under the live
  exclusive borrow and one that re-enters the model panics exactly as every
  mutator used to. **With any observer installed, edit through
  `model.write_guard()` rather than `view.scene_mut()`.** The guard derefs to
  `&mut Scene`, so the call sites read the same.
- The four constraint signals (`pan_axes`, `zoomable`, `pan_bounds`,
  `zoom_range`) carry scene *state*, read back by `current_pan_axes` and
  friends; queueing their writes would make the scene contradict itself inside a
  write scope, so they stay synchronous.
- `SceneModel::with_handlers_mut` runs the caller's closure inside the borrow,
  because it hands out a `&mut` into the scene. It is unwind-safe but not
  re-entrant.

If a panic unwinds out of an open write scope, the guard releases the borrow and
**skips** the fan-out — running observers from a `Drop` during an unwind would
abort the process on the first one that panicked. The abandoned batch stays
queued; a `catch_unwind` recovery path that intends to keep using the scene
calls `SceneModel::flush_changes()` once. A scene dropped with a batch still
queued discards it.

`flush_changes()` is that recovery door, so it never panics, from any call site.
It is a no-op — and says so rather than delivering half a batch — when nothing is
queued, when a drain is already running (a call from inside an observer collapses
into the outer drain), or when **any** borrow on the scene is outstanding. That
last case covers both a write scope, which owns its batch and fans out when it
closes, and a live *shared* borrow such as a `SceneView::focus_order` callback or
a magnetism predicate: an observer is invited to write, and a write needs the
cell exclusively, so draining under a read-only closure would hand out that
invitation and then panic on it.

Constraining a move *before* it is applied — snap-to-grid on the drag ghost,
axis lock, bounds clamping — is a different problem, and an observer is the
wrong tool for it even now: it runs after the write, so it produces a second
`LocalPosChanged` for one gesture and cannot touch the mid-drag ghost at all.
That needs a pre-mutation hook and does not exist yet.

---

## Collision API

```rust
scene.item_at(scene_pt) -> Option<ItemId>          // topmost-z hit
scene.items_at(scene_pt) -> Vec<ItemId>             // all hits, sorted by z
scene.items_in_rect(scene_rect) -> Vec<ItemId>
scene.colliding_items(id) -> Vec<ItemId>            // items whose AABB intersects id's
scene.items_along_path(&path) -> Vec<ItemId>        // items under a connector polyline
```

Backed by the spatial index (default `GridHashIndex`). All query cost
is O(visible × chain-depth), independent of total scene size.

`GridHashIndex` buckets an item into every grid cell its AABB overlaps.
Cell count grows as `(width / cell_size) * (height / cell_size)`, and
`cell_size` clamps to a 1.0 minimum, so nothing bounds it on its own — a
single `Scene::add_item` with a full-document backdrop or canvas rect at a
small `cell_size` can ask for billions of cells. An item whose AABB would
span more than `MAX_CELLS_PER_ITEM` (1024) cells is therefore **not**
bucketed cell-by-cell at all; it is kept in a separate always-scanned
`oversized` set and checked against every query with an exact AABB
intersection test instead. At the default 256 px `cell_size` that threshold
is an ~8192 px square item; at the clamped-minimum `cell_size` of 1.0 it's
~32 px. The query rect itself gets the same treatment — `query` /
`items_in_rect` take an arbitrary caller rect, so a "select everything"
query over a huge area hits the identical hazard on the query side; when
the query rect's own span exceeds the cap, the index scans the populated
cell map directly instead of enumerating the rect's cells, bounded by how
many cells are actually occupied rather than by the rect's area. Both
paths preserve `GridHashIndex::query`'s broad-phase invariant: it may
over-report (a cell-granularity false positive) but must never
under-report — miss an item whose bounds genuinely intersect the query
rect. `Scene::items_in_rect` (and the other collision queries above)
narrow-phase every candidate through their own exact AABB check, so the
over-report never reaches the app; it matters only to a caller that
queries `GridHashIndex` directly.

---

## Magnetism

Magnetism is typed snap-and-connect between anchor points ("magnets") on
scene items. It is general node-graph / diagram machinery: drag an item
so its magnets snap to compatible magnets on other items, drag a wire
from a magnet handle, or connect two magnets from the keyboard, and on
release a connection event carries the magnet payloads to the consumer.

The governing principle is **mechanism in scene, policy in the
consumer**. Scene owns the geometry, the broad-phase, the snap math, the
feedback rendering, the predicate hook, and the connection event. Scene
does **not** own which magnet types are compatible, what a connection
means, or whether connections persist. Compatibility is decided by the
predicate the consumer supplies; the meaning of a connection is decided
by the consumer's `on_connect`.

### The magnet model

A `Magnet` is a local point on an item (in the item's frame, so it
follows the item under any move / rotate / scale), carrying a directional
`MagnetRole` and an optional type-erased payload (`'static`,
downcastable):

```rust
let out = scene.add_magnet(
    node,
    Magnet::new(Point::new(node_w, node_h * 0.5))
        .role(MagnetRole::Source)          // advisory: Source | Target | Bidirectional
        .payload(PortId { node, kind: Out }) // any 'static value
        .label(tr!(node_output())),         // AT name
);
```

`MagnetRole` is generic diagram vocabulary (every node-graph has output
and input ports). It is **advisory** — the scene uses it for default
feedback (which end is the source) and to order the keyboard cycle, but
the predicate is always the authority on whether two magnets connect.

Mutators (all `&self` on `SceneModel`, `&mut self` on `Scene`):
`add_magnet` / `remove_magnet` / `clear_magnets` / `set_magnet_local_pos`
/ `set_magnet_enabled`. Reads: `magnet_ids_of` / `magnet` (a borrow-free
`MagnetRef` snapshot) / `magnet_scene_pos` / `magnet_owner` /
`magnet_enabled`. Removing an item drops its magnets automatically.

### The predicate and the connection event

The predicate is `Fn(&MagnetRef, &MagnetRef) -> MagnetVerdict`, where
`MagnetVerdict` is `Reject` or `Accept(Option<Rc<dyn Any>>)` — "both
payloads in, reject or accept-with-payload out". It runs over owned
magnet snapshots while a shared (read-only) scene borrow is held, so it
may read the model but must not mutate it.

`on_connect` is `Fn(&MagnetConnection, &mut EventContext)`. It fires on
mouse release or keyboard confirm, after every borrow is dropped, so it
may freely mutate the model, declare an AT relation, or fire an intent.

### Three input methods, one mechanism

Install per view via `SceneView::magnetism(MagnetismConfig)`:

```rust
let cfg = MagnetismConfig::new(|a, b| {
        // policy: accept Source -> Target on different items
        if a.item != b.item && /* roles compatible */ true {
            MagnetVerdict::accept()
        } else {
            MagnetVerdict::Reject
        }
    })
    .on_connect(|conn, _ctx| { /* add an edge, fire an intent, … */ })
    .capture_px(14.0)                 // screen-space capture + grab radius
    .markers(MarkerVisibility::DuringInteraction)
    .connect_key(Key::Character('m')); // keyboard connect-mode toggle

let view = SceneView::with_model(model).magnetism(cfg);
```

- **Item-drag-snap** (mouse): drag a lightweight item; its magnets ride
  along and snap to the closest accepting magnet within the capture
  radius; release fires the connection and lands the item snapped.
- **Port-drag wire** (mouse): press directly on a magnet handle to drag a
  transient wire that snaps to a compatible target; release connects, the
  item does not move.
- **Keyboard connect** (any item kind): focus the view, press the connect
  key to enter connect mode, arrow-keys / Home / End move a virtual focus
  through magnets (gated by the predicate once a source is activated),
  Enter activates the source then forms the connection, Esc cancels.

The capture radius is specified in **screen pixels** and divided by the
live zoom, so snapping feels constant at any zoom.

### Feedback

A built-in renderer paints magnet markers (coloured by state, constant
pixel size) plus a connector / ghost wire during an interaction, in the
post-paint pass over the content. Replace it with
`MagnetismConfig::feedback(|canvas, ctx, &MagnetFeedback| …)` for custom
chrome; `MarkerVisibility` (`Always` / `DuringInteraction` / `Never`)
controls when markers show.

### Persistent vs transient — the consumer chooses

Scene stores **no** connection state. It fires the event; the consumer
decides. A node-graph keeps connections as persistent edges (an added
`PathItem` wire, as in the `scene-magnetism` demo); a structural editor
consumes the event once as a reparent and shows containment by nesting.

### Lightweight vs heavyweight

The built-in mouse integration rides the SceneView's lightweight drag /
pointer path (the `RectItem::draggable(true)` substrate), because that is
the only tier the SceneView drags. The keyboard connect flow works for
magnets on any item (it never touches pointer routing). For heavyweight
items (which the SceneView does not drag), originate the drag inside the
item's own widget and call the reusable snap helpers directly:
`SceneModel::compute_item_snap(dragged, drag_delta, capture_radius, &predicate)`
and `compute_port_snap(source, cursor, capture_radius, &predicate)` — the
same mechanism, reachable from any drag origin.

Demo: `cargo run -p scene-magnetism`. Accessibility shaping for magnets
(synthetic nodes + `active_descendant`) is covered in
[docs/teksilo-scene-a11y.md](teksilo-scene-a11y.md).

---

## Background / foreground hooks

Closures injected at the SceneView level for app-supplied chrome.
Both run with the view-transform scope pushed (paint in scene coords)
and receive the visible scene region so geometry off-screen is
trivially cullable:

```rust
SceneView::new(scene)
    .background(|canvas, _ctx, region| {
        // Zoom-aware 50-unit grid, only the visible cells.
        let step = 50.0;
        let mut x = (region.x / step).floor() * step;
        while x < region.x + region.width {
            canvas.draw_line(/* … */);
            x += step;
        }
    })
    .foreground(|canvas, _ctx, region| {
        // Snap-line indicators, ruler chrome, drop-zone hints.
    })
```

Paint order, bottom to top: `background` → **Under** items → heavyweight
children → **Over** items → marquee → `foreground` → debug overlay. The
`background` hook runs in the SceneView's `paint` (a backdrop, before the
heavyweight children); the `foreground` hook runs in its `post_paint` (after the
children), so it paints over the cards. See [Z-order and paint bands](#z-order-and-paint-bands)
for the Under/Over band and the three-pass model.

---

## Cache modes

Items override `cache_mode()` to opt into per-item paint caching:

```rust
impl SceneItem for HeavyDecoration {
    fn cache_mode(&self) -> CacheMode { CacheMode::ItemCoordinate }
    // ...
}
```

`ItemCoordinate` records the item's first paint into a sub-canvas as
a `RenderFrame` in **local** coordinates, replays that frame on
subsequent paints. The cache is invalidated automatically on
`LocalBoundsChanged` / `Removed` via an observer wired in
`SceneView::build()`. Items that mutate visual state without going
through a Scene mutator call `view.invalidate_item_cache(id)`
manually.

Don't use `ItemCoordinate` for items whose `paint` reads external
signal state (e.g. a `TextItem::with_signal_text` — its visual
depends on signal updates that don't dirty the cache). Default is
`CacheMode::None`.

---

## Dynamic bounds (signal-driven)

Most items snapshot `local_bounds` at insert time. For items whose
bounds depend on a `Signal<Rect>` read at paint time:

```rust
scene.add_item_dynamic(MyDynItem { ... }, Point::ZERO);
// SceneView::build() calls scene.refresh_dynamic_bounds() each
// rebuild — the spatial index re-buckets on change.
```

---

## Selection

```rust
SceneView::new(scene).selection_mode(SceneSelectionMode::Multi)
// Single | Multi | None
```

Click-to-select (with Ctrl/Shift modifiers for extend / toggle),
marquee box-select. The selection state is a `Signal<HashSet<ItemId>>`
exposed via [`SceneSelection`](../crates/teksilo-scene/src/selection.rs).

---

## Z-order and paint bands

A `SceneView` paints in three passes — the per-widget `paint → children →
post_paint` model applied at the scene level:

| Pass | What paints | Tier |
| --- | --- | --- |
| `paint` (backdrop) | lightweight **Under** items, z-sorted | lightweight |
| arena child-walk | heavyweight widgets, z-sorted | heavyweight |
| `post_paint` (foreground) | lightweight **Over** items, then the selection marquee / app foreground hook / debug overlays | lightweight |

So the stacking order, bottom to top, is **Under items → heavyweight cards →
Over items**.

### Within a tier

```rust
scene.set_z(id, 5.0);          // higher z paints later (on top)
scene.z(id) -> Option<f32>;
scene.bring_to_front(id);      // z = current max + 1
scene.send_to_back(id);        // z = current min − 1
```

`set_z` works for **both** tiers. Lightweight items re-sort within their band on
the next paint. Heavyweight widget entries restack the arena children on the next
rebuild — the SceneView reorders `node.children` by z *without recreating the
widgets*, so a dragged card keeps its focus, text-edit cursor and in-flight
animations across the restack. Equal-`z` falls back to insertion order (stable).

`bring_to_front` is the drag-to-front primitive: call it on drag-start (via
[`SceneView::scene_mut`]) so the grabbed card — and its text — render over the
others.

### Across the tiers — the Over band

```rust
scene.set_layer(id, SceneLayer::Over);   // raise a lightweight item above the cards
scene.layer(id) -> Option<SceneLayer>;   // Under (default) | Over
```

Lightweight items default to `Under` (background furniture: connector lines,
grids, decorations). `Over` raises an item into the foreground pass so it paints
*above* the heavyweight widgets — selection halos, highlighted connectors,
annotations. Within each band `z` still orders items among themselves.

This is a **binary band, not a continuous z across the tiers**, because the
render walker offers exactly two lightweight paint positions (before and after
the child subtree). The heavyweight tier is one contiguous block in between. To
place a lightweight item *between* two specific cards, promote it to a
heavyweight widget and give it a z between theirs.

### Nested P-C-AP — a node is one widget

The `paint → children → post_paint` model is per-widget and *nests*. A scene's
bands are for **furniture** (connectors, the nodes-as-units, the lasso); each
**node** is itself a P-C-AP scope — its `paint` draws the container, its children
are the text. **Keep a node whole: build it as one heavyweight widget; never
split its container into the lightweight tier and its text into the heavyweight
tier.** The render walker paints each heavyweight child's entire subtree
atomically, so a node ordered last paints its container *and* its text on top of
the node beneath — drag-to-front "with text included" is structural, not
something you arrange. Splitting a node across tiers tears it: every container
would sit in one band and every text in the band above, so a raised card's
neighbour would have its text leak on top of it.

### Hit-testing irregular nodes

Z-order is paint-only; it does not change hit-test priority between tiers
(heavyweight widgets win heavyweight-vs-lightweight collisions). For a node whose
visible shape isn't its bounding box — an ellipse, a cloud — override
`Widget::hit_shape` so a click lands on the silhouette you see, not the
rectangle. Returning `false` for an in-bounds point makes the click fall through
to whatever node is painted underneath; this mirrors the lightweight tier's
`SceneItem::shape_contains`.

---

## Removal

```rust
scene.remove(id);     // recursive — id + all descendants
scene.orphan(id);     // promote children to root, leave them alive
```

Recursive remove is the Qt `removeItem` convention: deleting a parent
deletes its children. Apps wanting to drop the parent without losing
the children call `orphan(id)` first (which detaches children and
re-buckets them in the spatial index), then `remove(id)`.

`remove` also cleans the logical-AT maps for the removed item(s) — parents,
relations, live, landmarks, categories — and re-roots any still-alive node
that was AT-parented under a removed item, so the separate AccessKit tree
never carries a dangling reference. See *Runtime mutation* below.

---

## Shared model & multi-view

A `Scene` lives behind a cloneable [`SceneModel`] handle — `Rc<RefCell<Scene>>`,
the same share-by-handle pattern as `teksilo-data`'s `ListModel`. Clone the
handle into several `SceneView::with_model(model.clone())` panes to render **one
scene many ways**: an overview + a detail pane, the same document in two
windows, or a headless model a tool mutates with no view at all. Mutate the
model once and **every** attached view reconciles.

```rust
let model = SceneModel::new();
let id = model.add_widget_item(CardData { /* … */ }, rect);   // a typed payload

let editor = SceneView::with_model(model.clone())
    .delegate_typed::<CardData>(|card, id| build_card(card, id));
let overview = SceneView::with_model(model.clone())   // same model, own camera
    .delegate_typed::<CardData>(|card, id| build_card(card, id));

// Later, from any handler holding a clone — no `with_widget_mut`:
model.set_payload(id, CardData { /* … */ });   // both panes rebuild that card
```

### Heavyweight content: payload + per-view delegate

A heavyweight `Widget` instance lives in exactly one arena, so a shared model
can't hand the *same* `Box<dyn Widget>` to two views. Two ways to add one:

- **Single-view** — `model.add_widget(widget, rect)` (or `Scene::add_widget`)
  stores the instance in a one-shot slot, drained by the **first** view that
  builds. A second view sharing the model produces no child for it. Use it when
  the scene has exactly one view.
- **Multi-view** — `model.add_widget_item(payload, rect)` stores a type-erased
  `payload` (any `'static` type). Each view supplies a delegate —
  `.delegate_typed::<P>(|&P, ItemId| -> Box<dyn Widget>)` (downcasts;
  debug-asserts on a type mismatch) or the untyped
  `.delegate(|&dyn Any, ItemId| -> Box<dyn Widget>)` — and builds its **own**
  instance per item. `model.set_payload(id, new)` replaces the data and
  re-invokes the delegate for that item in every view (so a card with transient
  widget state — caret, focus — should bind a `Signal` for those fields rather
  than rely on the rebuild).

Lightweight `SceneItem`s (`add_item`) are shared automatically — painted
read-only from each view's paint walk, so no per-view instance is needed.

### Selection across panes

Selection is **per-view by default**. To sync panes, build a `SceneSelection`
and pass a clone to each view via `.selection_model(sel.clone())`; capture the
same `sel.selection_signal()` in your delegate so each card derives its
highlight reactively — selecting in one pane repaints the border in every pane,
with no rebuild. (`SceneSelection` is itself a cheap-clone shared handle.)

### Single-view ergonomics

`SceneView::new(scene)` still takes a `Scene` by value (it wraps a fresh
`SceneModel` internally); `view.scene()` / `view.scene_mut()` return borrow
guards for ad-hoc single-view access; `view.model()` hands out the shared handle.

---

## `SceneListAdapter` — sync items from a `ListModel`

[`SceneListAdapter<T>`](../crates/teksilo-scene/src/scene_list_adapter.rs)
keeps a run of lightweight `SceneItem`s in lock-step with a
`teksilo_data::ListModel<T>` (or any `ListDataSource<Item = T>`), so a
data-driven collection of dots / markers / cards doesn't need hand-rolled
reconciliation against `DataChange`. It is a plain non-`Widget` struct —
construct it once, hold onto it, and it does the rest via an internal
`ObserverHandle`.

```rust
use teksilo_scene::SceneListAdapter;

let model = SceneModel::new();
let adapter = SceneListAdapter::from_model(&list_model, model.clone(), |row, _index| {
    Box::new(RectItem::new(Rect::new(0.0, 0.0, 12.0, 12.0)).fill(row.color)) as Box<dyn SceneItem>
});

let id = adapter.item_id_at(0);   // scene ItemId for row 0, if materialised
```

The delegate is `Fn(&T, usize) -> Box<dyn SceneItem>`; the returned item
positions itself via its own `local_bounds` — the adapter inserts it at the
scene origin through
[`Scene::add_boxed_item`](../crates/teksilo-scene/src/scene.rs) (the
boxed-`dyn` counterpart of `add_item`, needed because a trait-object item
can't go through the generic `add_item<I: SceneItem>`). On construction the
adapter materialises every current row; afterwards it reconciles from the
model's `DataChange` stream: a structural change (insert / remove / move /
reset) rebuilds every adapter-owned item (simple and always correct), an
`ItemUpdated` rebuilds just that one row's item in place, and a lazy-loading
source's `WindowLoaded` rebuilds only the newly-loaded range. `item_id_at`,
`ids`, `len`, `is_empty` read the current index → `ItemId` mapping; `clear`
removes every adapter-owned item from the scene. Dropping the adapter stops
observing the model but does **not** remove its items — call `clear()` first
if you want them gone.

Use `SceneListAdapter::from_source` instead of `from_model` to drive the
same reconciliation off a custom `ListDataSource<Item = T>` (the escape
hatch for huge / external sources) rather than an in-memory `ListModel<T>`.

## Runtime mutation (after mount)

The cleanest way to mutate a mounted scene is through the shared [`SceneModel`]
handle: every mutator is `&self`, so a handler holding `view.model()` (a cheap
clone) drives the scene directly and **all** views reconcile — no
`with_widget_mut` needed for content:

```rust
let model = view.model();             // a clone captured in the handler
let act = model.add_a11y_group(A11yGroup::builder().label(lit!("Act IV")));
model.set_a11y_live(A11yNode::Group(act), Live::Polite);
let card = model.add_widget_item(CardData { /* … */ }, rect);
model.set_a11y_parent(A11yNode::Item(card), Some(A11yNode::Group(act)));
```

`with_widget_mut` remains the channel for **per-view** state a handler can't
otherwise reach — e.g. animating one pane's camera:

```rust
ctx.with_widget_mut::<SceneView>(view_id, BindingLevel::Relayout, |view| {
    view.ensure_visible(rect, 40.0);
});
```

Each view **self-reconciles** on every scene mutation — visual *and*
accessibility:

- **Add** (`add_widget_item` / `add_widget` / `add_item`) materialises into the
  arena on the next rebuild; the spatial index already holds it from insertion.
- **Payload change** (`set_payload`) re-invokes the delegate for that item in
  every view, rebuilding its widget with the new data.
- **Remove** (`remove`) destroys the orphaned arena widget (no leak), drops it
  from the materialised maps, and cleans the logical-AT maps.
- **Move / transform / reparent / visibility / opacity / z / layer** — every
  `ItemChange` variant drives a reconcile pass, so paint *and* the
  screen-projected AccessKit bounds follow.
- **Pure-a11y mutations** (`add_a11y_group`, `set_a11y_parent`, relations,
  live, landmark, categories) don't change item geometry, so they ride a
  *separate* `Scene::a11y_change_signal` — the AccessKit tree still re-walks.

A relayout no longer re-walks the AccessKit tree on its own (it's gated on
`a11y_dirty`), so `SceneView::build()` calls `ctx.request_accessibility_update()`
when it reconciles — the lever that keeps assistive tech in lock-step with the
visual scene. The call is **gated on a mutation-version delta**: `build()`
re-walks AT only when [`Scene::mutation_version`] advanced since the last walk
(any add / remove / move / reparent / visibility / a11y change). A `build()`
driven *purely* by a per-frame `add_item_dynamic` animation does **not** re-walk
AT every frame — re-walking 60×/s for sub-pixel bounds drift is waste a screen
reader can't use — but when that animation **settles**, the final bounds are
walked into AT exactly once. Discrete mutations always re-walk, even interleaved
with an animation. Demo: `cargo run -p scene-corkboard` ("Add Act").

### App-owned view state

Pan / zoom / rotation default to view-owned signals. Inject app-owned ones with
`view_state(pan_x, pan_y, zoom, rotation)` so view state survives a
rebuild-from-state, a "Reset View" button can snap it home, and a toolbar can
read it. `initial_pan` / `initial_zoom` / `initial_rotation` seed starting
values without giving up ownership. These builders run pre-mount, like the
others.

---

## Minimap

[`SceneMinimap`](../crates/teksilo-scene/src/minimap.rs) is a **standalone
sibling** of the view, not something `SceneView` embeds: the app places it
where it wants (typically a bottom-trailing overlay, as in `scene_showcase`)
and feeds it three things.

```rust
let view = SceneView::new(scene);
let minimap = SceneMinimap::new(
        view.scene_content_bounds().unwrap_or(Rect::new(0.0, 0.0, 1000.0, 1000.0)),
        view.viewport_in_scene_signal(),
    )
    .items(view.scene().item_thumbnails())   // both tiers
    .size(200.0, 150.0)
    // The contract is "centre the view on this scene point". `SceneView` has
    // no one-call form, so the app moves the camera it owns — see
    // "It is a control, not a picture" below.
    .on_click(move |scene_pt, _ctx| { /* centre the camera on scene_pt */ });
```

`items` is a **snapshot** — rebuild the widget tree (or wire a
`Signal<Vec<…>>`) when items move. The viewport overlay is reactive on its own:
the minimap binds `viewport_in_scene` at `RepaintOnly`, so pan / zoom re-render
it with no plumbing.

### `content_bounds` is a floor, not a frame

The rect actually projected onto the drawing area is the **effective extent**:

```text
effective_extent = content_bounds ∪ every item rect ∪ viewport_in_scene
```

This is not a refinement — it is what keeps the minimap inside its own frame.
The inputs are unrelated: `content_bounds` is usually the union of item rects
(`scene_content_bounds`), while `viewport_in_scene` is the viewport through the
*inverse view transform*, and **pan is unbounded by default**
(`Scene::current_pan_bounds()` is `None`). Zoom in and pan away, or zoom out
past the content, and the viewport sits wholly outside `content_bounds`. A
minimap that mapped `content_bounds` alone would draw the indicator outside
itself, over whatever sibling widget is there.

Expanding the extent — tldraw's `Box.Expand(contentBounds, viewportBounds)` —
is chosen over the two alternatives because it is the only one that stays
*useful*: a **clamped** indicator lies about position and size and freezes
while the user is still panning; a **cropped** one disappears exactly when the
user most needs to know where they are. With expansion, position and size stay
truthful and the content visibly shrinks as you wander off — which *is* the
"you are out here, the content is over there" signal.

Consequences worth knowing:

- Pass a **larger** `content_bounds` than the item union to keep the scale
  steady while the user pans inside it.
- `content_outline(Some(..))` outlines `content_bounds` *through the same
  projection*, so it shrinks and offsets away from the frame as the extent
  grows. That is the cue, not a bug.
- The fit is **uniform** — one scale for both axes, centred — so a square item
  reads as a square. Expect letterbox margins when the widget's aspect ratio
  differs from the extent's.
- `on_click` inverts the **exact projection the last paint used** — `paint`
  stores it, the tap handler reads it back — so click-to-recentre lands on what
  the user is looking at even while the viewport is off the content, or the
  parent handed the minimap less room than it asked for. The extent formula
  lives in one place (`effective_extent`) and the tap does not re-derive it;
  two copies of it is exactly how a paint-vs-click desync gets in.

### It clips its own paint — and `clips_children` cannot

`SceneMinimap::paint` wraps **everything it emits** — background, content
outline, thumbnails, viewport indicator *and the widget's own border* — in one
`Canvas::set_clip` / `clear_clip`. Nothing it draws leaves its frame, at any
caller-supplied stroke width. That is a structural backstop under the extent
policy above: the policy is something a later edit — or an app that expects a
hand-picked `content_bounds` honoured verbatim — could regress; the clip is a
guarantee.

The clip is set **after** the canvas is translated to `bounds.origin`, and is
given the widget-*local* area, so it lands on the widget wherever the parent
placed it. At the root of a tree that translation is the identity and the
ordering is unobservable — which is why `minimap_clip.rs` runs the load-bearing
cases inside a `Padding` as well.

It is also the **only** mechanism available. `Widget::clips_children` does not
clip a widget's *own* `paint()`: the render walker emits a plain clipping node's
`SetClip` **after** that node's paint
([`rendering_impl.rs`](../crates/teksilo-core/src/widget_tree/rendering_impl.rs)),
so for a self-painting leaf with no children it is a complete no-op. Any scene
widget that paints unbounded geometry has to reach for `set_clip` by hand, as
`ListView`, `TableView`, `CodeEditor` and `Terminal` already do.

`SceneView` is the exception that proves the rule: its own paint *is* clipped,
because it is a `clips_children` **plus** content-transform node, the one
combination for which the walker emits the clip before the node's paint.

### The border is drawn inside the frame

`Canvas::stroke_rect` centres each edge line on the rect boundary. Stroking the
widget area directly would therefore hang half the width *outside* the widget —
12 px on every side for `border(Some((c, 24.0)))`, over whatever sibling is
underneath, from a public builder with no clamp and no warning. So the border is
stroked on the area **inset by half its width**: the band lands wholly inside,
the CSS `border-box` convention. `width` is consumed from the picture, never
from the neighbours, and it is honoured as asked rather than clamped (clamping
would silently render something other than the request, and still leave the last
half-pixel straddling). The clip then bounds even a width wider than the widget.

### It is a control, not a picture

The minimap answers two questions — *where am I* and *take me somewhere else* —
and both are on its AccessKit node. `Role::Group`, named `"Scene minimap"`, with
the position as its **value**:

> Viewport at 42% across, 17% down; showing 25% of the width and 33% of the height

A 2-D position has no ARIA role (there is no `slider2d`) and `numeric_value`
holds one number where this reading has four, so the value is text — the same
answer `HsvCanvas` reaches for the same reason. The value is recomputed on every
AT walk from the same `effective_extent` the picture is projected through, so
the words are the picture; the viewport signal is bound at `AccessibilityOnly`
as well as `RepaintOnly` so a pan refreshes both with no rebuild.
`access_readout(|MinimapReadout| …)` replaces the phrasing — that is the seam
for `tr!`, and for saying something the widget cannot know (page numbers, map
coordinates).

Installing `on_click` is what turns the read-out into a control. It then takes
focus and offers three routes into that one callback:

| | pointer | keyboard | assistive technology |
| --- | --- | --- | --- |
| move the view | tap a point | arrows; `Shift` = a whole viewport | `ScrollLeft` / `Right` / `Up` / `Down` |
| centre on the content | — | `Home`, `Enter`, `Space` | `Click` |

`Action::Click` is the position-free half of the tap: a click needs a point and
an AT client has none, so the primary action is the one destination the widget
can name by itself. Arrow steps are a fraction of the **viewport** (a tenth, or
a whole one with `Shift`) — a scroll view's line-and-page pair, scaled to what
the user is looking at instead of to a count of scene units that would be a
screenful at one zoom and a hair at another. They do not mirror under RTL,
because the picture does not either.

`on_click`'s contract is specifically *centre the view on this scene point*,
because the keyboard and AT routes compute their destination from the current
viewport and then say where the move left it — and both of those are only true
if the app centres. `SceneView` has no single call for that, so the app drives
the camera it already owns: either its `pan_x_signal` / `pan_y_signal` (or the
app-owned pair handed to `view_state`), or, when the view is wrapped in a
`SceneScrollView`, that wrapper's `scroll_pos_x_signal` / `scroll_pos_y_signal`
— the same pan through the door the scroll bars use, already expressed against
the scrollable extent so a target can be clamped to it. `scene_showcase` wires
the second, and its tests tap the minimap and watch the view move.

### Who announces, and who stays quiet

The keyboard and assistive-technology routes announce where they left the
viewport, through the same phrasing the value uses. **The pointer route does
not.**

An arrow press on an unannotated graphic tells a screen-reader user nothing at
all, and an AT client invoking `Click` was never told where "the content" is —
for those two the utterance is the entire feedback. A tap already has some: the
user picked the destination by aiming at the picture, and clicking a minimap is
a gesture people repeat, so one utterance per click is a metronome over
whatever was being read.

That is the rule the workspace already follows, from both directions.
`HsvCanvas` — the other 2-D manipulator — announces from its arrows and its
custom actions and not from its drag or its tap. The five data views' row
reorder announces from `common::ordered_move`, which is the *non-drag*
alternative; the drop itself is silent. The one pointer route that does speak
is the charts' readout, and only for a **coarse** pointer that pressed and
released without travelling — because that tap is an *inspection* standing in
for a hover a finger cannot perform, so the utterance is its whole product, and
a scrub is deliberately coalesced into nothing. A minimap tap is a *command*
whose destination the user chose, so the exception does not reach it.

Nothing is lost by the silence: the node's value is the same sentence, so a
client that re-reads the control after a click gets the new position — it is
simply not interrupted with it.

What *is* announced describes the move that was **asked for** rather than one
read back afterwards: the app owns the pan and may not have applied it yet
(`with_widget_mut` lands after the handler returns, an animated pan later
still), so re-reading the signal would announce the position just left.

### Why a read-out is not in the Tab order

Without `on_click` the node is still emitted and still says where the viewport
is — but it takes no focus and advertises no action.

Worth arguing rather than assuming, because a minimap *displays* something and
a display is worth reaching. The answer is that reaching it and focusing it are
separate questions: the node is in the accessibility tree with a name and a
value, so object / browse navigation and the rotor all arrive at it and read
the position out. What the gate withholds is the **Tab** order, and Tab is for
things you can operate. A read-only minimap answers Tab with nothing — no arrow
does anything, `Enter` does nothing, no action is advertised, and the focus ring
has parked on a picture. That is the dead stop the ARIA practices warn about,
and it costs every keyboard user a press on the way past.

An app that disagrees for its own layout is not blocked: `.focusable(true)` from
the framework's ordinary `WidgetBuilder` chain puts any widget in the Tab order,
this one included. The default is the answer that is right without knowing the
app. Renaming likewise goes through that chain (`.access_label(tr!(…))`), not
through a second set of builders here.

---

## i18n

User-visible strings on `SceneItem` builders (`label`, `tooltip`,
`access_label`, `access_description`, `A11yGroupBuilder::label`,
`SceneView::a11y_label`, `TextItem::new`) accept `impl Into<LocalizedString>`.
Pass the result of `tr!(...)` directly:

```rust
RectItem::new(rect).access_label(tr!(save_card()))
```

Each translated method has an `_literal` `#[doc(hidden)]` twin (e.g.
`access_label_literal`, `tooltip_literal`, `TextItem::new_literal`)
that takes `impl Into<String>`. Use the twin for engine-internal
debug copy or scaffolding where translation is overkill — they're a
grep marker for "intentionally untranslated."

---

## Worked example: corkboard

```rust
let mut scene = Scene::new();
scene.set_scene_rect(Some(Rect::new(0.0, 0.0, 4000.0, 3000.0)));

// Background grid as decoration — lightweight closure, no items.
let view = SceneView::new(scene)
    .selection_mode(SceneSelectionMode::Multi)
    .background(|canvas, _ctx, region| draw_grid(canvas, region, 50.0));

// Add cards as heavyweight widgets.
let card1 = view.scene_mut().add_widget(card("Idea 1"), Rect::new(0.0, 0.0, 200.0, 120.0));
let card2 = view.scene_mut().add_widget(card("Idea 2"), Rect::new(300.0, 200.0, 200.0, 120.0));

// Connector line as a lightweight item beneath the cards.
let path = Path::new()
    .move_to(Point::new(200.0, 60.0))
    .line_to(Point::new(300.0, 260.0));
view.scene_mut().add_item(
    PathItem::new(path, Rect::new(200.0, 60.0, 100.0, 200.0))
        .stroke(Color::BLACK, 2.0),
    Point::ZERO,
);
```

These `scene_mut()` calls run **pre-mount** — the app still owns `view`. To
mutate the same scene from a handler *after* the view is added to the tree, go
through `ctx.with_widget_mut::<SceneView>(view_id, …)` (see *Runtime mutation*
above); the live `scene-corkboard` example does exactly that for its "Add Act"
button.

`scene_mut()` hands out a raw `RefMut` into the shared cell, so it does **not**
open a write scope: its changes fan out synchronously, under the live exclusive
borrow, and an observer that re-enters the model from one of them panics. With
any observer installed, take `model.write_guard()` instead — it derefs to
`&mut Scene`, so every line above reads identically, and it batches. See
*Deferred fan-out* above.

---

## Worked example: simple node-graph editor

```rust
// Each node is a draggable RectItem with a child TextItem label.
let model = SceneModel::new();
let node = model.add_item(
    RectItem::new(Rect::new(0.0, 0.0, 120.0, 60.0))
        .fill(Color::WHITE).stroke(Color::BLACK, 1.0)
        .draggable(true),
    Point::new(100.0, 100.0),
);
let label = model.add_item(
    TextItem::new(tr!(node_name()), Rect::new(8.0, 8.0, 100.0, 24.0)),
    Point::ZERO,
);
model.set_item_parent(label, Some(node));

// Snap onto a 20 dp grid once a drag has landed. The observer writes the
// model back, which is legal (see *Deferred fan-out*) — but the write must be
// guarded, or each correction produces the next and the drain never settles.
// Note this snaps on *commit* only: the drag ghost is not the model, so the
// item slides freely and lands on the grid when released.
let writer = model.clone();
let _h = model.item_change_signal().observe(move |c| {
    if let ItemChange::LocalPosChanged { id, new, .. } = *c {
        let snapped = Point::new((new.x / 20.0).round() * 20.0,
                                 (new.y / 20.0).round() * 20.0);
        if snapped != new {
            writer.set_local_pos(id, snapped);
        }
    }
});

let view = SceneView::with_model(model);
```

With that observer installed, later edits to this scene go through the model
(`model.set_local_pos(..)`) or `model.write_guard()`, never
`view.scene_mut()` — the raw `RefMut` the latter hands out leaves the write scope
closed, so the observer above would run under a live exclusive borrow and panic
on its own write-back. See *Deferred fan-out*.

---

## Reference

- Implementation: [`crates/teksilo-scene/src/`](../crates/teksilo-scene/src/)
- Accessibility-shaping API: [`docs/teksilo-scene-a11y.md`](teksilo-scene-a11y.md)
- Showcase demo: `cargo run -p scene-showcase`
- Corkboard demo: `cargo run -p scene-corkboard`
