// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Properties of the four [`ItemSelectionMode`]s against arbitrary items and
//! regions.
//!
//! These exist because the two halves of the geometry algebra are *two*
//! implementations of one rule: a one-compare fast path for a plain box
//! against an axis-aligned band, and outline-vs-outline algebra for everything
//! else. When they disagreed about whether an edge counts, a rounded tile was
//! picked by a band its square twin was not, and `IntersectsItemShape`
//! reported a hit where `IntersectsItemBoundingRect` reported none — which is
//! impossible for a shape that lies inside its own bounding rect. The
//! committed unit tests only exercised the plain box, which is the one case
//! that takes the fast path; a property sweep is what reaches the other one.
//!
//! Every implication is asserted against a region **grown by [`SLACK`]** on
//! the right-hand side, and that is not a hedge: the two selection-mode
//! families compute in two coordinate spaces (`…ItemBoundingRect` compares
//! scene AABBs, while `…ItemShape` answers wherever both sides' stroke bands
//! stay round — the item's own frame when the region's band can be mapped into
//! it, scene space when it cannot), so at an exact tangency — a band whose
//! edge falls precisely on an item's — they round differently and *must*.
//! `SLACK` is 0.05 against a generator on a 0.5 grid, so it swallows the
//! rounding and nothing else: every defect this suite has caught was off by a
//! factor, not by a hundredth.
//!
//! The anisotropic transforms in `arb_transform` are load-bearing and must
//! stay: `scale(sx, sy)` with `sx != sy` is the only input that separates the
//! two frames, and it is what caught the band-mapping defect the frame
//! decision now exists for.
//!
//! Everything is queried at **unit view scale**, deliberately: a
//! [`StrokeSpace::Device`](teksilo_canvas::StrokeSpace) band is wider than the
//! item's own bounds below 1x zoom, which is the one documented case where a
//! shape genuinely does leave its box (see `ItemShape::bounding_rect`). At 1x
//! the containment the modes depend on holds for every item.
//!
//! Run deeper: `PROPTEST_CASES=4096 cargo test -p teksilo-scene --test prop_selection_modes`.
//! Per [docs/property-testing.md](../../../docs/property-testing.md), build
//! with `--no-run` and run the binary under `ulimit -v`/`-t` rather than
//! executing a suite directly.

use std::collections::BTreeSet;

use proptest::prelude::*;
use teksilo_canvas::{FillRule, Path, Point, Rect, StrokeSpace, StrokeStyle, Transform2D};
use teksilo_scene::{
    ItemId, ItemSelectionMode, PathItem, RectItem, Scene, SceneRegion, ShapeGeometry,
};

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------
//
// Cost: the worst case is 6 items, each a 6-point `PathItem` with a stroke, in
// one scene, against a 6-point lasso — so the narrow phase walks at most
// 6 x (6 x 6) segment pairs per mode, four modes, per case. Microseconds. Every
// coordinate is bounded to +/-200 and every extent to 200, so no generated
// value can reach the spatial index's oversized-item path (the incident
// documented in docs/property-testing.md) or produce a non-finite rectangle.

fn arb_coord() -> impl Strategy<Value = f32> {
    (-200i32..=200i32).prop_map(|v| v as f32 * 0.5)
}

fn arb_extent() -> impl Strategy<Value = f32> {
    (0i32..=200i32).prop_map(|v| v as f32)
}

fn arb_rect() -> impl Strategy<Value = Rect> {
    (arb_coord(), arb_coord(), arb_extent(), arb_extent())
        .prop_map(|(x, y, w, h)| Rect::new(x, y, w, h))
}

fn arb_point() -> impl Strategy<Value = Point> {
    (arb_coord(), arb_coord()).prop_map(|(x, y)| Point::new(x, y))
}

fn arb_transform() -> impl Strategy<Value = Transform2D> {
    prop_oneof![
        Just(Transform2D::identity()),
        (-31i32..=31i32).prop_map(|t| Transform2D::rotate(t as f32 * 0.1)),
        (1i32..=6i32, 1i32..=6i32)
            .prop_map(|(sx, sy)| Transform2D::scale(sx as f32 * 0.5, sy as f32 * 0.5)),
        ((-31i32..=31i32), 1i32..=6i32).prop_map(|(t, sx)| Transform2D::rotate(t as f32 * 0.1)
            .then(&Transform2D::scale(sx as f32 * 0.5, sx as f32 * 0.5))),
    ]
}

/// One item plus where it sits and how it is transformed. `RectItem` covers the
/// default box shape and the rounded one; `PathItem` covers a filled outline, a
/// stroke band, and the box fallback a path with neither gets.
#[derive(Debug, Clone)]
enum Spec {
    Tile {
        bounds: Rect,
        radius: f32,
    },
    Wire {
        points: Vec<Point>,
        stroke: Option<f32>,
        filled: bool,
        cosmetic: bool,
    },
}

fn arb_spec() -> impl Strategy<Value = Spec> {
    prop_oneof![
        (arb_rect(), 0i32..=60i32).prop_map(|(bounds, r)| Spec::Tile {
            bounds,
            radius: r as f32 * 0.5,
        }),
        (
            prop::collection::vec(arb_point(), 2..=6),
            prop::option::of(0i32..=40i32),
            any::<bool>(),
            any::<bool>(),
        )
            .prop_map(|(points, stroke, filled, cosmetic)| Spec::Wire {
                points,
                stroke: stroke.map(|w| w as f32 * 0.5),
                filled,
                cosmetic,
            }),
    ]
}

fn add(scene: &mut Scene, spec: &Spec, at: Point, xform: Transform2D) -> ItemId {
    let id = match spec {
        Spec::Tile { bounds, radius } => scene.add_item(
            RectItem::new(*bounds)
                .fill(teksilo_tokens::Color::RED)
                .corner_radius(*radius),
            at,
        ),
        Spec::Wire {
            points,
            stroke,
            filled,
            cosmetic,
        } => {
            let mut path = Path::new();
            path.move_to(points[0]);
            for p in &points[1..] {
                path.line_to(*p);
            }
            let mut item = PathItem::new(path);
            if *filled {
                item = item.fill(teksilo_tokens::Color::BLUE);
            }
            if let Some(w) = stroke {
                item = item.stroke_styled(
                    teksilo_tokens::Color::BLACK,
                    StrokeStyle {
                        width: *w,
                        space: if *cosmetic {
                            StrokeSpace::Device
                        } else {
                            StrokeSpace::Logical
                        },
                        ..StrokeStyle::solid(*w)
                    },
                );
            }
            scene.add_item(item, at)
        }
    };
    scene.set_transform(id, xform);
    id
}

/// How much the right-hand side of every implication is grown by, to absorb
/// the rounding two coordinate spaces cannot avoid at an exact tangency. Far
/// below the generator's 0.5 coordinate grid, and far below any defect.
const SLACK: f32 = 0.05;

/// A region and the same region grown by [`SLACK`] in every direction.
///
/// Growing is exact for each form: a rectangle expands, and an outline gains a
/// stroke band `2 * SLACK` wide *around* whatever it already had — which is
/// precisely "every point within `SLACK` of the original".
#[derive(Debug, Clone)]
struct Regions {
    exact: SceneRegion,
    grown: SceneRegion,
}

fn arb_regions() -> impl Strategy<Value = Regions> {
    prop_oneof![
        arb_rect().prop_map(|r| Regions {
            exact: SceneRegion::rect(r),
            grown: SceneRegion::rect(r.expand(SLACK)),
        }),
        prop::collection::vec(arb_point(), 3..=6).prop_map(|pts| {
            let path = Path::polygon(&pts);
            Regions {
                exact: SceneRegion::lasso(path.clone()),
                grown: SceneRegion::from_geometry(
                    ShapeGeometry::shared(path),
                    Some(FillRule::Winding),
                    Some(SLACK * 2.0),
                ),
            }
        }),
        (prop::collection::vec(arb_point(), 2..=5), 0i32..=40i32).prop_map(|(pts, w)| {
            let mut path = Path::new();
            path.move_to(pts[0]);
            for p in &pts[1..] {
                path.line_to(*p);
            }
            let width = w as f32 * 0.5;
            Regions {
                exact: SceneRegion::stroke(path.clone(), width),
                grown: SceneRegion::stroke(path, width + SLACK * 2.0),
            }
        }),
    ]
}

/// The scene under test: a handful of items, each with its own placement and
/// transform, plus the region every mode is asked about.
fn arb_case() -> impl Strategy<Value = (Vec<(Spec, Point, Transform2D)>, Regions)> {
    (
        prop::collection::vec((arb_spec(), arb_point(), arb_transform()), 1..=6),
        arb_regions(),
    )
}

fn pick(scene: &Scene, region: &SceneRegion, mode: ItemSelectionMode) -> BTreeSet<ItemId> {
    scene
        .items_in_region(region, mode, 1.0)
        .into_iter()
        .collect()
}

fn build(specs: &[(Spec, Point, Transform2D)]) -> (Scene, Vec<ItemId>) {
    let mut scene = Scene::new();
    let ids = specs
        .iter()
        .map(|(spec, at, xform)| add(&mut scene, spec, *at, *xform))
        .collect();
    (scene, ids)
}

// ── 1. A shape lies inside its own bounding rect, so anything the shape meets
//      the box meets too. This is the monotonicity the two selection-mode
//      families owe each other, and the one the fast path's edge rule broke ──
proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
    #[test]
    fn a_shape_hit_is_always_a_bounding_rect_hit((specs, regions) in arb_case()) {
        let (scene, _ids) = build(&specs);
        let by_shape = pick(&scene, &regions.exact, ItemSelectionMode::IntersectsItemShape);
        let by_box = pick(&scene, &regions.grown, ItemSelectionMode::IntersectsItemBoundingRect);
        prop_assert!(
            by_shape.is_subset(&by_box),
            "shape mode picked {:?} that box mode did not; specs={:?} region={:?}",
            by_shape.difference(&by_box).collect::<Vec<_>>(),
            specs,
            regions.exact,
        );
    }
}

// ── 2. The containment direction of the same fact: a box inside the region
//      drags its shape in with it ──
proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
    #[test]
    fn a_contained_bounding_rect_contains_its_shape((specs, regions) in arb_case()) {
        let (scene, _ids) = build(&specs);
        let by_box = pick(&scene, &regions.exact, ItemSelectionMode::ContainsItemBoundingRect);
        let by_shape = pick(&scene, &regions.grown, ItemSelectionMode::ContainsItemShape);
        prop_assert!(
            by_box.is_subset(&by_shape),
            "box mode contained {:?} the shape mode did not; specs={:?} region={:?}",
            by_box.difference(&by_shape).collect::<Vec<_>>(),
            specs,
            regions.exact,
        );
    }
}

// ── 3. Containing something is a stronger claim than overlapping it, in both
//      families. Two implementations answer these (a rectangle fast path and
//      the outline algebra), and this is what stops them drifting apart ──
proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
    #[test]
    fn containment_implies_intersection((specs, regions) in arb_case()) {
        let (scene, _ids) = build(&specs);
        for (contains, intersects) in [
            (ItemSelectionMode::ContainsItemShape, ItemSelectionMode::IntersectsItemShape),
            (
                ItemSelectionMode::ContainsItemBoundingRect,
                ItemSelectionMode::IntersectsItemBoundingRect,
            ),
        ] {
            let inner = pick(&scene, &regions.exact, contains);
            let outer = pick(&scene, &regions.grown, intersects);
            prop_assert!(
                inner.is_subset(&outer),
                "{contains:?} picked {:?} that {intersects:?} did not; specs={:?} region={:?}",
                inner.difference(&outer).collect::<Vec<_>>(),
                specs,
                regions.exact,
            );
        }
    }
}

// ── 4. A point inside an item's shape is a point the region containing that
//      item's whole shape must also contain — the bridge between the point
//      query and the region query, which are separate code paths over the same
//      `ItemShape` ──
proptest! {
    #[test]
    fn a_contained_shape_contains_no_point_outside_the_region(
        (specs, regions) in arb_case(),
        probe in arb_point(),
    ) {
        let (scene, ids) = build(&specs);
        let contained = pick(&scene, &regions.exact, ItemSelectionMode::ContainsItemShape);
        for id in ids.iter().filter(|id| contained.contains(id)) {
            if scene.item_contains(*id, probe, 1.0) {
                prop_assert!(
                    regions.grown.contains_point(probe),
                    "item {id:?} is wholly inside the region yet owns {probe:?}, \
                     which the region does not; specs={specs:?} region={:?}",
                    regions.exact,
                );
            }
        }
    }
}

// ── 5. The four modes answer, in any order, for any input, without panicking
//      or depending on which was asked first ──
proptest! {
    #[test]
    fn every_mode_answers_and_answers_the_same_way_twice((specs, regions) in arb_case()) {
        let (scene, _ids) = build(&specs);
        let region = regions.exact;
        for mode in [
            ItemSelectionMode::IntersectsItemShape,
            ItemSelectionMode::ContainsItemShape,
            ItemSelectionMode::IntersectsItemBoundingRect,
            ItemSelectionMode::ContainsItemBoundingRect,
        ] {
            let first = pick(&scene, &region, mode);
            let second = pick(&scene, &region, mode);
            prop_assert_eq!(&first, &second, "{:?} is not deterministic", mode);
        }
    }
}

// ── 6. A rotation is an isometry, so rotating a scene and its lasso together
//      cannot move an item across the lasso's boundary. Stated as a sandwich
//      because the answer at the boundary itself is not exact: a circle is
//      flattened to `SHAPE_FLATTEN_TOLERANCE` (0.25) in whichever frame it is
//      flattened in, so an item *tangent* to the loop can legitimately fall
//      either way. Shrinking and growing the loop by 2 units — eight times the
//      tolerance — brackets that without letting the property go vacuous: the
//      two brackets differ by 4 units of radius, so any item further than that
//      from the boundary is pinned. Before `Path::transformed` learned to
//      carry an arc, a rotated circular lasso came back sqrt(2) *wider*, which
//      is 30-odd units here — far outside the bracket ──
proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
    #[test]
    fn rotating_a_scene_and_its_lasso_together_selects_the_same_items(
        specs in prop::collection::vec((arb_spec(), arb_point()), 1..=5),
        centre in arb_point(),
        radius in 10i32..=120i32,
        turn in -31i32..=31i32,
    ) {
        const MARGIN: f32 = 2.0;
        let radius = radius as f32;
        let rotate = Transform2D::rotate(turn as f32 * 0.1);

        let mut plain = Scene::new();
        let mut turned = Scene::new();
        let mut plain_ids = Vec::new();
        let mut turned_ids = Vec::new();
        for (spec, at) in &specs {
            plain_ids.push(add(&mut plain, spec, *at, Transform2D::identity()));
            turned_ids.push(add(&mut turned, spec, rotate.apply_point(*at), rotate));
        }

        let turned_lasso = SceneRegion::lasso(Path::circle(centre, radius).transformed(&rotate));
        let by_index = |scene: &Scene, ids: &[ItemId], region: &SceneRegion, mode| {
            let picked = pick(scene, region, mode);
            ids.iter()
                .enumerate()
                .filter(|(_, id)| picked.contains(id))
                .map(|(i, _)| i)
                .collect::<BTreeSet<usize>>()
        };

        for mode in [
            ItemSelectionMode::IntersectsItemShape,
            ItemSelectionMode::ContainsItemShape,
        ] {
            let tight = SceneRegion::lasso(Path::circle(centre, radius - MARGIN));
            let loose = SceneRegion::lasso(Path::circle(centre, radius + MARGIN));
            let floor = by_index(&plain, &plain_ids, &tight, mode);
            let ceiling = by_index(&plain, &plain_ids, &loose, mode);
            let actual = by_index(&turned, &turned_ids, &turned_lasso, mode);
            prop_assert!(
                floor.is_subset(&actual),
                "{mode:?}: turning the scene lost {:?}; specs={specs:?} centre={centre:?} r={radius} turn={turn}",
                floor.difference(&actual).collect::<Vec<_>>(),
            );
            prop_assert!(
                actual.is_subset(&ceiling),
                "{mode:?}: turning the scene gained {:?}; specs={specs:?} centre={centre:?} r={radius} turn={turn}",
                actual.difference(&ceiling).collect::<Vec<_>>(),
            );
        }
    }
}
