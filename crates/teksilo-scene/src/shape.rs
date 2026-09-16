// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`ItemShape`] — the one geometry source of truth for the lightweight tier.
//!
//! A [`SceneItem`](crate::SceneItem) declares *what it is*, once, as a value:
//! [`SceneItem::shape`](crate::SceneItem::shape) returns an `ItemShape` in the
//! item's **local** coordinates. Every geometric question the scene can ask —
//! point hit-test, marquee selection, collision, "what lies along this path?"
//! — is then answered by deriving from that one value. There is no second
//! predicate to keep in sync, which is the whole point: the pair of methods
//! this replaced (`shape_contains` plus a snapshot-cloneable `clone_shape_test`
//! closure) had to agree by discipline, and the trait's own documentation
//! called the divergence "a silent dispatch bug".
//!
//! # What a shape is
//!
//! A shape is the **union** of at most two parts:
//!
//! * an **interior** — the filled region. Always present for
//!   [`ItemShape::bounds`], [`ItemShape::rounded_rect`] and
//!   [`ItemShape::ellipse`]; present for [`ItemShape::path`] under whichever
//!   [`FillRule`] it carries, and removable with [`ItemShape::unfilled`].
//! * a **stroke band** — a ribbon of a given width centred on the outline,
//!   added by [`ItemShape::stroked`]. A [`StrokeSpace::Device`] (cosmetic)
//!   band is specified in device pixels, so its width in local coordinates is
//!   divided by the live view scale at test time — that is what keeps a
//!   hairline connector's clickable band tracking the *rendered* line at any
//!   zoom.
//!
//! [`ItemShape::none`] is neither: it is never hit, never selected, never
//! collides. That is the logical-only [`GroupItem`](crate::GroupItem) — a
//! container that must let clicks fall through to what it contains.
//!
//! Every band carries a fixed [`HIT_BAND_SLACK`] of grab tolerance on top of
//! its half-width, so a one-pixel connector is clickable without pixel-perfect
//! aim. A coarse pointer gets more on top of that, from the view's miss-only
//! slop pass, which widens an item's *box* rather than its shape.
//!
//! # Cost
//!
//! Cloning an `ItemShape` is O(1): the path variant is one
//! [`Rc<ShapeGeometry>`] refcount bump and every other variant is small and
//! `Copy`-sized. The view clones one per item per layout pass, so that matters.
//! Curve flattening is memoised **inside** the `Rc` — built once per
//! `ShapeGeometry`, never once per pass — which is why
//! [`PathItem`](crate::PathItem) builds its `ShapeGeometry` in its constructor
//! and hands out clones of the handle.
//!
//! Flattening tolerance is fixed at [`SHAPE_FLATTEN_TOLERANCE`] in **local**
//! coordinates, so the memo needs no cache key. At 20× zoom a curve therefore
//! hit-tests about 5 device pixels coarser than it looks — well inside the
//! grab band of any real stroke, but an approximation rather than exactness.
//!
//! # Spaces
//!
//! `ItemShape` is **local**. [`SceneRegion`] — the thing a marquee, a lasso or
//! a collision query asks about — is **scene**-space until
//! [`SceneRegion::to_local`] maps it into an item's frame. The two
//! `…ItemBoundingRect` selection modes deliberately stay in scene space and
//! compare the item's scene AABB; the two `…ItemShape` modes map the region
//! into local space. For an item with a non-identity transform those are
//! genuinely different tests — see [`ItemSelectionMode`].

use std::cell::OnceCell;
use std::rc::Rc;

use teksilo_canvas::{
    FillRule, Path, Point, Rect, StrokeSpace, Subpath, Transform2D, subpaths_contain_point,
};

/// Curve-flattening tolerance, in **local item coordinates**.
///
/// Fixed rather than derived from the live zoom so a [`ShapeGeometry`] can
/// memoise its polyline behind one cell with no cache key. `0.25` keeps the
/// approximation under one device pixel out to 4× zoom and under ~5 px at 20×.
pub const SHAPE_FLATTEN_TOLERANCE: f32 = 0.25;

/// Grab tolerance added to every stroke band's half-width, in local
/// coordinates.
///
/// A rendered one-pixel connector is not a one-pixel target: this is what
/// makes a thin stroke clickable without pixel-perfect aim, and it is the same
/// `2.0` the per-segment hit-test carried before `ItemShape` existed. It is
/// **not** the pointer-kind allowance — a coarse pointer earns more, from the
/// view's separate miss-only slop pass.
pub const HIT_BAND_SLACK: f32 = 2.0;

// ---------------------------------------------------------------------------
// ShapeGeometry — a path plus its memoised flattening
// ---------------------------------------------------------------------------

/// How many consecutive segments share one rejection box. Small enough that a
/// long stroke gets real pruning, large enough that the index costs a fraction
/// of the vertices it covers.
const SEGMENTS_PER_CHUNK: usize = 16;

#[derive(Debug)]
struct Flattened {
    subpaths: Vec<Subpath>,
    /// One AABB per subpath, in the same order. First-level rejection for a
    /// shape made of several subpaths.
    boxes: Vec<Rect>,
    /// Per subpath, one AABB per run of [`SEGMENTS_PER_CHUNK`] consecutive
    /// segments. Second-level rejection, and the one that matters for a single
    /// long stroke: without it a 500-segment ink line answers "no" only after
    /// 500 point-to-segment distances, **per pointer sample, per item**. The
    /// old per-segment walk had an accidental O(1) guard here — it bailed to
    /// the AABB on the first curve command — and flattening removed it.
    chunks: Vec<Vec<Rect>>,
    /// AABB of the whole flattened outline (curve-accurate, unlike
    /// [`Path::bounds`]).
    bounds: Rect,
}

/// AABBs for each run of [`SEGMENTS_PER_CHUNK`] segments of each subpath, in
/// the order [`Subpath::segments`] yields them (closing edge included when the
/// subpath is closed).
fn chunk_boxes(subpaths: &[Subpath]) -> Vec<Vec<Rect>> {
    subpaths
        .iter()
        .map(|sp| {
            let mut out: Vec<Rect> = Vec::new();
            let mut acc: Option<Rect> = None;
            for (i, (a, b)) in sp.segments(false).enumerate() {
                let seg = Rect::new(
                    a.x.min(b.x),
                    a.y.min(b.y),
                    (a.x - b.x).abs(),
                    (a.y - b.y).abs(),
                );
                acc = Some(match acc {
                    None => seg,
                    Some(r) => union_two(r, seg),
                });
                if (i + 1) % SEGMENTS_PER_CHUNK == 0 {
                    out.push(acc.take().expect("just set"));
                }
            }
            if let Some(r) = acc {
                out.push(r);
            }
            out
        })
        .collect()
}

fn union_two(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Rect::new(
        x,
        y,
        a.right().max(b.right()) - x,
        a.bottom().max(b.bottom()) - y,
    )
}

/// A [`Path`] plus its flattening, computed once and shared by handle.
///
/// Build one in your item's **constructor** and hold it as
/// `Rc<ShapeGeometry>`; [`SceneItem::shape`](crate::SceneItem::shape) then
/// hands out clones of the handle, so the per-layout-pass cost of publishing a
/// shape is a refcount bump rather than a flatten (or, as it used to be, a
/// full `Vec<PathCommand>` copy).
///
/// Neither `Send` nor `Sync` — the memo is a [`OnceCell`] behind an [`Rc`],
/// like the rest of the scene.
#[derive(Debug)]
pub struct ShapeGeometry {
    path: Path,
    flat: OnceCell<Flattened>,
}

impl ShapeGeometry {
    /// Wrap a path. Nothing is flattened until something asks.
    pub fn new(path: Path) -> Self {
        Self {
            path,
            flat: OnceCell::new(),
        }
    }

    /// Wrap a path in a fresh shared handle — the form
    /// [`ItemShape::path`] takes.
    pub fn shared(path: Path) -> Rc<Self> {
        Rc::new(Self::new(path))
    }

    /// The path this geometry was built from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The flattened outline, one [`Subpath`] per `MoveTo` run. Memoised.
    pub fn outline(&self) -> &[Subpath] {
        &self.flattened().subpaths
    }

    /// Curve-accurate AABB of the outline. Memoised. [`Rect::ZERO`] for an
    /// empty path.
    pub fn bounds(&self) -> Rect {
        self.flattened().bounds
    }

    fn flattened(&self) -> &Flattened {
        self.flat.get_or_init(|| {
            let subpaths = self.path.flatten(SHAPE_FLATTEN_TOLERANCE);
            let boxes: Vec<Rect> = subpaths.iter().map(|sp| sp.bounds()).collect();
            let bounds = union_all(&boxes);
            let chunks = chunk_boxes(&subpaths);
            Flattened {
                subpaths,
                boxes,
                chunks,
                bounds,
            }
        })
    }
}

// ---------------------------------------------------------------------------
// ItemShape
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum ShapeKind {
    /// Never hit, never selected, never collides.
    None,
    /// The plain local AABB — the trait default, and the one kind that costs
    /// nothing to build or test.
    Bounds(Rect),
    RoundedRect {
        rect: Rect,
        radius: f32,
    },
    Ellipse(Rect),
    Path(Rc<ShapeGeometry>),
}

#[derive(Debug, Clone, Copy)]
struct HitBand {
    width: f32,
    space: StrokeSpace,
}

/// What an item's geometry **is**, in local item coordinates — the single
/// source of truth for hit-test, marquee, collision and path queries.
///
/// The interior-plus-band model, the cost story and the local-vs-scene
/// split are spelled out on the module this type lives in.
#[derive(Debug, Clone)]
pub struct ItemShape {
    kind: ShapeKind,
    /// Present when the shape has an interior *and* that interior needs a
    /// rule — i.e. for the path kind. The closed forms are always filled.
    fill: Option<FillRule>,
    band: Option<HitBand>,
}

impl ItemShape {
    // -- construction -------------------------------------------------

    /// The item's local AABB. This is the trait default and it allocates
    /// nothing.
    pub fn bounds(rect: Rect) -> Self {
        Self {
            kind: ShapeKind::Bounds(rect),
            fill: None,
            band: None,
        }
    }

    /// A rectangle with rounded corners — the corners are **outside** the
    /// shape, so a click in the transparent corner of a rounded card misses
    /// it, exactly as the pixels suggest. Closed form; never flattened for a
    /// point test.
    pub fn rounded_rect(rect: Rect, radius: f32) -> Self {
        if radius <= 0.0 {
            return Self::bounds(rect);
        }
        Self {
            kind: ShapeKind::RoundedRect {
                rect,
                radius: radius.min(rect.width * 0.5).min(rect.height * 0.5),
            },
            fill: None,
            band: None,
        }
    }

    /// The ellipse inscribed in `rect`. Closed form; never flattened for a
    /// point test.
    pub fn ellipse(rect: Rect) -> Self {
        Self {
            kind: ShapeKind::Ellipse(rect),
            fill: None,
            band: None,
        }
    }

    /// An arbitrary local path, filled under [`FillRule::Winding`].
    ///
    /// Takes the **shared** handle so the flattening is paid once per item
    /// lifetime; build the [`ShapeGeometry`] in your constructor.
    pub fn path(geometry: Rc<ShapeGeometry>) -> Self {
        Self {
            kind: ShapeKind::Path(geometry),
            fill: Some(FillRule::Winding),
            band: None,
        }
    }

    /// [`ItemShape::path`] for a one-off path, allocating a fresh
    /// [`ShapeGeometry`] on the spot.
    ///
    /// Convenient for a test or a shape built once and thrown away. An item
    /// that publishes its shape every layout pass should own an
    /// `Rc<ShapeGeometry>` instead — this one re-flattens every time it is
    /// asked.
    pub fn from_path(path: Path) -> Self {
        Self::path(ShapeGeometry::shared(path))
    }

    /// Never hit, never selected, never collides.
    ///
    /// The logical-only [`GroupItem`](crate::GroupItem) case, and any pure
    /// accessibility container: clicks fall straight through to whatever is
    /// beneath.
    pub fn none() -> Self {
        Self {
            kind: ShapeKind::None,
            fill: None,
            band: None,
        }
    }

    // -- how the outline is interpreted -------------------------------

    /// Fill the interior under `rule`.
    ///
    /// Pass [`FillRule::EvenOdd`] so a ring authored as two subpaths has a
    /// real hole. A [`PathItem`](crate::PathItem) takes its rule from its own
    /// `fill_rule`, which is the same rule it **paints** with — a hit-only
    /// fill rule would be a second source of truth, which is precisely what
    /// this type exists to abolish.
    pub fn filled(mut self, rule: FillRule) -> Self {
        self.fill = Some(rule);
        self
    }

    /// Drop the interior, leaving only the stroke band (if any).
    ///
    /// A stroke-only connector: clicking the empty middle of its bounding box
    /// must miss, and clicking anywhere along the drawn line must hit. Has no
    /// effect on the closed forms, which are regions by construction.
    pub fn unfilled(mut self) -> Self {
        if matches!(self.kind, ShapeKind::Path(_)) {
            self.fill = None;
        }
        self
    }

    /// Add a stroke band `width` wide, centred on the outline.
    ///
    /// [`StrokeSpace::Device`] is a **cosmetic** stroke: its width is in
    /// device pixels, so the local-coordinate band is divided by the live view
    /// scale at test time and tracks the rendered line at any zoom.
    /// [`StrokeSpace::Logical`] is already in local units and is unaffected by
    /// zoom.
    pub fn stroked(mut self, width: f32, space: StrokeSpace) -> Self {
        self.band = Some(HitBand {
            width: width.max(0.0),
            space,
        });
        self
    }

    /// Replace the band's width for **hit** purposes only, independent of what
    /// is painted (Konva's `hitStrokeWidth`): a 1 dp connector wire made 12 dp
    /// grabbable. Always in local units — a hit band is a target size, not a
    /// rendered thickness, so it does not shrink as you zoom in.
    pub fn hit_stroke_width(mut self, width: f32) -> Self {
        self.band = Some(HitBand {
            width: width.max(0.0),
            space: StrokeSpace::Logical,
        });
        self
    }

    // -- queries ------------------------------------------------------

    /// Whether `local_pt` is inside the shape.
    ///
    /// `view_scale` is the live view zoom — the uniform scale of the view
    /// transform's linear part. It is consulted only by a
    /// [`StrokeSpace::Device`] band; pass `1.0` for an item pinned in screen
    /// space (see [`ItemFlags::IGNORES_TRANSFORMATIONS`](crate::ItemFlags)),
    /// whose local coordinates *are* screen coordinates and which therefore
    /// has no zoom to convert.
    pub fn contains(&self, local_pt: Point, view_scale: f32) -> bool {
        if matches!(self.kind, ShapeKind::None) {
            return false;
        }
        let half = self.band_half_width(view_scale);
        // One rect compare stands between a miss and any segment walk. This is
        // what keeps a long ink stroke's per-pointer-sample cost O(1) for the
        // overwhelmingly common answer.
        if !self
            .geometry_bounds()
            .expand(half.unwrap_or(0.0))
            .contains(local_pt)
        {
            return false;
        }
        if self.interior_contains(local_pt) {
            return true;
        }
        match half {
            Some(h) => self.outline_within(local_pt, h),
            None => false,
        }
    }

    /// The shape's own local AABB at **unit** view scale, inflated by any
    /// stroke band at its logical width.
    ///
    /// This is the shape's extent, *not* the rectangle the spatial index
    /// buckets on — that stays the entry's `local_bounds`, composed into a
    /// scene AABB. Every geometry-bearing built-in derives its `local_bounds`
    /// from exactly this value, and [`SceneItem::shape`](crate::SceneItem::shape)
    /// asks an app item for the same, so at unit scale the box encloses the
    /// shape and a broad phase on the box can never reject a point the narrow
    /// phase would have accepted.
    ///
    /// **At other zooms that holds only for a [`StrokeSpace::Logical`] band
    /// and the closed forms.** A [`StrokeSpace::Device`] (cosmetic) band is
    /// specified in device pixels, so its width in local coordinates is
    /// `w / view_scale` and it is *wider* than this rectangle at any zoom
    /// below 1×. The narrow phase is right there — the band tracks what is
    /// painted — but the broad phase bucketed on the unit-scale box, so a
    /// point on a cosmetic stroke zoomed far out can be rejected before the
    /// narrow phase sees it. Use
    /// [`bounding_rect_at`](ItemShape::bounding_rect_at) to measure the real
    /// extent at a zoom. This is not closed by clamping: the divergence grows
    /// without bound as the zoom approaches zero, so the only rectangle that
    /// would always cover it is an infinite one, and paying for the *largest*
    /// band any item could ever need would bloat every cosmetic-stroke item's
    /// index bucket and its advertised accessibility box at every zoom. The
    /// mitigation is that zooming out that far turns a hairline into a hair:
    /// at 0.25× a 40 px cosmetic stroke covers 160 local units, which is a
    /// blot on screen rather than a line anyone is aiming at.
    pub fn bounding_rect(&self) -> Rect {
        self.bounding_rect_at(1.0)
    }

    /// [`ItemShape::bounding_rect`] at an explicit view zoom — the rectangle
    /// that really does enclose everything [`ItemShape::contains`] accepts at
    /// that zoom.
    ///
    /// Differs from `bounding_rect()` only for a [`StrokeSpace::Device`] band,
    /// and only away from 1×.
    pub fn bounding_rect_at(&self, view_scale: f32) -> Rect {
        self.geometry_bounds()
            .expand(self.band_half_width(view_scale).unwrap_or(0.0))
    }

    /// Whether this shape can never be hit ([`ItemShape::none`]).
    pub fn is_none(&self) -> bool {
        matches!(self.kind, ShapeKind::None)
    }

    /// Whether the shape and `region` share **area** — the `Intersects…`
    /// half of [`ItemSelectionMode`].
    ///
    /// `region` must already be in **this shape's** coordinate space — call
    /// [`SceneRegion::to_local`] first. `view_scale` is as for
    /// [`ItemShape::contains`].
    ///
    /// # Edges belong to no one
    ///
    /// Sharing *area* is the rule, so two sets that meet only along their
    /// boundaries do **not** intersect: a marquee laid exactly on an item's
    /// edge picks it up under no mode. That is what `Scene::items_in_rect`
    /// has always answered (its rectangle compare is strict on all four
    /// sides), and both paths below answer it — the one-compare fast path for
    /// a plain box against an axis-aligned band, and the general outline
    /// algebra for everything else. The two disagreeing is what let a rounded
    /// tile be picked by a band its square twin was not, and made
    /// `IntersectsItemShape` report a hit where `IntersectsItemBoundingRect`
    /// reported none — impossible for a shape that lies inside its own box.
    ///
    /// [`ItemShape::contains`], by contrast, is **closed**: a point on the
    /// edge of a rectangle is on the rectangle. A point has no area to share,
    /// so the two rules are about different questions and neither follows
    /// from the other.
    ///
    /// The one exception is a region — or a shape — that *is* all boundary: a
    /// zero-width path, which is exactly what
    /// [`Scene::items_along_path`](crate::Scene::items_along_path) builds, or
    /// a degenerate box. An area rule would answer "no" to every such query,
    /// so when either side has no area of its own the question becomes "does
    /// this line touch the item?" and contact is the answer — the behaviour
    /// that query has always had.
    ///
    /// # How the general path witnesses an overlap
    ///
    /// Four witnesses, any one of which **proves** shared area, checked in
    /// increasing cost order: a probe point of the shape's outline strictly
    /// inside the region; a probe point of the region strictly inside the
    /// shape; a pair of boundary segments that properly cross (or, with a
    /// band, come closer than the two half-widths together); and — last —
    /// every vertex of the shape inside the *closed* region, which is the only
    /// witness that survives two outlines lying exactly on top of each other.
    /// That last case is not exotic: it is a duplicate of a rotated item laid
    /// over the original, which `colliding_items` really does get asked about,
    /// and without it two identical rotated cards would report no collision.
    ///
    /// A **probe point** is a vertex *or a segment midpoint*. The midpoints
    /// are load-bearing rather than belt-and-braces: two rectangles with the
    /// same top and bottom edges, overlapping in x — a marquee dragged to
    /// exactly an item's height, which is a gesture, not a curiosity — put
    /// every vertex of each on the other's boundary and cross nothing
    /// transversally, so the vertices alone witness nothing.
    ///
    /// Every witness is **sound** (each really does imply shared area), and
    /// together they are not quite **complete**: a sliver of overlap that no
    /// probe point lands in and no segment pair crosses transversally reports
    /// no intersection. Erring that way is deliberate — a false positive here
    /// would let a `…ItemShape` query pick an item its own bounding rect does
    /// not, which is impossible for a shape that lies inside its box, whereas
    /// a false negative only ever agrees with the cheaper mode.
    ///
    /// Where a probe lands exactly on a freeform outline, "strictly inside"
    /// is decided within [`SHAPE_FLATTEN_TOLERANCE`]: a ray cast against a
    /// polyline has no meaningful answer on the polyline itself. Against a
    /// rectangle, the closed form every default shape and every marquee band
    /// uses, it is exact.
    pub fn intersects_region(&self, region: &SceneRegion, view_scale: f32) -> bool {
        if self.is_none() || region.is_empty() {
            return false;
        }
        // Area on both sides, or the all-boundary exception above.
        let strict = self.has_area(view_scale) && region.has_area();
        // The common case: an untransformed default-shape item against an
        // axis-aligned band. One compare, and the same answer the general
        // path below would have reached.
        if let (Some(r), Some(reg)) = (self.plain_rect(), region.as_rect()) {
            return if strict {
                rects_intersect_exclusive(r, reg)
            } else {
                rects_overlap(r, reg)
            };
        }
        let item_half = self.band_half_width(view_scale).unwrap_or(0.0);
        // Inclusive, because this only ever rejects: a conservative reject
        // cannot drop a pair the exact test would have kept.
        if !rects_overlap(
            self.geometry_bounds().expand(item_half),
            region.bounding_rect(),
        ) {
            return false;
        }

        let item = self.outline_ref();
        let item_out = item.as_slice();
        for p in probe_points(item_out) {
            if region.contains_point_at(p, strict) {
                return true;
            }
        }
        let reg = region.outline_ref();
        let reg_out = reg.as_slice();
        for p in probe_points(reg_out) {
            if self.contains_at(p, view_scale, strict) {
                return true;
            }
        }
        let slack = item_half + region.band_half_width();
        if outlines_meet(item_out, reg_out, slack, strict) {
            return true;
        }
        // Coincident boundaries: no vertex is strictly inside anything and
        // nothing crosses, yet the two cover the same ground. (Without
        // `strict` the inclusive tests above have already caught this.)
        strict
            && item_out.iter().any(|sp| !sp.points.is_empty())
            && item_out
                .iter()
                .all(|sp| sp.points.iter().all(|p| region.clearance(*p) >= 0.0))
    }

    /// Whether the shape lies entirely inside `region` — the `Contains…` half
    /// of [`ItemSelectionMode`].
    ///
    /// `region` must already be in this shape's coordinate space. A shape with
    /// a stroke band is contained only when the **band** is — the whole
    /// clickable ribbon, not merely the centreline.
    ///
    /// # Edges are inside
    ///
    /// Containment is of **closed** sets, so a shape resting exactly on the
    /// region's boundary is contained: an item and a band drawn at precisely
    /// the same rectangle select under `Contains…`. That is what the
    /// rectangle fast path has always answered, and the general path answers
    /// it too — an outline lying *along* the region's edge is not a crossing,
    /// only an outline passing *through* it is. (Before, a collinear edge
    /// counted as a crossing, so a rounded tile an exact-fit band contained
    /// was rejected while its square twin was accepted.)
    ///
    /// Note the asymmetry with [`ItemShape::intersects_region`], and that it
    /// is the only consistent pair: an exact-fit band both contains the item
    /// and shares its whole area, while a band merely *abutting* the item
    /// does neither.
    ///
    /// # Two kinds of boundary
    ///
    /// For a region with an **interior**, "every vertex is inside and no edge
    /// crosses the outline" settles it: an outline that never crosses the
    /// region's boundary cannot have its inside on the other side of it.
    ///
    /// A **stroke band** has no such outline. Its boundary is an offset curve
    /// the region never stores, and the outline it does store is the band's
    /// *centreline* — which runs down the middle of the region rather than
    /// around it. Testing crossings against that gets both answers wrong: an
    /// item sitting squarely on a wire crosses the centreline and is reported
    /// as escaping, while a box whose four corners are all within a bent
    /// wire's band bulges out across the inside of the bend, crosses nothing,
    /// and is reported as contained. Each item edge is therefore checked
    /// against the band itself, by an exact segment-in-band test.
    ///
    /// A region with both is inside-the-fill **or** inside-the-band; an item
    /// weaving between the two, inside their union but inside neither part, is
    /// reported as not contained. That is the one deliberately conservative
    /// answer here, and it is one-sided: never a false containment.
    pub fn contained_by_region(&self, region: &SceneRegion, view_scale: f32) -> bool {
        if self.is_none() || region.is_empty() {
            return false;
        }
        if let (Some(r), Some(reg)) = (self.plain_rect(), region.as_rect()) {
            return rect_contains_rect(reg, r);
        }
        let item_half = self.band_half_width(view_scale).unwrap_or(0.0);
        // Containment in the region implies containment in its AABB, so this
        // only ever rejects.
        if !rect_contains_rect(
            region.bounding_rect(),
            self.geometry_bounds().expand(item_half),
        ) {
            return false;
        }
        let item = self.outline_ref();
        let item_out = item.as_slice();
        if item_out.iter().all(|sp| sp.points.is_empty()) {
            return false;
        }
        for sp in item_out {
            for p in &sp.points {
                if region.clearance(*p) < item_half {
                    return false;
                }
            }
        }
        let reg = region.outline_ref();
        // Inside the ribbon: every edge, whole, within what the band has left
        // over after the item's own band is taken out of it.
        let band_half = region.band_half_width();
        if band_half > 0.0 {
            let allowance = band_half - item_half;
            if allowance > 0.0
                && item_out.iter().all(|sp| {
                    sp.segments(true)
                        .all(|(a, b)| segment_within_band(a, b, reg.as_slice(), allowance))
                })
            {
                return true;
            }
        }
        // Inside the interior: a region with a hole can contain every vertex
        // and still be crossed, and a band on the item can spill out between
        // two vertices that are both clear.
        region.has_fill() && !outlines_meet(item_out, reg.as_slice(), item_half, true)
    }

    /// Re-publish this shape as a [`SceneRegion`] in another frame — its own
    /// scene transform, so a collision query can ask the rest of the scene
    /// about it.
    ///
    /// The outline goes through [`Path::transformed`], which is exact for
    /// every affine (an arc a transform cannot carry as an arc is expanded
    /// into the cubics the renderer paints it as), so the republished region
    /// keeps its curves and is re-flattened at
    /// [`SHAPE_FLATTEN_TOLERANCE`] in the *target* frame rather than
    /// inheriting a polyline sampled in this one.
    ///
    /// A band's width scales by the transform's smallest stretch — see
    /// [`SceneRegion::to_local`], which faces the same anisotropy.
    pub fn to_scene_region(&self, local_to_scene: &Transform2D) -> SceneRegion {
        if self.is_none() {
            return SceneRegion::empty();
        }
        if let Some(r) = self.plain_rect() {
            return if axis_preserving(local_to_scene) {
                SceneRegion::rect(local_to_scene.apply_rect(r))
            } else {
                SceneRegion::lasso(transformed_rect_path(r, local_to_scene))
            };
        }
        let path = match &self.kind {
            // Handled above.
            ShapeKind::None => Path::new(),
            ShapeKind::Bounds(r) => Path::rect(*r).transformed(local_to_scene),
            ShapeKind::RoundedRect { rect, radius } => {
                Path::rounded_rect(*rect, teksilo_tokens::CornerRadius::uniform(*radius))
                    .transformed(local_to_scene)
            }
            ShapeKind::Ellipse(r) => Path::ellipse(*r).transformed(local_to_scene),
            ShapeKind::Path(g) => g.path().transformed(local_to_scene),
        };
        let (stretch, _) = linear_scale_bounds(local_to_scene);
        let band = self.band_half_width(1.0).map(|h| h * 2.0 * stretch);
        // The closed forms are regions by construction; only the path kind
        // carries an optional fill.
        let fill = match &self.kind {
            ShapeKind::Path(_) => self.fill,
            _ => Some(FillRule::Winding),
        };
        SceneRegion::from_geometry(ShapeGeometry::shared(path), fill, band)
    }

    // -- internals ----------------------------------------------------

    /// `Some(rect)` iff this is the plain-AABB default with no band — the one
    /// case where a shape query and a bounding-rect query are the same test.
    fn plain_rect(&self) -> Option<Rect> {
        match (&self.kind, self.band) {
            (ShapeKind::Bounds(r), None) => Some(*r),
            _ => None,
        }
    }

    /// The shape's extent *before* any band inflation.
    fn geometry_bounds(&self) -> Rect {
        match &self.kind {
            ShapeKind::None => Rect::ZERO,
            ShapeKind::Bounds(r) => *r,
            ShapeKind::RoundedRect { rect, .. } => *rect,
            ShapeKind::Ellipse(r) => *r,
            ShapeKind::Path(g) => g.bounds(),
        }
    }

    /// Half the band's width in **local** coordinates at `view_scale`, grab
    /// slack included. `None` when the shape has no band.
    fn band_half_width(&self, view_scale: f32) -> Option<f32> {
        let band = self.band?;
        let local = if band.space == StrokeSpace::Device && view_scale > 1e-3 {
            band.width / view_scale
        } else {
            band.width
        };
        Some(local * 0.5 + HIT_BAND_SLACK)
    }

    /// [`ItemShape::contains`] with the boundary excluded — inside with room
    /// to spare.
    ///
    /// The `Intersects…` modes ask this rather than `contains`, because they
    /// are about shared *area*: a point resting on the outline witnesses no
    /// overlap. For a freeform outline "on the outline" is resolved within
    /// the flattening tolerance (a ray cast has no meaningful answer on the
    /// polyline it is cast against); for the closed forms it is exact.
    fn contains_at(&self, p: Point, view_scale: f32, strict: bool) -> bool {
        if strict {
            self.contains_strictly(p, view_scale)
        } else {
            self.contains(p, view_scale)
        }
    }

    fn contains_strictly(&self, p: Point, view_scale: f32) -> bool {
        if matches!(self.kind, ShapeKind::None) {
            return false;
        }
        let half = self.band_half_width(view_scale);
        if !self
            .geometry_bounds()
            .expand(half.unwrap_or(0.0))
            .contains(p)
        {
            return false;
        }
        let interior = match &self.kind {
            ShapeKind::None => false,
            ShapeKind::Bounds(r) => rect_contains_strictly(*r, p),
            ShapeKind::RoundedRect { rect, radius } => {
                rounded_rect_contains_strictly(*rect, *radius, p)
            }
            ShapeKind::Ellipse(r) => ellipse_contains_strictly(*r, p),
            ShapeKind::Path(g) => match self.fill {
                Some(rule) => {
                    subpaths_contain_point(g.outline(), p, rule)
                        && !outline_within(&self.outline_ref(), p, 0.0)
                }
                None => false,
            },
        };
        if interior {
            return true;
        }
        match half {
            Some(h) => h > 0.0 && outline_within_strict(&self.outline_ref(), p, h),
            None => false,
        }
    }

    /// Whether the shape covers any area at all — the guard on the
    /// coincident-boundary witness in [`ItemShape::intersects_region`]. A
    /// zero-extent box, or a path with neither a fill nor a band, covers
    /// nothing and cannot overlap anything.
    fn has_area(&self, view_scale: f32) -> bool {
        if self.band_half_width(view_scale).is_some_and(|h| h > 0.0) {
            return true;
        }
        match &self.kind {
            ShapeKind::None => false,
            ShapeKind::Bounds(r) | ShapeKind::Ellipse(r) => r.width > 0.0 && r.height > 0.0,
            ShapeKind::RoundedRect { rect, .. } => rect.width > 0.0 && rect.height > 0.0,
            ShapeKind::Path(_) => self.fill.is_some(),
        }
    }

    fn interior_contains(&self, p: Point) -> bool {
        match &self.kind {
            ShapeKind::None => false,
            ShapeKind::Bounds(r) => r.contains(p),
            ShapeKind::RoundedRect { rect, radius } => rounded_rect_contains(*rect, *radius, p),
            ShapeKind::Ellipse(r) => ellipse_contains(*r, p),
            ShapeKind::Path(g) => match self.fill {
                Some(rule) => subpaths_contain_point(g.outline(), p, rule),
                None => false,
            },
        }
    }

    fn outline_within(&self, p: Point, half: f32) -> bool {
        outline_within(&self.outline_ref(), p, half)
    }

    fn outline_ref(&self) -> Outline<'_> {
        match &self.kind {
            ShapeKind::None => Outline::Empty,
            ShapeKind::Bounds(r) => Outline::owned(Path::rect(*r)),
            ShapeKind::RoundedRect { rect, radius } => Outline::owned(Path::rounded_rect(
                *rect,
                teksilo_tokens::CornerRadius::uniform(*radius),
            )),
            ShapeKind::Ellipse(r) => Outline::owned(Path::ellipse(*r)),
            ShapeKind::Path(g) => Outline::Borrowed {
                subpaths: g.outline(),
                boxes: &g.flattened().boxes,
                chunks: &g.flattened().chunks,
            },
        }
    }
}

impl Default for ItemShape {
    /// [`ItemShape::none`] — a shape that has not been declared cannot be
    /// claimed to cover anything.
    fn default() -> Self {
        Self::none()
    }
}

/// A flattened outline plus its per-subpath boxes, borrowed from a memo when
/// there is one and built on the spot for the closed forms (which only need it
/// for a region query — never for a point test).
enum Outline<'a> {
    Empty,
    Borrowed {
        subpaths: &'a [Subpath],
        boxes: &'a [Rect],
        chunks: &'a [Vec<Rect>],
    },
    Owned {
        subpaths: Vec<Subpath>,
        boxes: Vec<Rect>,
        chunks: Vec<Vec<Rect>>,
    },
}

impl Outline<'_> {
    fn owned(path: Path) -> Self {
        let subpaths = path.flatten(SHAPE_FLATTEN_TOLERANCE);
        let boxes = subpaths.iter().map(|sp| sp.bounds()).collect();
        let chunks = chunk_boxes(&subpaths);
        Outline::Owned {
            subpaths,
            boxes,
            chunks,
        }
    }

    fn as_slice(&self) -> &[Subpath] {
        match self {
            Outline::Empty => &[],
            Outline::Borrowed { subpaths, .. } => subpaths,
            Outline::Owned { subpaths, .. } => subpaths,
        }
    }

    fn boxes(&self) -> &[Rect] {
        match self {
            Outline::Empty => &[],
            Outline::Borrowed { boxes, .. } => boxes,
            Outline::Owned { boxes, .. } => boxes,
        }
    }

    fn chunks(&self, subpath: usize) -> &[Rect] {
        match self {
            Outline::Empty => &[],
            Outline::Borrowed { chunks, .. } => chunks.get(subpath).map_or(&[], |c| c.as_slice()),
            Outline::Owned { chunks, .. } => chunks.get(subpath).map_or(&[], |c| c.as_slice()),
        }
    }
}

// ---------------------------------------------------------------------------
// ItemSelectionMode
// ---------------------------------------------------------------------------

/// How a region query decides whether an item is picked — Qt's
/// `Qt::ItemSelectionMode`, one variant for one.
///
/// The `…ItemShape` modes consult [`SceneItem::shape`](crate::SceneItem::shape)
/// with the region mapped into the item's **local** frame; the
/// `…ItemBoundingRect` modes compare the item's **scene** AABB, in scene
/// space. For an item whose transform is identity and whose shape is the
/// default AABB the two coincide exactly. For an item carrying a rotation or
/// a scale they do **not**: the scene AABB of a rotated box is the enlarged
/// axis-aligned hull of it, so a bounding-rect query is strictly the looser
/// test. That is a real behaviour difference, not a rounding one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ItemSelectionMode {
    /// Picked when its **shape** intersects the region. Qt's default and
    /// Teksilo's: it is the rule that makes a rubber band agree with a click
    /// about what an item is.
    #[default]
    IntersectsItemShape,
    /// Picked when its shape lies entirely inside the region — "select what
    /// the lasso fully encircles".
    ContainsItemShape,
    /// Picked when its scene AABB intersects the region. The cheap mode, and
    /// the one that reproduces the pre-`ItemShape` marquee.
    IntersectsItemBoundingRect,
    /// Picked when its scene AABB lies entirely inside the region.
    ContainsItemBoundingRect,
}

impl ItemSelectionMode {
    /// Whether this mode consults the item's shape (rather than its AABB).
    pub fn uses_shape(self) -> bool {
        matches!(self, Self::IntersectsItemShape | Self::ContainsItemShape)
    }

    /// Whether this mode requires full containment (rather than any overlap).
    pub fn requires_containment(self) -> bool {
        matches!(
            self,
            Self::ContainsItemShape | Self::ContainsItemBoundingRect
        )
    }
}

// ---------------------------------------------------------------------------
// SceneRegion
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum RegionKind {
    Rect(Rect),
    Path {
        geometry: Rc<ShapeGeometry>,
        fill: Option<FillRule>,
        band: Option<f32>,
    },
}

/// The region a geometry query asks about.
///
/// Two forms, and the difference matters:
///
/// * a **rectangle** — the rubber band under an unrotated view, and the one
///   form that keeps a query on the same one-rect-compare fast path the
///   pre-`ItemShape` marquee took;
/// * a **path** — the exact quadrilateral of a rubber band under a *rotated*
///   view (rather than its enlarged axis-aligned hull, which over-selects), a
///   freehand lasso, or a stroked path for "what lies along this connector?".
///
/// A region is in **scene** coordinates until [`SceneRegion::to_local`] maps
/// it into an item's frame; a rectangle that survives that map as a rectangle
/// stays one, and one that does not becomes the exact transformed
/// quadrilateral rather than being rounded up to an AABB.
#[derive(Debug, Clone)]
pub struct SceneRegion {
    kind: RegionKind,
}

impl SceneRegion {
    /// An axis-aligned box.
    pub fn rect(rect: Rect) -> Self {
        Self {
            kind: RegionKind::Rect(rect),
        }
    }

    /// A region that covers nothing. Every query against it is `false`.
    pub fn empty() -> Self {
        Self {
            kind: RegionKind::Path {
                geometry: ShapeGeometry::shared(Path::new()),
                fill: None,
                band: None,
            },
        }
    }

    /// A closed freehand region, filled under [`FillRule::Winding`].
    pub fn lasso(path: Path) -> Self {
        Self::lasso_with_rule(path, FillRule::Winding)
    }

    /// A closed freehand region under an explicit fill rule.
    pub fn lasso_with_rule(path: Path, rule: FillRule) -> Self {
        Self {
            kind: RegionKind::Path {
                geometry: ShapeGeometry::shared(path),
                fill: Some(rule),
                band: None,
            },
        }
    }

    /// The **stroke band** of a path, `width` wide — "what lies along this
    /// line?" rather than "what lies inside this loop?".
    pub fn stroke(path: Path, width: f32) -> Self {
        Self {
            kind: RegionKind::Path {
                geometry: ShapeGeometry::shared(path),
                fill: None,
                band: Some(width.max(0.0)),
            },
        }
    }

    /// A region from a shared geometry handle, with an explicit interior
    /// and/or band. The general constructor the others are sugar for; used to
    /// re-publish an item's own shape as the region a collision query asks
    /// about.
    pub fn from_geometry(
        geometry: Rc<ShapeGeometry>,
        fill: Option<FillRule>,
        band: Option<f32>,
    ) -> Self {
        Self {
            kind: RegionKind::Path {
                geometry,
                fill,
                band: band.map(|w| w.max(0.0)),
            },
        }
    }

    /// A screen-space rectangle mapped through `screen_to_scene`.
    ///
    /// Stays a rectangle when the transform preserves axes, and becomes the
    /// exact transformed quadrilateral when it does not — which is what stops
    /// a rubber band under a rotated view from selecting everything in the
    /// enlarged hull of itself.
    pub fn from_screen_rect(screen_rect: Rect, screen_to_scene: &Transform2D) -> Self {
        if axis_preserving(screen_to_scene) {
            Self::rect(screen_to_scene.apply_rect(screen_rect))
        } else {
            Self::lasso(transformed_rect_path(screen_rect, screen_to_scene))
        }
    }

    /// Broad-phase AABB — what goes to the spatial index. Band-inflated.
    pub fn bounding_rect(&self) -> Rect {
        match &self.kind {
            RegionKind::Rect(r) => *r,
            RegionKind::Path { geometry, .. } => geometry.bounds().expand(self.band_half_width()),
        }
    }

    /// `Some(rect)` iff this region is an axis-aligned box with no band.
    pub fn as_rect(&self) -> Option<Rect> {
        match &self.kind {
            RegionKind::Rect(r) => Some(*r),
            _ => None,
        }
    }

    /// Whether the region covers nothing at all.
    pub fn is_empty(&self) -> bool {
        match &self.kind {
            RegionKind::Rect(_) => false,
            RegionKind::Path {
                geometry,
                fill,
                band,
            } => geometry.outline().is_empty() || (fill.is_none() && band.is_none()),
        }
    }

    /// Map into another coordinate frame — an item's local space, via the
    /// inverse of its scene transform.
    ///
    /// A rectangle survives as a rectangle only when the transform preserves
    /// axes; otherwise it becomes the exact transformed quadrilateral. A path
    /// goes through [`Path::transformed`], which is exact for every affine —
    /// including an arc, which it expands into cubics rather than moving the
    /// rectangle an arc is stored as and keeping its angles.
    ///
    /// # A band under an anisotropic map
    ///
    /// A band is a *distance*, and an affine that stretches one axis more than
    /// another does not carry a distance: the exact image of a round band is
    /// an elliptical one, which no single width can describe. A rotation, a
    /// uniform scale and a translation are all isotropic, so for every one of
    /// them the mapped width is exact and none of this applies.
    ///
    /// Under a genuinely anisotropic map the width is scaled by the
    /// transform's **smallest** stretch — its least singular value — which is
    /// the widest uniform band that fits *inside* the true image. Under
    /// `scale(1, 10)` the true band is between one and ten times as wide
    /// depending on the direction, and the mapped one claims one: it can
    /// under-reach along the stretched axis, and it never claims ground the
    /// true band does not cover.
    ///
    /// That direction is not a coin flip. A region query that over-reached
    /// could pick an item under `IntersectsItemShape` that
    /// `IntersectsItemBoundingRect` did not — impossible for a shape that lies
    /// inside its own bounding rect, and the exact incoherence between a
    /// marquee and a click that `ItemShape` exists to remove. Under-reaching
    /// only ever agrees with the cheaper mode.
    /// `Transform2D::geometric_scale` — the geometric *mean* of the two
    /// stretches — would err in both directions at once, over-reaching along
    /// one axis while under-reaching along the other, which is why it is not
    /// used here.
    pub fn to_local(&self, scene_to_local: &Transform2D) -> SceneRegion {
        match &self.kind {
            RegionKind::Rect(r) => {
                if axis_preserving(scene_to_local) {
                    SceneRegion::rect(scene_to_local.apply_rect(*r))
                } else {
                    SceneRegion::lasso(transformed_rect_path(*r, scene_to_local))
                }
            }
            RegionKind::Path {
                geometry,
                fill,
                band,
            } => {
                let (stretch, _) = linear_scale_bounds(scene_to_local);
                SceneRegion {
                    kind: RegionKind::Path {
                        geometry: ShapeGeometry::shared(
                            geometry.path().transformed(scene_to_local),
                        ),
                        fill: *fill,
                        band: band.map(|w| w * stretch),
                    },
                }
            }
        }
    }

    /// Whether `p` — in the region's own coordinate space — is inside it.
    pub fn contains_point(&self, p: Point) -> bool {
        match &self.kind {
            RegionKind::Rect(r) => r.contains(p),
            RegionKind::Path {
                geometry,
                fill,
                band,
            } => {
                if let Some(rule) = fill
                    && subpaths_contain_point(geometry.outline(), p, *rule)
                {
                    return true;
                }
                match band {
                    Some(_) => {
                        let _ = geometry;
                        outline_within(&self.outline_ref(), p, self.band_half_width())
                    }
                    None => false,
                }
            }
        }
    }

    /// Whether the region covers any **area** — `false` for a zero-width
    /// path (a bare line: the `items_along_path` query) and for a degenerate
    /// box. See [`ItemShape::intersects_region`], which relaxes its rule for
    /// exactly this case.
    ///
    /// A filled outline needs at least three points on some subpath to
    /// enclose anything; two points are a line, which
    /// [`subpaths_contain_point`] already declines to fill.
    pub fn has_area(&self) -> bool {
        match &self.kind {
            RegionKind::Rect(r) => r.width > 0.0 && r.height > 0.0,
            RegionKind::Path {
                geometry,
                fill,
                band,
            } => {
                if band.is_some_and(|w| w > 0.0) {
                    return true;
                }
                fill.is_some() && geometry.outline().iter().any(|sp| sp.points.len() >= 3)
            }
        }
    }

    fn contains_point_at(&self, p: Point, strict: bool) -> bool {
        if strict {
            self.contains_point_strictly(p)
        } else {
            self.contains_point(p)
        }
    }

    /// [`SceneRegion::contains_point`] with the boundary excluded — see
    /// [`ItemShape::intersects_region`] for why an `Intersects…` query asks
    /// this instead.
    fn contains_point_strictly(&self, p: Point) -> bool {
        match &self.kind {
            RegionKind::Rect(r) => rect_contains_strictly(*r, p),
            RegionKind::Path {
                geometry,
                fill,
                band,
            } => {
                if let Some(rule) = fill
                    && subpaths_contain_point(geometry.outline(), p, *rule)
                    && !outline_within(&self.outline_ref(), p, 0.0)
                {
                    return true;
                }
                match band {
                    Some(_) => {
                        let _ = geometry;
                        let half = self.band_half_width();
                        half > 0.0 && outline_within_strict(&self.outline_ref(), p, half)
                    }
                    None => false,
                }
            }
        }
    }

    /// How far `p` is from leaving the region: positive inside, negative
    /// outside, in the region's own units. Used by the containment modes to
    /// ask whether a *band* — not merely a centreline — fits inside.
    pub fn clearance(&self, p: Point) -> f32 {
        match &self.kind {
            RegionKind::Rect(r) => (p.x - r.x)
                .min(r.right() - p.x)
                .min(p.y - r.y)
                .min(r.bottom() - p.y),
            RegionKind::Path {
                geometry,
                fill,
                band,
            } => {
                let dist = min_distance_to_outline(geometry.outline(), p);
                let mut best = f32::NEG_INFINITY;
                if let Some(rule) = fill {
                    let inside = subpaths_contain_point(geometry.outline(), p, *rule);
                    best = best.max(if inside { dist } else { -dist });
                }
                if band.is_some() {
                    best = best.max(self.band_half_width() - dist);
                }
                best
            }
        }
    }

    fn band_half_width(&self) -> f32 {
        match &self.kind {
            RegionKind::Rect(_) => 0.0,
            RegionKind::Path { band, .. } => band.map(|w| w * 0.5).unwrap_or(0.0),
        }
    }

    /// Whether the region has an interior at all — a rectangle always does,
    /// and a path does when it carries a [`FillRule`]. A region without one is
    /// a bare ribbon, whose outline is a centreline rather than a boundary.
    fn has_fill(&self) -> bool {
        match &self.kind {
            RegionKind::Rect(_) => true,
            RegionKind::Path { fill, .. } => fill.is_some(),
        }
    }

    fn outline_ref(&self) -> Outline<'_> {
        match &self.kind {
            RegionKind::Rect(r) => Outline::owned(Path::rect(*r)),
            RegionKind::Path { geometry, .. } => Outline::Borrowed {
                subpaths: geometry.outline(),
                boxes: &geometry.flattened().boxes,
                chunks: &geometry.flattened().chunks,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Geometry helpers
// ---------------------------------------------------------------------------

/// Whether `p` is within `half` of any segment of `out`.
///
/// Two levels of rejection stand in front of the distance maths: the subpath's
/// own box, then a box per run of [`SEGMENTS_PER_CHUNK`] segments inside it.
/// The second is what matters for a single long stroke — one subpath, hundreds
/// of segments — which is the shape a freehand ink line takes and the one a
/// hover sample has to answer "no" for on every frame.
fn outline_within(out: &Outline<'_>, p: Point, half: f32) -> bool {
    outline_within_cmp(out, p, half, false)
}

/// [`outline_within`] with the band's own edge excluded — `distance < half`
/// rather than `<=`. The strict twin the `Intersects…` rule needs: a point at
/// exactly the band's edge touches it and shares no area with it.
fn outline_within_strict(out: &Outline<'_>, p: Point, half: f32) -> bool {
    outline_within_cmp(out, p, half, true)
}

fn outline_within_cmp(out: &Outline<'_>, p: Point, half: f32, strict: bool) -> bool {
    let boxes = out.boxes();
    let half_sq = half * half;
    for (i, sp) in out.as_slice().iter().enumerate() {
        if !boxes[i].expand(half).contains(p) {
            continue;
        }
        let chunks = out.chunks(i);
        let mut live = chunks
            .first()
            .map(|r| r.expand(half).contains(p))
            .unwrap_or(true);
        for (seg, (a, b)) in sp.segments(false).enumerate() {
            if seg > 0 && seg.is_multiple_of(SEGMENTS_PER_CHUNK) {
                live = chunks
                    .get(seg / SEGMENTS_PER_CHUNK)
                    .map(|r| r.expand(half).contains(p))
                    .unwrap_or(true);
            }
            if !live {
                continue;
            }
            let d_sq = point_to_segment_distance_sq(p, a, b);
            if if strict {
                d_sq < half_sq
            } else {
                d_sq <= half_sq
            } {
                return true;
            }
        }
    }
    false
}

/// Every vertex of `out`, plus the midpoint of every segment.
///
/// Both are points of the outline, so a probe strictly inside the other set
/// proves the two share area. The midpoints are what catch an overlap whose
/// whole boundary contribution from one side lies *between* two vertices of
/// the other — see [`ItemShape::intersects_region`]. Only real segments are
/// walked (`segments(false)`), never a subpath's implied closing chord, which
/// is not part of a stroke band.
fn probe_points(out: &[Subpath]) -> impl Iterator<Item = Point> + '_ {
    out.iter().flat_map(|sp| {
        sp.points.iter().copied().chain(
            sp.segments(false)
                .map(|(a, b)| Point::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5)),
        )
    })
}

/// Whether **every** point of the segment `a → b` lies within `half` of
/// `outline` — whether the segment stays inside the band, rather than merely
/// starting and ending inside it.
///
/// A band is a union of capsules, one per outline segment. A capsule is
/// **convex**, so it covers a single closed interval of the segment's
/// parameter; collect one interval per capsule and ask whether they cover
/// `[0, 1]`. Exact, and O(segments) — no sampling, so an edge that dips out of
/// the band between two points that are both inside it cannot slip through.
///
/// `false` for a non-positive `half`: a ribbon of no width contains nothing.
fn segment_within_band(a: Point, b: Point, outline: &[Subpath], half: f32) -> bool {
    if !half.is_finite() || half <= 0.0 {
        return false;
    }
    let mut spans: Vec<(f32, f32)> = Vec::new();
    for sp in outline {
        for (c, d) in sp.segments(false) {
            if let Some(span) = capsule_span(a, b, c, d, half) {
                spans.push(span);
            }
        }
    }
    covers_unit_interval(&mut spans)
}

/// The interval of `a → b`'s parameter that lies inside the capsule of radius
/// `half` around `c → d`.
///
/// The capsule is a rectangle plus a disc at each end; each piece contributes
/// an interval, and because the capsule is convex their union is the single
/// interval from the earliest start to the latest end.
fn capsule_span(a: Point, b: Point, c: Point, d: Point, half: f32) -> Option<(f32, f32)> {
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for piece in [
        disc_span(a, b, c, half),
        disc_span(a, b, d, half),
        slab_span(a, b, c, d, half),
    ]
    .into_iter()
    .flatten()
    {
        lo = lo.min(piece.0);
        hi = hi.max(piece.1);
    }
    (lo <= hi).then_some((lo, hi))
}

/// The part of `a → b` inside the disc of radius `half` about `centre`, as a
/// parameter interval clipped to `[0, 1]`.
fn disc_span(a: Point, b: Point, centre: Point, half: f32) -> Option<(f32, f32)> {
    let ex = b.x - a.x;
    let ey = b.y - a.y;
    let fx = a.x - centre.x;
    let fy = a.y - centre.y;
    let qa = ex * ex + ey * ey;
    let qc = fx * fx + fy * fy - half * half;
    if qa < 1e-12 {
        // A degenerate edge is one point: in or out, whole.
        return (qc <= 0.0).then_some((0.0, 1.0));
    }
    let qb = 2.0 * (fx * ex + fy * ey);
    let disc = qb * qb - 4.0 * qa * qc;
    if disc < 0.0 {
        return None;
    }
    let root = disc.sqrt();
    let t0 = ((-qb - root) / (2.0 * qa)).max(0.0);
    let t1 = ((-qb + root) / (2.0 * qa)).min(1.0);
    (t0 <= t1).then_some((t0, t1))
}

/// The part of `a → b` inside the capsule's rectangular body — between the two
/// end caps and within `half` of the line through `c → d`.
fn slab_span(a: Point, b: Point, c: Point, d: Point, half: f32) -> Option<(f32, f32)> {
    let ux = d.x - c.x;
    let uy = d.y - c.y;
    let len_sq = ux * ux + uy * uy;
    if len_sq < 1e-12 {
        // No body: the end caps are the whole capsule.
        return None;
    }
    let ex = b.x - a.x;
    let ey = b.y - a.y;
    let wx = a.x - c.x;
    let wy = a.y - c.y;
    // Along the body: 0 <= (w + t e) . u <= |u|^2.
    let along = wx * ux + wy * uy;
    let along_rate = ex * ux + ey * uy;
    // Across it: |(w + t e) x u| <= half * |u|.
    let across = wx * uy - wy * ux;
    let across_rate = ex * uy - ey * ux;
    let reach = half * len_sq.sqrt();

    let mut lo = 0.0_f32;
    let mut hi = 1.0_f32;
    for (rate, offset) in [
        (along_rate, along),
        (-along_rate, len_sq - along),
        (across_rate, across + reach),
        (-across_rate, reach - across),
    ] {
        if !clip_half_plane(&mut lo, &mut hi, rate, offset) {
            return None;
        }
    }
    (lo <= hi).then_some((lo, hi))
}

/// Narrow `[lo, hi]` to where `rate * t + offset >= 0`. `false` when nothing
/// is left.
fn clip_half_plane(lo: &mut f32, hi: &mut f32, rate: f32, offset: f32) -> bool {
    if rate.abs() < 1e-12 {
        return offset >= 0.0;
    }
    let t = -offset / rate;
    if rate > 0.0 {
        *lo = lo.max(t);
    } else {
        *hi = hi.min(t);
    }
    *lo <= *hi
}

/// Whether the spans, in any order, cover `[0, 1]` with no gap.
fn covers_unit_interval(spans: &mut [(f32, f32)]) -> bool {
    const EPS: f32 = 1e-5;
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut reach = 0.0_f32;
    for (start, end) in spans.iter() {
        if *start > reach + EPS {
            return false;
        }
        reach = reach.max(*end);
        if reach >= 1.0 - EPS {
            return true;
        }
    }
    reach >= 1.0 - EPS
}

/// Shortest distance from a point to a line segment.
pub(crate) fn point_to_segment_distance(p: Point, a: Point, b: Point) -> f32 {
    point_to_segment_distance_sq(p, a, b).sqrt()
}

/// [`point_to_segment_distance`] without the square root — compare against a
/// squared threshold. The hot path takes this one: a band test runs once per
/// surviving segment per pointer sample.
fn point_to_segment_distance_sq(p: Point, a: Point, b: Point) -> f32 {
    let abx = b.x - a.x;
    let aby = b.y - a.y;
    let len2 = abx * abx + aby * aby;
    if len2 < 1e-6 {
        let dx = p.x - a.x;
        let dy = p.y - a.y;
        return dx * dx + dy * dy;
    }
    let apx = p.x - a.x;
    let apy = p.y - a.y;
    let t = ((apx * abx + apy * aby) / len2).clamp(0.0, 1.0);
    let cx = a.x + t * abx;
    let cy = a.y + t * aby;
    let dx = p.x - cx;
    let dy = p.y - cy;
    dx * dx + dy * dy
}

fn min_distance_to_outline(subpaths: &[Subpath], p: Point) -> f32 {
    let mut best = f32::INFINITY;
    for sp in subpaths {
        for (a, b) in sp.segments(true) {
            best = best.min(point_to_segment_distance(p, a, b));
        }
    }
    if best.is_finite() { best } else { 0.0 }
}

/// Whether the two outlines meet: a segment pair that crosses, or — when
/// `slack` is positive — a pair closer together than `slack`.
///
/// Under `strict` both tests exclude contact: only a *transversal* crossing
/// counts, and two bands whose edges exactly touch do not. Two boundaries
/// lying along each other share no area, and reading a collinear overlap as a
/// crossing is what made a band exactly fitting a rounded item report the item
/// as not contained while its square twin was. Without `strict` — the
/// all-boundary case, where there is no area to share — contact is the whole
/// question and a touch counts.
fn outlines_meet(a: &[Subpath], b: &[Subpath], slack: f32, strict: bool) -> bool {
    for sa in a {
        let abox = sa.bounds().expand(slack);
        for sb in b {
            if !rects_overlap(abox, sb.bounds()) {
                continue;
            }
            for (a1, a2) in sa.segments(true) {
                for (b1, b2) in sb.segments(true) {
                    if segments_cross_properly(a1, a2, b1, b2) {
                        return true;
                    }
                    if !strict && segments_touch_collinearly(a1, a2, b1, b2) {
                        return true;
                    }
                    if slack > 0.0 {
                        let d = segment_distance(a1, a2, b1, b2);
                        if if strict { d < slack } else { d <= slack } {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

fn segment_distance(a1: Point, a2: Point, b1: Point, b2: Point) -> f32 {
    point_to_segment_distance(a1, b1, b2)
        .min(point_to_segment_distance(a2, b1, b2))
        .min(point_to_segment_distance(b1, a1, a2))
        .min(point_to_segment_distance(b2, a1, a2))
}

fn cross(o: Point, a: Point, b: Point) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// Whether the two segments cross **transversally** — each strictly
/// separates the other's endpoints.
///
/// Touching at an endpoint, and lying collinearly along one another, both
/// return `false`: each is boundary contact, and boundary contact is not
/// shared area.
fn segments_cross_properly(p1: Point, p2: Point, p3: Point, p4: Point) -> bool {
    let d1 = cross(p3, p4, p1);
    let d2 = cross(p3, p4, p2);
    let d3 = cross(p1, p2, p3);
    let d4 = cross(p1, p2, p4);
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

/// Whether the two segments touch without crossing — an endpoint on the other
/// segment, or a collinear overlap. Only the all-boundary case asks this: a
/// zero-width query path entering and leaving a rectangle exactly through two
/// of its corners crosses nothing transversally, and still goes straight
/// through the middle of it.
fn segments_touch_collinearly(p1: Point, p2: Point, p3: Point, p4: Point) -> bool {
    let d1 = cross(p3, p4, p1);
    let d2 = cross(p3, p4, p2);
    let d3 = cross(p1, p2, p3);
    let d4 = cross(p1, p2, p4);
    (d1 == 0.0 && on_segment(p3, p4, p1))
        || (d2 == 0.0 && on_segment(p3, p4, p2))
        || (d3 == 0.0 && on_segment(p1, p2, p3))
        || (d4 == 0.0 && on_segment(p1, p2, p4))
}

fn on_segment(a: Point, b: Point, p: Point) -> bool {
    p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

/// Inclusive rectangle overlap, used **only** to reject: a conservative
/// reject never drops a pair the exact test would have kept. Consistent with
/// [`Rect::contains`], which is what a point test uses.
fn rects_overlap(a: Rect, b: Rect) -> bool {
    a.x <= b.right() && b.x <= a.right() && a.y <= b.bottom() && b.y <= a.bottom()
}

/// Exclusive rectangle overlap — the scene's own `rects_intersect`, repeated
/// here so the box-against-box fast path answers exactly what
/// [`Scene::items_in_rect`](crate::Scene::items_in_rect) has always answered.
fn rects_intersect_exclusive(a: Rect, b: Rect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

/// `Rect::contains` with the boundary excluded.
fn rect_contains_strictly(r: Rect, p: Point) -> bool {
    p.x > r.x && p.x < r.right() && p.y > r.y && p.y < r.bottom()
}

fn rect_contains_rect(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.right() <= outer.right()
        && inner.bottom() <= outer.bottom()
}

fn rounded_rect_contains(rect: Rect, radius: f32, p: Point) -> bool {
    rect.contains(p) && corner_distance_sq(rect, radius, p).is_none_or(|(d, r2)| d <= r2)
}

fn rounded_rect_contains_strictly(rect: Rect, radius: f32, p: Point) -> bool {
    rect_contains_strictly(rect, p)
        && corner_distance_sq(rect, radius, p).is_none_or(|(d, r2)| d < r2)
}

/// `(distance², radius²)` from `p` to the inner (corner-centre) rectangle — a
/// point further than the radius from it is in a transparent corner. `None`
/// when there is no rounding to test against.
fn corner_distance_sq(rect: Rect, radius: f32, p: Point) -> Option<(f32, f32)> {
    let r = radius.min(rect.width * 0.5).min(rect.height * 0.5);
    if r <= 0.0 {
        return None;
    }
    let cx = p.x.clamp(rect.x + r, rect.right() - r);
    let cy = p.y.clamp(rect.y + r, rect.bottom() - r);
    let dx = p.x - cx;
    let dy = p.y - cy;
    Some((dx * dx + dy * dy, r * r))
}

fn ellipse_contains(rect: Rect, p: Point) -> bool {
    ellipse_norm(rect, p).is_some_and(|n| n <= 1.0)
}

fn ellipse_contains_strictly(rect: Rect, p: Point) -> bool {
    ellipse_norm(rect, p).is_some_and(|n| n < 1.0)
}

/// `(p - centre)` in units of the ellipse's own radii, squared: `<= 1` inside,
/// `== 1` on the outline. `None` for a degenerate ellipse, which contains
/// nothing.
fn ellipse_norm(rect: Rect, p: Point) -> Option<f32> {
    let rx = rect.width * 0.5;
    let ry = rect.height * 0.5;
    if rx <= 0.0 || ry <= 0.0 {
        return None;
    }
    let dx = (p.x - (rect.x + rx)) / rx;
    let dy = (p.y - (rect.y + ry)) / ry;
    Some(dx * dx + dy * dy)
}

/// The largest and smallest factor by which `t`'s linear part stretches a
/// direction — the singular values of its 2x2 matrix, smallest first.
///
/// [`Transform2D::geometric_scale`] is their geometric mean (`sqrt |det|`),
/// which is the one right number only when the two agree. Under `scale(1, 10)`
/// they are 1 and 10 and the mean is 3.16 — too wide for the untouched axis
/// and far too narrow for the stretched one, so a band scaled by it is wrong
/// in both directions at once. Returns `(1.0, 1.0)` for a non-finite matrix.
fn linear_scale_bounds(t: &Transform2D) -> (f32, f32) {
    let [a, b, c, d, _, _] = t.m;
    let sum_sq = a * a + b * b + c * c + d * d;
    let det = (a * d - b * c).abs();
    let disc = (sum_sq * sum_sq - 4.0 * det * det).max(0.0).sqrt();
    let hi = ((sum_sq + disc) * 0.5).max(0.0).sqrt();
    let lo = ((sum_sq - disc) * 0.5).max(0.0).sqrt();
    if hi.is_finite() && lo.is_finite() {
        (lo, hi)
    } else {
        (1.0, 1.0)
    }
}

/// Whether the transform maps an axis-aligned rectangle onto another
/// axis-aligned rectangle — a pure translate/scale, or a quarter turn.
fn axis_preserving(t: &Transform2D) -> bool {
    const EPS: f32 = 1e-5;
    let [a, b, c, d, _, _] = t.m;
    (b.abs() < EPS && c.abs() < EPS) || (a.abs() < EPS && d.abs() < EPS)
}

fn transformed_rect_path(r: Rect, t: &Transform2D) -> Path {
    Path::polygon(&[
        t.apply_point(Point::new(r.x, r.y)),
        t.apply_point(Point::new(r.right(), r.y)),
        t.apply_point(Point::new(r.right(), r.bottom())),
        t.apply_point(Point::new(r.x, r.bottom())),
    ])
}

fn union_all(boxes: &[Rect]) -> Rect {
    let mut acc: Option<Rect> = None;
    for b in boxes {
        acc = Some(match acc {
            None => *b,
            Some(a) => {
                let x = a.x.min(b.x);
                let y = a.y.min(b.y);
                Rect::new(
                    x,
                    y,
                    a.right().max(b.right()) - x,
                    a.bottom().max(b.bottom()) - y,
                )
            }
        });
    }
    acc.unwrap_or(Rect::ZERO)
}

#[cfg(test)]
mod tests;
