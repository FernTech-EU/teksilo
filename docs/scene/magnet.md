<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Magnet

Magnetism: typed snap-and-connect between anchor points on scene items.

## Public types

| Kind | Name |
| ---: | :--- |
| `struct` | [`MagnetId`](#magnetid) — Opaque identifier for a `Magnet` inside a `Scene` |
| `enum` | [`MagnetRole`](#magnetrole) — The direction a magnet faces in a connection |
| `struct` | [`Magnet`](#magnet) — A magnetism anchor attached to a scene item |
| `struct` | [`MagnetRef`](#magnetref) — An owned, borrow-free snapshot of one magnet, handed to the accept/reject predicate and carried in a `MagnetConnection` |
| `enum` | [`MagnetVerdict`](#magnetverdict) — The result of running the accept/reject predicate on a candidate magnet pair |
| `struct` | [`MagnetConnection`](#magnetconnection) — A formed connection between two magnets, delivered to the consumer's `on_connect` handler on release (mouse) or confirm (keyboard) |
| `struct` | [`MagnetSnap`](#magnetsnap) — The chosen snap when a dragged item's magnet aligns onto another item's magnet |
| `enum` | [`MarkerVisibility`](#markervisibility) — When the `SceneView` paints magnet markers |
| `enum` | [`MagnetVisualState`](#magnetvisualstate) — The visual state of a magnet as the feedback renderer sees it |
| `struct` | [`MagnetMarker`](#magnetmarker) — One magnet's render data, handed to the feedback renderer |
| `struct` | [`MagnetFeedback`](#magnetfeedback) — Everything the magnetism feedback renderer needs for one frame, in scene coordinates (the canvas is already in the view-transform scope) |
| `struct` | [`MagnetismConfig`](#magnetismconfig) — Per-view magnetism configuration, installed via `SceneView::magnetism` |

## Public functions

### `Magnet`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(local_pos: Point)`](#magnet-new) |
| | **Builder methods** |
| `Self` | [`role(role: MagnetRole)`](#magnet-role) |
| `Self` | [`payload<P: 'static>(payload: P)`](#magnet-payload) |
| `Self` | [`payload_rc(payload: Rc<dyn Any>)`](#magnet-payload_rc) |
| `Self` | [`label(label: impl Into<LocalizedString>)`](#magnet-label) |
| `Self` | [`enabled(on: bool)`](#magnet-enabled) |

### `MagnetId`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `u64` | [`as_u64()`](#magnetid-as_u64) |

### `MagnetRef`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(id: MagnetId, item: ItemId, role: MagnetRole, payload: Option<Rc<dyn Any>>, scene_pos: Point)`](#magnetref-new) |
| | **Methods** |
| `Option<&P>` | [`payload_as<P: 'static>()`](#magnetref-payload_as) |

### `MagnetVerdict`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`accept()`](#magnetverdict-accept) |
| `Self` | [`accept_with<P: 'static>(payload: P)`](#magnetverdict-accept_with) |
| | **Methods** |
| `bool` | [`is_accept()`](#magnetverdict-is_accept) |

### `MagnetConnection`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(from: MagnetRef, to: MagnetRef, payload: Option<Rc<dyn Any>>)`](#magnetconnection-new) |
| | **Methods** |
| `Option<&P>` | [`payload_as<P: 'static>()`](#magnetconnection-payload_as) |

### `MagnetSnap`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(from: MagnetId, to: MagnetId, snap_vector: Vec2, payload: Option<Rc<dyn Any>>, distance: f32)`](#magnetsnap-new) |

### `MagnetMarker`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(id: MagnetId, scene_pos: Point, role: MagnetRole, state: MagnetVisualState)`](#magnetmarker-new) |

### `MagnetFeedback`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(zoom: f32, markers: Vec<MagnetMarker>, connector: Option<(Point, Point, bool)>)`](#magnetfeedback-new) |

### `MagnetismConfig`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(predicate: impl Fn(&MagnetRef, &MagnetRef) -> MagnetVerdict + 'static)`](#magnetismconfig-new) |
| | **Builder methods** |
| `Self` | [`on_connect(f: impl Fn(&MagnetConnection, &mut EventContext) + 'static)`](#magnetismconfig-on_connect) |
| `Self` | [`capture_px(px: f32)`](#magnetismconfig-capture_px) |
| `Self` | [`markers(markers: MarkerVisibility)`](#magnetismconfig-markers) |
| `Self` | [`feedback(f: impl Fn(&mut Canvas, &PaintContext, &MagnetFeedback) + 'static)`](#magnetismconfig-feedback) |
| `Self` | [`connect_key(key: Key)`](#magnetismconfig-connect_key) |
| `Self` | [`enabled(on: impl Into<Prop<bool>>)`](#magnetismconfig-enabled) |
| | **Methods** |
| `Signal<bool>` | [`enabled_signal()`](#magnetismconfig-enabled_signal) |
| `bool` | [`is_enabled()`](#magnetismconfig-is_enabled) |

## Detailed description

A **magnet** is a local point on an item (relative to the item's
anchor, like a child point), carrying a type-erased payload
(`'static`, downcastable) and a directional `MagnetRole`. An item
can carry several. During an interaction the scene broad-phases
nearby magnets, runs an accept/reject `predicate` per
candidate pair, snaps so the closest accepting pair aligns, and on
release a connection event carries the payloads to the consumer.

### Mechanism in scene, policy in the consumer

This module and `Scene` own the *mechanism*: magnet
geometry, broad-phase, snap math, and the connection result. They do
**not** own *policy* — which magnet types are compatible, what a
connection means, or whether a connection persists. Compatibility is
decided entirely by the predicate the consumer supplies to
`Scene::compute_item_snap` /
`Scene::compute_port_snap`; the
meaning of a formed connection is decided by the consumer's
`on_connect` handler. No widget-tree or designer concept (slot,
category, insertion index) leaks into this API; those live in the
payloads and the predicate.

`MagnetRole` is generic node-graph / diagram vocabulary used by the
scene only for default feedback (which end is the source) and for
ordering the keyboard connect flow. It is advisory: the predicate is
always authoritative on whether two magnets may connect.

#### Example — two items connected by a typed magnet pair

```rust
use teksilo_scene::{Scene, RectItem, Magnet, MagnetRole, MagnetRef, MagnetVerdict};
use teksilo_canvas::{Point, Rect, Vec2};

// A predicate that accepts Source → Target pairs on different items.
fn source_to_target(a: &MagnetRef, b: &MagnetRef) -> MagnetVerdict {
    if a.item == b.item { return MagnetVerdict::Reject; }
    match (a.role, b.role) {
        (MagnetRole::Source, MagnetRole::Target)
        | (MagnetRole::Target, MagnetRole::Source) => MagnetVerdict::accept(),
        _ => MagnetVerdict::Reject,
    }
}

let mut scene = Scene::new();

// Dragged item with a Source magnet at its local origin.
let dragged = scene.add_item(
    RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
    Point::ZERO,
);
scene.add_magnet(dragged, Magnet::new(Point::ZERO).role(MagnetRole::Source));

// Target item 100 px to the right with a Target magnet at its local origin.
let target = scene.add_item(
    RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
    Point::new(100.0, 0.0),
);
scene.add_magnet(target, Magnet::new(Point::ZERO).role(MagnetRole::Target));

// The dragged item is 5 px away from snapping; capture radius 20 px.
if let Some(snap) = scene.compute_item_snap(dragged, Vec2::new(95.0, 0.0), 20.0, &source_to_target) {
    // snap_vector carries the dragged item exactly onto the target magnet.
    assert!((snap.snap_vector.x - 5.0).abs() < 1e-3);
}
```

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/index.html)

<a id="magnetid"></a>

## `pub struct MagnetId`

Opaque identifier for a `Magnet` inside a `Scene`.

Globally unique within a process, minted by `MagnetId::next`. Stable
across the magnet's lifetime; removing a magnet (or its owning item)
retires its id permanently — ids are never reused.

```rust
pub struct MagnetId(pub(crate) u64);
```

### Methods

<a id="magnetid-as_u64"></a>

#### `pub fn as_u64(self) -> u64`

Raw numeric value, used by AccessKit's synthetic-NodeId derivation.

<a id="magnetrole"></a>

## `pub enum MagnetRole`

The direction a magnet faces in a connection.

Advisory only — the scene uses it for default feedback (arrow
direction, which end starts the keyboard flow), but the
accept/reject predicate is always the authority on compatibility. A
node-graph output port is a `Source`, an input
port is a `Target`; a snap point that can be
either end is `Bidirectional`.

```rust
pub enum MagnetRole { /* variants */ }
```

### Variants

- **`Source`** — Originates a connection (e.g. a node-graph output port).
- **`Target`** — Receives a connection (e.g. a node-graph input port).
- **`Bidirectional`** — Can be either end of a connection.

<a id="magnet"></a>

## `pub struct Magnet`

A magnetism anchor attached to a scene item.

Built fluently and handed to
`SceneModel::add_magnet`. Carries a
local-frame position, a `MagnetRole`, an optional type-erased
payload, an enabled flag, and an optional accessibility label.

```rust
pub struct Magnet { /* fields */ }
```

### Methods

<a id="magnet-new"></a>

#### `pub fn new(local_pos: Point) -> Self`

A magnet at `local_pos` in the owning item's local frame, role
`Bidirectional`, no payload, enabled.

<a id="magnet-role"></a>

#### `pub fn role(mut self, role: MagnetRole) -> Self`

Set the connection direction (advisory — see `MagnetRole`).

<a id="magnet-payload"></a>

#### `pub fn payload<P: 'static>(mut self, payload: P) -> Self`

Attach a type-erased payload the predicate and the connection
event can downcast. Cheap to carry around (held in an `Rc`).

<a id="magnet-payload_rc"></a>

#### `pub fn payload_rc(mut self, payload: Rc<dyn Any>) -> Self`

Attach an already-`Rc`-wrapped payload (use when several magnets
share one payload object).

<a id="magnet-label"></a>

#### `pub fn label(mut self, label: impl Into<LocalizedString>) -> Self`

The accessibility name announced for this magnet's synthetic AT
node. Defaults to a generic role-based label when unset.

<a id="magnet-enabled"></a>

#### `pub fn enabled(mut self, on: bool) -> Self`

Disabled magnets are skipped by broad-phase, feedback, the
keyboard cycle, and AT emission. Enabled by default.

<a id="magnetref"></a>

## `pub struct MagnetRef`

An owned, borrow-free snapshot of one magnet, handed to the
accept/reject predicate and carried in a `MagnetConnection`.

The payload is an `Rc` clone, so a snapshot can outlive the borrow
taken to collect candidates. The predicate inspects these snapshots
while a shared (read-only) scene borrow is held — it may read the
model but must not mutate it. The `on_connect` handler, by contrast,
runs after every borrow is dropped and may freely mutate the model
(add an edge item, reparent, fire an intent).

`#[non_exhaustive]`: the crate hands this *to* consumer code — it is what a
predicate inspects — the same reason `MagnetVisualState` carries it. Build
one with `new`.

```rust
pub struct MagnetRef { /* fields */ }
```

### Methods

<a id="magnetref-new"></a>

#### `pub fn new( id: MagnetId, item: ItemId, role: MagnetRole, payload: Option<Rc<dyn Any>>, scene_pos: Point, ) -> Self`

A snapshot stated field by field — the constructor
[`#[non_exhaustive]`](Self) takes the place of a struct literal for.
The scene builds its own; this is for a consumer testing its predicate.

<a id="magnetref-payload_as"></a>

#### `pub fn payload_as<P: 'static>(&self) -> Option<&P>`

Borrow the payload downcast to `P`, or `None` if absent or a
different type. The ergonomic way to read a typed payload inside
a predicate.

<a id="magnetverdict"></a>

## `pub enum MagnetVerdict`

The result of running the accept/reject predicate on a candidate
magnet pair. "Both payloads in, reject or accept-with-payload out."

```rust
pub enum MagnetVerdict { /* variants */ }
```

### Variants

- **`Reject`** — The pair may not connect; the scene skips it.
- **`Accept`** — The pair may connect. The optional payload is attached to the resulting `MagnetConnection` (e.g. a derived edge descriptor).

### Methods

<a id="magnetverdict-accept"></a>

#### `pub fn accept() -> Self`

Accept with no extra connection payload.

<a id="magnetverdict-accept_with"></a>

#### `pub fn accept_with<P: 'static>(payload: P) -> Self`

Accept and attach a typed connection payload.

<a id="magnetverdict-is_accept"></a>

#### `pub fn is_accept(&self) -> bool`

Whether this verdict accepts the pair.

<a id="magnetconnection"></a>

## `pub struct MagnetConnection`

A formed connection between two magnets, delivered to the consumer's
`on_connect` handler on release (mouse) or confirm (keyboard).

`from` is the magnet that initiated the connection (the dragged
item's magnet, the grabbed port, or the keyboard-activated source);
`to` is the magnet it connected onto. `payload` is whatever the
predicate's `MagnetVerdict::Accept` carried.

`#[non_exhaustive]`: the crate hands this *to* consumer code — it is the
argument of `on_connect`. Build one with `new`.

```rust
pub struct MagnetConnection { /* fields */ }
```

### Methods

<a id="magnetconnection-new"></a>

#### `pub fn new(from: MagnetRef, to: MagnetRef, payload: Option<Rc<dyn Any>>) -> Self`

A connection stated field by field — the constructor
[`#[non_exhaustive]`](Self) takes the place of a struct literal for.
The scene builds its own; this is for a consumer testing `on_connect`.

<a id="magnetconnection-payload_as"></a>

#### `pub fn payload_as<P: 'static>(&self) -> Option<&P>`

Borrow the connection payload downcast to `P`.

<a id="magnetsnap"></a>

## `pub struct MagnetSnap`

The chosen snap when a dragged item's magnet aligns onto another
item's magnet. Returned by
`Scene::compute_item_snap`.

A heavyweight consumer that drives its own drag uses `snap_vector` to
place the item so `from` lands on `to`, and resolves `from` / `to`
via `Scene::magnet` to build the connection
for its own `on_connect`.

`#[non_exhaustive]`: the crate hands this *to* consumer code — a
heavyweight consumer driving its own drag reads it. Build one with
`new`.

```rust
pub struct MagnetSnap { /* fields */ }
```

### Methods

<a id="magnetsnap-new"></a>

#### `pub fn new( from: MagnetId, to: MagnetId, snap_vector: Vec2, payload: Option<Rc<dyn Any>>, distance: f32, ) -> Self`

A snap stated field by field — the constructor
[`#[non_exhaustive]`](Self) takes the place of a struct literal for.
The scene computes its own; this is for a consumer's own tests.

<a id="markervisibility"></a>

## `pub enum MarkerVisibility`

When the `SceneView` paints magnet markers.

```rust
pub enum MarkerVisibility { /* variants */ }
```

### Variants

- **`Always`** — Always draw a marker for every enabled magnet (busy, but the clearest discoverability — good for a dedicated editor).
- **`DuringInteraction`** — Draw markers only while an interaction is in progress (an item drag, a port drag, or keyboard connect mode). The default — keeps an idle scene clean.
- **`Never`** — Never draw markers (the consumer paints its own via the feedback hook, or wants no visual at all).

<a id="magnetvisualstate"></a>

## `pub enum MagnetVisualState`

The visual state of a magnet as the feedback renderer sees it.

`#[non_exhaustive]`: the crate hands this *to* consumer code — a custom
`MagnetFeedback` must match it — and the set grows with every interaction
state magnetism learns. A renderer's wildcard arm then keeps meaning "draw
it plainly" instead of failing to compile.

```rust
pub enum MagnetVisualState { /* variants */ }
```

### Variants

- **`Idle`** — A normal, idle magnet.
- **`Candidate`** — A magnet the current interaction could connect to (it passes the predicate against the active source).
- **`Snapped`** — The magnet the active interaction is currently snapped onto.
- **`Focused`** — The keyboard-focused magnet (connect mode).
- **`PendingSource`** — The keyboard-activated source magnet awaiting a target.

<a id="magnetmarker"></a>

## `pub struct MagnetMarker`

One magnet's render data, handed to the feedback renderer.

`#[non_exhaustive]`: the crate hands this *to* consumer code — it is what a
custom `MagnetismConfig::feedback` draws, and it is the struct
`MagnetVisualState` lives inside, which already says so for itself. Build
one with `new`.

```rust
pub struct MagnetMarker { /* fields */ }
```

### Methods

<a id="magnetmarker-new"></a>

#### `pub fn new(id: MagnetId, scene_pos: Point, role: MagnetRole, state: MagnetVisualState) -> Self`

A marker stated field by field — the constructor
[`#[non_exhaustive]`](Self) takes the place of a struct literal for.

<a id="magnetfeedback"></a>

## `pub struct MagnetFeedback`

Everything the magnetism feedback renderer needs for one frame, in
scene coordinates (the canvas is already in the view-transform
scope). The built-in renderer draws markers plus a connector; a
custom `MagnetismConfig::feedback` closure receives the same data.

`#[non_exhaustive]`: the crate hands this *to* consumer code — it is the
argument of a custom feedback renderer, and the set grows with every cue
magnetism learns to draw. Build one with `new`.

```rust
pub struct MagnetFeedback { /* fields */ }
```

### Methods

<a id="magnetfeedback-new"></a>

#### `pub fn new( zoom: f32, markers: Vec<MagnetMarker>, connector: Option<(Point, Point, bool)>, ) -> Self`

A frame of feedback stated field by field — the constructor
[`#[non_exhaustive]`](Self) takes the place of a struct literal for.

<a id="magnetismconfig"></a>

## `pub struct MagnetismConfig`

Per-view magnetism configuration, installed via
`SceneView::magnetism`.

Holds the consumer's *policy* — the accept/reject predicate and the
`on_connect` handler — plus presentation knobs. The scene supplies
the mechanism (snap math, broad-phase, feedback rendering, the
connection event); this config is where the consumer plugs its
policy in.

```rust
pub struct MagnetismConfig { /* fields */ }
```

### Methods

<a id="magnetismconfig-new"></a>

#### `pub fn new(predicate: impl Fn(&MagnetRef, &MagnetRef) -> MagnetVerdict + 'static) -> Self`

A config with the given accept/reject `predicate` and defaults:
14 px capture radius, markers during interaction, the built-in
feedback renderer, `m` to toggle keyboard connect mode, enabled.
Install an `on_connect` handler to actually do something on
connect.

<a id="magnetismconfig-on_connect"></a>

#### `pub fn on_connect( mut self, f: impl Fn(&MagnetConnection, &mut EventContext) + 'static, ) -> Self`

The handler invoked when a connection is formed (mouse release or
keyboard confirm). Runs with a live `EventContext` and no scene
borrow held, so it may mutate the model (add an edge item,
reparent), call `scene.add_a11y_relation`, or fire an intent.

<a id="magnetismconfig-capture_px"></a>

#### `pub fn capture_px(mut self, px: f32) -> Self`

Capture and grab radius in **screen pixels** (converted to scene
units by dividing by the live zoom, so snapping feels consistent
at any zoom). Default 14.

<a id="magnetismconfig-markers"></a>

#### `pub fn markers(mut self, markers: MarkerVisibility) -> Self`

When magnet markers are painted. Default
`MarkerVisibility::DuringInteraction`.

<a id="magnetismconfig-feedback"></a>

#### `pub fn feedback( mut self, f: impl Fn(&mut Canvas, &PaintContext, &MagnetFeedback) + 'static, ) -> Self`

Replace the built-in feedback renderer with a custom one. The
closure paints in scene coordinates (the canvas already has the
view transform pushed).

<a id="magnetismconfig-connect_key"></a>

#### `pub fn connect_key(mut self, key: Key) -> Self`

The key that toggles keyboard connect mode while the SceneView is
focused. Default `m`.

<a id="magnetismconfig-enabled"></a>

#### `pub fn enabled(mut self, on: impl Into<Prop<bool>>) -> Self`

Set the enabled state, statically or reactively (an app-owned
signal drives enabled/disabled from e.g. a toolbar toggle).

<a id="magnetismconfig-enabled_signal"></a>

#### `pub fn enabled_signal(&self) -> Signal<bool>`

The reactive enabled signal, for a toolbar to read or bind.

<a id="magnetismconfig-is_enabled"></a>

#### `pub fn is_enabled(&self) -> bool`

Whether magnetism is currently enabled.
