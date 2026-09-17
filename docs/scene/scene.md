<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Scene

The `Scene` data model — the owner of all items in a pannable/zoomable
scene.

`Scene` holds a flat list of entries in a parent-relative scene-graph, plus
a pluggable `SpatialIndex` for rectangular queries. Items are positioned
by `local_pos` (in their parent's coordinate frame, or scene-root if they
have none) and an optional `transform` (rotation/scale around the local
origin); the Scene composes those up the parent chain to derive each item's
`scene_transform` and axis-aligned bounding box for hit-test, paint, and
culling. Two content tiers coexist in one `Scene`: heavyweight `Widget`s
(full focus/animation/DnD/AT — placed at scene coordinates) and lightweight
`SceneItem`s (paint-only, no arena overhead, thousands
cheap). All mutations update the `SpatialIndex` in lockstep, so
`Scene::items_in_rect` and `Scene::item_at` stay `O(visible)`.

`Scene` is rarely used directly. The normal entry point is
`SceneModel`, a cloneable `Rc<RefCell<Scene>>` handle
with `&self` mutators (the `ListModel` pattern) that lets multiple handlers
and multiple `SceneView`s share one model.

## When to use

Use `Scene` (via `SceneModel`) when you need a pannable/zoomable canvas —
story corkboards, node-graph editors, mind maps, timeline views, CAD
canvases, or simple spatial maps. Prefer a plain `ListView` or `TreeView`
when the content is linear or tree-shaped without spatial relationships.

## Example

```rust
use teksilo_scene::{Scene, ItemChange, SceneLayer};
use teksilo_scene::{RectItem, ItemId};
use teksilo_canvas::{Point, Rect};
use teksilo_tokens::Color;

let mut scene = Scene::new();

// Add a lightweight rectangle item at scene coordinates (50, 50).
let id: ItemId = scene.add_item(
    RectItem::new(Rect::new(0.0, 0.0, 80.0, 40.0)).fill(Color::BLUE),
    Point::new(50.0, 50.0),
);

// Observe every mutation — fires after the change is already applied.
// Each notification is a `SceneChange`: the change itself, plus the
// transaction it belongs to, whose it was, and whether it counts as
// history. See `teksilo_scene::SceneChange`.
let _guard = scene.item_change_signal().observe(|notification| {
    if let ItemChange::LocalPosChanged { id: _, old: _, new } = &notification.change {
        let _ = new; // react to the new position
    }
});

// Move the item; the observer fires and the spatial index updates.
scene.set_local_pos(id, Point::new(100.0, 100.0));
assert_eq!(scene.scene_pos(id), Some(Point::new(100.0, 100.0)));
```

## Builder methods at a glance

`with_index`, `add_widget`, `add_item`, `add_item_dynamic`, `refresh_dynamic_bounds`, `item_change_signal`, `a11y_change_signal`, `cascade_budget`, `set_cascade_budget`, `open_transaction_depth`, `transaction_signal`, `mutation_version`, `structural_version`, `local_pos`, `set_local_pos`, `local_bounds`, `set_local_bounds`, `size_policy`, `set_size_policy`, `set_measured_size`, `transform`, `set_transform`, `scene_transform`, `scene_pos`, `scene_rect`, `map_to_scene`, `map_from_scene`, `flags`, `set_flags`, `set_flag`, `set_visible`, `is_effectively_visible`, `opacity`, `set_opacity`, `set_item_fill`, `clear_item_fill`, `set_item_stroke`, `clear_item_stroke`, `add_boxed_item`, `set_item_handlers`, `handlers_mut`, `handlers`, `effective_opacity`, `set_scene_rect`, `scene_rect_extent`, `pan_axes`, `current_pan_axes`, `zoomable`, `is_zoomable`, `set_pan_bounds`, `current_pan_bounds`, `set_zoom_range`, `current_zoom_range`, `pan_axes_signal`, `pan_bounds_signal`, `zoom_range_signal`, `zoomable_signal`, `constraints`, `set_z`, `bring_to_front`, `send_to_back`, `z`, `set_layer`, `layer`, `set_item_parent`, `parent_of`, `is_descendant_of`, `collect_descendants`, `set_geometry_constraint`, `clear_geometry_constraint`, `has_geometry_constraint`, `in_geometry_constraint`, `selection_roots`, `transformable_roots`, `scene_rotation`, `transform_frame`, `apply_transform_delta`, `item`, `paint_key`, `remove`, `take`, `restore`, `restore_all`, `replace_item`, `placement`, `set_placement`, `reparent_keeping_scene_pos`, `z_between`, `orphan`, `items_in_rect`, `item_thumbnails`, `item_shape`, `item_contains`, `item_region`, `items_in_region`, `item_at`, `item_at_scaled`, `entry_at`, `items_at`, `items_at_scaled`, `item_at_in_view`, `is_hit_testable`, `colliding_items`, `colliding_items_with`, `items_along_path`, `items_along_path_with`, `len`, `is_empty`, `ids`, `index`, `add_magnet`, `remove_magnet`, `clear_magnets`, `set_magnet_local_pos`, `set_magnet_enabled`, `magnet_ids_of`, `magnet_owner`, `magnet_enabled`, `magnet_scene_pos`, `magnet`, `compute_item_snap`, `compute_port_snap`, `nearest_magnet`, `add_a11y_group`, `remove_a11y_group`, `a11y_group`, `set_a11y_parent`, `a11y_parent_of`, `add_a11y_relation`, `a11y_relations`, `set_a11y_live`, `set_a11y_landmark`, `set_a11y_categories`, `a11y_categories_of`, `a11y_live_of`, `a11y_landmark_of`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/index.html)

## `pub enum ItemChange`

A change to an item's state, fired through
`Scene::item_change_signal` for every mutation. Apps observe to wire
validation, persistence, telemetry, mirroring to a data layer, and other
side effects. The model is "fire after the change has been applied" — by
the time the observer sees the event, the Scene already reflects it, and
(when the mutation came through a `SceneModel`) the
observer may freely read *and* write the scene back.

# Edits and derived notifications

Most variants are **edits**: one mutation, one variant, both sides of the
value it replaced, and `SceneTransactionRecord::edits`
carries them so a data layer can invert the transaction by walking them in
reverse.

`VisibilityChanged` is not one. It is a
*derived* notification — a convenience beside the
`FlagsChanged` that actually describes the mutation,
emitted by both flag doors whenever `IS_VISIBLE` flips so a consumer need
not diff two bitsets. It rides the signal and stays out of the record, so
hiding a card is one edit whichever door hid it.
`is_edit` is the test, for a consumer that counts changes
off the signal and wants the same number the record has.

`#[non_exhaustive]`: this is the crate's outbound event vocabulary, matched
by every observer, and it grows whenever the scene learns to report
something new — `HandlersChanged` is the most recent. Without the
attribute each such addition would stop a downstream `match` from
compiling; with it, a consumer's wildcard arm keeps meaning "a change I do
not act on".

```rust
pub enum ItemChange { /* variants */ }
```

### Variants

- **`LocalPosChanged`** — `set_local_pos`: position in parent coords moved.
- **`LocalBoundsChanged`** — `set_local_bounds`: AABB in local coords changed.
- **`MeasuredSizeChanged`** — `set_measured_size`: a **derived** geometry write — the size an entry under a non-`Fixed` `SizePolicy` measured from its widget, or one an app computed from content it already owns.  Separate from `LocalBoundsChanged` because it means something different to every consumer of the channel:  * It is **not an edit**. `is_edit` is `false` and the   change is stamped `ephemeral`, so a   history above the scene does not gain an undo step because a   paragraph re-wrapped. A reflow is a function of content the document   already holds; undoing it would mean undoing the width or the words. * It is **not structural**. It is counted out of   `Scene::structural_version`, the way   `refresh_dynamic_bounds`'s   per-frame churn is, so a line wrap does not buy an AccessKit re-walk. * The observing `SceneView` answers it with a   **relayout**, not a rebuild: the geometry moved, nothing was   materialised or reaped, and the delegate does not need re-running.  An app mirroring scene geometry into its document should match `LocalBoundsChanged` and ignore this one. An app that wants the measured height persisted anyway (so a cold start opens at the right size without waiting for a measurement) can read it and store it as a hint.
- **`SizePolicyChanged`** — `set_size_policy`: who decides this entry's box changed.  An **edit**, unlike the `MeasuredSizeChanged` that follows it: "this note's height follows its words" is a decision someone made about the document, the way a flag change is, and a history above the scene should be able to put it back.  It carries no geometry — the policy decides how the *next* layout pass computes one. It exists at all because nothing else would tell a view that the answer to a question it asks every pass has changed: a policy written silently would take effect whenever something unrelated happened to dirty the view, which in a test is never and in an app is worse — unpredictable.
- **`TransformChanged`** — `set_transform`: local→parent transform changed.
- **`VisibilityChanged`** — `IS_VISIBLE` flipped, by whichever door flipped it — `set_visible`, `set_flag` or a wholesale `set_flags`.  A **derived notification**, always emitted immediately before the `FlagsChanged` that describes the same mutation. It exists so a consumer that only cares about visibility need not diff two `ItemFlags` bitsets, and it is deliberately not an edit: `is_edit` is `false` for it and it never reaches a transaction record, so one mutation is one recorded edit no matter which door made it. (It used to be emitted by `set_flag` and not by `set_flags`, which made hiding a card two recorded changes through one door and one through the other.)
- **`FlagsChanged`** — `set_flags` / `set_flag` changed the bitset. The edit that describes a flag change, visibility included.
- **`OpacityChanged`** — `set_opacity`: local opacity multiplier changed.
- **`ZChanged`** — `set_z`: paint z-order changed.
- **`LayerChanged`** — `set_layer`: the Under/Over paint band changed.
- **`ParentChanged`** — `set_item_parent`: logical parent changed.
- **`Removed`** — `remove`: item is gone.
- **`Added`** — `add_item` / `add_widget`: item was inserted.
- **`PayloadChanged`** — `set_payload`: the type-erased payload of a `Delegated` heavyweight entry was replaced. A `SceneView` rebuilds that entry's widget (re-invokes its delegate) on the next build. Routed through `emit_item_change`, so `mutation_seq` advances and the AT-walk gate notices.
- **`AppearanceChanged`** — `set_item_fill` / `set_item_stroke` / `clear_item_*`: a lightweight item's paint-only appearance (fill / stroke colour or style) changed. Never moves geometry, so the observing `SceneView` evicts the item's cached frame and repaints **without** relayout or rebuild.  It *can* move the item's hit **shape**, though: a stroked `PathItem` derives its hit band from the stroke it draws, so a view caching hit geometry must re-read this item's shape even while skipping relayout.
- **`ItemReplaced`** — `replace_item`: the lightweight item box at this id was swapped for a different one, keeping the entry — position, transform, z, layer, parent, flags, opacity, handlers, magnets and logical-AT decorations all survive, and so does the `ItemId`.  One change, not `Removed` + `Added`, because nothing about the item's identity changed. Consumers must treat it as a **geometry** change: the new item carries its own `local_bounds` and its own hit shape.  `old_bounds` / `new_bounds` are the entry's AABB either side of the swap; the replaced box itself is returned to whoever called `Scene::replace_item`.
- **`PlacementChanged`** — `set_placement`: parent, z, local position and transform written together as one property.  The atomic form of the four separate mutators. A visually-stable reparent ("drag this card into that group") is one edit here, where `set_item_parent` + `set_local_pos` + `set_transform` is three edits, three undo steps, and two intermediate states in which the item is visibly in the wrong place.
- **`HandlersChanged`** — `set_item_handlers` / `handlers_mut`: the item's handler set was replaced or handed out for mutation.  Always means at least "this item's handlers are no longer what you last read", which is what a consumer caching them — the `SceneView`'s dispatch snapshot — needs. Without it, the two handler mutators were the only doors in the model that changed observable state silently.  Whether it *also* describes the change is `replaced`; see `HandlerReplacement`.

### Methods

#### `pub fn id(&self) -> ItemId`

The item this change is about.

Every variant names exactly one item, so an observer that only needs
*which* item moved need not match the whole enum. The fan-out's runaway
detector reads it too: it charges each delivery to the subject it is
about, so it needs that subject without caring what kind of change it
is. (Those per-subject counts name the culprit in the panic; the bound
that trips is a flat total — see `CascadeBudget`.)

#### `pub fn is_edit(&self) -> bool`

Whether this change is an **edit** — a mutation a transaction record
carries — rather than a *derived notification* the scene emits beside
one for a consumer's convenience.

Three variants are not edits:

* `VisibilityChanged`, which always
  accompanies the `FlagsChanged` that describes
  the same mutation.
* `HandlersChanged` with no
  `replaced` — the `handlers_mut`
  door, which cannot say what the handlers became.
* `MeasuredSizeChanged`, which is a size
  *derived* from content the document already holds, not a change to it.

Filtering the change signal by this gives the same count the
transaction record has, which is what an app showing "N changes in this
edit" needs: hiding a card is one change through
`Scene::set_visible` and one through `Scene::set_flags`.

## `pub struct HandlerReplacement`

The two sides of a `Scene::set_item_handlers`, as they ride an
`ItemChange::HandlersChanged`.

`None` on either side means "no handler set at all", which is a state an
item can be in and is distinct from an empty one.

`SceneItemHandlerSet` stores its closures as `Rc<dyn Fn>`, so carrying
both sides is a handful of refcount bumps rather than a deep copy — which
is why this could be carried and, until it was, simply was not.

`#[non_exhaustive]`: this crate has out-of-tree consumers.

```rust
pub struct HandlerReplacement { /* fields */ }
```

## `pub struct ItemPayload`

A `Delegated` heavyweight entry's type-erased payload, as it rides an
`ItemChange::PayloadChanged`.

A newtype rather than a bare `Rc<dyn Any>` for one reason: `dyn Any` is not
`Debug`, and `ItemChange` is. Cloning one is a refcount bump, never a deep
copy, so carrying both sides of a payload swap costs two increments.

```rust
pub struct ItemPayload(Rc<dyn std::any::Any>);
```

### Methods

#### `pub fn as_rc(&self) -> &Rc<dyn std::any::Any>`

The underlying handle, for a consumer that wants to downcast it itself.

#### `pub fn into_rc(self) -> Rc<dyn std::any::Any>`

Consume the wrapper for the handle.

#### `pub fn downcast<T: 'static>(&self) -> Option<Rc<T>>`

Downcast to the concrete payload type the app stored, or `None` when it
is something else.

## `pub enum AppearanceChange`

Which paint-only appearance slot changed, with both sides of the write.

One variant per slot rather than one struct carrying both, because a write
touches exactly one of them and a struct would have to invent a value for
the other — which is the class of "plausible but wrong" data a journal must
not contain.

`#[non_exhaustive]`: this crate has out-of-tree consumers, and an item may
grow a third appearance slot.

```rust
pub enum AppearanceChange { /* variants */ }
```

### Variants

- **`Fill`** — `Scene::set_item_fill` / `Scene::clear_item_fill`. `None` on either side means "no fill".
- **`Stroke`** — `Scene::set_item_stroke` / `Scene::clear_item_stroke`. `None` on either side means "no stroke".

## `pub struct Placement`

Where an item sits, as **one** property: logical parent, paint z, position
in the parent frame, and local to parent transform.

# Why these four together

`Scene::set_item_parent` deliberately does not rebase `local_pos` — a
child's position is stated in its parent's frame, so adopting a new parent
moves the item unless the caller compensates. A visually-stable reparent is
therefore `set_item_parent` + `set_local_pos` + `set_transform`: three
events, three things for a history to undo separately, and two intermediate
states in which the item is visibly somewhere it never was.

Writing all four at once removes all three, and it is the half of Figma's
"parent + fractional index as one property" that Teksilo was missing. The
*other* half — O(1) reorder with no sibling rewrite — is already here: `z`
is an `f32` and `Scene::set_z` writes one field, so
`Scene::z_between` is a fractional insert.

`#[non_exhaustive]`: the crate hands this *to* consumer code and takes it
back — and it is the one place a caller states a whole placement, so a fifth
property joining the set is exactly the growth this guards. Build one with
`new`.

```rust
pub struct Placement { /* fields */ }
```

### Methods

#### `pub fn new(parent: Option<ItemId>, z: f32, local_pos: Point, transform: Transform2D) -> Self`

A placement stated field by field — the constructor
[`#[non_exhaustive]`](Self) takes the place of a struct literal for.

## `pub struct CascadeBudget`

The runaway-detection budget for one scene's change fan-out — how much work
**observers** may generate from one batch before the drain declares the
cascade non-terminating and panics.

# Why a budget at all

The drain runs until its queue is empty, which is what lets an observer read
the scene and write it back. An observer whose write never converges
therefore never empties it, and — unlike `Signal::try_set`, which recurses
and so blows the stack loudly on its own — this is an iterative loop whose
only other exit is an empty queue. Unchecked it is a frozen UI thread with
no diagnostic. The limit is enforced in **every** build profile for that
reason.

"Never converges" is the precise condition, and it is not the same as
"unguarded". The geometry mutators already suppress a write that changes
nothing — `Scene::set_local_pos` returns without emitting when the
position is unchanged — so an observer that keeps writing the *same*
position settles on its own. A geometry cascade that does not settle is one
computing a *different* value every time: an accumulating offset, a rounding
drift, a spring with no rest state. The `set_a11y_*` mutators do not
self-suppress; they bump on every call, so there an equality check before
the call is a real guard.

# One trip condition, and why it is flat

**Total observer-generated deliveries in a single drain**, counted across
both channels and every subject. Nothing else trips.

Two earlier shapes of this budget each fixed the previous one's false
positive and bought a worse problem, and the third is why this one is flat:

- A cap on **rounds** aborted a legitimate 300-link settling chain, because a
  round is what was queued when it began, so an N-link chain costs N rounds.
- A cap **per subject** moved the limit onto fan-in: a guarded relaxation
  over a hub passed at 4000 incident edges and aborted at 4200.
- **Scaling** the per-subject cap with the scene's entry count fixed the
  fan-in false positive and destroyed the guard, because the bound it
  resolved grew with the model.

A flat total is the only one of the three that bounds a runaway to the same
amount of work whatever the scene's size. Measured in release, changing only
this budget: an unguarded write-back aborts after 100 001 deliveries in a
10 000-entry scene and in a 50 000-entry one alike, where the budget the
scaled design resolved for those scenes (640 000 and 3 200 000) let the same
loop run 6× and 32× longer — 3.10 s and 131 s against 520 ms and 3.95 s. A
spawner allocates ~100 000 items before tripping at either size, against
640 002 and **3 200 002** under the scaled budget.

(The flat write-back's wall clock still grows with the scene, and that is the
*app's* write, not the drain: `Scene::set_local_pos` re-buckets the moved
subtree, which walks every entry. The identical runaway on the logical-AT
channel, whose mutator does not, aborts in 4.8 ms at both sizes — that is
what this loop itself costs. The budget bounds how many times an observer's
write runs, not what one costs.)

The default also clears every legitimate shape this tier runs by more than an
order of magnitude: a 4200-edge guarded aggregate is about 4 200 deliveries,
and a 2000-link chain that re-places eight port magnets and refreshes three
AT properties per link is about 24 000.

# What it still does not decide

"Guarded cascade that is genuinely enormous" and "unguarded write-back" are
not distinguishable from the queue alone. This budget does not pretend
otherwise: it draws the line where the shapes this tier actually runs stop,
reports the subject with the highest delivery count so the diagnostic points
somewhere, and
`SceneModel::set_cascade_budget` is
the supported answer for a graph that genuinely lives past it. Mechanism in
the framework, policy in the consumer.

# What is exempt

The caller's own batch — everything already queued when the drain began — is
not charged, however large. A bulk load, a ten-thousand-item teardown, or a
scene-wide a11y re-tag inside one `SceneWriteGuard`
is finite by construction, so charging it would make the budget a cap on
batch size instead — the mistake a plain delivery counter makes, whose fix is
to raise it until it catches nothing. Cascade depth and the number of
*distinct* subjects a cascade touches are not limited either.

```
use teksilo_scene::{CascadeBudget, SceneModel};

let model = SceneModel::new();
model.set_cascade_budget(CascadeBudget::new(1_000_000));
assert_eq!(model.cascade_budget().total, 1_000_000);
```

`#[non_exhaustive]`: a budget is built through
`CascadeBudget::new` or `Default`, never by struct literal,
so a second dimension (a per-subject cap, a depth limit) can be added
without breaking a caller.

```rust
pub struct CascadeBudget { /* fields */ }
```

### Methods

#### `pub fn new(total: u64) -> Self`

A budget of `total` observer-generated deliveries per drain, clamped up
to the smallest usable value.

The clamp is here rather than at the trip so that a nonsensical budget is
rejected where it is written, instead of producing a panic whose numbers
describe no cascade that happened.

## `pub enum SceneLayer`

Which paint band a lightweight `SceneItem` sits in, relative to
the heavyweight widget tier.

The band is the leading digit of `PaintKey`, so it decides both what is
drawn on top and what the pointer picks — one rule, not two.

| band | rank | where it paints |
|------|------|-----------------|
| `Under` | `RANK_UNDER` | in `SceneView::paint`, a backdrop beneath every card |
| `Interleaved` | `RANK_WIDGET` | among the cards, ordered against them by `z` |
| `Over` | `RANK_OVER` | in `SceneView::post_paint`, above every card |

Within a band, `set_z` orders items among themselves; in
`Interleaved` that same `z` is compared against the **cards'** `z`.

# What `Interleaved` orders against, and what it does not

Cards, and only cards — the heavyweight widget children, which are what the
arena's child walk contains. It is **not** an ordering against the other two
bands, and in particular not against `Under`: the whole of the `Under` band
is painted inside `SceneView::paint`, which runs before the first child, so
every `Under` item is below every interleaved one no matter what the two
`z`s say. Likewise every `Over` item is above every interleaved one.

The consequence worth stating outright, because it surprises: in a scene
with **no heavyweight children at all**, an interleaved item is on top of
everything. There is nothing in the child walk for it to sit between, and
its `z` is compared against an empty set. A lightweight-only page that wants
ink under some of its content wants `Under` plus a `z`, or wants the content
it is drawing on to be cards.

# Why `Interleaved` is not free

`Under` and `Over` cost nothing: the render walker offers a paint position
before and after a node's child subtree, and those are the two bands. There
is no third position *between* two children, so an interleaved item is
painted by a node of its own that the view materialises and slots into the
z-sorted child list.

That node is **paint only**. It is
`event_pass_through`,
so the pointer still resolves through the lightweight hit snapshot exactly
as it does for the other two bands, and it is left out of
`SceneView::accessibility_children`, so the item keeps the same synthetic
`SyntheticKind::SceneItem`
node it had in `Under` — same off-screen culling, same reparenting, same
rotor category. Choosing this band changes what the item is painted
*between* and nothing else.

What it does cost is one arena node per interleaved item. Ink is the case
this exists for ("this stroke is above note A and below note B"), and a
page of ink wants a `GroupItem` per layer rather
than a band per stroke — see `docs/ink.md`.

`#[non_exhaustive]`: this enum has now grown a variant once, and the reason
it could is that the render walker's two paint positions were never the
whole answer. A downstream `match` gets a `_` arm today rather than a break
the next time.

```rust
pub enum SceneLayer { /* variants */ }
```

### Variants

- **`Under`** — Painted under the heavyweight widget children (the default).
- **`Interleaved`** — Painted **among** the heavyweight widget children, ordered against them by `z` — and against nothing else: still above every `Under` item and below every `Over` one, and on top of everything in a scene that has no heavyweight children. See the type docs for that and for what it costs.
- **`Over`** — Painted over the heavyweight widget children.

## `pub enum SizePolicy`

How an entry's `local_bounds` is decided each layout pass.

The default, `Fixed`, is what every scene had before this
existed: the model owns both axes and nothing measures anything. The other
two hand one or both axes to the widget, which is the only thing that can
answer "how tall is this text at this width".

# Heavyweight only, and why

A lightweight `SceneItem` has no `layout_response` to
ask — it publishes its own AABB, and
`Scene::add_item_dynamic` already re-reads that
each build. A widget's height is not a value it publishes; it is the answer
to a question asked at a width. So the two mechanisms are not variants of
each other, and this one is refused for a lightweight entry rather than
silently doing nothing: `Scene::set_size_policy`
returns `false`.

# What is measured, and what is not

A view measures exactly the cards it gives a **non-zero size** — the ones
inside its viewport. Not the ones it keeps alive in the retention band, and
not the one the user is interacting with if the camera has left it behind:
those are laid out at `Size::ZERO`, so there is nothing a measurement could
be for. A card the camera has never shown keeps the size `add_widget_item`
was handed, and is corrected on the first pass that places it, in the same
frame it appears. That is the
contract `ListView::auto_item_height` already makes for rows: the number in
the model is an **estimate** until the thing has been on screen, and every
whole-scene query that reads it —
`scene_rect_extent`, the spatial index, a marquee,
a minimap — reads the estimate for a card that has never been realised.
Give a plausible one; the alternative is measuring every card in the scene
on every pass, which is the cost the viewport cull exists to avoid.

# The measurement must be idempotent

A measured height feeds back into the model, and the model feeds the next
pass. A body whose `layout_response` answers differently for the same width
— because it mutates state, or because its natural height depends on the
height it was given — would oscillate forever. The view bounds that, and it
is careful about *what an oscillation is*: `100 → 120 → 100` is one when the
body is answering its own last answer, and is a user typing a character that
wraps and then deleting it when it is not. The two are indistinguishable
from the numbers, and freezing the second is the most ordinary editing
action there is.

What separates them is **who drove the pass**. A body's answer is a function
of its content and of the width it is offered; at a fixed width its content
cannot change without something dirtying the card — a `Signal` bound at
`Relayout`, a rebuild, a fresh node — whereas the view's own write dirties
only the view and what is above it. So on the pass a write causes, the card
reads clean, and a clean card that answers differently is reading back what
was written to it. The view takes the taller of the two answers, and after a
couple of such contradictions stops taking new ones for the rest of the
chain — which writes nothing, and a pass that writes nothing schedules no
successor. A non-idempotent body therefore costs a bounded number of passes
per external event rather than an unbounded number for ever, and an edited
one is never bounded at all. See
`teksilo_core::widget::Widget::cacheable_layout`
for the framework's own statement of the same obligation.

# A resize does not reach an axis the content owns

`HeightForWidth` keeps its width, so the selection
frame resizes it horizontally and the words decide the rest; the vertical
half of a corner drag is neutralised in
`Scene::apply_transform_delta`,
because a height written there is one the next pass measures straight back
over. `Intrinsic` owns neither axis, so
`Scene::transformable_roots` offers it
no resize handles at all. And a measurement taken while a gesture is
previewing a transform is placed but never written: the preview is
recomputed from the gesture's frozen start frame each sample, so a previewed
width that reached the model would be scaled again on the next one.

`#[non_exhaustive]`: this crate has out-of-tree consumers.

```rust
pub enum SizePolicy { /* variants */ }
```

### Variants

- **`Fixed`** — The model owns both axes. Today's behaviour, and the default, so every existing scene is bit-identical.
- **`HeightForWidth`** — The model owns the width; the height is **measured from the widget** at that width on every pass that lays it out.  The right policy for text: a note's width is authored (you drag its edge) and its height follows the words. `scene_rect(id).height` becomes advisory for such an entry — it reports the last measured height, not the one `add_widget_item` was handed.
- **`Intrinsic`** — Both axes are measured: the entry shrink-wraps its widget.  For content that knows its own size on both axes — a pinned label, a badge, a fixed-aspect thumbnail. The rect passed to `add_widget_item` supplies only the position.

## `pub enum PanAxes`

Which axes a `SceneView` is allowed to pan
along. Set on the `Scene` (not the View) because a given scene
model often makes sense at one orientation only — a horizontal
timeline, a vertical timeline, a fixed-extent diagram. All views
of the same scene inherit the constraint.

```rust
pub enum PanAxes { /* variants */ }
```

### Variants

- **`None`** — No user-driven pan in either axis. Programmatic `SceneView::set_pan` / `pan_to` become no-ops too.
- **`Horizontal`** — Pan only along X. Vertical scroll deltas pass through to ancestor scrollables.
- **`Vertical`** — Pan only along Y. Horizontal scroll deltas pass through to ancestor scrollables.
- **`Both`** — Default: pan freely in both axes.

## `pub struct SceneConstraints`

Reactive interaction-policy bundle owned by `Scene`. Apps
configure pan/zoom behaviour by writing to these signals; gesture
closures in `SceneView` read them live, so
runtime mode switches (e.g. a toolbar toggling pan locks) take
effect on the next event without rebuilding the view.

All four signals are exposed individually via `Scene` accessors
(`pan_axes_signal`, `pan_bounds_signal`, `zoom_range_signal`,
`zoomable_signal`). Per-(sub-)scene independence falls out of the
model: each nested `SceneView` carries its own `Scene` with its
own `SceneConstraints`.

View-level *tightening* overrides (`pan_bounds_override`,
`zoom_range_override`) layer on top per-`SceneView` — the
effective constraint is the intersection. Two views over the
same `Scene` can lock down independently; neither can loosen
what the `Scene` declares.

```rust
pub struct SceneConstraints { /* fields */ }
```

### Methods

#### `pub fn pan_axes_signal(&self) -> Signal<PanAxes>`

Reactive pan-axes signal. Gesture handlers read live.

#### `pub fn pan_bounds_signal(&self) -> Signal<Option<Rect>>`

Reactive pan-bounds signal. `None` = unconstrained.

#### `pub fn zoom_range_signal(&self) -> Signal<Option<std::ops::RangeInclusive<f32>>>`

Reactive zoom-range signal. `None` = unconstrained from
the Scene side.

#### `pub fn zoomable_signal(&self) -> Signal<bool>`

Reactive zoomable-on/off signal. Equivalent to a zero-width
zoom_range — kept as a separate boolean for clarity and
efficient short-circuit at gesture time.

## `pub struct Scene`

The data model behind a `SceneView`: a flat list of entries in a
parent-relative scene-graph plus a `SpatialIndex` for rectangular
queries.

The Scene itself does no rendering — it's a passive container the view
reads from at build / place / paint time. Mutations (`add_widget`,
`add_item`, `set_local_pos`, `set_transform`, `set_local_bounds`, `remove`)
update the spatial index in lockstep, so `items_in_rect`, `item_at`, and
SceneView's viewport-cull path are all `O(visible)` instead of `O(N)`. When
a parent's `local_pos` or `transform` changes, every descendant's
scene-AABB shifts; the Scene re-buckets the entire subtree.

In practice most callers operate on a `SceneModel`
handle (`Rc<RefCell<Scene>>` with `&self` mutators) rather than a bare
`Scene`. Prefer `SceneModel` for any widget or handler that needs to share
the scene across multiple owners.

```rust
pub struct Scene { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

An empty scene with the default `GridHashIndex`.

#### `pub fn with_index(index: Box<dyn SpatialIndex>) -> Self`

An empty scene with a custom `SpatialIndex`.

#### `pub fn add_widget<W: Widget + 'static>(&mut self, widget: W, local_rect: Rect) -> ItemId`

Place a heavyweight `Widget` at `local_rect`'s origin, sized
`local_rect.size`. The rect is interpreted as
`(local_pos = local_rect.origin, local_bounds = (0, 0, w, h))`.
Returns the `ItemId` for later mutation. The widget is
consumed at SceneView build time and added to the arena.

#### `pub fn add_item<I: SceneItem + 'static>(&mut self, item: I, local_pos: Point) -> ItemId`

Place a lightweight `SceneItem` at `local_pos`. The item's
`local_bounds` and `initial_flags` are read once at insert
time. The item is **not** added to the arena — it's painted
directly from `SceneView::paint`.

#### `pub fn add_item_dynamic<I: SceneItem + 'static>( &mut self, item: I, local_pos: Point, ) -> ItemId`

Like `add_item` but flags the entry as
having signal-driven `local_bounds`. The Scene re-reads
`item.local_bounds()` each rebuild via
`refresh_dynamic_bounds` — the
SceneView calls that at the start of every build pass. The
spatial index gets re-bucketed when the read-back differs
from the cached value, so `items_in_rect` / hit-test stay
correct without app-side `set_local_bounds` plumbing.

Use only when the bounds genuinely depend on a `Signal<T>`
the item reads in `local_bounds`. Static items pay an
unnecessary per-rebuild bounds read otherwise; prefer
`add_item` for the common case.

#### `pub fn refresh_dynamic_bounds(&mut self) -> bool`

Re-read every dynamic item's current `local_bounds`, applying
`set_local_bounds` (and re-bucketing the spatial index) for
any entry whose value has changed. No-op for static entries.
Called by `SceneView` at the start of each
`build()` so signal-driven bounds propagate to bucketing
without explicit app-side calls.

Returns `true` if at least one dynamic entry's bounds changed this call.
`SceneView` uses the `true → false` transition (an animation settling) as
the one moment to walk the final animated bounds into the AccessKit tree,
since it otherwise suppresses per-frame AT re-walks during the animation.

#### `pub fn item_change_signal(&self) -> Signal<SceneChange>`

Reactive notification stream for every Scene mutation. Apps observe via
`signal.observe(|change| …)` to wire validation, persistence,
telemetry, or mirroring into a data layer without polling the Scene
each frame. The signal fires *after* the mutation has been applied — by
the time the observer runs, the Scene already reflects the new state.

# Notification timing

Where the fan-out happens depends on which door mutated the scene, and
the difference is exactly the `RefCell` an observer would have to
re-enter:

- **Through a `SceneModel`** (the normal path,
  including `SceneWriteGuard`) the mutation
  runs inside a *write scope*: changes queue in emission order and fan
  out once the model has released its `borrow_mut()`. An observer may
  therefore read the scene, and write it back — a write made from an
  observer queues in turn and is delivered by the same drain, after the
  changes already queued ahead of it.
- **Through a bare `&mut Scene`** there is nothing holding a `RefCell`
  open to escape from, so the fan-out is synchronous, inside the
  mutator, exactly as before.

  This is safe for a `Scene` you own outright — no second handle to it
  can exist. It is **not** safe for a `&mut Scene` reborrowed out of a
  `SceneModel`: a `RefMut` taken from the shared
  cell leaves `defer_depth` at zero, so observers still run under the
  live exclusive borrow and one that re-enters the model panics. Take
  `SceneModel::write_guard` instead —
  it derefs to `&mut Scene`, so the call sites are identical, and it
  opens the write scope the observers need.

Either way the notification has been delivered by the time the mutator
call returns.

# What is not deferred

The scene's constraint signals — `pan_axes_signal`,
`zoomable_signal`,
`pan_bounds_signal`,
`zoom_range_signal` — carry *state*, not
events (`current_pan_axes` and friends read
them back), so queueing their writes would make the scene contradict
itself inside an open write scope. They still fan out synchronously
under the borrow: an observer on one of those four must not re-enter
the `SceneModel`.

#### `pub fn a11y_change_signal(&self) -> Signal<u64>`

Reactive notification for logical-AT-structure mutations
(`add_a11y_group` / `remove_a11y_group` / `set_a11y_parent` /
`add_a11y_relation` / `set_a11y_live` / `set_a11y_landmark` /
`set_a11y_categories`). A monotonic counter bumped after each such
mutation. `SceneView` observes this to re-walk the AccessKit tree —
these changes don't flow through `item_change_signal`
because they aren't item geometry, and the AT tree is separate from the
visual scene.

#### `pub fn cascade_budget(&self) -> CascadeBudget`

The runaway-detection budget for this scene's change fan-out.

#### `pub fn set_cascade_budget(&self, budget: CascadeBudget)`

Replace the runaway-detection budget for this scene's change fan-out.

The default is tuned for the graph shapes this tier runs; see
`CascadeBudget` for the rule and for when raising it is the right
answer rather than a way to silence a real cycle.

A budget of zero is clamped to the smallest usable one rather than
stored: it would forbid the reactive write-back this channel exists for,
and would trip with no cascade for the diagnostic to describe.

Takes effect on the next drain — a budget raised from inside an observer
does not enlarge the drain already running, which is why the budget is
read once at its start.

#### `pub fn open_transaction_depth(&self) -> u32`

How many transaction / write scopes are open on this scene.

Zero between mutations. A non-zero value outside a mutator means a
`SceneTransaction` is being held, which is
only correct within one synchronous scope — see
`SceneModel::transaction`.

#### `pub fn transaction_signal(&self) -> Signal<crate::journal::TxnId>`

Fires once per committed transaction, after the edit sink, with the
scene unborrowed. See
`SceneModel::transaction_signal`.

#### `pub fn mutation_version(&self) -> u64`

Monotonic counter of every model mutation applied so far — item geometry
/ visibility / structure (each `ItemChange`) **and** logical-AT
structure (groups, parents, relations, live, landmarks, categories).

Includes the per-frame churn of
`refresh_dynamic_bounds`; a consumer
gating an expensive rebuild on "did anything *meaningful* change" wants
`structural_version` instead. The counter
wraps; compare for equality, not ordering.

#### `pub fn structural_version(&self) -> u64`

`mutation_version` with the per-frame
dynamic-bounds churn subtracted out: it advances on every mutation
**except** the `LocalBoundsChanged` events
`refresh_dynamic_bounds` emits.

The version to gate an expensive rebuild on.
`SceneView` snapshots it each `build()` and only
re-walks the (separate, expensive) AccessKit tree when it has advanced
since the previous walk — so an actively-animating
`add_item_dynamic` item, which rebuilds every
frame, does not issue an AT re-walk per frame for sub-pixel bounds drift
a screen reader cannot use.

Naming the exclusion is the point. The alternative — snapshotting
`mutation_version` before `refresh_dynamic_bounds` and again after, and
treating the difference as churn — is wrong now that the refresh fans
its changes out: an observer may legally mutate the scene from that
fan-out, and its mutation lands inside the bracket where it is
indistinguishable from churn. Folded into the baseline, an AT-structural
change made there would never un-gate a re-walk, in that build or any
later one. The counter wraps; compare for equality, not ordering.

#### `pub fn local_pos(&self, id: ItemId) -> Option<Point>`

Read an item's `local_pos` (its anchor in parent coords).

#### `pub fn set_local_pos(&mut self, id: ItemId, local_pos: Point)`

Move an item to a new `local_pos` in its parent's coordinate
frame. Re-buckets the item *and* every descendant in the
spatial index since the descendants' scene-AABBs shift along.
No-op if the id is unknown.

#### `pub fn local_bounds(&self, id: ItemId) -> Option<Rect>`

Read an item's `local_bounds` (its AABB in local coords).

#### `pub fn set_local_bounds(&mut self, id: ItemId, local_bounds: Rect)`

Update an item's `local_bounds`. For lightweight items this
also calls `SceneItem::set_local_bounds` on the item so its
next `paint` reflects the new geometry. The spatial index is
re-bucketed; only this item moves (descendants' local frames
are unchanged). No-op if the id is unknown.

# The item has the last word, and says it once

An item whose box is **derived** from its own geometry — a
`PathItem` — treats this as *"fit yourself to this
rectangle"* rather than *"adopt this rectangle"*, and the box stored
here is what it settled on, read back from the item. It lands on the
request on every axis the geometry has extent on; on an axis it has
none (a perfectly horizontal stroke has no height) the box stays the
stroke's own thickness, because there is nothing to stretch.

Either way the call is **idempotent**: asking twice for the same
rectangle emits one `ItemChange::LocalBoundsChanged` and re-buckets
the index once. An app driving this per frame — a resize handle, a
layout pass — therefore goes quiet as soon as it stops moving, rather
than emitting an endless series of nearly-identical changes.

#### `pub fn size_policy(&self, id: ItemId) -> SizePolicy`

Which axes of this entry's `local_bounds` its widget decides.
`SizePolicy::Fixed` for an unknown id, and for every lightweight item.

#### `pub fn set_size_policy(&mut self, id: ItemId, policy: SizePolicy) -> bool`

Hand one or both axes of `id`'s box to its widget. See `SizePolicy`.

Returns `false` — and changes nothing — for an unknown id or a
**lightweight** entry, which has no `layout_response` to ask. That is a
refusal rather than a silent no-op because the two tiers already have
different answers to "who decides my box"
(`add_item_dynamic` is the lightweight one),
and a policy that quietly did nothing on one of them would read as a
bug in the view rather than a misuse of the model.

Emits `ItemChange::SizePolicyChanged` when the value actually changes —
an edit, carrying no geometry. The geometry it enables arrives separately
as `ItemChange::MeasuredSizeChanged`, which is not an edit.

#### `pub fn set_measured_size(&mut self, id: ItemId, size: Size) -> bool`

Write a **derived** size into the model: the same geometry update and
index re-bucketing `set_local_bounds` does,
reported as `ItemChange::MeasuredSizeChanged` instead — not an edit,
not structural, answered by a relayout rather than a rebuild.

Returns whether anything moved. The origin of `local_bounds` is kept;
only the size is written, because that is the whole of what a
measurement can know.

# When this is the right door, and when it is not

Use it for a size that is a **pure function of content the app already
owns**: the height a paragraph wraps to at a given width, the bounding
box of an ink stroke after a segment dried, the extent of a generated
diagram. Recording such a write as an edit is what makes a text reflow
an undo step; bumping the structural version for it is what buys an
AccessKit re-walk for a two-pixel line-height change. This door does
neither, and the view that observes it relayouts instead of rebuilding.

Use `set_local_bounds` for anything the
**user** did: a resize handle, an import, a paste. Those are edits, and
a history above the scene must see them.

The `SizePolicy` machinery drives this from inside the view; an app
measuring something the framework cannot measure calls it directly.

#### `pub fn transform(&self, id: ItemId) -> Option<Transform2D>`

Read an item's local→parent transform (rotation/scale around
the local origin). Identity by default.

#### `pub fn set_transform(&mut self, id: ItemId, transform: Transform2D)`

Set an item's local→parent transform. Re-buckets the item's
subtree in the spatial index. No-op if the id is unknown.

#### `pub fn scene_transform(&self, id: ItemId) -> Transform2D`

The composed local→scene transform for this item, walking up
the parent chain. Identity for an item that doesn't exist.

#### `pub fn scene_pos(&self, id: ItemId) -> Option<Point>`

The item's anchor in scene coords (its local origin
transformed through the parent chain).

#### `pub fn scene_rect(&self, id: ItemId) -> Option<Rect>`

The AABB enclosing the item's `local_bounds` after composing
through the parent chain — i.e. the rectangle the spatial
index buckets on. `None` if the id is unknown.

#### `pub fn map_to_scene(&self, id: ItemId, local_pt: Point) -> Option<Point>`

Map a point in the item's local frame to scene coords.

#### `pub fn map_from_scene(&self, id: ItemId, scene_pt: Point) -> Option<Point>`

Map a point in scene coords to the item's local frame.
Returns `None` if the item is unknown or its scene transform
is degenerate (zero scale).

#### `pub fn flags(&self, id: ItemId) -> Option<ItemFlags>`

Read an item's `ItemFlags` bitset.

#### `pub fn set_flags(&mut self, id: ItemId, flags: ItemFlags)`

Replace an item's flags wholesale. No-op if unknown.

Announces exactly what `set_flag` announces for the
same net change: the derived `ItemChange::VisibilityChanged` when
`IS_VISIBLE` flipped, then the `ItemChange::FlagsChanged` that
describes the mutation.

#### `pub fn set_flag(&mut self, id: ItemId, flag: ItemFlags, on: bool)`

Set or clear a single flag on an item. No-op if unknown.

#### `pub fn set_visible(&mut self, id: ItemId, visible: bool)`

Toggle the `ItemFlags::IS_VISIBLE` bit. Convenience for
the common "hide this item" operation.

#### `pub fn is_effectively_visible(&self, id: ItemId) -> bool`

Whether the item is visible AND every ancestor in its chain
is visible. Returns `true` when nothing in the chain has
`IS_VISIBLE` cleared. `false` for unknown ids.

#### `pub fn opacity(&self, id: ItemId) -> Option<f32>`

Read an item's local opacity multiplier (`1.0` by default).

#### `pub fn set_opacity(&mut self, id: ItemId, opacity: f32)`

Set an item's local opacity, clamped to `[0.0, 1.0]`.

#### `pub fn set_item_fill(&mut self, id: ItemId, fill: impl Into<ColorProp>)`

Replace a lightweight item's fill colour live, emitting
`ItemChange::AppearanceChanged` — **always repaint-only**, never a
relayout, rebuild, or AccessKit re-walk. The colour is a `ColorProp`,
so it accepts a plain `Color`, a theme role, a
`Signal<Color>`, or a `Signal<Role>`. No-op for item kinds without a fill
(e.g. `ImageItem`).

# Reactivity contract

A colour becomes **continuously** reactive by being registered at build
time (`SceneItem::register_bindings`). So:

- **Construct** the item with a `Signal`/role colour (`.fill(my_signal)`)
  for a colour that tracks its signal forever. This is the recommended
  path and needs no mutator at all.
- **This mutator** installs a *snapshot*: it repaints immediately, which
  is all a static colour ever needs. If you pass a `Signal`/dynamic role
  here, it paints the signal's current value now and starts tracking it
  continuously from the owning view's next rebuild (whenever some other
  structural change re-runs `register_bindings`). Deliberately *not*
  forced: a colour change must never cost a rebuild + AT re-walk.

#### `pub fn clear_item_fill(&mut self, id: ItemId)`

Clear a lightweight item's fill (Rect/Path/Group become fill-less),
emitting `ItemChange::AppearanceChanged` (repaint-only). No-op for items
whose fill can't be cleared (e.g. `TextItem`, which always has a
foreground colour).

#### `pub fn set_item_stroke(&mut self, id: ItemId, color: impl Into<ColorProp>, style: StrokeStyle)`

Replace a lightweight item's stroke (colour + `StrokeStyle`) live,
emitting `ItemChange::AppearanceChanged` (repaint-only). No-op for item
kinds without a stroke slot (`TextItem` / `ImageItem`). See
`set_item_fill` for the reactivity contract.

#### `pub fn clear_item_stroke(&mut self, id: ItemId)`

Clear a lightweight item's stroke, emitting
`ItemChange::AppearanceChanged` (repaint-only). No-op for item kinds
without a stroke.

#### `pub fn add_boxed_item(&mut self, item: Box<dyn SceneItem>, local_pos: Point) -> ItemId`

Insert an already-boxed lightweight item at `local_pos`, returning its
id. The boxed-`dyn` counterpart of `add_item` — used by
`SceneListAdapter` whose delegate yields
`Box<dyn SceneItem>`.

#### `pub fn set_item_handlers(&mut self, id: ItemId, handlers: Option<SceneItemHandlerSet>)`

Replace an item's handler set. Pass `None` to clear.

Fires `ItemChange::HandlersChanged` carrying **both sides**
(`HandlerReplacement`), like every other mutator on this type: a
consumer that caches handlers has no other way to learn of it, and a
data layer inverting a transaction has no other way to put them back.
This is the handler door that produces a reversible edit; see
`handlers_mut` for the one that cannot.

#### `pub fn handlers_mut(&mut self, id: ItemId) -> Option<&mut SceneItemHandlerSet>`

Mutably borrow an item's handler set, lazily creating an
empty one if none exists. Returns `None` for unknown ids.
Allows fluent chains: `scene.handlers_mut(id).unwrap().on_tap(…).cursor(…);`.

Fires `ItemChange::HandlersChanged` *before* handing the set out, with
no `HandlerReplacement` — `&mut` cannot report back what the caller
does with it, so the notification means "no longer what you last read"
and nothing finer. A caller that takes the borrow and changes nothing
therefore costs one spurious invalidation, which is the right way round:
the alternative is a consumer serving stale handlers.

Because it describes nothing, it is **not** recorded as an edit
(`ItemChange::is_edit`) — a transaction record whose every entry
carries both sides of what it replaced must not carry one that carries
neither. Make a handler change the history should be able to reverse
through `set_item_handlers`, which knows both
sides.

#### `pub fn handlers(&self, id: ItemId) -> Option<&SceneItemHandlerSet>`

Read-only access to an item's handler set, if one is set.

#### `pub fn effective_opacity(&self, id: ItemId) -> f32`

Effective opacity composed up the parent chain — the product
of every ancestor's opacity and this item's. `1.0` for an
unknown id (so callers don't end up multiplying by a stale
value).

#### `pub fn set_scene_rect(&mut self, rect: Option<Rect>)`

Declare the scene's logical extent. `None` (the default)
means "auto-compute from items each query"; `Some(rect)`
fixes the extent regardless of item placement. Used by
`SceneView` for pan clamping and `fit_to_content`.

#### `pub fn scene_rect_extent(&self) -> Option<Rect>`

The resolved scene extent — user-declared via
`Scene::set_scene_rect` if set, otherwise the AABB
enclosing every item's scene rect. `None` when neither is
available (the user didn't declare and the scene is empty).

#### `pub fn pan_axes(&mut self, axes: PanAxes)`

Set the axes the view may pan along. Default
`PanAxes::Both`. Writes to the reactive signal; gesture
closures pick the change up on the next event.

#### `pub fn current_pan_axes(&self) -> PanAxes`

The currently-declared pan axes. Live read of the signal.

#### `pub fn zoomable(&mut self, on: bool)`

Set whether the view honors zoom gestures. Default `true`.
Writes to the reactive signal.

#### `pub fn is_zoomable(&self) -> bool`

Whether the scene currently allows zoom. Live read.

#### `pub fn set_pan_bounds(&mut self, bounds: Option<Rect>)`

Clamp the visible viewport to this scene-coord rect. `None`
(default) leaves pan unconstrained. When `Some(r)`, the
`SceneView`'s pan is clamped so the
visible scene region overlaps `r`. When `r` is smaller than
the visible viewport, the rect is centered.

Distinct from `set_scene_rect`:
`scene_rect` declares the scene's logical extent (used by
`adopt_scene_size`); `pan_bounds` controls what region the
user can scroll to. A doc-style app typically sets both to
the same rect.

#### `pub fn current_pan_bounds(&self) -> Option<Rect>`

The currently-declared pan-bounds rect. Live read.

#### `pub fn set_zoom_range(&mut self, range: Option<std::ops::RangeInclusive<f32>>)`

Inclusive `[min, max]` zoom-factor clamp. `None` (default)
is unconstrained from the `Scene` side — the `SceneView`
may still impose its own override.

The effective range applied by the `SceneView` is the
intersection of `Scene` + view-level override, so apps
cannot loosen a `Scene`-declared range by setting a wider
override on the view.

#### `pub fn current_zoom_range(&self) -> Option<std::ops::RangeInclusive<f32>>`

The currently-declared zoom range. Live read.

#### `pub fn pan_axes_signal(&self) -> Signal<PanAxes>`

Reactive accessors for live observation.

#### `pub fn pan_bounds_signal(&self) -> Signal<Option<Rect>>`

Reactive pan-bounds signal.

#### `pub fn zoom_range_signal(&self) -> Signal<Option<std::ops::RangeInclusive<f32>>>`

Reactive zoom-range signal.

#### `pub fn zoomable_signal(&self) -> Signal<bool>`

Reactive zoomable on/off signal.

#### `pub fn constraints(&self) -> &SceneConstraints`

Read-only view of the full constraint bundle. Useful when
passing all four signals to a custom view implementation.

#### `pub fn set_z(&mut self, id: ItemId, z: f32)`

Set paint z-order for an entry. Higher z paints later (on top);
equal-z falls back to insertion order. Default 0.0.

Works for **both** tiers: lightweight items re-sort within their
band on the next paint, and heavyweight widget entries restack the
arena children on the next rebuild (the SceneView reorders
`node.children` by z without recreating the widgets, so focus /
text-edit / animation state survives the restack). No-op for
unknown ids.

# The no-op test is exact

A write is ignored only when the entry already holds **that** value.
The guard used to be an absolute `|old - z| < f32::EPSILON`, which is
the wrong metric in both directions: `f32::EPSILON` is the spacing of
the representable numbers at 1.0, so out at `z = 1e6` (where the real
spacing is ~0.06) no two distinct floats are ever within it and the
guard never fires, while near zero (where the spacing is ~1e-45) it
swallows millions of distinct values.

That second half was not theoretical: `z_between`
bisects **relatively** and existed precisely so a caller is never told
"there is room" and then gets a silent no-op — and near zero the two
disagreed, so `z_between` returned `Some(6.25e-8)` and `set_z` dropped
it, emitting no `ItemChange::ZChanged` and leaving the order wrong
with nothing to observe. One metric now, and it is the one `z_between`
already used: two `z`s are the same iff they are the same float.

#### `pub fn bring_to_front(&mut self, id: ItemId)`

Raise an entry above all current entries by giving it a z one
greater than the current maximum. The drag-to-front primitive —
call it on drag-start so the grabbed card (and its text) renders
over the others. Works for both tiers (see `set_z`).

#### `pub fn send_to_back(&mut self, id: ItemId)`

Lower an entry below all current entries by giving it a z one less
than the current minimum. Works for both tiers (see
`set_z`).

#### `pub fn z(&self, id: ItemId) -> Option<f32>`

Read an entry's z-order.

#### `pub fn set_layer(&mut self, id: ItemId, layer: SceneLayer)`

Set the paint band for a lightweight entry — see `SceneLayer` for
what each one means and what `Interleaved`
costs. Within a band, `set_z` still orders items among
themselves. No-op for unknown ids.

#### `pub fn layer(&self, id: ItemId) -> Option<SceneLayer>`

Read an entry's paint band. `None` for unknown ids.

#### `pub fn set_item_parent(&mut self, child: ItemId, parent: Option<ItemId>)`

Declare a parent/child relationship. `child`'s `local_pos`
and `transform` are reinterpreted as relative to the new
parent's local frame — the visual position changes unless
the caller compensates. Re-buckets `child`'s subtree.

Pass `parent = None` to detach (child's local frame becomes
scene-rooted again).

**Cycle guard:** if the proposed parent is `child` itself
or a descendant of `child`, the call is a no-op (no parent
change, no rebucket, no signal fire). Without this guard
the downstream `rebucket_subtree` walk loops indefinitely.

#### `pub fn parent_of(&self, id: ItemId) -> Option<ItemId>`

Parent of `id`, if any.

#### `pub fn is_descendant_of(&self, id: ItemId, ancestor: ItemId) -> bool`

Whether `id`'s ancestor chain contains `ancestor`.

#### `pub fn collect_descendants(&self, id: ItemId, out: &mut Vec<ItemId>)`

Append every direct + transitive descendant of `id` into `out`, each
parent before its own children. The id itself is **not** included.

Costs the subtree, not the scene: the walk steps through the kept
`SceneEntry::children` adjacency rather than rescanning every entry per
visited node. A cycle in the parent graph is bounded by the visited set
rather than looping forever.

#### `pub fn set_geometry_constraint( &mut self, f: impl Fn(&crate::constrain::ProposedChange<'_>) -> crate::constrain::ChangeVerdict + 'static, )`

Install the document's standing geometry rule — snap-to-grid, axis lock,
page-bounds clamp — consulted before a **user-driven** gesture applies
anything.

Replaces any previous constraint; there is one per scene, because a
document has one geometry. See `ProposedChange`
for what the closure is handed, and
`SceneModel::set_geometry_constraint`
for the door an app normally uses.

```
use teksilo_canvas::{Point, Rect};
use teksilo_scene::{ChangeVerdict, RectItem, Scene};

let mut scene = Scene::new();
scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)), Point::ZERO);
// A 25-unit grid, snapping the moved box's own top-leading corner.
scene.set_geometry_constraint(|c| {
    let mut f = c.proposed;
    f.rect.x = (f.rect.x / 25.0).round() * 25.0;
    f.rect.y = (f.rect.y / 25.0).round() * 25.0;
    ChangeVerdict::Adjust(f)
});
assert!(scene.has_geometry_constraint());
```

#### `pub fn clear_geometry_constraint(&mut self)`

Remove the geometry constraint. Gestures then apply their raw proposal.

#### `pub fn has_geometry_constraint(&self) -> bool`

Whether a geometry constraint is installed.

#### `pub fn in_geometry_constraint(&self) -> bool`

Whether a geometry constraint is running **right now** on this thread.

The probe `SceneModel::write_guard`
uses to turn "a constraint tried to write the scene" into a diagnostic
that names the hook.

#### `pub fn selection_roots(&self, ids: &[ItemId]) -> Vec<ItemId>`

`ids` pruned to its **roots**: any item whose ancestor is also in `ids`
is dropped.

Moving an ancestor already moves its descendants — their `local_pos` is
parent-relative and is not touched — so transforming both would apply
the change twice. This is the mutation-side twin of the filter the paint
preview already runs over the same set.

Order is preserved. **O(n·depth)**: the set is hashed once and each id
then walks its own ancestor chain, asking the set rather than asking
every other id whether it is an ancestor. The hop cap is the one
`is_descendant_of` uses, so a malformed parent
cycle bounds rather than hangs, and an id that is its own ancestor is
kept (a cycle has no root to prefer).

The complexity is load-bearing, not incidental: every selection-transform
recompute runs this once (plus one
`transformable_roots` per operation), and so
does the item-drag group and the keyboard nudge — so a quadratic form
here is a frozen window on a large selection rather than a slow one.
`selection_roots_is_linear_in_the_selection`, in
`tests/selection_roots_scaling_probe.rs`, pins it: 7.03 ms against
36.3 µs for a 1 000-item selection, measured.

#### `pub fn transformable_roots( &self, ids: &[ItemId], op: crate::transform_session::TransformOp, ) -> Vec<ItemId>`

The roots of `ids` that may take part in `op`.

Three filters, in this order. The item must carry the operation's flag
(`TransformOp::required_flag`).
For `TransformOp::Rotate` it must be a
lightweight entry, because a heavyweight card is sized from the AABB of
its transformed bounds and a rotation there inflates its layout box
without turning anything. For
`TransformOp::Resize` it must own at least
one axis of its own box: an entry on `SizePolicy::Intrinsic` has
handed **both** to its widget, so every number a resize could write is
one the next layout pass measures back over — a handle that produces a
reversible step and no visible change is worse than no handle.
`SizePolicy::HeightForWidth` still owns its width and is offered the
full set; the height half of a corner drag is neutralised at the model
door, in `apply_transform_delta`.

The flag is checked **before** the descendant pruning, so a selected
child of a selected-but-locked parent still takes part on its own.

#### `pub fn scene_rotation(&self, id: ItemId) -> Option<f32>`

The item's own rotation in scene space, in radians — the angle of its
composed `local → scene` basis. `None` for an unknown id.

#### `pub fn transform_frame( &self, roots: &[ItemId], ) -> Option<crate::transform_session::TransformFrame>`

The selection frame enclosing `roots` — the box a transform controller
draws its handles on. `None` when `roots` is empty or none of them
resolve.

A **single** root's frame takes that item's own rotation, so resizing a
rotated item happens along its own axes and is exact. A multi-item
frame is axis-aligned, because the union of differently-rotated boxes
has no well-defined angle — Konva does the same.

#### `pub fn apply_transform_delta( &mut self, roots: &[ItemId], delta: &crate::transform_session::TransformDelta, ) -> usize`

Apply `delta` to `roots` as a **single** operation, and the only place a
transform controller writes the model.

Returns how many entry fields actually changed.

Three writes per root at most, and each is the existing setter, so the
change stream, the spatial index and every observer see nothing new:

* **position** — the item's scene anchor is mapped through the delta and
  restated in its parent's frame, so a scale about a pivot moves it and a
  rotation orbits it.
* **extent** — `local_bounds` is scaled, which makes the item *reflow*
  into the new box rather than be drawn at a stretched scale. The scale
  is resolved along the item's **own** axes, so a rotated item stays a
  rotated rectangle instead of shearing into a parallelogram; where the
  scale is non-uniform and the item is rotated relative to the frame,
  that is an approximation of the true (unrepresentable) result, and it
  is exact whenever the item is aligned with the frame — which includes
  every single-item selection. **An axis the entry's
  `SizePolicy` hands to its widget is not scaled**, and is not scaled
  for the anchor either, so a card whose height its content owns neither
  resizes nor *moves* when someone drags its top edge; see the comment at
  the loop head.
* **orientation** — the item's own `Transform2D` is post-rotated. Skipped
  for a heavyweight entry, whose layout box is the AABB of its
  transformed bounds: a rotation there would inflate the box and turn
  nothing.

The transaction boundary is this call. One gesture is one call, so an
app-level reversible-edit layer has exactly one thing to record — and
this crate ships no history of its own.

#### `pub fn item(&self, id: ItemId) -> Option<&dyn SceneItem>`

Borrow a lightweight `SceneItem` by id. `None` for unknown
ids and for heavyweight widget entries.

#### `pub fn paint_key(&self, id: ItemId) -> Option<PaintKey>`

Where `id` sits in this scene's single paint order — the value every
picker in the crate compares. `None` for unknown ids.

Defined for **both tiers**: a lightweight entry's rank is its
`SceneLayer` band, and a heavyweight widget entry's is
`RANK_WIDGET` — which is exactly where the
arena's child walk paints it, between the `Under` and `Over` bands. An
`Interleaved` item shares that rank, so it
sorts against the cards by `z`. See `PaintKey` for the ordering and
for the equal-`z` tie-break.

#### `pub fn remove(&mut self, id: ItemId)`

Remove an item by id, recursively dropping every descendant.

Mirrors Qt's `QGraphicsScene::removeItem` semantics: deleting
a parent deletes its children too. No-op if `id` is unknown.
Fires one `ItemChange::Removed` per id, descendants first
then the named parent — observers see a consistent
"leaves-then-root" order.

To remove `id` without deleting its children, call
`Scene::orphan` first to promote them to root-level, then
`remove(id)`.

# What happens to what it destroyed

This is `Scene::take` with the salvage routed to the edit sink
instead of to the caller — identical work, identical events. With a
sink installed
(`SceneModel::set_edit_sink`), each
removed entry arrives as a
`SceneEdit::Removed` carrying its
`RemovedItem`, which `Scene::restore` puts back. With none, the
salvage is dropped, which is what a removal has always done.

#### `pub fn take(&mut self, id: ItemId) -> Vec<RemovedItem>`

#### `pub fn restore(&mut self, salvage: RemovedItem) -> Result<ItemId, RestoreError>`

Re-insert a salvaged entry **at its original `ItemId`**, with its
magnets and its logical-AT decorations.

# Identity is the point

`SceneSelection` is keyed by `ItemId`,
`MagnetId → ItemId` is keyed by it, the whole logical AT tree is keyed
by `A11yNode::Item(ItemId)`, and — because a `SceneItem` has no
downcast — an app's own side map is the *only* way to reach item-specific
state, so it is keyed by it too. A restore that minted a fresh id would
restore the pixels and lose every one of them.

# Order

Roots before children: a salvage naming a parent that is not in the
scene fails with `RestoreError::MissingParent`.
`Scene::restore_all` orders a whole `Scene::take` result for you.

# What comes back, and what does not

Geometry, transform, z, layer, parent link, flags, opacity, handlers,
magnets (ids included) and every logical-AT decoration whose other
endpoint is still alive. The entry returns to its recorded position in
declaration order when that index still exists, so the accessibility
reading order is preserved rather than the item being appended last.

An edge to a node that has since been removed is **not** re-attached:
re-inserting it would name something the AT walker cannot resolve. Edges
are therefore attached **after** the entry is in, and — under
`restore_all` — after the whole batch is in, so an
edge between two items of one removed subtree is not dropped merely
because its far end had not arrived yet.

A single-view heavyweight widget (`Scene::add_widget`) whose one
`Box<dyn Widget>` a view already drained restores as an entry with no
instance to materialise — unless the take and the restore happen inside
one build cycle, in which case the view's orphan reap never runs and the
arena instance survives. The restore is accepted either way, because
refusing it would refuse the restores that work; see
`RemovedItem::widget_instance_present`. Content that must survive an
undo in every view goes in through
`SceneModel::add_widget_item`.

# Restoring into a *different* scene is supported

A `RemovedItem` taken from one scene may be restored into another,
and that is the move-between-documents door: cut a card out of one
board with `take`, restore it into a second, and it
arrives whole — item box, geometry, flags, handlers and magnets, at the
same `ItemId`.

Nothing special makes it work and nothing needs to refuse it.
`ItemId` is minted from a process-global counter, so an id from
another scene can never collide with one here; a salvage carries its
whole entry rather than an index into the scene it came from; and the
three consequences of crossing the boundary are exactly the rules that
already apply within one scene:

* **A parented salvage is refused** with
  `RestoreError::MissingParent`, because the parent it names is still
  in the other scene. Move the parent first and the child follows — the
  same roots-before-children rule, doing the same job — or call
  `RemovedItem::detach` to bring the item over as a root.
* **Logical-AT edges whose far end is not here are dropped**, by the
  same test that drops an edge to a since-removed node.
* **Declaration order is clamped**: the recorded index is where the
  entry sat in the *source* scene's reading order, and a shorter target
  takes it at the end.

The salvage is consumed either way, so a refused move does not leave a
second copy behind — but it does drop the item, so check the `Result`.

#### `pub fn restore_all(&mut self, salvage: Vec<RemovedItem>) -> Result<Vec<ItemId>, RestoreError>`

Restore a whole `Scene::take` result, roots first.

`take` hands back leaves-then-root; restoring in that order would fail
the first child on `RestoreError::MissingParent`, so this reverses it
for you — which also puts each parent's `children` list back in its
original order.

Logical-AT edges are attached once the **whole batch** is in, so an edge
between two items of the removed subtree survives whatever order the two
ends land in.

Stops at the first failure and returns it; everything restored before it
**stays restored**, because a rollback here would be the framework
applying an inverse, which is the data layer's job.

#### `pub fn replace_item( &mut self, id: ItemId, item: Box<dyn SceneItem>, ) -> Result<Box<dyn SceneItem>, ReplaceRejected>`

Swap the lightweight item box at `id`, keeping the entry — and the
`ItemId`. Returns the box that was there.

Position, transform, z, layer, parent, flags, opacity, handlers, magnets
and logical-AT decorations all survive, so a data row whose *content*
changed does not lose its selection membership, its connections or its
place in the accessibility tree. Emits one
`ItemChange::ItemReplaced`, not `Removed` + `Added`.

# Bounds and the spatial index

The entry's AABB is **derived from the item**, so the new item's
`local_bounds` is read and the subtree re-bucketed. Skipping that would
leave the old rectangle in the index and quietly break
`items_in_rect`, `item_at`, the
cull path and every cached hit snapshot.

# Flags

The **entry's** flags are kept; the replacement's
`initial_flags` are not consulted. They are
*initial* flags — they apply where an item is inserted — and an app that
has since called `set_visible` or
`set_flag` would otherwise have that silently undone
by a content refresh. Call `set_flags` afterwards to
adopt the new item's instead.

# Errors

`ReplaceItemError::UnknownItem` for an id that is not in the scene,
`ReplaceItemError::NotLightweight` for a heavyweight widget entry —
swap a `Delegated` entry's *data* with
`SceneModel::set_payload` instead.

Either way the `ReplaceRejected` hands the item **back**: a refused
write must not eat a value the caller cannot clone and may have paid to
build.

#### `pub fn placement(&self, id: ItemId) -> Option<Placement>`

Where `id` sits, as one value. `None` for an unknown id.

#### `pub fn set_placement(&mut self, id: ItemId, placement: Placement)`

Write parent, z, position and transform **together**, emitting one
`ItemChange::PlacementChanged`.

The atomic alternative to four mutators and four events. No-op when the
placement is unchanged, when `id` is unknown, and — as with
`set_item_parent` — when the proposed parent is
`id` itself or one of its own descendants, which would make a cycle.
The whole write is refused in that last case rather than applied with
the old parent, because a partly-applied atomic write is the thing this
door exists to prevent.

#### `pub fn reparent_keeping_scene_pos(&mut self, id: ItemId, parent: Option<ItemId>)`

Reparent `id` while holding it **visually still** — "drag this card into
that group", correctly.

`set_item_parent` reinterprets `local_pos` and
`transform` in the new parent's frame, so the item jumps unless the
caller compensates. This derives the local frame that leaves the item's
scene transform where it is, and applies parent and frame as **one**
`set_placement` write.

The derived frame is normalised: the rotation/scale go in `transform`
(around the local origin, which is what that field means) and all of the
translation in `local_pos`. An item whose `transform` carried a
translation of its own comes back with the same visual result and that
translation folded into its position.

No-op for an unknown id, for a parent that is already the current one,
for a cycle, and for a new parent whose scene transform is degenerate
(a zero scale somewhere in its chain) — there is no frame under it to
land in.

#### `pub fn z_between(&self, below: ItemId, above: ItemId) -> Option<f32>`

A `z` strictly between two items' — the fractional insert that puts one
item between two others without renumbering a single sibling.

`None` when there is no such value: the two are at the same `z`, either
id is unknown, or `f32` precision is exhausted at that locus. An `f32`
carries ~24 mantissa bits, so about two dozen bisections at one point in
the order run out — reachable in any card-shuffling UI.

Asking first is what makes that visible. A caller that computed an
exhausted midpoint itself would get `mid == lo`, hand it to
`set_z`, and be given a **silent no-op**: the card simply
would not move, with no error and no event. The answer to `None` is to
renumber the band (a pass of evenly-spaced `z` values) and bisect again.

The two agree on one metric, and it is this one. `set_z` ignores a write
only when the entry already holds that exact float, so **every `Some`
this returns is a value `set_z` will apply** — which is the whole
contract. (An absolute epsilon there used to break it near zero: with
`lo = 0.0` and `hi = 1.25e-7` this answers `Some(6.25e-8)` and the write
was dropped.)

```
use teksilo_canvas::{Point, Rect};
use teksilo_scene::{RectItem, Scene};

let mut scene = Scene::new();
let r = || RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0));
let lower = scene.add_item(r(), Point::ZERO);
let upper = scene.add_item(r(), Point::ZERO);
scene.set_z(lower, 1.0);
scene.set_z(upper, 2.0);

let middle = scene.add_item(r(), Point::ZERO);
let z = scene.z_between(lower, upper).expect("room between 1.0 and 2.0");
scene.set_z(middle, z);
assert!(scene.z(middle).unwrap() > 1.0 && scene.z(middle).unwrap() < 2.0);

// No room between an item and itself.
assert_eq!(scene.z_between(lower, lower), None);
```

#### `pub fn orphan(&mut self, id: ItemId)`

Promote `id`'s direct children to root-level (clear their
`parent` field). Used when an app wants to remove `id` without
dropping its children — call `orphan(id)` then `remove(id)`.
No-op when `id` is unknown or has no children.

Fires one `ItemChange::ParentChanged` per detached child and
re-buckets every detached subtree in the spatial index — the
children's `scene_transform` shifts (no longer composes
`id`'s) so their scene-space AABBs change. Without re-bucketing
the index, `items_in_rect` and
`item_at` would return stale results.

Apps wanting *visual* stability across the orphan call should
first bake `id`'s `scene_transform` into each child's
`local_pos` + `transform`; otherwise children visibly jump.

#### `pub fn items_in_rect(&self, scene_rect: Rect) -> Vec<ItemId>`

All items whose scene-AABB intersects `scene_rect`.

Broad phase: the spatial index returns every id bucketed in
any cell touched by `scene_rect`. Narrow phase: each candidate
goes through `scene_rect`, which itself
dispatches via `entry_index` (an `HashMap<ItemId, usize>`),
so the per-candidate cost is O(parent-chain-depth) — not
O(N). Total query is O(visible × chain) instead of O(N).

#### `pub fn item_thumbnails(&self) -> Vec<(Rect, teksilo_tokens::Color)>`

Snapshot every visible item — **both tiers** — as a `(scene_rect,
color)` pair suitable for a minimap thumbnail. Filters out items with
`HAS_NO_CONTENTS` (logical-only) and items hidden by `IS_VISIBLE` / a
hidden ancestor — the visible-effective set matches what the SceneView's
paint walk renders.

Ordered by insertion (low z first). A lightweight item's color comes
from `SceneItem::thumbnail_color` (its fill / stroke / a neutral grey);
a heavyweight widget entry has no `SceneItem`, so it's shown in a neutral
tint — a minimap that omitted the heavyweight tier would misrepresent a
widget-heavy scene (cards, nodes), so both tiers are included.

#### `pub fn item_shape(&self, id: ItemId) -> Option<ItemShape>`

The shape of any entry, in its **local** coordinates.

For a lightweight item this is `SceneItem::shape`. A **heavyweight**
widget entry has no `SceneItem` at all — `Scene::item` returns `None`
for it — so its shape is defined to be
`ItemShape::bounds` of its `local_bounds`. That is not a placeholder:
a widget's silhouette is its layout box, the arena hit-tests it as one,
and the marquee has always selected heavyweight entries through the
same index query as lightweight ones. Stating it here is what keeps a
`…ItemShape` selection mode meaningful for a scene whose primary
objects are cards.

`None` for an unknown id.

#### `pub fn item_contains(&self, id: ItemId, scene_pt: Point, view_scale: f32) -> bool`

Whether `id`'s shape contains `scene_pt`.

Works for both tiers (see `Scene::item_shape`). `view_scale` is the
live view zoom, consulted only by a cosmetic stroke band; pass `1.0`
when there is no view.

#### `pub fn item_region(&self, id: ItemId) -> Option<SceneRegion>`

The item's own shape re-published as a **scene**-space region — what a
collision query asks the rest of the scene about. `None` for an unknown
id, a shape of `ItemShape::none`, or a screen-anchored
(`IGNORES_TRANSFORMATIONS`)
item, whose silhouette is not in scene space at all.

#### `pub fn items_in_region( &self, region: &SceneRegion, mode: ItemSelectionMode, view_scale: f32, ) -> Vec<ItemId>`

Items matching `region` under `mode` — **both tiers**, exactly like
`Scene::items_in_rect`, which this generalises.

Broad-phased by the spatial index on `region.bounding_rect()`, then
narrow-phased per `mode`: the `…ItemBoundingRect` modes compare the
item's **scene** AABB against the region in scene space; the
`…ItemShape` modes map the region into the item's **local** frame and
compare it against `Scene::item_shape`. For an item with a
non-identity transform those are different tests — see
`ItemSelectionMode`.

`view_scale` is the live view zoom, consulted only by a cosmetic stroke
band; pass `1.0` when there is no view. Visibility and selectability
are **not** filtered here — this is a pure geometry query, and the
caller (e.g. `SceneSelection::commit_marquee`)
applies its own flag policy.

Items flagged
`IGNORES_TRANSFORMATIONS`
**are skipped**, for the reason `Scene::item_at` skips them: they are
anchored in screen space, so their `local_bounds` is a screen rectangle
and the scene AABB the index holds for them is a fiction. Comparing a
scene-space region against that fiction is a guess, and the two query
families used to disagree about whether to make it — the point queries
declined and the marquee did not. Reaching screen-pinned chrome needs a
region that arrives in *screen* space, which is what
`Scene::item_at_in_view` does for a point; there is no region twin of
it yet, so a marquee cannot select pinned chrome at all.

#### `pub fn item_at(&self, scene_pt: Point) -> Option<ItemId>`

Topmost **lightweight** item whose shape contains `scene_pt`, at unit
view scale. See `Scene::item_at_scaled` for the zoom-aware form and
`Scene::item_at_in_view` for the one that also places screen-anchored
items.

Heavyweight widget entries are skipped: their hit-testing is the
arena's job, and a scene-space answer would contradict it.

Items flagged
`IGNORES_TRANSFORMATIONS`
are **also skipped**, deterministically. They are anchored in screen
space, so a scene-space point cannot place them at all; answering with
a coin-flip (which is what comparing them against a scene AABB amounts
to) is worse than not answering. Use `Scene::item_at_in_view`, or
`SceneView` dispatch, when the query needs them.

"Topmost" is `Scene::paint_key` order, so an
`Over`-band item beats a higher-`z`
`Under` one, and two equal-`z` items resolve to the
later-inserted one — the same answer `SceneView` dispatch gives.

Hidden and disabled entries are excluded: this is a *hit* test, and
`ItemFlags::IS_VISIBLE` and `ItemFlags::IS_ENABLED` both say so. See
`Scene::is_hit_testable`. For a pure geometry query that ignores flags,
use `Scene::items_in_region` or `Scene::item_contains`.

#### `pub fn item_at_scaled(&self, scene_pt: Point, view_scale: f32) -> Option<ItemId>`

`Scene::item_at` at an explicit view zoom.

The zoom reaches exactly one thing: a **cosmetic** stroke band, whose
width is in device pixels and therefore covers fewer scene units the
further you zoom in. Passing the live scale is what makes this agree
with `SceneView`'s own dispatch, which has always had it.

#### `pub fn entry_at(&self, scene_pt: Point, view_scale: f32) -> Option<ItemId>`

Topmost entry of **either** tier whose geometry contains `scene_pt`, in
this scene's one paint order.

The deliberate counterpart to `Scene::item_at_scaled`, which skips
heavyweight entries because the arena owns their hit-testing. This one
answers the different question a *selection* asks — "which entry is on
top here?" — and answers it for a card with the card's scene rectangle,
which is exactly the rectangle `SceneView` laid the card out at. It is
not a substitute for arena dispatch and must not be used to route an
event to a widget; it is for deciding whether a press that already
reached the view landed on something the view is holding.

Screen-anchored entries are skipped for the same reason `item_at` skips
them: a scene-space point cannot place them.

#### `pub fn items_at(&self, scene_pt: Point) -> Vec<ItemId>`

All lightweight items whose shape contains `scene_pt`, topmost-first by
z. Same tier and `IGNORES_TRANSFORMATIONS` rules as `Scene::item_at`.

#### `pub fn items_at_scaled(&self, scene_pt: Point, view_scale: f32) -> Vec<ItemId>`

`Scene::items_at` at an explicit view zoom.

#### `pub fn item_at_in_view(&self, screen_pt: Point, view_transform: Transform2D) -> Option<ItemId>`

Topmost lightweight item under a **screen** point, resolving *both*
hit spaces the way `SceneView` dispatch does.

A normal item is tested in scene space, at the view transform's zoom; a
screen-anchored
(`IGNORES_TRANSFORMATIONS`)
item is tested against its `local_bounds` rooted at its projected
anchor, at unit scale — its local coordinates *are* screen
coordinates, so it has no zoom to convert. This is the query to use
when a scene may contain screen-pinned chrome; `Scene::item_at`
deliberately declines to guess.

#### `pub fn is_hit_testable(&self, id: ItemId) -> bool`

Whether `id` takes part in pointer hit-testing — visible along its whole
ancestor chain AND enabled.

This is `crate::pick::hit_testable` resolved against the scene, and it
is the one place the two flag contracts in `ItemFlags` are honoured:
`IS_VISIBLE` ("neither painted nor hit-tested")
and `IS_ENABLED` ("pass clicks through to items
beneath"). `false` for unknown ids.

#### `pub fn colliding_items(&self, id: ItemId) -> Vec<ItemId>`

Items overlapping `id`'s **shape**, excluding `id` itself.

Apps use this for "which other items overlap this card?" — graph
editors checking node-on-node overlap, CAD canvases finding adjacent
geometry. Backed by the spatial index, so the cost is `O(visible)` not
`O(N)`.

Defaults to `ItemSelectionMode::IntersectsItemShape`, so a
stroke-only connector collides along its line rather than across its
bounding box. Pass
`IntersectsItemBoundingRect`
to `Scene::colliding_items_with` for the cheaper box test.

Screen-anchored items neither collide nor are collided with — see
`Scene::items_in_region`, which this is built on, and
`Scene::item_region`, which declines to publish one for them.

#### `pub fn colliding_items_with(&self, id: ItemId, mode: ItemSelectionMode) -> Vec<ItemId>`

`Scene::colliding_items` under an explicit `ItemSelectionMode`.

#### `pub fn items_along_path(&self, path: &Path) -> Vec<ItemId>`

Items lying along `path` — a real region query, not the AABB-of-the-path
approximation this used to be.

The path is treated as a zero-width closed region: an item is picked
when the path crosses it or encloses it. For a *connector* — a line
with a width — pass that width to `Scene::items_along_path_with`, so
the query asks about the band the user can see.

#### `pub fn items_along_path_with( &self, path: &Path, stroke_width: f32, mode: ItemSelectionMode, ) -> Vec<ItemId>`

`Scene::items_along_path` with an explicit stroke width and
`ItemSelectionMode`.

`stroke_width` greater than zero makes the region the path's **band**
rather than its interior: "what does this 4 dp connector touch?".

#### `pub fn len(&self) -> usize`

Number of entries in the scene.

#### `pub fn is_empty(&self) -> bool`

Whether the scene is empty.

#### `pub fn ids(&self) -> Vec<ItemId>`

All ids in insertion order.

#### `pub fn index(&self) -> &dyn SpatialIndex`

Borrow the spatial index (diagnostics / tests).

#### `pub fn add_magnet(&mut self, item: ItemId, magnet: Magnet) -> MagnetId`

Attach a `Magnet` to `item` and return its `MagnetId`.

Magnets are local to their item (their `local_pos` is in the
item's frame), so they follow the item under any move / rotate /
scale via the same `scene_transform` the item uses. No-op
returning a fresh-but-unowned id if `item` is unknown — callers
add magnets to items they just created.

Bumps the AT-structure change counter (magnets are AT structure)
so a `SceneView` with magnetism enabled re-walks its synthetic
magnet nodes.

#### `pub fn remove_magnet(&mut self, magnet: MagnetId)`

Remove a magnet by id. No-op if the id is unknown.

#### `pub fn clear_magnets(&mut self, item: ItemId)`

Remove every magnet attached to `item`. No-op if none.

#### `pub fn set_magnet_local_pos(&mut self, magnet: MagnetId, local_pos: Point)`

Move a magnet to a new position in its owning item's local
frame. No-op if the id is unknown.

#### `pub fn set_magnet_enabled(&mut self, magnet: MagnetId, enabled: bool)`

Enable or disable a magnet. Disabled magnets are skipped by
broad-phase, feedback, the keyboard cycle, and AT emission.
No-op if the id is unknown.

#### `pub fn magnet_ids_of(&self, item: ItemId) -> Vec<MagnetId>`

The ids of every magnet attached to `item`, in insertion order
(enabled and disabled alike). Empty if `item` is unknown or has
no magnets.

#### `pub fn magnet_owner(&self, magnet: MagnetId) -> Option<ItemId>`

The owning item of a magnet, or `None` if the id is unknown.

#### `pub fn magnet_enabled(&self, magnet: MagnetId) -> bool`

Whether a magnet is enabled. `false` for an unknown id.

#### `pub fn magnet_scene_pos(&self, magnet: MagnetId) -> Option<Point>`

A magnet's position in scene coordinates (its local position
projected through its owning item's `scene_transform`). `None`
for an unknown id or a degenerate item transform.

#### `pub fn magnet(&self, magnet: MagnetId) -> Option<MagnetRef>`

Resolve a magnet to a borrow-free `MagnetRef` snapshot (id,
owning item, role, payload clone, current scene position).
`None` for an unknown id or a degenerate item transform.

#### `pub fn compute_item_snap( &self, dragged: ItemId, drag_delta: Vec2, capture_radius: f32, predicate: &dyn Fn(&MagnetRef, &MagnetRef) -> MagnetVerdict, ) -> Option<MagnetSnap>`

Compute the best item-drag snap: the dragged item is visually
offset by `drag_delta`, and each of its enabled magnets seeks the
nearest *accepting* magnet on another item within `capture_radius`
(in scene units). Returns the globally closest accepting pair, or
`None` if nothing accepts within range.

Pure mechanism: it collects candidates under a brief read, then
runs the consumer `predicate` with no scene borrow held, so the
predicate may inspect payloads freely. `snap_vector` added to
`drag_delta` aligns the dragged magnet onto its target.

#### `pub fn compute_port_snap( &self, source: MagnetId, cursor_scene: Point, capture_radius: f32, predicate: &dyn Fn(&MagnetRef, &MagnetRef) -> MagnetVerdict, ) -> Option<(MagnetRef, Option<Rc<dyn std::any::Any>>)>`

Compute the best port-drag snap: a single `source` magnet is
dragging a transient wire whose free end is at `cursor_scene`.
Finds the nearest *accepting* target magnet within
`capture_radius` (scene units), excluding the source's own
magnet. Returns the target `MagnetRef` and the accepting
verdict's payload, or `None`.

#### `pub fn nearest_magnet(&self, scene_pt: Point, radius: f32) -> Option<MagnetId>`

The nearest enabled magnet to `scene_pt` within `radius` (scene
units), or `None`. Used by the view to start a port-drag from a
grabbed magnet handle (the handle's grab area is a screen-pixel
disc, converted to scene units by the caller).

#### `pub fn add_a11y_group(&mut self, builder: A11yGroupBuilder) -> A11yGroupId`

Declare a virtual AT group. The group has no visual
counterpart — it exists so the AT walker can emit an AT node
under which items / other groups / widgets can be reparented.

#### `pub fn remove_a11y_group(&mut self, id: A11yGroupId)`

Remove a logical group; orphaned references fall back to
SceneView root. Relations / live / landmarks / categories
targeting this group are cleaned up too.

#### `pub fn a11y_group(&self, id: A11yGroupId) -> Option<&A11yGroup>`

Borrow a logical group by id.

#### `pub fn set_a11y_parent(&mut self, child: A11yNode, parent: Option<A11yNode>)`

Declare a logical-parent relationship for AT (independent of
visual placement).

#### `pub fn a11y_parent_of(&self, child: A11yNode) -> Option<A11yNode>`

The currently-declared logical parent of a node.

#### `pub fn add_a11y_relation(&mut self, from: A11yNode, kind: A11yRelation, to: A11yNode)`

Declare an AT relationship between two nodes.

#### `pub fn a11y_relations(&self) -> &[(A11yNode, A11yRelation, A11yNode)]`

All declared AT relations.

#### `pub fn set_a11y_live(&mut self, node: A11yNode, live: accesskit::Live)`

Mark a node as a live region. Pass `Live::Off` to clear.

#### `pub fn set_a11y_landmark(&mut self, node: A11yNode, role: accesskit::Role)`

Mark a node as a landmark by overriding its role. Pass
`Role::Unknown` to clear.

#### `pub fn set_a11y_categories(&mut self, node: A11yNode, categories: &[A11yCategory])`

Tag a node with rotor / quick-nav categories.

#### `pub fn a11y_categories_of(&self, node: A11yNode) -> Option<&[A11yCategory]>`

Read declared categories for a node.

#### `pub fn a11y_live_of(&self, node: A11yNode) -> Option<accesskit::Live>`

Read a node's declared live-region politeness. `None` when it is not a
live region.

The read half of `set_a11y_live`, added because a
consumer restoring a `RemovedItem` has to be able to check that the
semantics came back — and because the other four decorations already
had one.

#### `pub fn a11y_landmark_of(&self, node: A11yNode) -> Option<accesskit::Role>`

Read a node's declared landmark role. `None` when it is not a landmark.
