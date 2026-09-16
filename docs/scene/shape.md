<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ShapeGeometry

`ItemShape` — the one geometry source of truth for the lightweight tier.

A `SceneItem` declares *what it is*, once, as a value:
`SceneItem::shape` returns an `ItemShape` in the
item's **local** coordinates. Every geometric question the scene can ask —
point hit-test, marquee selection, collision, "what lies along this path?"
— is then answered by deriving from that one value. There is no second
predicate to keep in sync, which is the whole point: the pair of methods
this replaced (`shape_contains` plus a snapshot-cloneable `clone_shape_test`
closure) had to agree by discipline, and the trait's own documentation
called the divergence "a silent dispatch bug".

# What a shape is

A shape is the **union** of at most two parts:

* an **interior** — the filled region. Always present for
  `ItemShape::bounds`, `ItemShape::rounded_rect` and
  `ItemShape::ellipse`; present for `ItemShape::path` under whichever
  `FillRule` it carries, and removable with `ItemShape::unfilled`.
* a **stroke band** — a ribbon of a given width centred on the outline,
  added by `ItemShape::stroked`. A `StrokeSpace::Device` (cosmetic)
  band is specified in device pixels, so its width in local coordinates is
  divided by the live view scale at test time — that is what keeps a
  hairline connector's clickable band tracking the *rendered* line at any
  zoom.

`ItemShape::none` is neither: it is never hit, never selected, never
collides. That is the logical-only `GroupItem` — a
container that must let clicks fall through to what it contains.

Every band carries a fixed `HIT_BAND_SLACK` of grab tolerance on top of
its half-width, so a one-pixel connector is clickable without pixel-perfect
aim. A coarse pointer gets more on top of that, from the view's miss-only
slop pass, which widens an item's *box* rather than its shape.

# Cost

Cloning an `ItemShape` is O(1): the path variant is one
`Rc<ShapeGeometry>` refcount bump and every other variant is small and
`Copy`-sized. The view clones one per item per layout pass, so that matters.
Curve flattening is memoised **inside** the `Rc` — built once per
`ShapeGeometry`, never once per pass — which is why
`PathItem` builds its `ShapeGeometry` in its constructor
and hands out clones of the handle.

Flattening tolerance is fixed at `SHAPE_FLATTEN_TOLERANCE` in **local**
coordinates, so the memo needs no cache key. At 20× zoom a curve therefore
hit-tests about 5 device pixels coarser than it looks — well inside the
grab band of any real stroke, but an approximation rather than exactness.

# Spaces

`ItemShape` is **local**. `SceneRegion` — the thing a marquee, a lasso or
a collision query asks about — is **scene**-space. The two
`…ItemBoundingRect` selection modes deliberately stay there and compare the
item's scene AABB; the two `…ItemShape` modes compare the shape itself. For
an item with a non-identity transform those are genuinely different tests —
see `ItemSelectionMode`.

Which frame a `…ItemShape` query meets the region in is a **decision**, not
a detail, because a stroke band is a *distance* and only a similarity
carries one. Usually the region comes down into the item's frame via
`SceneRegion::to_local`; when it cannot, the item goes up instead. The
rule, and why no approximation substitutes for it, is on
`ItemShape::contained_by_scene_region`.

## Builder methods at a glance

`shared`, `path`, `outline`, `bounds`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/shape/index.html)

## `pub const SHAPE_FLATTEN_TOLERANCE`

Curve-flattening tolerance, in **local item coordinates**.

Fixed rather than derived from the live zoom so a `ShapeGeometry` can
memoise its polyline behind one cell with no cache key. `0.25` keeps the
approximation under one device pixel out to 4× zoom and under ~5 px at 20×.

```rust
pub const SHAPE_FLATTEN_TOLERANCE: f32 = 0.25;
```

## `pub const HIT_BAND_SLACK`

Grab tolerance added to every stroke band's half-width, in local
coordinates.

A rendered one-pixel connector is not a one-pixel target: this is what
makes a thin stroke clickable without pixel-perfect aim, and it is the same
`2.0` the per-segment hit-test carried before `ItemShape` existed. It is
**not** the pointer-kind allowance — a coarse pointer earns more, from the
view's separate miss-only slop pass.

```rust
pub const HIT_BAND_SLACK: f32 = 2.0;
```

## `pub struct ShapeGeometry`

A `Path` plus its flattening, computed once and shared by handle.

Build one in your item's **constructor** and hold it as
`Rc<ShapeGeometry>`; `SceneItem::shape` then
hands out clones of the handle, so the per-layout-pass cost of publishing a
shape is a refcount bump rather than a flatten (or, as it used to be, a
full `Vec<PathCommand>` copy).

Neither `Send` nor `Sync` — the memo is a `OnceCell` behind an `Rc`,
like the rest of the scene.

```rust
pub struct ShapeGeometry { /* fields */ }
```

### Methods

#### `pub fn new(path: Path) -> Self`

Wrap a path. Nothing is flattened until something asks.

#### `pub fn shared(path: Path) -> Rc<Self>`

Wrap a path in a fresh shared handle — the form
`ItemShape::path` takes.

#### `pub fn path(&self) -> &Path`

The path this geometry was built from.

#### `pub fn outline(&self) -> &[Subpath]`

The flattened outline, one `Subpath` per `MoveTo` run. Memoised.

#### `pub fn bounds(&self) -> Rect`

Curve-accurate AABB of the outline. Memoised. `Rect::ZERO` for an
empty path.

## `pub struct ItemShape`

What an item's geometry **is**, in local item coordinates — the single
source of truth for hit-test, marquee, collision and path queries.

The interior-plus-band model, the cost story and the local-vs-scene
split are spelled out on the module this type lives in.

```rust
pub struct ItemShape { /* fields */ }
```

### Methods

#### `pub fn bounds(rect: Rect) -> Self`

The item's local AABB. This is the trait default and it allocates
nothing.

#### `pub fn rounded_rect(rect: Rect, radius: f32) -> Self`

A rectangle with rounded corners — the corners are **outside** the
shape, so a click in the transparent corner of a rounded card misses
it, exactly as the pixels suggest. Closed form; never flattened for a
point test.

#### `pub fn ellipse(rect: Rect) -> Self`

The ellipse inscribed in `rect`. Closed form; never flattened for a
point test.

#### `pub fn path(geometry: Rc<ShapeGeometry>) -> Self`

An arbitrary local path, filled under `FillRule::Winding`.

Takes the **shared** handle so the flattening is paid once per item
lifetime; build the `ShapeGeometry` in your constructor.

#### `pub fn from_path(path: Path) -> Self`

`ItemShape::path` for a one-off path, allocating a fresh
`ShapeGeometry` on the spot.

Convenient for a test or a shape built once and thrown away. An item
that publishes its shape every layout pass should own an
`Rc<ShapeGeometry>` instead — this one re-flattens every time it is
asked.

#### `pub fn none() -> Self`

Never hit, never selected, never collides.

The logical-only `GroupItem` case, and any pure
accessibility container: clicks fall straight through to whatever is
beneath.

#### `pub fn filled(mut self, rule: FillRule) -> Self`

Fill the interior under `rule`.

Pass `FillRule::EvenOdd` so a ring authored as two subpaths has a
real hole. A `PathItem` takes its rule from its own
`fill_rule`, which is the same rule it **paints** with — a hit-only
fill rule would be a second source of truth, which is precisely what
this type exists to abolish.

#### `pub fn unfilled(mut self) -> Self`

Drop the interior, leaving only the stroke band (if any).

A stroke-only connector: clicking the empty middle of its bounding box
must miss, and clicking anywhere along the drawn line must hit. Has no
effect on the closed forms, which are regions by construction.

#### `pub fn stroked(mut self, width: f32, space: StrokeSpace) -> Self`

Add a stroke band `width` wide, centred on the outline.

`StrokeSpace::Device` is a **cosmetic** stroke: its width is in
device pixels, so the local-coordinate band is divided by the live view
scale at test time and tracks the rendered line at any zoom.
`StrokeSpace::Logical` is already in local units and is unaffected by
zoom.

#### `pub fn hit_stroke_width(mut self, width: f32) -> Self`

Replace the band's width for **hit** purposes only, independent of what
is painted (Konva's `hitStrokeWidth`): a 1 dp connector wire made 12 dp
grabbable. Always in local units — a hit band is a target size, not a
rendered thickness, so it does not shrink as you zoom in.

#### `pub fn contains(&self, local_pt: Point, view_scale: f32) -> bool`

Whether `local_pt` is inside the shape.

`view_scale` is the live view zoom — the uniform scale of the view
transform's linear part. It is consulted only by a
`StrokeSpace::Device` band; pass `1.0` for an item pinned in screen
space (see `ItemFlags::IGNORES_TRANSFORMATIONS`),
whose local coordinates *are* screen coordinates and which therefore
has no zoom to convert.

#### `pub fn bounding_rect(&self) -> Rect`

The shape's own local AABB at **unit** view scale, inflated by any
stroke band at its logical width.

This is the shape's extent, *not* the rectangle the spatial index
buckets on — that stays the entry's `local_bounds`, composed into a
scene AABB. Every geometry-bearing built-in derives its `local_bounds`
from exactly this value, and `SceneItem::shape`
asks an app item for the same, so at unit scale the box encloses the
shape and a broad phase on the box can never reject a point the narrow
phase would have accepted.

**At other zooms that holds only for a `StrokeSpace::Logical` band
and the closed forms.** A `StrokeSpace::Device` (cosmetic) band is
specified in device pixels, so its width in local coordinates is
`w / view_scale` and it is *wider* than this rectangle at any zoom
below 1×. The narrow phase is right there — the band tracks what is
painted — but the broad phase bucketed on the unit-scale box, so a
point on a cosmetic stroke zoomed far out can be rejected before the
narrow phase sees it. Use
`bounding_rect_at` to measure the real
extent at a zoom. This is not closed by clamping: the divergence grows
without bound as the zoom approaches zero, so the only rectangle that
would always cover it is an infinite one, and paying for the *largest*
band any item could ever need would bloat every cosmetic-stroke item's
index bucket and its advertised accessibility box at every zoom. The
mitigation is that zooming out that far turns a hairline into a hair:
at 0.25× a 40 px cosmetic stroke covers 160 local units, which is a
blot on screen rather than a line anyone is aiming at.

#### `pub fn bounding_rect_at(&self, view_scale: f32) -> Rect`

`ItemShape::bounding_rect` at an explicit view zoom — the rectangle
that really does enclose everything `ItemShape::contains` accepts at
that zoom.

Differs from `bounding_rect()` only for a `StrokeSpace::Device` band,
and only away from 1×.

#### `pub fn is_none(&self) -> bool`

Whether this shape can never be hit (`ItemShape::none`).

#### `pub fn intersects_region(&self, region: &SceneRegion, view_scale: f32) -> bool`

Whether the shape and `region` share **area** — the `Intersects…`
half of `ItemSelectionMode`.

`region` must already be in **this shape's** coordinate space — call
`SceneRegion::to_local` first. `view_scale` is as for
`ItemShape::contains`.

# Edges belong to no one

Sharing *area* is the rule, so two sets that meet only along their
boundaries do **not** intersect: a marquee laid exactly on an item's
edge picks it up under no mode. That is what `Scene::items_in_rect`
has always answered (its rectangle compare is strict on all four
sides), and both paths below answer it — the one-compare fast path for
a plain box against an axis-aligned band, and the general outline
algebra for everything else. The two disagreeing is what let a rounded
tile be picked by a band its square twin was not, and made
`IntersectsItemShape` report a hit where `IntersectsItemBoundingRect`
reported none — impossible for a shape that lies inside its own box.

`ItemShape::contains`, by contrast, is **closed**: a point on the
edge of a rectangle is on the rectangle. A point has no area to share,
so the two rules are about different questions and neither follows
from the other.

The one exception is a region — or a shape — that *is* all boundary: a
zero-width path, which is exactly what
`Scene::items_along_path` builds, or
a degenerate box. An area rule would answer "no" to every such query,
so when either side has no area of its own the question becomes "does
this line touch the item?" and contact is the answer — the behaviour
that query has always had.

# How the general path witnesses an overlap

Four witnesses, any one of which **proves** shared area, checked in
increasing cost order: a probe point of the shape's outline strictly
inside the region; a probe point of the region strictly inside the
shape; a pair of boundary segments that properly cross (or, with a
band, come closer than the two half-widths together); and — last —
every vertex of the shape inside the *closed* region, which is the only
witness that survives two outlines lying exactly on top of each other.
That last case is not exotic: it is a duplicate of a rotated item laid
over the original, which `colliding_items` really does get asked about,
and without it two identical rotated cards would report no collision.

A **probe point** is a vertex *or a segment midpoint*. The midpoints
are load-bearing rather than belt-and-braces: two rectangles with the
same top and bottom edges, overlapping in x — a marquee dragged to
exactly an item's height, which is a gesture, not a curiosity — put
every vertex of each on the other's boundary and cross nothing
transversally, so the vertices alone witness nothing.

Every witness is **sound** (each really does imply shared area), and
together they are not quite **complete**: a sliver of overlap that no
probe point lands in and no segment pair crosses transversally reports
no intersection. Erring that way is deliberate — a false positive here
would let a `…ItemShape` query pick an item its own bounding rect does
not, which is impossible for a shape that lies inside its box, whereas
a false negative only ever agrees with the cheaper mode.

Where a probe lands exactly on a freeform outline, "strictly inside"
is decided within [`SHAPE_FLATTEN_TOLERANCE`]: a ray cast against a
polyline has no meaningful answer on the polyline itself. Against a
rectangle, the closed form every default shape and every marquee band
uses, it is exact.

#### `pub fn contained_by_region(&self, region: &SceneRegion, view_scale: f32) -> bool`

Whether the shape lies entirely inside `region` — the `Contains…` half
of `ItemSelectionMode`.

`region` must already be in this shape's coordinate space. A shape with
a stroke band is contained only when the **band** is — the whole
clickable ribbon, not merely the centreline.

# Edges are inside

Containment is of **closed** sets, so a shape resting exactly on the
region's boundary is contained: an item and a band drawn at precisely
the same rectangle select under `Contains…`. That is what the
rectangle fast path has always answered, and the general path answers
it too — an outline lying *along* the region's edge is not a crossing,
only an outline passing *through* it is. (Before, a collinear edge
counted as a crossing, so a rounded tile an exact-fit band contained
was rejected while its square twin was accepted.)

Note the asymmetry with `ItemShape::intersects_region`, and that it
is the only consistent pair: an exact-fit band both contains the item
and shares its whole area, while a band merely *abutting* the item
does neither.

# Two kinds of boundary

For a region with an **interior**, "every vertex is inside and no edge
crosses the outline" settles it: an outline that never crosses the
region's boundary cannot have its inside on the other side of it.

A **stroke band** has no such outline. Its boundary is an offset curve
the region never stores, and the outline it does store is the band's
*centreline* — which runs down the middle of the region rather than
around it. Testing crossings against that gets both answers wrong: an
item sitting squarely on a wire crosses the centreline and is reported
as escaping, while a box whose four corners are all within a bent
wire's band bulges out across the inside of the bend, crosses nothing,
and is reported as contained. Each item edge is therefore checked
against the band itself, by an exact segment-in-band test.

A region with both is inside-the-fill **or** inside-the-band; an item
weaving between the two, inside their union but inside neither part, is
reported as not contained. That is the one deliberately conservative
answer here, and it is one-sided: never a false containment.

#### `pub fn intersects_scene_region( &self, region: &SceneRegion, local_to_scene: &Transform2D, view_scale: f32, ) -> bool`

Whether this shape, placed by `local_to_scene`, shares area with a
**scene**-space `region` — the `Intersects…` half of
`ItemSelectionMode`, with the frame chosen rather than assumed.

This is the door a region query comes in by, and the reason it exists
is spelled out on `ItemShape::contained_by_scene_region`.

#### `pub fn contained_by_scene_region( &self, region: &SceneRegion, local_to_scene: &Transform2D, view_scale: f32, ) -> bool`

Whether this shape, placed by `local_to_scene`, lies entirely inside a
**scene**-space `region` — the `Contains…` half of
`ItemSelectionMode`, with the frame chosen rather than assumed.

# Why the frame is a decision

A query has two frames to choose from and they are not
interchangeable, because a **stroke band is a distance** and only a
similarity — a rotation, a uniform scale, a translation, a reflection —
carries one. Under any other affine the exact image of a round band is
an *elliptical* one, which neither `ItemShape` nor `SceneRegion`
can hold: both store a single width.

Each side's band is round in its own frame. The region's is round in
**scene** space. The item's is round in **local** space — that is not a
convention but what the renderer does, since
`PathItem` strokes its path in local coordinates and
the item's transform is pushed on the canvas around it, so an
anisotropically scaled wire really is painted with an elliptical pen.

So: when the region carries no band, or the transform is a similarity,
the local frame is exact and the query takes it — one map, the same
cost it has always had. Otherwise the region cannot come down, so the
**item goes up**: its outline is a polyline and
`Path::transformed` is exact for every affine, so lifting it costs
nothing in accuracy.

A *mapped* band is then the only thing left, and it is dealt with by
not mapping one: `in_scene_frame` writes the item's band
out as an explicit outline **in local space, where it is round**, and
lifts that. What comes back is a union of band-free parts, and a union
is contained iff every part is.

Approximating instead — scaling the band by one of the two stretches —
cannot work here, and the reason is worth stating because it looks like
it should. The two selection-mode families owe each other implications
in *opposite* directions: `IntersectsItemShape` must never exceed
`IntersectsItemBoundingRect`, which wants a band that under-reaches,
while `ContainsItemBoundingRect` must never exceed
`ContainsItemShape`, which wants one that over-reaches. No single
scalar satisfies both, so the only answer that keeps both is the exact
one.

#### `pub fn to_scene_region(&self, local_to_scene: &Transform2D) -> SceneRegion`

Re-publish this shape as a `SceneRegion` in another frame — its own
scene transform, so a collision query can ask the rest of the scene
about it.

The outline goes through `Path::transformed`, which is exact for
every affine (an arc a transform cannot carry as an arc is expanded
into the cubics the renderer paints it as), so the republished region
keeps its curves and is re-flattened at
`SHAPE_FLATTEN_TOLERANCE` in the *target* frame rather than
inheriting a polyline sampled in this one.

# A band's width is still scaled here

A region query picks the frame that keeps each band round
(`ItemShape::contained_by_scene_region`); this cannot, because it
returns one `SceneRegion` and one region holds one band width. So
under a transform that is not a similarity — where the exact image of a
round band is elliptical — the width is scaled by the transform's
**smallest** stretch: the widest uniform band that fits inside the true
image, under-reaching along the stretched axis and never claiming
ground the true band does not cover.

Writing the band out as an outline, which is what the query path does
and which *is* exact, does not work here: a band is a union of
overlapping capsules and reads only under `FillRule::Winding`, while
the interior it would have to be unioned with keeps its author's rule
and its author's subpath orientations. The query path escapes that by
keeping the two apart as separate parts, and a single region has
nowhere to put a second part. The consequence is confined to
`Scene::item_region` and the collision
query built on it: an anisotropically scaled item carrying a stroke
collides as though its band were the narrowest the map allows.

## `pub enum ItemSelectionMode`

How a region query decides whether an item is picked — Qt's
`Qt::ItemSelectionMode`, one variant for one.

The `…ItemShape` modes consult `SceneItem::shape`,
in whichever frame keeps both sides' bands round — usually the item's
**local** one, with the region mapped down into it, and scene space when
the region's band cannot make that trip
(`ItemShape::contained_by_scene_region`). The `…ItemBoundingRect` modes
compare the item's **scene** AABB, in scene space.

For an item whose transform is identity and whose shape is the default AABB
the two families coincide exactly. For an item carrying a rotation or a
scale they do **not**: the scene AABB of a rotated box is the enlarged
axis-aligned hull of it, so a bounding-rect query is strictly the looser
test. That is a real behaviour difference, not a rounding one — and it is
an *implication*, in both directions. A shape lies inside its own box, so
a shape the region meets its box meets too, and a box the region contains
drags its shape in with it. Both are pinned by
`tests/prop_selection_modes.rs`, and holding them is what decides how a
band may be mapped.

```rust
pub enum ItemSelectionMode { /* variants */ }
```

### Variants

- **`IntersectsItemShape`** — Picked when its **shape** intersects the region. Qt's default and Teksilo's: it is the rule that makes a rubber band agree with a click about what an item is.
- **`ContainsItemShape`** — Picked when its shape lies entirely inside the region — "select what the lasso fully encircles".
- **`IntersectsItemBoundingRect`** — Picked when its scene AABB intersects the region. The cheap mode, and the one that reproduces the pre-`ItemShape` marquee.
- **`ContainsItemBoundingRect`** — Picked when its scene AABB lies entirely inside the region.

### Methods

#### `pub fn uses_shape(self) -> bool`

Whether this mode consults the item's shape (rather than its AABB).

#### `pub fn requires_containment(self) -> bool`

Whether this mode requires full containment (rather than any overlap).

## `pub struct SceneRegion`

The region a geometry query asks about.

Two forms, and the difference matters:

* a **rectangle** — the rubber band under an unrotated view, and the one
  form that keeps a query on the same one-rect-compare fast path the
  pre-`ItemShape` marquee took;
* a **path** — the exact quadrilateral of a rubber band under a *rotated*
  view (rather than its enlarged axis-aligned hull, which over-selects), a
  freehand lasso, or a stroked path for "what lies along this connector?".

A region is in **scene** coordinates until `SceneRegion::to_local` maps
it into an item's frame; a rectangle that survives that map as a rectangle
stays one, and one that does not becomes the exact transformed
quadrilateral rather than being rounded up to an AABB.

```rust
pub struct SceneRegion { /* fields */ }
```

### Methods

#### `pub fn rect(rect: Rect) -> Self`

An axis-aligned box.

#### `pub fn empty() -> Self`

A region that covers nothing. Every query against it is `false`.

#### `pub fn lasso(path: Path) -> Self`

A closed freehand region, filled under `FillRule::Winding`.

#### `pub fn lasso_with_rule(path: Path, rule: FillRule) -> Self`

A closed freehand region under an explicit fill rule.

#### `pub fn stroke(path: Path, width: f32) -> Self`

The **stroke band** of a path, `width` wide — "what lies along this
line?" rather than "what lies inside this loop?".

#### `pub fn from_geometry( geometry: Rc<ShapeGeometry>, fill: Option<FillRule>, band: Option<f32>, ) -> Self`

A region from a shared geometry handle, with an explicit interior
and/or band. The general constructor the others are sugar for; used to
re-publish an item's own shape as the region a collision query asks
about.

#### `pub fn from_screen_rect(screen_rect: Rect, screen_to_scene: &Transform2D) -> Self`

A screen-space rectangle mapped through `screen_to_scene`.

Stays a rectangle when the transform preserves axes, and becomes the
exact transformed quadrilateral when it does not — which is what stops
a rubber band under a rotated view from selecting everything in the
enlarged hull of itself.

#### `pub fn bounding_rect(&self) -> Rect`

Broad-phase AABB — what goes to the spatial index. Band-inflated.

#### `pub fn as_rect(&self) -> Option<Rect>`

`Some(rect)` iff this region is an axis-aligned box with no band.

#### `pub fn is_empty(&self) -> bool`

Whether the region covers nothing at all.

#### `pub fn to_local(&self, scene_to_local: &Transform2D) -> SceneRegion`

Map into another coordinate frame — an item's local space, via the
inverse of its scene transform.

A rectangle survives as a rectangle only when the transform preserves
axes; otherwise it becomes the exact transformed quadrilateral. A path
goes through `Path::transformed`, which is exact for every affine —
including an arc, which it expands into cubics rather than moving the
rectangle an arc is stored as and keeping its angles.

# A band under an anisotropic map

A band is a *distance*, and an affine that stretches one axis more than
another does not carry a distance: the exact image of a round band is
an elliptical one, which no single width can describe. A rotation, a
uniform scale and a translation are all isotropic, so for every one of
them the mapped width is exact and none of this applies.

Under a genuinely anisotropic map the width is scaled by the
transform's **smallest** stretch — its least singular value — which is
the widest uniform band that fits *inside* the true image. Under
`scale(1, 10)` the true band is between one and ten times as wide
depending on the direction, and the mapped one claims one: it can
under-reach along the stretched axis, and it never claims ground the
true band does not cover. `Transform2D::geometric_scale` — the
geometric *mean* of the two stretches — would err in both directions at
once, over-reaching along one axis while under-reaching along the
other, which is why it is not used here.

**This is a one-sided approximation, and the framework's own region
queries do not rely on it.** Under-reaching keeps
`IntersectsItemShape` inside `IntersectsItemBoundingRect`, but it puts
`ContainsItemShape` *outside* `ContainsItemBoundingRect` — the two
families want opposite errors, so no scalar serves both. A query
therefore never maps a banded region down into an anisotropic item's
frame: it lifts the item into scene space instead, where the band is
still round. See `ItemShape::contained_by_scene_region`, which is the
door `Scene::items_in_region` comes in by. What is left here is the
honest answer for a caller who has asked for a mapped region and will
measure distances in the target frame.

#### `pub fn contains_point(&self, p: Point) -> bool`

Whether `p` — in the region's own coordinate space — is inside it.

#### `pub fn has_area(&self) -> bool`

Whether the region covers any **area** — `false` for a zero-width
path (a bare line: the `items_along_path` query) and for a degenerate
box. See `ItemShape::intersects_region`, which relaxes its rule for
exactly this case.

A filled outline needs at least three points on some subpath to
enclose anything; two points are a line, which
`subpaths_contain_point` already declines to fill.

#### `pub fn clearance(&self, p: Point) -> f32`

How far `p` is from leaving the region: positive inside, negative
outside, in the region's own units. Used by the containment modes to
ask whether a *band* — not merely a centreline — fits inside.
