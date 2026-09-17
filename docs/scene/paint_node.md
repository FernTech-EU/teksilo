<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# WetLayer

The two paint positions a `SceneView` cannot reach from its own `paint`.

The render walker gives a widget exactly two places to draw: before its
child subtree (`paint`) and after it (`post_paint`). Those are the
`Under` and `Over`
bands, and between them sits the whole heavyweight tier as one block. Two
things need a position *inside* that block:

- **`SceneLayer::Interleaved`** — a
  lightweight item ordered against the cards by `z`, so a stroke can be
  above note A and below note B. `SceneBandProxy` is the node that carries
  one, slotted into the child list by `PaintKey`.
- **A wet surface** — content being authored right now, which must repaint
  on its own without dragging the item bands through a pass with it.
  `WetLayer` is the handle; `WetLayerNode` is its node.

Both nodes are **paint only**, and that is what makes them affordable:

| | consequence |
|---|---|
| `event_pass_through` | the pointer resolves through the lightweight hit snapshot exactly as before — one picker, still |
| left out of `accessibility_children` | the AT tree is bit-identical; an interleaved item keeps the same synthetic node it had in `Under` |
| not focusable, no handlers | not a Tab stop, so `tab_stops_within` is unchanged |

# Why there is a bridge rather than a `&SceneView`

A child node cannot borrow its parent widget. Everything the per-item paint
loop reads is already a cloneable handle on the view — the model, the
item cache, the four camera signals, the drag cell, the transform driver —
so `ScenePaintBridge` is those handles in a bundle, and
`ScenePaintBridge::paint_items` is the loop, called by the view's own
`paint_band` **and** by these nodes. One implementation, so an interleaved
item cannot render differently from the same item in `Under`.

# Layer order

Stated here because it is otherwise implicit in three files:

1. `SceneView::paint` — the app background closure, then the `Under` band
2. the arena's child walk, sorted by [`PaintKey`]:
   `SceneBandProxy` nodes and heavyweight cards interleaved by `z`
3. `WetLayerNode` — always the last child, so wet content sits above
   every card and every interleaved item
4. `SceneView::post_paint` — the `Over` band, the selection marquee, the app
   foreground closure, magnetism feedback, the transform chrome, the debug
   overlay

So a wet stroke is drawn over the content it is being drawn *on*, and under
the view's own chrome — a marquee, a magnet ghost and the debug overlay stay
visible through it. An app whose dried ink lives in the `Over` band will see
the stroke rise one step at the moment it dries; `Interleaved` (or `Under`)
has no such step, which is the other reason ink belongs in a band of its
own.

## Builder methods at a glance

`request_repaint`, `nodes`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/view/index.html)

## `pub struct WetLayer`

A surface for content being authored **right now**, which repaints without
taking the scene's item bands with it.

The case is ink. A stroke in flight changes on every pointer sample, and
there is no way to invalidate a `SceneView`'s foreground alone: the render
walker computes **one** `needs_paint` per node and gates that node's `paint`
and its `post_paint` with it, so asking the view to repaint re-runs the
whole `Under` band as well. A wet layer is its own node, so
`request_repaint` marks that node and nothing else.

The painter draws in **scene** coordinates — the node lives inside the
view's content transform, so a wet stroke pans and zooms with the page for
free, and a scene point can be drawn at its own value.

Where it lands in the paint order, and why: above every card and every
`Interleaved` item, under the `Over` band
and the view's own chrome (marquee, magnet feedback, transform frame, debug
overlay). `docs/teksilo-scene.md` states the whole order.

```no_run
# use std::cell::RefCell;
# use std::rc::Rc;
# use teksilo_scene::{Scene, SceneView, WetLayer};
# use teksilo_canvas::Point;
# use teksilo_tokens::Color;
let points: Rc<RefCell<Vec<Point>>> = Rc::default();
let wet = {
    let points = points.clone();
    WetLayer::new(move |canvas, _ctx| {
        for p in points.borrow().iter() {
            canvas.fill_circle(*p, 2.0, Color::BLACK);
        }
    })
};
let view = SceneView::new(Scene::new()).wet_layer(wet.clone());
// …and from a pointer handler, after pushing a point:
// wet.request_repaint(ctx);
```

A cloneable handle: clone it into the pointer handler that feeds it and into
`SceneView::wet_layer`, the way a `SceneModel` is cloned into its views.

# More than one view

The comparison with `SceneModel` is meant literally. Mount the same layer
in two views — an overview beside a detail pane — and **both** paint it, and
`request_repaint` repaints both. A second view does
not displace the first.

One thing does not carry across, and it is a property of the door rather
than of this type: a repaint is requested through an
`EventContext`, and a context belongs to one
window's tree. So `request_repaint` repaints the mounts **in the calling
context's own window** and leaves a mount in another window to that window's
own frame — an id from the other tree could not reach it anyway, because
ids are per-arena slot keys and two trees mint the same ones.

That last sentence is the reason `request_repaint` does not simply push the
ids it holds: it resolves each one in the calling tree through a typed door
first, so a stale id can only ever land on one of this layer's own nodes
there — or trip an assertion. The rule and its one remaining edge (the same
layer mounted in two **window-less** trees) are written out at
`request_repaint`.

```rust
pub struct WetLayer(Rc<WetLayerInner>);
```

### Methods

#### `pub fn new(painter: impl Fn(&mut Canvas, &SceneItemPaintContext<'_>) + 'static) -> Self`

A wet surface painted by `painter`, in **scene** coordinates.

The closure is called on every repaint of a node this layer is mounted
in, and on nothing else. It owns whatever the app is authoring — a
stroke's points, a rubber-band shape, a live measurement — typically
through an `Rc<RefCell<…>>` shared with the pointer handler that feeds
it.

#### `pub fn request_repaint(&self, ctx: &mut teksilo_core::EventContext<'_>)`

Repaint the wet surface, and only it.

No relayout, no rebuild, no accessibility re-walk — and, crucially, no
repaint of the scene's item bands: this marks nodes, one per view that
mounted the layer in `ctx`'s own window.

A no-op until the layer has been mounted through
`SceneView::wet_layer` and the view has built, which is the frame
before any handler can run.

# Why the id goes through a typed door

A `WidgetId` is a slot key in *some* arena, and every tree mints the
same ones. The window a mount was made in narrows the candidates — a
tree belongs to exactly one window — but a headless tree has no window,
so `None == None` is not a match, it is two unknowns comparing equal.
Marking on that alone is how a repaint from one test's tree lands on
whatever unrelated widget happens to hold that slot in another's.

So the mark is not pushed as a bare
`request_repaint`. It
goes through
`with_widget_mut`, which
resolves the id **in the calling tree** and hands the node over only if
it downcasts to this crate's own `WetLayerNode` — and the closure then
checks it is a node of *this* layer. Both checks fail loudly
(`debug_assert`) rather
than quietly, so a cross-tree repaint is a test failure instead of a
stray dirty bit.

What that buys, stated exactly:

- An id naming a slot the calling tree does not fill, or fills with a
  widget that has not opted into `Widget::as_any_mut` — which is every
  ordinary widget, the default being `None` — reaches **nothing**, in
  both profiles. That is the case this used to get wrong: the foreign id
  was marked, and whatever held the slot repainted.
- An id naming a `WetLayerNode` of *this* layer is marked, which is
  right: that node's own mount is in the same list, so it wanted
  repainting anyway.
- An id naming a node that *has* opted into `as_any_mut` and is not one
  of this layer's trips the assertion, so in a test or a dev build it is
  a panic. In release the closure is skipped and the node still takes the
  `RepaintOnly` mark, which costs one repaint of something already on
  screen — never a mutation, never a relayout, rebuild or accessibility
  change.

So this does **not** make one layer in two window-less trees work; it
makes the attempt fail where it can be seen. Drive each tree's repaint
itself, from `nodes`.

#### `pub fn nodes(&self) -> Vec<WetNode>`

The nodes this layer paints into — one per mounted view, in mount order,
each paired with the window whose tree holds it.

Exposed for a caller that wants to drive the repaint through some other
door than `request_repaint` — a
`WidgetTree::mark_needs_paint` in a test, say. Such a caller owns the
question `request_repaint` answers from its context: **which tree** a
given node belongs to, which is why this reports
`WetNode::window` beside the id rather than the id alone. With one
view (the overwhelmingly common case) that is the one tree there is.

## `pub struct WetNode`

One of a `WetLayer`'s mounted paint nodes, as
`WetLayer::nodes` reports it.

A bare `WidgetId` is a slot key in *some* arena and says which one it is
not: two trees mint the same ids, so a caller holding several trees cannot
tell whose node it has. This pairs the id with the window whose tree it
belongs to, which is the identity the framework has — `None` for a headless
tree, where the caller owns the question and the count is the warning.

```rust
pub struct WetNode { /* fields */ }
```

### Methods

#### `pub fn new(id: WidgetId, window: Option<teksilo_core::window::TeksiloWindowId>) -> Self`

A node report. The framework builds these; the constructor exists so a
consumer can build one in a test of its own.

#### `pub fn id(&self) -> WidgetId`

The paint node, in the arena of the window below.

#### `pub fn window(&self) -> Option<teksilo_core::window::TeksiloWindowId>`

The window whose tree holds `id`, or `None` for a tree that
has no window.
