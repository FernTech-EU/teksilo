<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ProposedChange

The **geometry constraint**: one closure that rewrites a gesture's proposed
geometry *before* anything is applied.

Snap-to-grid, axis lock, page-bounds clamping and "this item may not leave
its lane" are all one hook. It is Qt's
`QGraphicsItem::itemChange(ItemPositionChange, value) -> value` in the shape
Rust's borrow rules allow: the scene is lent **read-only** and the decision
is *returned* rather than written.

# The quantity is a frame, not a pointer

A constraint is offered a `TransformFrame` — the scene-space box the
gesture's items occupy — and hands one back. That choice is the whole reason
the obvious snap-to-grid example is actually correct:

* A **pointer** position carries the grab offset. Snapping the cursor to a
  25 dp grid leaves an item grabbed 37 dp from its corner sitting 37 dp
  off-grid, on every drag, for ever.
* A **`local_pos`** is stated in the item's *parent's* frame, so a
  constraint written against a scene grid silently means something else for
  a parented item.
* A **frame** is the box the user can see, in scene coordinates, for one
  item or for a whole selection. Snapping `proposed.rect.x` snaps the thing
  the user is looking at.

# It runs on every route, and it cannot disagree with itself

There is deliberately **no phase parameter**. The constraint is a pure
function of the proposal, re-run from scratch on every sample including the
release one, so the frame the last preview drew *is* the frame the commit
writes — by construction, not by convention. A hook that could snap loosely
while dragging and hard on release would be a hook that guarantees a jump at
the release, which is the one failure this whole design exists to remove.

Four routes consult it, and they all consult the same closure:

| route | who runs it |
| --- | --- |
| the selection transform controller (move / resize / rotate; pointer, keyboard and AT) | `SceneView` |
| the lightweight item drag | `SceneView` |
| the `Alt`+arrow keyboard nudge | `SceneView` |
| an app's own drag — a heavyweight card's `on_drag` handler | the app, via `SceneModel::constrain_move` |

The fourth is why the constraint lives on the **model** rather than on a
view: a card's widget is built by a per-view delegate and holds a
`SceneModel` clone, never a `&SceneView`, so a
view-installed closure would be unreachable from the one tier the motivating
use cases (a corkboard of cards on a grid) actually live in. Geometry policy
is a property of the *document* — "this corkboard is on a 25 dp grid" — not
of a pane looking at it, and two panes onto one scene that snapped
differently would be a bug rather than a feature. Contrast
`SceneView::focus_order`,
`SceneView::drag_mode` and
`SceneView::magnetism`, which are per-view
because they say what *this pane* lets you do.

# A programmatic write is never constrained

`SceneModel::set_local_pos` and every
other mutator land **exactly** where they say. Snapping a document load, a
`SceneListAdapter` rebuild or a data-layer replay
is Qt's own best-known footgun with `itemChange`, and it is avoided here not
by where the closure is stored but by *who consults it*: the gesture paths
do, the mutators do not.

# What it costs

A scene with **no** constraint pays one `Option` test per gesture sample and
never builds a call. Measured on a 20 000-item scene, a pointer sample costs
the same with the mechanism present and absent — the difference is below the
run-to-run noise of the measurement.

A scene **with** one pays for the closure, and the closure is a *pure
function re-run per query* rather than a per-gesture callback: one sample
that also lays out, paints and re-walks the accessibility tree asks it
several times (six, at the time of writing). That count is bounded and
independent of both the scene's size and the selection's — pinned by
`the_constraint_is_asked_a_bounded_number_of_times_per_sample` — but it is
not one. Keep the closure cheap; a constraint that must run an expensive
spatial query should memoise on `proposed` inside its own capture.

Re-running rather than caching is deliberate. The answer has to be the same
for the chrome, the preview, the commit and the announcement, and the
cheapest way to guarantee that is for there to be nothing to invalidate.

# Testing a policy

A `ProposedChange` is built by the framework and is deliberately not
consumer-constructible. To exercise a policy closure without a widget tree,
drive it through the public door the app-owned drag uses — a bare
`SceneModel` is enough:

```
# use teksilo_canvas::{Point, Rect, Vec2};
# use teksilo_scene::{ChangeVerdict, RectItem, SceneModel, TransformSource};
let model = SceneModel::new();
let a = model.add_item(RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)), Point::ZERO);
model.set_geometry_constraint(|c| {
    let t = c.translation();
    ChangeVerdict::Adjust(c.translated(Vec2::new(t.x, 0.0)))   // horizontal only
});

let start = model.transform_frame(&[a]).expect("resolves");
let applied =
    model.constrain_move(&[a], start, Vec2::new(30.0, 12.0), TransformSource::Pointer);
assert_eq!(applied, Vec2::new(30.0, 0.0));
```

# What it may touch

It may **read** the scene through the `&Scene` it is handed — a shared
borrow is already open, and shared plus shared is legal. It may **not**
write: a write needs the cell exclusively. That is enforced rather than
documented — `SceneModel::write_guard`
checks the constraint flag and panics naming this hook and what to do
instead (return the verdict), so the failure reads as the bug it is rather
than as `RefCell already borrowed`.

# Do not capture a `SceneModel`

`ProposedChange::scene` is the *whole* read surface — every query
`SceneModel` offers is a shared borrow delegating to
the same `Scene`, so a captured handle buys a constraint nothing it is
allowed to do.

It costs something, though. The scene **owns** the closure
(`Rc<RefCell<Scene>>` → `Scene::geometry_constraint` → the closure), so a
closure holding a `SceneModel` clone closes that ring and nothing in it is
ever dropped: every item, every heavyweight payload, the journal and the
spatial index stay alive for the life of the process. It is the ordinary
`Rc` cycle, and it is invisible — the scene keeps working perfectly.

Two safe forms, in order of preference:

```
# use teksilo_canvas::{Point, Rect};
# use teksilo_scene::{ChangeVerdict, RectItem, SceneModel};
# let model = SceneModel::new();
# let lane = model.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);
// 1. Read through the scene you are handed. Nothing captured, nothing to leak.
model.set_geometry_constraint(move |c| match c.scene.scene_rect(lane) {
    Some(r) if r.contains(Point::new(c.proposed.rect.x, c.proposed.rect.y)) => {
        ChangeVerdict::Accept
    }
    _ => ChangeVerdict::Reject,
});

// 2. When a policy object genuinely holds a model for its *other* work,
//    capture the weak handle and upgrade inside the call.
let weak = model.downgrade();
model.set_geometry_constraint(move |c| match weak.upgrade() {
    Some(m) if m.len() > 1 => ChangeVerdict::Accept,
    _ => ChangeVerdict::Adjust(c.start),
});
```

`constraint_capturing_the_scene_strongly_leaks_it` (in
`tests/constraint_lifetime.rs`) pins both halves with a `Drop` sentinel.

# What it may hand back

An applicable frame: every field finite, neither extent negative. That is
not a formality. A one-character slip in the snap closure above —
`let grid = 0.0;` — makes `(x / grid).round() * grid` a `NaN`, and a `NaN`
frame applied verbatim gives the item a `NaN` position, an inverted
infinite AABB, and a permanent absence from hit-testing, from the marquee
and from every spatial query, with no panic to say so. So an
`ChangeVerdict::Adjust` whose frame cannot be applied is **refused**: the
sample behaves as `ChangeVerdict::Reject`, and in a debug build it panics
naming the offending frame rather than losing the item quietly.

## Builder methods at a glance

`translation`, `translated`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/constrain/index.html)

## `pub struct ProposedChange`

A change a gesture is about to make, offered to the geometry constraint.

Every field is a *question*, never a channel: the answer is the returned
`ChangeVerdict`.

```rust
pub struct ProposedChange<'a> { /* fields */ }
```

### Methods

#### `pub fn translation(&self) -> Vec2`

The scene-space translation from `start` to
`proposed`.

The quantity an axis lock or a grid snap works in. Exact for a
`TransformOp::Move`; for a resize or a rotate it is the movement of
the frame's origin, and the extent change is in the two frames
themselves.

#### `pub fn translated(&self, translation: Vec2) -> TransformFrame`

`start` moved by a scene-space translation — the frame to
hand back from `ChangeVerdict::Adjust` once a move has been rewritten.

The exact inverse of `translation`, so
`c.translated(c.translation())` is `c.proposed` for a move.

## `pub enum ChangeVerdict`

What a geometry constraint decides.

```rust
pub enum ChangeVerdict { /* variants */ }
```

### Variants

- **`Accept`** — Apply `ProposedChange::proposed` unchanged.
- **`Adjust`** — Apply this frame instead. The delta is **re-derived** from it, so the preview, the commit, the chrome and the announcement cannot disagree about what happened.
- **`Reject`** — Apply nothing: this sample leaves the items where the gesture found them.  Exactly equivalent to `Adjust(change.start)`, and deliberately so — a "freeze at the last accepted sample" rejection would need history, and the next accepted sample would then jump by everything the frozen ones travelled. Stateless refusal means a rejected gesture shows nothing happening and resumes the moment the proposal becomes acceptable, and a gesture that *ends* rejected commits nothing at all — its delta is the identity, so `TransformOutcome::Cancelled` is what `on_end` reports.
