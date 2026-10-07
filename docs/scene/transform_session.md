<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TransformSession

The selection **transform controller**: group move, group resize and
group rotate, with a frame and handles drawn over the selection.

## Public types

| Kind | Name |
| ---: | :--- |
| `enum` | [`TransformHandle`](#transformhandle) — One grab affordance on the selection frame |
| `enum` | [`TransformStep`](#transformstep) — One directional nudge on a transform handle |
| `enum` | [`TransformOp`](#transformop) — What one gesture asks of the selection |
| `struct` | [`TransformHandleSet`](#transformhandleset) — A bitset of `TransformHandle`s — Konva's `enabledAnchors`, in the house style of `ItemFlags` |
| `struct` | [`TransformDelta`](#transformdelta) — What one in-flight gesture asks of the selection, stated **once** in scene coordinates |
| `struct` | [`TransformFrame`](#transformframe) — The box the handles are drawn on |
| `enum` | [`LivePreview`](#livepreview) — Whether the items follow the gesture or only the frame does |
| `enum` | [`TransformSource`](#transformsource) — Which route produced a session |
| `enum` | [`TransformOutcome`](#transformoutcome) — How a session finished |
| `struct` | [`TransformSession`](#transformsession) — A snapshot of the live session, handed to the three event hooks and published by… |
| `struct` | [`TransformChrome`](#transformchrome) — Everything the chrome painter is told |
| `struct` | [`TransformLabels`](#transformlabels) — The assistive-technology names the controller publishes |
| `struct` | [`TransformConfig`](#transformconfig) — Per-view transform-controller configuration, installed via `SceneView::transform_controller` |
| `const` | [`DEFAULT_PADDING_PX`](#default_padding_px) — Default frame padding, in screen pixels |
| `const` | [`DEFAULT_HANDLE_PX`](#default_handle_px) — Default handle size, in screen pixels |
| `const` | [`ROTATE_OFFSET_PX`](#rotate_offset_px) — How far above the frame the rotate puck floats, in screen pixels |
| `const` | [`DEFAULT_EDGE_PAN_PX`](#default_edge_pan_px) — Default distance from the viewport edge at which a pointer gesture starts panning the view, in screen pixels |
| `const` | [`DEFAULT_EDGE_PAN_SPEED`](#default_edge_pan_speed) — Default auto-pan speed, in screen pixels per second |

## Public functions

### `TransformSession`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(items: Rc<[ItemId]>, handle: TransformHandle, start_frame: TransformFrame, frame: TransformFrame, delta: TransformDelta, source: TransformSource)`](#transformsession-new) |
| | **Methods** |
| `TransformOp` | [`op()`](#transformsession-op) |

### `TransformHandle`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `TransformOp` | [`op()`](#transformhandle-op) |
| `CursorIcon` | [`cursor()`](#transformhandle-cursor) |
| `bool` | [`is_scalar()`](#transformhandle-is_scalar) |
| `&'static [TransformStep]` | [`at_steps(increment: bool)`](#transformhandle-at_steps) |

### `TransformStep`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Option<Self>` | [`from_index(index: usize)`](#transformstep-from_index) |
| | **Methods** |
| `usize` | [`index()`](#transformstep-index) |
| | **Constants and types** |
| `[TransformStep` | [`ALL`](#transformstep-all) |

### `TransformOp`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `crate::flags::ItemFlags` | [`required_flag()`](#transformop-required_flag) |

### `TransformHandleSet`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`from_bits(bits: u16)`](#transformhandleset-from_bits) |
| | **Builder methods** |
| `Self` | [`with(h: TransformHandle)`](#transformhandleset-with) |
| `Self` | [`without(h: TransformHandle)`](#transformhandleset-without) |
| | **Methods** |
| `bool` | [`contains(h: TransformHandle)`](#transformhandleset-contains) |
| `u16` | [`bits()`](#transformhandleset-bits) |
| | **Constants and types** |
| `Self` | [`NONE`](#transformhandleset-none) |
| `Self` | [`MOVE`](#transformhandleset-move) |
| `Self` | [`CORNERS`](#transformhandleset-corners) |
| `Self` | [`EDGES`](#transformhandleset-edges) |
| `Self` | [`ALL_RESIZE`](#transformhandleset-all_resize) |
| `Self` | [`ROTATE`](#transformhandleset-rotate) |
| `Self` | [`ALL`](#transformhandleset-all) |

### `TransformDelta`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(pivot: Point, basis: f32, scale: Vec2, rotation: f32, translation: Vec2)`](#transformdelta-new) |
| `Self` | [`between(start: &TransformFrame, end: &TransformFrame, pivot: Point)`](#transformdelta-between) |
| | **Methods** |
| `bool` | [`is_identity()`](#transformdelta-is_identity) |
| `Transform2D` | [`to_scene_transform()`](#transformdelta-to_scene_transform) |
| `TransformFrame` | [`frame_after(start: &TransformFrame)`](#transformdelta-frame_after) |
| | **Constants and types** |
| `Self` | [`IDENTITY`](#transformdelta-identity) |

### `TransformFrame`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(rect: Rect, rotation: f32, count: usize)`](#transformframe-new) |
| | **Methods** |
| `Point` | [`to_scene(p: Point)`](#transformframe-to_scene) |
| `Point` | [`from_scene(p: Point)`](#transformframe-from_scene) |
| `Point` | [`centre_scene()`](#transformframe-centre_scene) |
| `Rect` | [`outline(padding: f32)`](#transformframe-outline) |
| `Point` | [`handle_point(handle: TransformHandle, padding: f32, rotate_offset: f32)`](#transformframe-handle_point) |

### `TransformLabels`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new()`](#transformlabels-new) |
| | **Builder methods** |
| `Self` | [`frame(label: impl Into<Prop<String>>)`](#transformlabels-frame) |
| `Self` | [`handle(handle: TransformHandle, label: impl Into<Prop<String>>)`](#transformlabels-handle) |
| `Self` | [`step(step: TransformStep, label: impl Into<Prop<String>>)`](#transformlabels-step) |
| | **Methods** |
| `String` | [`frame_name()`](#transformlabels-frame_name) |
| `String` | [`handle_name(handle: TransformHandle)`](#transformlabels-handle_name) |
| `String` | [`step_name(step: TransformStep)`](#transformlabels-step_name) |

### `TransformConfig`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new()`](#transformconfig-new) |
| | **Builder methods** |
| `Self` | [`handles(set: TransformHandleSet)`](#transformconfig-handles) |
| `Self` | [`keep_ratio(on: impl Into<Prop<bool>>)`](#transformconfig-keep_ratio) |
| `Self` | [`centered_scaling(on: impl Into<Prop<bool>>)`](#transformconfig-centered_scaling) |
| `Self` | [`rotation_snaps(radians: impl IntoIterator<Item = f32>)`](#transformconfig-rotation_snaps) |
| `Self` | [`rotation_snap_tolerance(radians: f32)`](#transformconfig-rotation_snap_tolerance) |
| `Self` | [`padding(screen_px: f32)`](#transformconfig-padding) |
| `Self` | [`handle_px(px: f32)`](#transformconfig-handle_px) |
| `Self` | [`min_size(w: f32, h: f32)`](#transformconfig-min_size) |
| `Self` | [`body_drag(on: bool)`](#transformconfig-body_drag) |
| `Self` | [`live_preview(mode: LivePreview)`](#transformconfig-live_preview) |
| `Self` | [`transform_key(key: Key)`](#transformconfig-transform_key) |
| `Self` | [`edge_pan_px(px: f32)`](#transformconfig-edge_pan_px) |
| `Self` | [`edge_pan_speed(px_per_second: f32)`](#transformconfig-edge_pan_speed) |
| `Self` | [`labels(labels: TransformLabels)`](#transformconfig-labels) |
| `Self` | [`enabled(on: impl Into<Prop<bool>>)`](#transformconfig-enabled) |
| `Self` | [`on_start(f: impl Fn(&TransformSession, &mut EventContext) + 'static)`](#transformconfig-on_start) |
| `Self` | [`on_change(f: impl Fn(&TransformSession, &mut EventContext) + 'static)`](#transformconfig-on_change) |
| `Self` | [`on_end(f: impl Fn(&TransformSession, TransformOutcome, &mut EventContext) + 'static)`](#transformconfig-on_end) |
| `Self` | [`chrome(f: impl Fn(&mut Canvas, &PaintContext, &TransformChrome) + 'static)`](#transformconfig-chrome) |
| | **Methods** |
| `Signal<bool>` | [`enabled_signal()`](#transformconfig-enabled_signal) |
| `bool` | [`is_enabled()`](#transformconfig-is_enabled) |

### `LiveSession`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `ResolvedSession` | [`resolve(cfg: &TransformConfig, view_scale: f32, to_scene: impl Fn(Point) -> Point, constraint: Option<&crate::constrain::ConstraintCall<'_>>)`](#livesession-resolve) |
| `TransformSession` | [`snapshot(resolved: ResolvedSession)`](#livesession-snapshot) |

### `TransformRuntime`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `Option<LiveSession>` | [`abort()`](#transformruntime-abort) |
|  | [`bump()`](#transformruntime-bump) |
|  | [`bump_all()`](#transformruntime-bump_all) |

## Detailed description

### The one design choice everything else follows from

A transform is a **per-view, model-free session**. It reads
`SceneSelection`, it paints a frame, and it
mutates the scene **exactly once** — at the end of the gesture — through a
single `Scene::apply_transform_delta`.
Nothing is written while the pointer moves.

Three things fall out of that and none of them had to be built:

* **Cancel is `session = None`.** There is no rollback, so there is no
  rollback to get wrong — and the crate has already shipped one drag whose
  cancel arm left a stranded translation behind.
* **One gesture is one reversible step.** `apply_transform_delta` is the
  only write, so the transaction boundary is structural rather than a
  convention someone has to remember. (The *history* that makes use of it
  belongs to the data layer, not here — this crate builds the mechanism and
  stops.)
* **A pointer sample costs a relayout, not N model writes.** A ten-item
  selection emits zero `ItemChange`s until the release.

### What a resize does, and why there is only one answer

The **commit** writes `local_bounds`, and the item reflows into the new box.
Not a visual scale that is later baked: a heavyweight card relayouts, a
`RectItem` redraws, a `PathItem` fits
its geometry to the rectangle. A heavyweight entry's `local_bounds` *is* its
layout size, so there is no second route for it, and since `PathItem` grew a
real fit there is no second route for the lightweight tier either.

#### The preview is not that, on the lightweight tier

While the pointer is down nothing is written, so a resize can only be
*shown*, and the two tiers show it differently:

| tier | how the preview is shown | what the user sees |
| --- | --- | --- |
| heavyweight card | the placement rectangle is the previewed one, so the widget lays out at the new size | a live reflow; text re-wraps as the handle moves |
| lightweight item | the preview affine is composed into the item's `local → scene` | a **visual scale**; strokes thicken, glyphs stretch |

Both land on the same `local_bounds` write at the release, and both preview
the same *outline* — the box never jumps — but a wrapped
`TextItem` re-wraps at the instant the handle is let go,
and a stroked item's hairline snaps back to its real width. That is the
honest description of the current mechanism and it is pinned by
`a_lightweight_resize_previews_the_box_it_commits`.

Making the lightweight preview a reflow too needs a channel the paint pass
does not have: an item reflows through `SceneItem::set_local_bounds`, which
takes `&mut self`, and the preview must not write the model — that is what
makes "cancel is `session = None`" true. A previewed-bounds field on
`SceneItemPaintContext` would close it; until then the translation and the
rotation halves of the preview are exact on both tiers and the scale half is
exact only on the heavyweight one.

### What a rotate does, and what it refuses

It post-rotates the item's own `Transform2D` and orbits its `local_pos`
about the pivot. It is refused for a **heavyweight** entry, and the reason
is not that the framework cannot hit-test a rotated widget — it can, a self
transform scope inverse-transforms before testing. It is that `SceneView`
sizes a card from the AABB of its transformed bounds, so a rotation there
inflates the layout box and rotates nothing visible. Giving cards a real
per-child rotation scope is a change to how they are placed and to how their
accessibility rectangles are derived; it is not a line in this module.

### Where the gesture is picked up, and the one honest limitation

A `SceneView` sees a press over a heavyweight child only when that child
does not claim it — a card that calls `capture_pointer` or carries its own
`on_drag` (a `Splitter` handle, a `SpinBox` step button, a `TextInput`'s
selection drag) wins its own press, by design. So **body drag over a card is
best-effort**, and the reliable route is the controller's own chrome: the
frame outline is drawn `padding` **outside** the selection, and both the
frame band and every handle therefore sit on pixels the card does not own.
Grab the frame, not the card.

### The three routes, and what a screen reader can reach

The pointer drags a handle. The keyboard enters a roving mode with
`TransformConfig::transform_key` (`t` by default), `Tab`s between handles
and drives the focused one with the arrows. Assistive technology has neither
a pointer nor arrow keys — it has *verbs* — so each handle publishes the
verbs that suit what it is:

| handle | AccessKit role | value | `Increment` / `Decrement` | custom actions |
| --- | --- | --- | --- | --- |
| an edge midpoint | `Slider` | that edge's coordinate | moves **that** coordinate by one | — |
| the rotate puck | `Slider` | the angle in degrees | turns it by one degree | — |
| a corner | `Button` | none — it is two numbers | moves **both** of its coordinates by one | one per direction |
| the frame band | `Button` | none | moves the whole selection on both axes | one per direction |

One rule covers the whole table: **a verb moves every coordinate the handle
drives, and leaves the rest alone.** See `TransformHandle::at_steps` for
why a corner answers a one-dimensional verb with a diagonal, and
`TransformStep` for the four named actions that give one axis at a time.

Every route ends in the same `LiveSession` and the same commit, so
`docs/a11y/non-drag-alternatives.md`'s "the alternative must make the same
model change the drag makes" is structural rather than asserted. The names
are `TransformLabels`, which takes `tr!`.

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/index.html)

<a id="transformhandle"></a>

## `pub enum TransformHandle`

One grab affordance on the selection frame.

`Move` is the frame band itself (and the body of a selected
item); `Rotate` is the detached puck above the top edge; the
other eight are the resize anchors.

```rust
pub enum TransformHandle { /* variants */ }
```

### Variants

- **`Move`** — The frame band — drag it to move the whole selection.
- **`TopLeading`** — Top-leading corner.
- **`Top`** — Top edge midpoint.
- **`TopTrailing`** — Top-trailing corner.
- **`Leading`** — Leading edge midpoint.
- **`Trailing`** — Trailing edge midpoint.
- **`BottomLeading`** — Bottom-leading corner.
- **`Bottom`** — Bottom edge midpoint.
- **`BottomTrailing`** — Bottom-trailing corner.
- **`Rotate`** — The rotate puck, above the top edge.

### Methods

<a id="transformhandle-op"></a>

#### `pub const fn op(self) -> TransformOp`

Which operation dragging this handle performs.

<a id="transformhandle-cursor"></a>

#### `pub const fn cursor(self) -> CursorIcon`

The pointer cursor for this handle, in an **unrotated** frame. A rotated
frame keeps these — the four diagonal / axis cursors do not have enough
resolution to track an arbitrary angle, and guessing wrong is worse than
a cursor that is merely approximate.

<a id="transformhandle-is_scalar"></a>

#### `pub const fn is_scalar(self) -> bool`

Whether this handle's state is **one** number or two.

An edge midpoint carries the coordinate of the edge it drags; the rotate
puck carries an angle. Both are one number, so both are a `Slider` with
a `numeric_value`, and `Increment` / `Decrement` mean exactly "make that
number bigger / smaller".

A corner carries two coordinates and the frame band carries the whole
selection's position, so neither has a single value to announce. They
are `Button`s, and their one-dimensional verbs move **both** coordinates
— see `at_steps`.

Three things read this and must agree: the published role, whether a
`numeric_value` is set, and what one assistive-technology verb does.

<a id="transformhandle-at_steps"></a>

#### `pub const fn at_steps(self, increment: bool) -> &'static [TransformStep]`

The steps one assistive-technology `Increment` (or `Decrement`) is made
of, as directions the keyboard route already understands.

**One rule covers every handle: a step moves each coordinate the handle
drives by `+1` (`Increment`) or `-1` (`Decrement`), and leaves the rest
alone.**

For a `scalar` handle that is the coordinate its
`numeric_value` announces, which is what makes the slider contract true:
`Increment` on "Resize bottom" raises the bottom edge's `y`, and the
announced number goes up by one. Before this existed every verb was a
horizontal nudge, so `Top` and `Bottom` reported "handled" and moved
nothing at all — the item's height could not be changed by any
assistive-technology route.

For a corner the same rule gives the diagonal: `Increment` on "Resize
top leading" moves that corner's `x` **and** `y` up by one, which is
exactly `Increment` on "Resize top" plus `Increment` on "Resize
leading" — the two edges the corner sits between, together. That is the
only motion a corner can express with a one-dimensional verb, and it
keeps the corner reachable when a configuration offers corners and
nothing else. A user who needs one axis at a time reaches it through the
four named custom actions a non-scalar handle also publishes (see
`TransformStep`); the frame band works the same way.

<a id="transformstep"></a>

## `pub enum TransformStep`

One directional nudge on a transform handle.

The vocabulary the controller's non-pointer routes are stated in: the
keyboard's four arrows, the two halves of an assistive-technology
`Increment` / `Decrement`, and the four **custom actions** a corner or the
frame band publishes so that a screen-reader user can drive one axis at a
time. Named for the frame's own axes, like every other name in this crate —
`Leading` is `-x` and `Trailing` is `+x`, whatever the reading direction.

```rust
pub enum TransformStep { /* variants */ }
```

### Variants

- **`Up`** — Decrease the driven vertical coordinate.
- **`Down`** — Increase it.
- **`Leading`** — Decrease the driven horizontal coordinate.
- **`Trailing`** — Increase it.

### Methods

<a id="transformstep-all"></a>

#### `pub const ALL: [TransformStep;`

Every step, in the order a non-scalar handle publishes its custom
actions. The position in this array **is** the
`accesskit::CustomAction` id, so it is part of the published
contract: reorder it and an assistive technology holding a stale tree
invokes the wrong direction.

<a id="transformstep-index"></a>

#### `pub const fn index(self) -> usize`

This step's index in `ALL`.

<a id="transformstep-from_index"></a>

#### `pub const fn from_index(index: usize) -> Option<Self>`

The step at `index`, or `None` — the inverse of
`index`, used to resolve a custom-action id arriving
from an assistive technology.

<a id="transformop"></a>

## `pub enum TransformOp`

What one gesture asks of the selection.

```rust
pub enum TransformOp { /* variants */ }
```

### Variants

- **`Move`** — Translate the selection.
- **`Resize`** — Change the selection's extent.
- **`Rotate`** — Turn the selection about its centre.

### Methods

<a id="transformop-required_flag"></a>

#### `pub const fn required_flag(self) -> crate::flags::ItemFlags`

The per-item flag an item must carry to take part in this operation.

<a id="transformhandleset"></a>

## `pub struct TransformHandleSet`

A bitset of `TransformHandle`s — Konva's `enabledAnchors`, in the house
style of `ItemFlags`.

```rust
pub struct TransformHandleSet(u16);
```

### Methods

<a id="transformhandleset-none"></a>

#### `pub const NONE: Self = Self(0);`

No handles at all.

<a id="transformhandleset-move"></a>

#### `pub const MOVE: Self = Self(1 << 0);`

The frame band only.

<a id="transformhandleset-corners"></a>

#### `pub const CORNERS: Self = Self((1 << 1) | (1 << 3) | (1 << 6) | (1 << 8));`

The four diagonal corners.

<a id="transformhandleset-edges"></a>

#### `pub const EDGES: Self = Self((1 << 2) | (1 << 4) | (1 << 5) | (1 << 7));`

The four edge midpoints.

<a id="transformhandleset-all_resize"></a>

#### `pub const ALL_RESIZE: Self = Self(Self::CORNERS.0 | Self::EDGES.0);`

Corners and edges.

<a id="transformhandleset-rotate"></a>

#### `pub const ROTATE: Self = Self(1 << 9);`

The rotate puck only.

<a id="transformhandleset-all"></a>

#### `pub const ALL: Self = Self(Self::MOVE.0 | Self::ALL_RESIZE.0 | Self::ROTATE.0);`

Everything — the default.

<a id="transformhandleset-with"></a>

#### `pub const fn with(self, h: TransformHandle) -> Self`

This set plus `h`.

<a id="transformhandleset-without"></a>

#### `pub const fn without(self, h: TransformHandle) -> Self`

This set minus `h`.

<a id="transformhandleset-contains"></a>

#### `pub const fn contains(self, h: TransformHandle) -> bool`

Whether `h` is in the set.

<a id="transformhandleset-bits"></a>

#### `pub const fn bits(self) -> u16`

Raw bits (debug / serialization).

<a id="transformhandleset-from_bits"></a>

#### `pub const fn from_bits(bits: u16) -> Self`

Construct from raw bits.

<a id="transformdelta"></a>

## `pub struct TransformDelta`

What one in-flight gesture asks of the selection, stated **once** in scene
coordinates.

The preview paints it, the commit applies it, the scene's geometry
constraint rewrites it and
the announcement describes it — so those four can never disagree.

The affine it denotes is, innermost first: move the pivot to the origin,
rotate into the frame's basis, scale, rotate back out, rotate by
`rotation`, move the pivot back, translate. The scale is
therefore taken **along the frame's own axes**, which is what makes a
rotated single-item frame resize exactly rather than shear.

`#[non_exhaustive]`: the crate hands this *to* consumer code — a
`ProposedChange` reads it and
`TransformConfig::on_end` records it — and the affine it denotes may grow
a term. Build one with `new`, `IDENTITY` or
`between`; the fields stay public and assignable.

```rust
pub struct TransformDelta { /* fields */ }
```

### Methods

<a id="transformdelta-identity"></a>

#### `pub const IDENTITY: Self = Self { pivot: Point { x: 0.0, y: 0.0 }, basis: 0.0, scale: Vec2 { x: 1.0, y: 1.0 }, rotation: 0.0, translation: Vec2 { x: 0.0, y: 0.0 }, };`

The delta that changes nothing.

<a id="transformdelta-new"></a>

#### `pub fn new(pivot: Point, basis: f32, scale: Vec2, rotation: f32, translation: Vec2) -> Self`

A delta stated field by field — the constructor
[`#[non_exhaustive]`](Self) takes the place of a struct literal for.

The one an app reaches for to apply a programmatic transform through
`Scene::apply_transform_delta`
without driving a gesture. See the type's own documentation for the
order the five terms compose in.

<a id="transformdelta-is_identity"></a>

#### `pub fn is_identity(&self) -> bool`

Whether applying this delta would leave every item where it is.

<a id="transformdelta-to_scene_transform"></a>

#### `pub fn to_scene_transform(&self) -> Transform2D`

The scene-space affine this delta denotes.

<a id="transformdelta-between"></a>

#### `pub fn between(start: &TransformFrame, end: &TransformFrame, pivot: Point) -> Self`

The delta that takes `start` to `end` about `pivot`.

The inverse of `frame_after`, and the reason a
`geometry constraint` can hand back an adjusted
*frame* and have the applied delta follow it exactly.

<a id="transformdelta-frame_after"></a>

#### `pub fn frame_after(&self, start: &TransformFrame) -> TransformFrame`

The frame `start` becomes under this delta.

<a id="transformframe"></a>

## `pub struct TransformFrame`

The box the handles are drawn on.

`rect` is stated in the frame's **own** basis: a point `p` of
it sits at `Rot(rotation) · p` in scene coordinates.

`#[non_exhaustive]`: the crate hands this *to* consumer code and asks for it
back — a `ProposedChange` is handed the proposed
frame and may answer with an adjusted one — so it is both a receive type and
a construct type. Build one with `new`.

```rust
pub struct TransformFrame { /* fields */ }
```

### Methods

<a id="transformframe-new"></a>

#### `pub fn new(rect: Rect, rotation: f32, count: usize) -> Self`

A frame stated field by field — the constructor
[`#[non_exhaustive]`](Self) takes the place of a struct literal for.

A geometry constraint that wants to adjust the frame it was handed
normally derives one from that frame rather than building it here; this
is for the constraint that states its answer outright, and for a
consumer's own tests.

<a id="transformframe-to_scene"></a>

#### `pub fn to_scene(&self, p: Point) -> Point`

Map a point of this frame's basis into scene coordinates.

<a id="transformframe-from_scene"></a>

#### `pub fn from_scene(&self, p: Point) -> Point`

Map a scene point into this frame's basis.

<a id="transformframe-centre_scene"></a>

#### `pub fn centre_scene(&self) -> Point`

The frame's centre, in scene coordinates.

<a id="transformframe-outline"></a>

#### `pub fn outline(&self, padding: f32) -> Rect`

The outline the chrome is drawn on: `rect` grown by
`padding` scene units on every side.

<a id="transformframe-handle_point"></a>

#### `pub fn handle_point(&self, handle: TransformHandle, padding: f32, rotate_offset: f32) -> Point`

Where `handle` sits, in this frame's basis.

`padding` and `rotate_offset` are scene units (a caller converts from
screen pixels by dividing by the live view scale, so the chrome keeps a
constant on-screen size at any zoom).

<a id="livepreview"></a>

## `pub enum LivePreview`

Whether the items follow the gesture or only the frame does.

```rust
pub enum LivePreview { /* variants */ }
```

### Variants

- **`Live`** — Items follow the gesture. One relayout of this view per pointer sample.
- **`Ghost`** — Only the frame moves; the items jump at the commit.

<a id="transformsource"></a>

## `pub enum TransformSource`

Which route produced a session.

```rust
pub enum TransformSource { /* variants */ }
```

### Variants

- **`Pointer`** — A pointer drag on the chrome or on a selected item's body.
- **`Keyboard`** — The keyboard transform mode, or an assistive-technology action.

<a id="transformoutcome"></a>

## `pub enum TransformOutcome`

How a session finished.

```rust
pub enum TransformOutcome { /* variants */ }
```

### Variants

- **`Committed`** — The delta was written to the scene.
- **`Cancelled`** — Nothing was written.

<a id="transformsession"></a>

## `pub struct TransformSession`

A snapshot of the live session, handed to the three event hooks and
published by
`SceneView::transform_session_signal`.

`#[non_exhaustive]`: the crate hands this *to* consumer code — it is the
argument of all three hooks — and a session may learn to carry more. Build
one with `new`, which is what a consumer's own test of its
`on_end` needs.

```rust
pub struct TransformSession { /* fields */ }
```

### Methods

<a id="transformsession-new"></a>

#### `pub fn new( items: Rc<[ItemId]>, handle: TransformHandle, start_frame: TransformFrame, frame: TransformFrame, delta: TransformDelta, source: TransformSource, ) -> Self`

A session snapshot stated field by field — the constructor
[`#[non_exhaustive]`](Self) takes the place of a struct literal for.

The controller builds its own; this exists so a consumer can drive its
`on_start` / `on_change` / `on_end` from a test without a live view.

<a id="transformsession-op"></a>

#### `pub fn op(&self) -> TransformOp`

The operation this session performs.

<a id="transformchrome"></a>

## `pub struct TransformChrome`

Everything the chrome painter is told. Scene coordinates throughout — the
view transform is already on the canvas, exactly as for
`MagnetismConfig::feedback`.

```rust
pub struct TransformChrome { /* fields */ }
```

<a id="transformlabels"></a>

## `pub struct TransformLabels`

The assistive-technology names the controller publishes.

Defaults are the crate's own untranslated English, the same way a magnet's
node is named "Connection point". Every setter takes
`impl Into<Prop<String>>`, so an app hands it `tr!(…)` and the names follow
the locale.

```rust
pub struct TransformLabels { /* fields */ }
```

### Methods

<a id="transformlabels-new"></a>

#### `pub fn new() -> Self`

English defaults.

<a id="transformlabels-frame"></a>

#### `pub fn frame(mut self, label: impl Into<Prop<String>>) -> Self`

The name of the frame node itself.

<a id="transformlabels-handle"></a>

#### `pub fn handle(mut self, handle: TransformHandle, label: impl Into<Prop<String>>) -> Self`

The name of one handle's node.

<a id="transformlabels-frame_name"></a>

#### `pub fn frame_name(&self) -> String`

Resolve the frame's name now.

<a id="transformlabels-step"></a>

#### `pub fn step(mut self, step: TransformStep, label: impl Into<Prop<String>>) -> Self`

The name of one per-axis custom action, published by every handle whose
verb is two-dimensional — the four corners and the frame band. The
handle's own name supplies the subject, so this names only the
direction ("Step up", not "Move the selection up").

<a id="transformlabels-handle_name"></a>

#### `pub fn handle_name(&self, handle: TransformHandle) -> String`

Resolve one handle's name now.

<a id="transformlabels-step_name"></a>

#### `pub fn step_name(&self, step: TransformStep) -> String`

Resolve one step's name now.

<a id="transformconfig"></a>

## `pub struct TransformConfig`

Per-view transform-controller configuration, installed via
`SceneView::transform_controller`.

Built the way `MagnetismConfig` is: `Rc`'d
closures so the view can clone it into every handler, `Prop`-accepting
reactive knobs, one `enabled` signal.

```rust
pub struct TransformConfig { /* fields */ }
```

### Methods

<a id="transformconfig-new"></a>

#### `pub fn new() -> Self`

A controller with every handle, no ratio lock, corner-anchored scaling,
6 px padding, 9 px handles, a 4 × 4 minimum, body drag on, live preview,
`t` for the keyboard route and edge auto-pan on.

<a id="transformconfig-handles"></a>

#### `pub fn handles(mut self, set: TransformHandleSet) -> Self`

Which handles the frame offers. Konva's `enabledAnchors`.

A handle in the set is still hidden when the selection cannot honour
it — see
`SceneView::transform_controller`
for the rule. What is drawn is exactly what will happen.

<a id="transformconfig-keep_ratio"></a>

#### `pub fn keep_ratio(mut self, on: impl Into<Prop<bool>>) -> Self`

Keep the selection's aspect ratio through a resize. Konva's `keepRatio`.

<a id="transformconfig-centered_scaling"></a>

#### `pub fn centered_scaling(mut self, on: impl Into<Prop<bool>>) -> Self`

Scale about the frame's centre rather than the opposite anchor.
Konva's `centeredScaling`.

<a id="transformconfig-rotation_snaps"></a>

#### `pub fn rotation_snaps(mut self, radians: impl IntoIterator<Item = f32>) -> Self`

Absolute angles, in radians, a rotation snaps to when it comes within
`rotation_snap_tolerance`.

<a id="transformconfig-rotation_snap_tolerance"></a>

#### `pub fn rotation_snap_tolerance(mut self, radians: f32) -> Self`

How close a rotation has to get to a snap before it takes it. Default 5°.

<a id="transformconfig-padding"></a>

#### `pub fn padding(mut self, screen_px: f32) -> Self`

How far outside the selection the frame is drawn, in screen pixels.
Konva's `padding`.

This is load-bearing, not decoration: it is what puts the frame band and
the handles on pixels a heavyweight card does not own, and therefore
what makes them grabbable over one. Setting it below
`handle_px / 2` lets a handle overlap a card, and a card that claims its
own press will then take the grab.

<a id="transformconfig-handle_px"></a>

#### `pub fn handle_px(mut self, px: f32) -> Self`

The on-screen size of a handle, in pixels. Default 9.

<a id="transformconfig-min_size"></a>

#### `pub fn min_size(mut self, w: f32, h: f32) -> Self`

The smallest frame a resize will produce, in scene units.

<a id="transformconfig-body_drag"></a>

#### `pub fn body_drag(mut self, on: bool) -> Self`

Whether a press on the body of a selected, movable item starts a group
move. Default on.

Best-effort over the heavyweight tier by construction: a card that
claims its own press — one carrying `capture_pointer` or its own
`on_drag` — wins it, and this view never sees the gesture. The frame
band is the route that always works.

<a id="transformconfig-live_preview"></a>

#### `pub fn live_preview(mut self, mode: LivePreview) -> Self`

Whether the items follow the gesture or only the frame does.

<a id="transformconfig-transform_key"></a>

#### `pub fn transform_key(mut self, key: Key) -> Self`

The key that enters the keyboard transform mode while the view is
focused. Default `t`, mirroring magnetism's `m`.

<a id="transformconfig-edge_pan_px"></a>

#### `pub fn edge_pan_px(mut self, px: f32) -> Self`

How close to the viewport edge a pointer gesture has to get before the
view starts panning, in screen pixels. Zero turns auto-pan off.

<a id="transformconfig-edge_pan_speed"></a>

#### `pub fn edge_pan_speed(mut self, px_per_second: f32) -> Self`

Auto-pan speed, in screen pixels per second.

<a id="transformconfig-labels"></a>

#### `pub fn labels(mut self, labels: TransformLabels) -> Self`

The names the frame and its handles publish to assistive technology.

<a id="transformconfig-enabled"></a>

#### `pub fn enabled(mut self, on: impl Into<Prop<bool>>) -> Self`

Set the enabled state, statically or reactively.

<a id="transformconfig-enabled_signal"></a>

#### `pub fn enabled_signal(&self) -> Signal<bool>`

The reactive enabled signal, for a toolbar to read or bind.

<a id="transformconfig-is_enabled"></a>

#### `pub fn is_enabled(&self) -> bool`

Whether the controller is currently enabled.

<a id="transformconfig-on_start"></a>

#### `pub fn on_start(mut self, f: impl Fn(&TransformSession, &mut EventContext) + 'static) -> Self`

Fired once when a gesture begins.

<a id="transformconfig-on_change"></a>

#### `pub fn on_change(mut self, f: impl Fn(&TransformSession, &mut EventContext) + 'static) -> Self`

Fired on every pointer sample and every keyboard step.

<a id="transformconfig-on_end"></a>

#### `pub fn on_end( mut self, f: impl Fn(&TransformSession, TransformOutcome, &mut EventContext) + 'static, ) -> Self`

Fired once when a gesture finishes, committed or cancelled.

The scene has already been written when the outcome is
`TransformOutcome::Committed`, so this is the hook an app records the
delta from — a reversible-edit layer lives above this crate, never
inside it.

<a id="transformconfig-chrome"></a>

#### `pub fn chrome( mut self, f: impl Fn(&mut Canvas, &PaintContext, &TransformChrome) + 'static, ) -> Self`

Replace the built-in chrome painter. Paints in scene coordinates.

<a id="default_padding_px"></a>

## `pub const DEFAULT_PADDING_PX`

Default frame padding, in screen pixels. Chosen so the handle discs clear
the selection's own bounds: a card never owns the pixels a handle is drawn
on, which is what makes the chrome grabbable over a heavyweight child.

```rust
pub const DEFAULT_PADDING_PX: f32 = 6.0;
```

<a id="default_handle_px"></a>

## `pub const DEFAULT_HANDLE_PX`

Default handle size, in screen pixels.

```rust
pub const DEFAULT_HANDLE_PX: f32 = 9.0;
```

<a id="rotate_offset_px"></a>

## `pub const ROTATE_OFFSET_PX`

How far above the frame the rotate puck floats, in screen pixels.

```rust
pub const ROTATE_OFFSET_PX: f32 = 22.0;
```

<a id="default_edge_pan_px"></a>

## `pub const DEFAULT_EDGE_PAN_PX`

Default distance from the viewport edge at which a pointer gesture starts
panning the view, in screen pixels.

```rust
pub const DEFAULT_EDGE_PAN_PX: f32 = 28.0;
```

<a id="default_edge_pan_speed"></a>

## `pub const DEFAULT_EDGE_PAN_SPEED`

Default auto-pan speed, in screen pixels per second.

```rust
pub const DEFAULT_EDGE_PAN_SPEED: f32 = 700.0;
```

<a id="livesession"></a>

## `impl LiveSession`  *(methods defined in this file)*

### Methods

<a id="livesession-resolve"></a>

#### `pub fn resolve( &self, cfg: &TransformConfig, view_scale: f32, to_scene: impl Fn(Point) -> Point, constraint: Option<&crate::constrain::ConstraintCall<'_>>, ) -> ResolvedSession`

Resolve the gesture into a frame and a delta, applying the config's
constraints and the scene's geometry constraint.

`view_scale` converts the screen-pixel padding into scene units;
`to_scene` projects the live pointer position; `constraint` is the
scene's `ProposedChange` hook, already bundled
with the shared scene borrow it reads (`None` when none is installed,
which is why an unconstrained scene pays one `Option` test here).

**Deterministic and stateless**, which is what keeps the preview and the
commit in agreement: the constraint sees the same proposal on the
release sample as on the one before it, so the frame the chrome last
drew is the frame that gets written. There is no phase to branch on and
deliberately so — a hook that could snap loosely while dragging and hard
on release would be a hook that guarantees a jump at the release.

<a id="livesession-snapshot"></a>

#### `pub fn snapshot(&self, resolved: ResolvedSession) -> TransformSession`

Build the public snapshot from a resolution.

<a id="transformruntime"></a>

## `impl TransformRuntime`  *(methods defined in this file)*

### Methods

<a id="transformruntime-abort"></a>

#### `pub fn abort(&self) -> Option<LiveSession>`

Drop the live gesture and repaint. There is nothing to roll back — that
is the whole point of the model-free session.

Returns the session that was dropped, if any.

<a id="transformruntime-bump"></a>

#### `pub fn bump(&self)`

<a id="transformruntime-bump_all"></a>

#### `pub fn bump_all(&self)`
