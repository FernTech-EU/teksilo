<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Ink

Teksilo does not ship an ink tool. It ships the four things an app writing one
cannot work around, and this page is the other half: what the app should build,
what it will cost, and the one accessibility decision that decides whether a
page of ink helps a screen-reader user or drowns them.

Working example: `cargo run -p scene-ink`.

---

## 1. What the framework provides

| need | where |
| --- | --- |
| every sample the platform delivered | [`EventContext::coalesced`](../crates/teksilo-core/src/widget/event_context.rs), populated by [`PenBatching::Coalesce`](../crates/teksilo-platform/src/pen.rs) |
| a stroke between two notes | [`SceneLayer::Interleaved`](../crates/teksilo-scene/src/scene.rs) |
| a surface that repaints on its own | [`WetLayer`](../crates/teksilo-scene/src/view/paint_node.rs) |
| a growing path that is not quadratic | [`Path::stamp`](../crates/teksilo-canvas/src/path.rs) → an O(1) mask-cache key |

Everything else — the brush, the outline, erase, persistence, undo — is the
app's. Brush feel is policy, and undo belongs to the data layer.

---

## 2. Every sample, and the handler that loses them

A digitizer runs at 200–360 Hz and a window's message rate does not, so a
platform hands over batches. Two answers, and the trade is real:

```rust
TeksiloAppBuilder::new()
    .pen_batching(PenBatching::Coalesce)   // one dispatch per drain
```

- `PerPacket` (the default) spends a whole tree dispatch — hit test,
  arbitration turn, handler walk — on every packet. Nothing is coalesced.
- `Coalesce` spends one dispatch per drain and puts the intermediate positions
  in `PointerSample::coalesced`, each keeping **its own time and its own axes**.

Transitions are never folded: Down, Up, a button change and proximity
enter/leave each keep their own sample under either mode, so no recognizer sees
a different sequence — only the number of `PointerMove`s between two transitions
changes.

A tool reads both, in this order, and is then correct under either mode and on
every backend:

```rust
for c in ctx.coalesced() {          // oldest first, excluding the packet's own
    stroke.extend(to_scene(c.window_position), c.axes.pressure.unwrap_or(0.5));
}
if let Some(p) = ctx.pointer_position() {   // …then the newest
    stroke.extend(to_scene(p), ctx.pointer().effective_pressure());
}
```

Both are **window**-logical, like `Scroll::window_position` and for the same
reason: a batch has no single widget to localise against. Project with
`SceneView::view_transform_signal`.

> A dispatch that is not a sample — a gesture the timer recognised, a
> drag-and-drop tick, an assistive-technology action — reports an empty list.
> It batched nothing, and handing back whichever sample arrived last would
> attribute its positions to a gesture that did not produce them.

### Use `on_pointer_event`, not `on_drag`

`on_drag` is where a canvas tool would naturally live, and it is the wrong
place. `DragRecognizer` returns `Pending` for every move inside the drag slop
and then reports `DragStarted` at the **press** position, so the samples that
crossed the slop — the first few millimetres of the stroke, where its taper and
pressure ramp live — are never delivered at all. Pinned by
`the_drag_recognizer_swallows_the_start_of_a_stroke` in
`crates/teksilo-scene/tests/ink_layer.rs`.

---

## 3. Where the ink lives

A dried stroke is a lightweight `PathItem` in
[`SceneLayer::Interleaved`](teksilo-scene.md#across-the-tiers--the-three-bands),
at the ink layer's `z`:

```rust
let id = model.add_item(PathItem::new(outline(&points, &brush)), Point::ZERO);
model.set_layer(id, SceneLayer::Interleaved);
model.set_z(id, INK_Z);
```

`Under` puts ink below **every** note and `Over` above **every** note; neither
is the OneNote case. `Interleaved` shares the cards' `PaintKey` rank, so the
stroke and the notes order against each other by `z` — in paint and in hit
alike.

### What it orders against — and what it does not

**Cards.** `Interleaved` slots the item into the arena's z-sorted *child* walk,
and the children of a `SceneView` are its heavyweight widgets. So the notes in
the paragraph above have to be heavyweight — `add_widget` / `add_widget_item`,
real widgets in the arena — for any of this to mean anything.

It does **not** order the item against the other two bands. The whole `Under`
band is painted inside `SceneView::paint`, which runs before the first child;
the whole `Over` band inside `post_paint`, after the last. So an interleaved
item is above every `Under` item and below every `Over` item whatever the `z`s
say, and — the case that surprises — **in a scene made only of lightweight
items an interleaved item is on top of everything**, because the child walk it
was slotted into is empty.

A lightweight-only page that wants ink under some of its content wants `Under`
and a `z`; a page that wants the OneNote behaviour wants the content it is
drawing on to be cards. `examples/scene_ink` is the second, and its
`the_page_paints_the_ink_between_its_two_notes` test renders the real page and
reads the draw order back rather than asserting that a field was assigned.

It costs one arena node per interleaved item, so **group the layer, do not band
each stroke**: a `GroupItem` holding a page's strokes is one node, one z, and —
see §6 — one AT node.

---

## 4. The wet surface

```rust
let wet = WetLayer::new(move |canvas, ctx| stroke.borrow().paint(canvas, ctx));
let view = SceneView::with_model(model).wet_layer(wet.clone());
// in the pointer handler, after appending:
wet.request_repaint(ctx);
```

`request_repaint` marks **one node per view that mounted the layer**. No
relayout, no rebuild, no accessibility walk, and — the point — no repaint of the
scene's item bands.

`WetLayer` is a cloneable handle in the same sense `SceneModel` is: mount it in
two views (an overview beside a detail pane) and both paint it, and both are
repainted. One qualification, and it is a property of the door rather than of
the handle: a repaint is requested through an `EventContext`, which belongs to
one window's tree, so `request_repaint` refreshes the mounts in the calling
context's own window and leaves a mount in another window to that window's own
frame.

A `WidgetId` is a per-arena slot key and every tree mints the same ones, so the
mount's window narrows the candidates without deciding them — two **window-less**
trees (two headless tests) both wear `None`, and `None == None` there is two
unknowns comparing equal, not a match. `request_repaint` therefore does not push
the ids it holds at `EventContext::request_repaint`; it resolves each one in the
calling tree through `EventContext::with_widget_mut`, which hands the node over
only if it downcasts to the wet node's own type, and the closure then checks the
node belongs to *this* layer. A foreign id reaches nothing, because an ordinary
widget does not opt into `Widget::as_any_mut`; one that does is a `debug_assert`
failure rather than a silent mark — in release the assertion is skipped and that
node takes the repaint-only mark anyway, which costs one repaint of something
already on screen and never a mutation. The two gates are
`a_repaint_asked_for_in_one_tree_does_not_mark_another_trees_slot`, which stages
exactly that collision, and
`a_handler_repaints_the_wet_surface_through_request_repaint`, which is the
positive half — a typed door reaches nothing at all if the node forgets to
override `as_any_mut`, and a wet stroke that silently stops refreshing is the
worse bug of the two.

`WetLayer::nodes()` reports `WetNode { id, window }` rather than a bare id for
the same reason: a caller driving the repaint itself — `mark_needs_paint` in a
test — is the one who has to say which tree the id is for, so the window it was
mounted in travels with it.

`SceneView::foreground` cannot do this. The render walker computes one
`needs_paint` per node and gates that node's `paint` *and* its `post_paint` with
it, so invalidating the foreground re-runs the whole `Under` band with it.

The wet node is the view's last child: above every card and every interleaved
item, under the `Over` band and the view's own chrome. An app whose dried ink
lives in `Over` will see a stroke rise one step as it dries; `Interleaved` has
no such step.

---

## 5. The cost, measured

A wet stroke grows by a point per sample and is re-filled every frame. Two
costs decide whether that is affordable, and only one of them is the rasterizer.

**The cache key used to be the whole story.** `PathCacheKey` hashed every
`PathCommand`, so a *cache hit* — the case where the mask is already resident
and the frame has nothing to do — cost O(n). `Path` now carries a rolling
content stamp maintained as commands are appended, and the key reads that one
word:

| points | cache hit, before | cache hit, after |
| --- | --- | --- |
| 100 | 1.2 µs | ~0.09 µs |
| 500 | 6.2 µs | ~0.09 µs |
| 2 000 | 10.6 µs | **~0.06 µs** |

Linear, to flat: an 8.6× spread across the three sizes became none. Over a
2 000-point stroke that alone was tens of milliseconds of hashing spent to
discover that nothing needed doing. The "before" column is the same harness with
the command walk put back — which is also how
`a_cache_hit_does_not_scale_with_the_paths_length` fails if the stamp is
removed.

**The rasterizer is irreducible, so do not re-fill the whole outline.** Even
with an O(1) key, re-filling the growing path every frame stays superlinear
because tiny-skia sees a longer path each time:

| points | whole outline, per sample | chunked, per sample |
| --- | --- | --- |
| 100 | 44 µs | 16 µs |
| 500 | 56 µs | 17 µs |
| 2 000 | **227 µs** | **18 µs** |

(Absolute figures move with the machine and with what else is running; the
*shape* — one column superlinear, the other flat — is what the gates assert.
One run, so the two columns and the figures quoted below them are comparable.)

The pattern that works is an immutable committed prefix plus a short live tail:
freeze the stroke into ~32-point chunks as it goes, and re-rasterize only the
tail. Note what the chunked column measures: one re-raster per *sample*. A real
tool rasterizes once per **frame** however many samples arrived, so the
per-frame cost is that figure once, not once per point.

Every chunk is a cache hit, so what the pattern costs per sample is `n / 32`
hits plus one rasterize of a tail of at most 33 points — **not** O(1), which
this page used to claim. The rasterize is the floor: the per-sample figure at
100 points, where there is barely a chunk to look up, is already 15.7 µs. The
hits are the part that grows with the stroke, and the key fix is what keeps that
growth small — measured per sample at 2 000 points, 18.3 µs with the stamp
against 24.0 with the command walk.

Which is to say the key fix is worth about a third of this column, not an order
of magnitude — the order of magnitude is between the two columns. So
`a_chunked_stroke_stays_flat` is **not** the stamp's gate and does not fail when
the stamp is removed; `a_cache_hit_does_not_scale_with_the_paths_length` is,
and does (~1× to 8.4×). Nor could the chunked test be made into one: the
comparison that shows the stamp is between two *builds*, and a timing test can
only compare measurements taken in one process. What the chunked test gates is
the pattern, which it asserts against a whole-outline measurement taken beside
it.

`scene-ink` does exactly this; the numbers come from `cargo test -p
teksilo-render --release --test wet_stroke_cost -- --ignored --nocapture`, and
the two shapes are pinned as gates in the same file.

**The brush's translucency picks the primitive, and there is no third option.**
Opaque ink can overlap chunk seams invisibly. A translucent highlighter cannot:
`SetOpacity` is a per-draw alpha multiply, not a group composite, so overlapping
stamps double-darken. A highlighter must be **one** filled outline per stroke,
pays the full re-raster, and must throttle its *outline rebuild* to the display
refresh while still recording every sample.

**Dried ink has an atlas ceiling.** Each stroke's mask is its bounding box ×
`scale_factor²` texels in a 4096² atlas with per-frame LRU eviction. A 40×40 dp
letter at 2× is ~6 400 texels; one long sweeping diagonal is ~800 000. It
degrades by re-rasterizing rather than failing, but a dense page of long strokes
will thrash — another reason to keep strokes short and grouped.

---

## 6. Accessibility — the decision that matters

Every visible lightweight item gets a synthetic `Role::GraphicsObject` AT node.
A page with 500 strokes would therefore publish 500 unnamed graphics nodes.
They are viewport-culled, which the heavyweight tier is not — but 500 nameless
nodes *in* the viewport is **worse than Qt's zero**, because Qt's zero is at
least honest.

So:

```rust
PathItem::new(outline).access_hidden(true)   // per stroke
```

and give the **layer** one named node with an app-supplied text alternative — a
`GroupItem` with a label, or the note container the ink belongs to. A screen
reader should hear "handwritten note: three lines", not five hundred
"graphic"s.

The band itself changes nothing here: an `Interleaved` item keeps exactly the
synthetic node it had in `Under`, and the paint node that carries it is left out
of `accessibility_children`. Pinned by
`the_interleaved_band_does_not_move_the_accessibility_tree`.

---

## 7. The representation to copy

Not framework code. This is the shape that works, assembled from the three
implementations worth copying.

**perfect-freehand's geometry.** A closed **outline polygon**, not a centre-line:
spline points offset perpendicular to the local tangent by a per-point radius.
Variable width is not expressible as a constant-width stroked path — which is
exactly why Fabric.js cannot do pressure, and why this is a `fill_path` and never
a `stroke_path`.

```rust
r(p) = size / 2 * (1 - thinning * (1 - p))        // radius from pressure
s[i] = s[i-1] + (raw[i] - s[i-1]) * (1 - streamline)   // smoothing, on INPUT
```

Smooth the **input**, before any geometry. Wet and dry must call the *identical*
outline function or the stroke visibly changes shape the instant it dries —
the classic ink bug, and nothing in the framework can enforce it for you.

**PencilKit's storage.** Keep the sampled control points and regenerate the
outline; do not store the polygon. A brush change is then a re-render, and the
points map 1:1 onto `PointerAxes` + `EventTime`. Teksilo reports W3C
`tilt_x`/`tilt_y`; convert to the azimuth/altitude every brush engine wants at
the use site:

```rust
let (tx, ty) = (tilt_x.to_radians(), tilt_y.to_radians());
let azimuth = ty.tan().atan2(tx.tan());
let altitude = FRAC_PI_2 - tx.tan().hypot(ty.tan()).atan();
```

**PencilKit's erase, which is the best idea in any of them.** Point-erase is a
**mask**, not geometry surgery: keep a list of closed polygons and subtract them
at fill time with `FillRule::EvenOdd`. Undo of an erase is `masks.pop()` — no
re-sampling, no stroke splitting, no `ItemId` churn — and it is undoable for
free, in the data layer, which is where undo belongs.

**Where pressure comes from when there is none.** `PointerAxes::pressure` is
`None` on a mouse and on most touchscreens. `PointerInfo::effective_pressure`
returns the W3C 0.5-while-buttons-down fallback, which is a flat stroke. A tool
that wants taper on a mouse derives width from **speed** instead — which is why
the per-sample `EventTime` on each coalesced position is load-bearing and not a
nicety.
