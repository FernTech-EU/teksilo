// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Unit tests for [`ItemShape`](super::ItemShape) and
//! [`SceneRegion`](super::SceneRegion).

use std::collections::BTreeSet;

use super::*;
use crate::{RectItem, Scene};

fn ring_path() -> Path {
    // Two concentric squares, same winding direction.
    let mut p = Path::new();
    p.move_to(Point::new(0.0, 0.0))
        .line_to(Point::new(100.0, 0.0))
        .line_to(Point::new(100.0, 100.0))
        .line_to(Point::new(0.0, 100.0))
        .close();
    p.move_to(Point::new(25.0, 25.0))
        .line_to(Point::new(75.0, 25.0))
        .line_to(Point::new(75.0, 75.0))
        .line_to(Point::new(25.0, 75.0))
        .close();
    p
}

fn diagonal() -> Path {
    Path::line(Point::new(0.0, 0.0), Point::new(100.0, 100.0))
}

fn horizontal() -> Path {
    Path::line(Point::new(0.0, 0.0), Point::new(100.0, 0.0))
}

// ---------------------------------------------------------------------------
// contains — the point path
// ---------------------------------------------------------------------------

#[test]
fn bounds_shape_is_its_rect() {
    let s = ItemShape::bounds(Rect::new(10.0, 10.0, 30.0, 30.0));
    assert!(s.contains(Point::new(20.0, 20.0), 1.0));
    assert!(s.contains(Point::new(10.0, 10.0), 1.0), "inclusive edge");
    assert!(!s.contains(Point::new(9.0, 20.0), 1.0));
    assert_eq!(s.bounding_rect(), Rect::new(10.0, 10.0, 30.0, 30.0));
}

#[test]
fn none_shape_is_never_hit() {
    let s = ItemShape::none();
    assert!(s.is_none());
    assert!(!s.contains(Point::ZERO, 1.0));
    assert!(!s.contains(Point::new(1e6, 1e6), 1.0));
    let everywhere = SceneRegion::rect(Rect::new(-1e6, -1e6, 2e6, 2e6));
    assert!(!s.intersects_region(&everywhere, 1.0));
    assert!(!s.contained_by_region(&everywhere, 1.0));
}

#[test]
fn default_shape_is_none() {
    assert!(ItemShape::default().is_none());
}

#[test]
fn rounded_rect_excludes_its_corners() {
    let s = ItemShape::rounded_rect(Rect::new(0.0, 0.0, 100.0, 100.0), 20.0);
    assert!(!s.contains(Point::new(1.0, 1.0), 1.0));
    assert!(!s.contains(Point::new(99.0, 99.0), 1.0));
    assert!(s.contains(Point::new(50.0, 50.0), 1.0));
    assert!(s.contains(Point::new(0.0, 50.0), 1.0), "mid-edge");
    assert!(s.contains(Point::new(20.0, 20.0), 1.0), "corner centre");
    // The advertised box is the full rectangle — only the silhouette narrows.
    assert_eq!(s.bounding_rect(), Rect::new(0.0, 0.0, 100.0, 100.0));
}

#[test]
fn a_zero_radius_rounded_rect_degrades_to_the_plain_box() {
    let s = ItemShape::rounded_rect(Rect::new(0.0, 0.0, 10.0, 10.0), 0.0);
    assert!(s.contains(Point::new(0.0, 0.0), 1.0));
}

#[test]
fn a_radius_larger_than_the_box_is_clamped_to_a_capsule() {
    // radius 500 on a 100x40 box: clamped to 20 (half the short axis), so the
    // shape is a stadium, not an empty set.
    let s = ItemShape::rounded_rect(Rect::new(0.0, 0.0, 100.0, 40.0), 500.0);
    assert!(s.contains(Point::new(50.0, 20.0), 1.0));
    assert!(!s.contains(Point::new(0.5, 0.5), 1.0));
}

#[test]
fn ellipse_excludes_the_box_corners() {
    let s = ItemShape::ellipse(Rect::new(0.0, 0.0, 100.0, 50.0));
    assert!(s.contains(Point::new(50.0, 25.0), 1.0));
    assert!(s.contains(Point::new(0.0, 25.0), 1.0), "left vertex");
    assert!(!s.contains(Point::new(2.0, 2.0), 1.0), "box corner");
    let degenerate = ItemShape::ellipse(Rect::new(0.0, 0.0, 0.0, 10.0));
    assert!(!degenerate.contains(Point::new(0.0, 5.0), 1.0));
}

#[test]
fn a_path_shape_is_filled_by_default_and_unfilled_on_request() {
    let filled = ItemShape::from_path(Path::rect(Rect::new(0.0, 0.0, 100.0, 100.0)));
    assert!(filled.contains(Point::new(50.0, 50.0), 1.0));
    let hollow = filled.clone().unfilled();
    assert!(!hollow.contains(Point::new(50.0, 50.0), 1.0));
    // With no band left there is nothing to hit at all.
    assert!(!hollow.contains(Point::new(0.0, 0.0), 1.0));
}

#[test]
fn fill_rule_decides_whether_a_ring_has_a_hole() {
    let winding = ItemShape::from_path(ring_path());
    assert!(winding.contains(Point::new(50.0, 50.0), 1.0));
    let even_odd = ItemShape::from_path(ring_path()).filled(FillRule::EvenOdd);
    assert!(!even_odd.contains(Point::new(50.0, 50.0), 1.0), "the hole");
    assert!(even_odd.contains(Point::new(10.0, 50.0), 1.0), "the band");
}

#[test]
fn a_band_is_unioned_with_the_interior_not_substituted_for_it() {
    // M4, pinned: `filled` + `stroked` is a union. The interior hits, and so
    // does a point just outside the outline but inside the band.
    let s = ItemShape::from_path(Path::rect(Rect::new(0.0, 0.0, 100.0, 100.0)))
        .stroked(10.0, StrokeSpace::Logical);
    assert!(s.contains(Point::new(50.0, 50.0), 1.0), "interior");
    assert!(
        s.contains(Point::new(-5.0, 50.0), 1.0),
        "outside, in the band"
    );
    assert!(!s.contains(Point::new(-20.0, 50.0), 1.0), "past the band");
}

#[test]
fn a_cosmetic_band_converts_from_device_pixels_at_test_time() {
    let s = ItemShape::from_path(diagonal())
        .unfilled()
        .stroked(4.0, StrokeSpace::Device);
    // Half-width 2 + 2 slack = 4 at 1x; 0.5 + 2 = 2.5 at 4x. A point 3 units
    // off the line straddles the two.
    let p = Point::new(50.0, 50.0 + 3.0 * std::f32::consts::SQRT_2);
    assert!(s.contains(p, 1.0));
    assert!(!s.contains(p, 4.0));
    // A logical band of the same width does not move.
    let logical = ItemShape::from_path(diagonal())
        .unfilled()
        .stroked(4.0, StrokeSpace::Logical);
    assert!(logical.contains(p, 1.0));
    assert!(logical.contains(p, 4.0));
}

#[test]
fn a_degenerate_view_scale_does_not_divide_the_band_away() {
    let s = ItemShape::from_path(diagonal())
        .unfilled()
        .stroked(4.0, StrokeSpace::Device);
    // A zero / near-zero scale falls back to the logical width rather than
    // producing an infinite band.
    assert!(s.contains(Point::new(50.0, 52.0), 0.0));
    assert!(!s.contains(Point::new(50.0, 90.0), 0.0));
}

#[test]
fn hit_stroke_width_overrides_the_painted_width_and_ignores_zoom() {
    let s = ItemShape::from_path(diagonal())
        .unfilled()
        .stroked(1.0, StrokeSpace::Device)
        .hit_stroke_width(20.0);
    let p = Point::new(50.0, 58.0);
    assert!(s.contains(p, 1.0));
    assert!(
        s.contains(p, 8.0),
        "a grab target is not a rendered thickness"
    );
}

#[test]
fn bounding_rect_includes_the_band() {
    let s = ItemShape::bounds(Rect::new(0.0, 0.0, 10.0, 10.0)).stroked(6.0, StrokeSpace::Logical);
    // half-width 3 + slack 2 = 5.
    assert_eq!(s.bounding_rect(), Rect::new(-5.0, -5.0, 20.0, 20.0));
}

// ---------------------------------------------------------------------------
// Region algebra
// ---------------------------------------------------------------------------

#[test]
fn a_plain_box_against_a_rect_region_takes_the_one_compare_fast_path() {
    // And gives exactly the pre-`ItemShape` answer, edge-exclusive.
    let s = ItemShape::bounds(Rect::new(0.0, 0.0, 10.0, 10.0));
    assert!(s.intersects_region(&SceneRegion::rect(Rect::new(5.0, 5.0, 10.0, 10.0)), 1.0));
    assert!(!s.intersects_region(&SceneRegion::rect(Rect::new(10.0, 0.0, 10.0, 10.0)), 1.0));
    assert!(s.contained_by_region(&SceneRegion::rect(Rect::new(-1.0, -1.0, 20.0, 20.0)), 1.0));
    assert!(!s.contained_by_region(&SceneRegion::rect(Rect::new(1.0, 1.0, 20.0, 20.0)), 1.0));
}

#[test]
fn intersection_catches_every_way_two_outlines_can_meet() {
    let s = ItemShape::from_path(Path::rect(Rect::new(0.0, 0.0, 100.0, 100.0)));
    // (a) a shape vertex inside the region
    assert!(s.intersects_region(
        &SceneRegion::lasso(Path::rect(Rect::new(-10.0, -10.0, 30.0, 30.0))),
        1.0
    ));
    // (b) a region vertex inside the shape
    assert!(s.intersects_region(
        &SceneRegion::lasso(Path::rect(Rect::new(40.0, 40.0, 10.0, 10.0))),
        1.0
    ));
    // (c) a cross: no vertex of either inside the other, but edges cross
    assert!(s.intersects_region(
        &SceneRegion::lasso(Path::rect(Rect::new(-10.0, 40.0, 200.0, 20.0))),
        1.0
    ));
    // (d) genuinely apart
    assert!(!s.intersects_region(
        &SceneRegion::lasso(Path::rect(Rect::new(500.0, 500.0, 10.0, 10.0))),
        1.0
    ));
}

#[test]
fn containment_requires_the_whole_band_not_just_the_centreline() {
    let s = ItemShape::from_path(diagonal())
        .unfilled()
        .stroked(20.0, StrokeSpace::Logical);
    // Band half-width is 10 + 2 slack = 12.
    let snug = SceneRegion::lasso(Path::rect(Rect::new(-5.0, -5.0, 110.0, 110.0)));
    assert!(!s.contained_by_region(&snug, 1.0), "the band spills out");
    let roomy = SceneRegion::lasso(Path::rect(Rect::new(-20.0, -20.0, 140.0, 140.0)));
    assert!(s.contained_by_region(&roomy, 1.0));
}

#[test]
fn a_lasso_selects_what_it_encircles_and_not_what_it_merely_boxes() {
    // A triangular lasso over the top-left. Its bounding box also covers the
    // bottom-right corner, which the loop itself does not.
    let lasso = SceneRegion::lasso(Path::polygon(&[
        Point::new(0.0, 0.0),
        Point::new(100.0, 0.0),
        Point::new(0.0, 100.0),
    ]));
    let inside = ItemShape::bounds(Rect::new(10.0, 10.0, 10.0, 10.0));
    let boxed_only = ItemShape::bounds(Rect::new(85.0, 85.0, 10.0, 10.0));
    assert!(inside.intersects_region(&lasso, 1.0));
    assert!(inside.contained_by_region(&lasso, 1.0));
    assert!(!boxed_only.intersects_region(&lasso, 1.0));
}

#[test]
fn an_even_odd_lasso_hole_excludes_what_sits_in_it() {
    let region = SceneRegion::lasso_with_rule(ring_path(), FillRule::EvenOdd);
    let in_the_hole = ItemShape::bounds(Rect::new(45.0, 45.0, 6.0, 6.0));
    let in_the_band = ItemShape::bounds(Rect::new(5.0, 45.0, 6.0, 6.0));
    assert!(!in_the_hole.intersects_region(&region, 1.0));
    assert!(in_the_band.intersects_region(&region, 1.0));
}

#[test]
fn a_band_region_asks_about_the_line_not_the_loop() {
    let region = SceneRegion::stroke(diagonal(), 10.0);
    let on_the_line = ItemShape::bounds(Rect::new(48.0, 48.0, 4.0, 4.0));
    let off_the_line = ItemShape::bounds(Rect::new(2.0, 85.0, 10.0, 10.0));
    assert!(on_the_line.intersects_region(&region, 1.0));
    assert!(!off_the_line.intersects_region(&region, 1.0));
    assert!(region.contains_point(Point::new(50.0, 52.0)));
    assert!(!region.contains_point(Point::new(50.0, 70.0)));
}

#[test]
fn an_empty_region_matches_nothing() {
    let r = SceneRegion::empty();
    assert!(r.is_empty());
    let s = ItemShape::bounds(Rect::new(0.0, 0.0, 10.0, 10.0));
    assert!(!s.intersects_region(&r, 1.0));
    assert!(!s.contained_by_region(&r, 1.0));
}

// ---------------------------------------------------------------------------
// Spaces — to_local / from_screen_rect / to_scene_region
// ---------------------------------------------------------------------------

#[test]
fn a_rect_region_survives_an_axis_preserving_map_as_a_rect() {
    let r = SceneRegion::rect(Rect::new(0.0, 0.0, 10.0, 10.0));
    let t = Transform2D::translate(5.0, 5.0).then(&Transform2D::scale(2.0, 2.0));
    let mapped = r.to_local(&t);
    assert_eq!(mapped.as_rect(), Some(Rect::new(10.0, 10.0, 20.0, 20.0)));
}

#[test]
fn a_quarter_turn_still_counts_as_axis_preserving() {
    let r = SceneRegion::rect(Rect::new(0.0, 0.0, 10.0, 20.0));
    let mapped = r.to_local(&Transform2D::rotate(std::f32::consts::FRAC_PI_2));
    let rect = mapped
        .as_rect()
        .expect("a quarter turn keeps it a rectangle");
    assert!((rect.width - 20.0).abs() < 1e-3 && (rect.height - 10.0).abs() < 1e-3);
}

#[test]
fn a_rotated_map_keeps_the_exact_quadrilateral_instead_of_its_hull() {
    let r = SceneRegion::rect(Rect::new(0.0, 0.0, 100.0, 100.0));
    let t = Transform2D::rotate(std::f32::consts::FRAC_PI_4);
    let mapped = r.to_local(&t);
    assert!(mapped.as_rect().is_none(), "no longer a rectangle");
    // The hull's corner is not in the rotated square.
    let hull = t.apply_rect(Rect::new(0.0, 0.0, 100.0, 100.0));
    assert!(!mapped.contains_point(Point::new(hull.x + 1.0, hull.y + 1.0)));
    // The rotated square's own centre is.
    assert!(mapped.contains_point(t.apply_point(Point::new(50.0, 50.0))));
}

#[test]
fn from_screen_rect_is_the_same_rule_in_the_other_direction() {
    let screen = Rect::new(0.0, 0.0, 40.0, 40.0);
    let plain = SceneRegion::from_screen_rect(screen, &Transform2D::scale(0.5, 0.5));
    assert_eq!(plain.as_rect(), Some(Rect::new(0.0, 0.0, 20.0, 20.0)));
    let turned = SceneRegion::from_screen_rect(screen, &Transform2D::rotate(0.3));
    assert!(turned.as_rect().is_none());
}

#[test]
fn a_bands_width_scales_with_the_map() {
    let region = SceneRegion::stroke(diagonal(), 10.0);
    // Half-width 5 at scale 1; a 2x map doubles it.
    assert!(!region.contains_point(Point::new(50.0, 58.0)));
    let doubled = region.to_local(&Transform2D::scale(2.0, 2.0));
    assert!(doubled.contains_point(Point::new(100.0, 107.0)));
}

#[test]
fn a_shape_republished_as_a_scene_region_keeps_its_silhouette() {
    // The collision path: a stroke-only diagonal becomes a band region in
    // scene space, translated by its own transform.
    let shape = ItemShape::from_path(diagonal())
        .unfilled()
        .stroked(4.0, StrokeSpace::Logical);
    let region = shape.to_scene_region(&Transform2D::translate(1000.0, 0.0));
    assert!(region.contains_point(Point::new(1050.0, 50.0)));
    assert!(!region.contains_point(Point::new(1050.0, 90.0)));
    assert!(!region.contains_point(Point::new(50.0, 50.0)), "it moved");
}

#[test]
fn a_rounded_rect_republished_under_a_rotation_keeps_its_corners_round() {
    // An arc a transform cannot carry as an arc is expanded into cubics by
    // `Path::transformed`. If it were not — if the arc's *rectangle* were
    // transformed and its angles kept — a rotated rounded rect's corner arcs
    // would land in the wrong quadrants.
    let shape = ItemShape::rounded_rect(Rect::new(-50.0, -50.0, 100.0, 100.0), 25.0);
    let t = Transform2D::rotate(std::f32::consts::FRAC_PI_4);
    let region = shape.to_scene_region(&t);
    assert!(region.contains_point(Point::ZERO), "centre is invariant");
    // The rotated silhouette is still a rounded square of the same radius, so
    // a point on the rotated corner-arc's far side stays outside.
    let corner = t.apply_point(Point::new(-49.0, -49.0));
    assert!(!region.contains_point(corner));
}

#[test]
fn a_none_shape_republishes_as_an_empty_region() {
    assert!(
        ItemShape::none()
            .to_scene_region(&Transform2D::identity())
            .is_empty()
    );
}

// ---------------------------------------------------------------------------
// Mode helpers
// ---------------------------------------------------------------------------

#[test]
fn selection_mode_predicates_agree_with_their_names() {
    use ItemSelectionMode::*;
    assert_eq!(ItemSelectionMode::default(), IntersectsItemShape);
    assert!(IntersectsItemShape.uses_shape());
    assert!(ContainsItemShape.uses_shape());
    assert!(!IntersectsItemBoundingRect.uses_shape());
    assert!(!ContainsItemBoundingRect.uses_shape());
    assert!(!IntersectsItemShape.requires_containment());
    assert!(ContainsItemShape.requires_containment());
    assert!(!IntersectsItemBoundingRect.requires_containment());
    assert!(ContainsItemBoundingRect.requires_containment());
}

// ---------------------------------------------------------------------------
// The point path's rejection index
// ---------------------------------------------------------------------------

/// The straightforward answer the chunked walk has to reproduce: distance from
/// `p` to any segment of the outline.
fn brute_force_within(path: &Path, p: Point, half: f32) -> bool {
    path.flatten(SHAPE_FLATTEN_TOLERANCE).iter().any(|sp| {
        sp.segments(false)
            .any(|(a, b)| point_to_segment_distance(p, a, b) <= half)
    })
}

/// `n` segments, each `step` wide in x, alternating between y = 0 and y = 8.
fn long_zigzag(n: usize, step: f32) -> Path {
    let mut path = Path::new();
    path.move_to(Point::new(0.0, 0.0));
    for i in 1..=n {
        path.line_to(Point::new(
            i as f32 * step,
            if i % 2 == 0 { 0.0 } else { 8.0 },
        ));
    }
    path
}

#[test]
fn the_chunked_band_walk_agrees_with_brute_force_everywhere() {
    // The rejection index is an optimisation, so the thing worth pinning is
    // that it never changes an answer. Sample a grid across and beyond a
    // 400-segment stroke — far more chunks than `SEGMENTS_PER_CHUNK` — and
    // compare against the plain per-segment walk.
    let path = long_zigzag(400, 1.0);
    let shape = ItemShape::from_path(path.clone())
        .unfilled()
        .stroked(2.0, StrokeSpace::Logical);
    let half = 1.0 + HIT_BAND_SLACK;
    let mut hits = 0;
    let mut misses = 0;
    for xi in -2..=41 {
        for yi in -2..=12 {
            let p = Point::new(xi as f32 * 10.0, yi as f32 * 1.0);
            let expected = brute_force_within(&path, p, half);
            assert_eq!(
                shape.contains(p, 1.0),
                expected,
                "chunked walk disagreed at {p:?}"
            );
            if expected {
                hits += 1;
            } else {
                misses += 1;
            }
        }
    }
    // Guard against the test passing because every sample missed.
    assert!(hits > 20, "only {hits} of the samples were hits");
    assert!(misses > 20, "only {misses} of the samples were misses");
}

#[test]
fn every_chunks_first_and_last_segment_is_still_walked() {
    // An off-by-one in the chunk stride drops exactly the segments at a chunk
    // boundary. The step is wide enough (20) that a neighbouring chunk's box,
    // even expanded by the band, cannot cover the aim point — so a stride bug
    // is a miss here rather than a near miss that happens to pass.
    const STEP: f32 = 20.0;
    let chunks = 5;
    let n = SEGMENTS_PER_CHUNK * chunks;
    let shape = ItemShape::from_path(long_zigzag(n, STEP))
        .unfilled()
        .stroked(1.0, StrokeSpace::Logical);
    for k in 0..chunks {
        for seg in [k * SEGMENTS_PER_CHUNK, (k + 1) * SEGMENTS_PER_CHUNK - 1] {
            // Midpoint of segment `seg`, which runs x = seg*STEP -> (seg+1)*STEP
            // between y = 0 and y = 8.
            let mid = Point::new((seg as f32 + 0.5) * STEP, 4.0);
            assert!(
                shape.contains(mid, 1.0),
                "segment {seg} (chunk {k}) was skipped"
            );
        }
    }
    // And a point well away from every tooth still misses.
    assert!(!shape.contains(Point::new(STEP * 0.5, 40.0), 1.0));
}

#[test]
fn a_many_subpath_shape_rejects_by_subpath_first() {
    // Two far-apart strokes in one shape: a point near one must not depend on
    // the other having been walked, and a point near neither must miss.
    let mut path = long_zigzag(40, 1.0);
    let mut far = Path::new();
    far.move_to(Point::new(10_000.0, 0.0))
        .line_to(Point::new(10_100.0, 0.0));
    path.append(&far);
    let shape = ItemShape::from_path(path)
        .unfilled()
        .stroked(2.0, StrokeSpace::Logical);
    assert!(shape.contains(Point::new(10_050.0, 1.0), 1.0));
    assert!(shape.contains(Point::new(20.5, 4.0), 1.0));
    assert!(!shape.contains(Point::new(5_000.0, 0.0), 1.0));
}

// ---------------------------------------------------------------------------
// Memoisation
// ---------------------------------------------------------------------------

#[test]
fn shape_geometry_flattens_once_and_shares_by_handle() {
    let geom = ShapeGeometry::shared(Path::circle(Point::new(0.0, 0.0), 50.0));
    let first = geom.outline().as_ptr();
    let a = ItemShape::path(geom.clone());
    let b = ItemShape::path(geom.clone());
    // Cloning the shape and asking again must not re-flatten: the slice is
    // the same allocation.
    assert!(a.contains(Point::ZERO, 1.0));
    assert!(b.clone().contains(Point::ZERO, 1.0));
    assert_eq!(geom.outline().as_ptr(), first);
    assert_eq!(Rc::strong_count(&geom), 3);
}

#[test]
fn shape_geometry_bounds_are_curve_accurate() {
    let geom = ShapeGeometry::shared(Path::circle(Point::new(0.0, 0.0), 50.0));
    let b = geom.bounds();
    assert!((b.width - 100.0).abs() < 1.0 && (b.height - 100.0).abs() < 1.0);
}

#[test]
fn an_empty_geometry_is_a_zero_box_not_a_panic() {
    let geom = ShapeGeometry::shared(Path::new());
    assert_eq!(geom.bounds(), Rect::ZERO);
    assert!(geom.outline().is_empty());
    let s = ItemShape::path(geom);
    assert!(!s.contains(Point::ZERO, 1.0));
}

#[test]
fn a_lasso_with_an_arc_survives_a_rotation_unchanged() {
    // The `to_local` counterpart of
    // `a_rounded_rect_republished_under_a_rotation_keeps_its_corners_round`,
    // and the defect it pins: a circular lasso mapped through a rotation used
    // to come back sqrt(2) wider, so a point 38 units from the centre of a
    // radius-30 loop fell *inside* it. A rotation is an isometry; it cannot
    // change what a region contains.
    let lasso = SceneRegion::lasso(Path::circle(Point::ZERO, 30.0));
    assert!(!lasso.contains_point(Point::new(38.0, 0.0)), "38 > 30");
    assert!(lasso.contains_point(Point::new(20.0, 0.0)));
    for turns in 1..8 {
        let t = Transform2D::rotate(turns as f32 * 0.4);
        let mapped = lasso.to_local(&t);
        let b = mapped.bounding_rect();
        assert!(
            (b.width - 60.0).abs() < 1.0 && (b.height - 60.0).abs() < 1.0,
            "turn {turns} resized the loop to {b:?}"
        );
        assert!(
            !mapped.contains_point(t.apply_point(Point::new(38.0, 0.0))),
            "turn {turns}: a rotation must not change containment"
        );
        assert!(mapped.contains_point(t.apply_point(Point::new(20.0, 0.0))));
    }
}

// ---------------------------------------------------------------------------
// One inclusivity rule for both paths
// ---------------------------------------------------------------------------

/// The same geometry twice: once as the plain box that takes the one-compare
/// fast path, once as an outline that cannot.
fn box_and_outline(rect: Rect) -> (ItemShape, ItemShape) {
    (
        ItemShape::bounds(rect),
        ItemShape::from_path(Path::rect(rect)),
    )
}

#[test]
fn the_fast_path_and_the_general_path_agree_about_an_edge() {
    // The defect: the box fast path is exclusive and the outline algebra used
    // to be inclusive, so a rounded tile was picked by a band its square twin
    // was not. Both now answer the area rule.
    let (boxed, outlined) = box_and_outline(Rect::new(0.0, 0.0, 40.0, 40.0));
    let abutting = SceneRegion::rect(Rect::new(40.0, 0.0, 10.0, 40.0));
    let overlapping = SceneRegion::rect(Rect::new(39.0, 0.0, 10.0, 40.0));
    for (name, s) in [("box", &boxed), ("outline", &outlined)] {
        assert!(
            !s.intersects_region(&abutting, 1.0),
            "{name}: touching an edge shares no area"
        );
        assert!(
            s.intersects_region(&overlapping, 1.0),
            "{name}: one unit in"
        );
    }
    // …and the rounded form, which is what made the divergence visible.
    let rounded = ItemShape::rounded_rect(Rect::new(0.0, 0.0, 40.0, 40.0), 2.0);
    assert!(!rounded.intersects_region(&abutting, 1.0));
    assert!(rounded.intersects_region(&overlapping, 1.0));
}

#[test]
fn an_exact_fit_region_both_contains_a_shape_and_shares_its_area() {
    // The other half of the same defect: a collinear edge used to read as a
    // crossing, so an exact-fit band contained the square and rejected the
    // rounded rect.
    let rect = Rect::new(0.0, 0.0, 40.0, 40.0);
    let exact = SceneRegion::rect(rect);
    let (boxed, outlined) = box_and_outline(rect);
    let rounded = ItemShape::rounded_rect(rect, 2.0);
    for (name, s) in [
        ("box", &boxed),
        ("outline", &outlined),
        ("rounded", &rounded),
    ] {
        assert!(s.contained_by_region(&exact, 1.0), "{name}: contained");
        assert!(s.intersects_region(&exact, 1.0), "{name}: same area");
    }
}

#[test]
fn two_identical_outlines_still_overlap() {
    // With no vertex strictly inside the other and nothing crossing, the
    // coincident-boundary witness is the only thing that answers this — and
    // `colliding_items` really does get asked it, by a duplicate of a rotated
    // item laid over the original.
    let square = Path::rect(Rect::new(0.0, 0.0, 30.0, 30.0));
    let shape = ItemShape::from_path(square.clone());
    let region = SceneRegion::lasso(square);
    assert!(shape.intersects_region(&region, 1.0));
    assert!(shape.contained_by_region(&region, 1.0));
}

#[test]
fn a_zero_area_shape_or_region_falls_back_to_contact() {
    // The stated exception: an area rule answers "no" to every query about a
    // bare line, which is what `items_along_path` builds. A line that runs
    // through a box from corner to corner crosses none of its edges
    // transversally and still goes straight through the middle.
    let line = Path::line(Point::new(15.0, 15.0), Point::new(40.0, 40.0));
    let region = SceneRegion::lasso(line);
    assert!(!region.has_area());
    let tile = ItemShape::bounds(Rect::new(20.0, 20.0, 10.0, 10.0));
    assert!(tile.intersects_region(&region, 1.0));
    // A loop of three points does enclose area, and then the area rule applies.
    let loop_region = SceneRegion::lasso(Path::polygon(&[
        Point::new(0.0, 0.0),
        Point::new(100.0, 0.0),
        Point::new(0.0, 100.0),
    ]));
    assert!(loop_region.has_area());
}

#[test]
fn a_shape_hit_implies_a_box_hit_for_every_way_a_band_can_meet_a_tile() {
    // Monotonicity in miniature, at unit scale: a shape lies inside its own
    // bounding rect, so anything the shape meets the box meets too. Swept over
    // a grid of band positions against a rounded tile — the shape that is
    // strictly smaller than its box.
    let rect = Rect::new(0.0, 0.0, 40.0, 40.0);
    let rounded = ItemShape::rounded_rect(rect, 18.0);
    let boxed = ItemShape::bounds(rect);
    let mut shape_hits = 0;
    let mut box_hits = 0;
    for xi in -1..=21 {
        for yi in -1..=21 {
            let band = SceneRegion::rect(Rect::new(xi as f32 * 2.0, yi as f32 * 2.0, 2.0, 2.0));
            let by_shape = rounded.intersects_region(&band, 1.0);
            let by_box = boxed.intersects_region(&band, 1.0);
            assert!(
                !by_shape || by_box,
                "shape hit without a box hit at ({xi}, {yi})"
            );
            assert!(
                !boxed.contained_by_region(&band, 1.0) || rounded.contained_by_region(&band, 1.0),
                "box contained without the shape being, at ({xi}, {yi})"
            );
            shape_hits += by_shape as u32;
            box_hits += by_box as u32;
        }
    }
    assert!(shape_hits > 20, "only {shape_hits} shape hits");
    assert!(
        box_hits > shape_hits,
        "the corners must cost the shape something"
    );
}

// ---------------------------------------------------------------------------
// What the broad phase can and cannot cover
// ---------------------------------------------------------------------------

#[test]
fn a_cosmetic_band_outgrows_its_own_bounding_rect_below_1x() {
    // The one documented case where the shape reaches past the rectangle the
    // spatial index bucketed on. Stated as a measurement so the doc is not
    // prose: at 0.25x a 40 px band covers 160 local units, and
    // `bounding_rect_at` is what reports it.
    let s = ItemShape::from_path(horizontal())
        .unfilled()
        .stroked(40.0, StrokeSpace::Device);
    let at_one = s.bounding_rect();
    assert_eq!(at_one, Rect::new(-22.0, -22.0, 144.0, 44.0));
    assert_eq!(
        at_one,
        s.bounding_rect_at(1.0),
        "bounding_rect is the 1x one"
    );
    // 30 units off the line: past the 22-unit half-band at 1x, inside the
    // 82-unit one at 0.25x.
    let p = Point::new(50.0, 30.0);
    assert!(!s.contains(p, 1.0));
    assert!(!at_one.contains(p), "consistent at 1x");
    assert!(s.contains(p, 0.25), "the band really is that wide at 0.25x");
    assert!(
        !at_one.contains(p),
        "and the 1x box — the one the index bucketed on — does not cover it"
    );
    assert!(
        s.bounding_rect_at(0.25).contains(p),
        "which is exactly what bounding_rect_at reports"
    );
    // A logical band, by contrast, is the same at every zoom.
    let logical = ItemShape::from_path(horizontal())
        .unfilled()
        .stroked(40.0, StrokeSpace::Logical);
    assert_eq!(logical.bounding_rect_at(0.25), logical.bounding_rect());
}

#[test]
fn a_rounded_shape_never_leaves_its_advertised_box() {
    // The accessibility-facing half: an AT node's bounds are the item's
    // rectangle, and the shape inside it is what a pointer meets. The gap is
    // bounded by `r * (sqrt(2) - 1)` at each corner, which is what this
    // measures — a rounded tile advertises corners it will not accept, and by
    // how much is a number rather than an impression.
    let r = 20.0_f32;
    let s = ItemShape::rounded_rect(Rect::new(0.0, 0.0, 100.0, 100.0), r);
    assert_eq!(s.bounding_rect(), Rect::new(0.0, 0.0, 100.0, 100.0));
    let reach = r * (std::f32::consts::SQRT_2 - 1.0);
    // Along the corner diagonal: just inside the silhouette, and just outside.
    let d = (r - r / std::f32::consts::SQRT_2) + 0.2;
    assert!(s.contains(Point::new(d, d), 1.0), "inside the arc");
    assert!(
        !s.contains(Point::new(0.0, 0.0), 1.0),
        "the advertised corner is not in the shape"
    );
    // Nothing further than that from the corner is excluded.
    assert!(
        s.contains(Point::new(reach + 0.5, reach + 0.5), 1.0),
        "the gap is bounded by r*(sqrt(2)-1) = {reach}"
    );
}

// ---------------------------------------------------------------------------
// Bands under an anisotropic map
// ---------------------------------------------------------------------------

#[test]
fn an_anisotropic_map_inscribes_a_band_rather_than_covering_it() {
    // A band is a distance, and a map that stretches y tenfold and x not at
    // all does not carry one: the true image is elliptical and no single width
    // describes it. The mapped width is the widest one that fits *inside* that
    // image, so the band can under-reach along the stretched axis and can
    // never claim ground the true band does not cover — which is what keeps a
    // shape-mode query from picking an item its own bounding rect does not.
    let region = SceneRegion::stroke(Path::line(Point::ZERO, Point::new(100.0, 0.0)), 4.0);
    let stretched = region.to_local(&Transform2D::scale(1.0, 10.0));
    // Half-width 2 stays 2 — the least stretch — where the true image reaches
    // 20 along y and 2 along x.
    assert!(stretched.contains_point(Point::new(50.0, 1.5)));
    assert!(
        !stretched.contains_point(Point::new(50.0, 18.0)),
        "under-reaches along the stretched axis, deliberately"
    );
    // The geometric mean (3.16) would have claimed this, and it is outside the
    // band along x — where the true image is only 2 wide.
    let across = region.to_local(&Transform2D::scale(10.0, 1.0));
    assert!(!across.contains_point(Point::new(500.0, 3.0)));
    // An isotropic map is exact, and unchanged by any of this.
    let doubled = region.to_local(&Transform2D::scale(2.0, 2.0));
    assert!(doubled.contains_point(Point::new(100.0, 3.5)));
    assert!(!doubled.contains_point(Point::new(100.0, 5.0)));
    // A rotation is an isometry: the band keeps its width.
    let t = Transform2D::rotate(0.7);
    let turned = region.to_local(&t);
    assert!(turned.contains_point(t.apply_point(Point::new(50.0, 1.5))));
    assert!(!turned.contains_point(t.apply_point(Point::new(50.0, 4.0))));
}

#[test]
fn linear_scale_bounds_reports_both_singular_values() {
    let (lo, hi) = linear_scale_bounds(&Transform2D::scale(1.0, 10.0));
    assert!(
        (lo - 1.0).abs() < 1e-3 && (hi - 10.0).abs() < 1e-3,
        "{lo} {hi}"
    );
    let (lo, hi) = linear_scale_bounds(&Transform2D::rotate(0.9));
    assert!(
        (lo - 1.0).abs() < 1e-3 && (hi - 1.0).abs() < 1e-3,
        "{lo} {hi}"
    );
    let (lo, hi) = linear_scale_bounds(&Transform2D::scale(3.0, 3.0));
    assert!(
        (lo - 3.0).abs() < 1e-3 && (hi - 3.0).abs() < 1e-3,
        "{lo} {hi}"
    );
    // A collapsed axis has a zero smallest stretch and does not panic.
    let (lo, _) = linear_scale_bounds(&Transform2D::scale(0.0, 4.0));
    assert!(lo.abs() < 1e-3);
}

// ---------------------------------------------------------------------------
// Containment inside a ribbon
// ---------------------------------------------------------------------------

/// A bent wire: down the left, then across. The inside of the bend is where a
/// band is thinner than the straight-line distance between two points either
/// side of it.
fn bent_wire() -> Path {
    let mut p = Path::new();
    p.move_to(Point::new(58.0, 6.5))
        .line_to(Point::new(3.0, -81.0))
        .line_to(Point::new(59.5, -97.0));
    p
}

#[test]
fn a_box_whose_corners_are_all_on_a_wire_is_not_therefore_inside_its_band() {
    // The defect a vertex-and-crossing test cannot see: every corner of this
    // box is within the band's 4 units of the wire, and no box edge crosses the
    // wire's *centreline* — but the box bulges out of the ribbon across the
    // inside of the bend. A band's boundary is an offset curve it never stores,
    // so the crossing test is blind to it and `segment_within_band` is what
    // answers instead.
    let region = SceneRegion::stroke(bent_wire(), 8.0);
    let box_rect = Rect::new(28.0, -88.0, 27.5, 39.5);
    for corner in [
        Point::new(box_rect.x, box_rect.y),
        Point::new(box_rect.right(), box_rect.y),
        Point::new(box_rect.right(), box_rect.bottom()),
        Point::new(box_rect.x, box_rect.bottom()),
    ] {
        assert!(
            region.clearance(corner) >= 0.0,
            "corner {corner:?} is inside the band"
        );
    }
    assert!(!ItemShape::bounds(box_rect).contained_by_region(&region, 1.0));
    // It does meet the band, though — this is a containment failure, not a
    // disappearance.
    assert!(ItemShape::bounds(box_rect).intersects_region(&region, 1.0));
}

#[test]
fn something_that_really_does_fit_inside_a_ribbon_still_does() {
    // The other direction, so the fix above is not simply "reject everything":
    // a small tile parked on a straight stretch of a wide band is contained,
    // and the same tile nudged past the band's edge is not.
    let mut straight = Path::new();
    straight
        .move_to(Point::new(0.0, 0.0))
        .line_to(Point::new(200.0, 0.0));
    let region = SceneRegion::stroke(straight, 40.0);
    let inside = ItemShape::bounds(Rect::new(80.0, -8.0, 16.0, 16.0));
    assert!(inside.contained_by_region(&region, 1.0));
    let straddling = ItemShape::bounds(Rect::new(80.0, 12.0, 16.0, 16.0));
    assert!(!straddling.contained_by_region(&region, 1.0));
    // And an item riding the whole length of the band, which is the case a
    // conservative edge-length bound would have wrongly rejected.
    let along = ItemShape::bounds(Rect::new(4.0, -6.0, 190.0, 12.0));
    assert!(along.contained_by_region(&region, 1.0));
}

#[test]
fn a_bands_own_width_is_charged_against_the_regions() {
    // A shape with a band of its own is inside only when the whole ribbon is:
    // 20 units of item band inside 40 units of region band leaves 10 either
    // side of the centreline.
    let mut straight = Path::new();
    straight
        .move_to(Point::new(0.0, 0.0))
        .line_to(Point::new(200.0, 0.0));
    let region = SceneRegion::stroke(straight.clone(), 40.0);
    let thin = ItemShape::from_path(Path::line(Point::new(50.0, 0.0), Point::new(150.0, 0.0)))
        .unfilled()
        .stroked(20.0, StrokeSpace::Logical);
    assert!(thin.contained_by_region(&region, 1.0));
    let fat = ItemShape::from_path(Path::line(Point::new(50.0, 0.0), Point::new(150.0, 0.0)))
        .unfilled()
        .stroked(40.0, StrokeSpace::Logical);
    assert!(
        !fat.contained_by_region(&region, 1.0),
        "its band spills out"
    );
}

#[test]
fn segment_within_band_is_exact_where_sampling_is_not() {
    // The primitive on its own. A chord across the inside of a right-angled
    // bend: both endpoints sit on the centreline and the middle is 10 units
    // from it, so any test that looks only at the ends says yes.
    let mut bend = Path::new();
    bend.move_to(Point::new(-20.0, 0.0))
        .line_to(Point::new(0.0, 0.0))
        .line_to(Point::new(0.0, -20.0));
    let outline = bend.flatten(SHAPE_FLATTEN_TOLERANCE);
    let a = Point::new(-14.0, 0.0);
    let b = Point::new(0.0, -14.0);
    assert!(
        !segment_within_band(a, b, &outline, 5.0),
        "the chord's middle is about 9.9 from the corner"
    );
    assert!(segment_within_band(a, b, &outline, 12.0));
    // A segment straight along the centreline is inside any positive band.
    assert!(segment_within_band(
        Point::new(-18.0, 0.0),
        Point::new(-2.0, 0.0),
        &outline,
        0.5
    ));
    // A zero-width ribbon holds nothing.
    assert!(!segment_within_band(
        Point::new(-18.0, 0.0),
        Point::new(-2.0, 0.0),
        &outline,
        0.0
    ));
    // A segment that starts inside and leaves.
    assert!(!segment_within_band(
        Point::new(-18.0, 0.0),
        Point::new(-18.0, 40.0),
        &outline,
        5.0
    ));
}

// ---------------------------------------------------------------------------
// A rotated frame must not change what a lasso selects
// ---------------------------------------------------------------------------

/// A lasso of two separate loops: a small square and, far to its right, a
/// circle drawn as a full-sweep `ArcTo`.
///
/// The circle's `ArcTo` follows the square's `Close`, which is the shape
/// [`Path::transformed`] used to mis-reconnect: under a transform it cannot
/// carry an arc through, it expanded the arc but joined it to the square with
/// a `line_to`, welding two loops into one and handing every outline-walking
/// query a 220-unit edge that is in neither loop.
fn two_loop_lasso() -> Path {
    let mut p = Path::new();
    p.move_to(Point::new(0.0, 0.0))
        .line_to(Point::new(10.0, 0.0))
        .line_to(Point::new(10.0, 10.0))
        .line_to(Point::new(0.0, 10.0))
        .close();
    p.arc_to(Rect::new(180.0, -20.0, 40.0, 40.0), 0.0, 360.0);
    p.close();
    p
}

/// Tile centres on a 5 dp grid spanning both loops and the empty ground
/// between them — which is where a spurious connector shows up.
fn tile_centres() -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    let mut y = -25;
    while y <= 25 {
        let mut x = -5;
        while x <= 230 {
            out.push((x, y));
            x += 5;
        }
        y += 5;
    }
    out
}

/// Lay a 2 x 2 tile on every grid point, give each item `transform`, and ask
/// which ones the lasso picks.
///
/// The tiles are authored in the items' **local** frame and the region is
/// handed over in **scene** space, so [`SceneRegion::to_local`] maps the lasso
/// back through `transform.inverse()` before it ever meets a shape — the step
/// that expands the arc. With no transform the arc survives as an arc, which
/// is the reference answer.
fn lasso_picks(transform: Option<Transform2D>) -> BTreeSet<(i32, i32)> {
    let lasso = two_loop_lasso();
    let mut scene = Scene::new();
    let mut label = Vec::new();
    for (cx, cy) in tile_centres() {
        let id = scene.add_item(
            RectItem::new(Rect::new(cx as f32 - 1.0, cy as f32 - 1.0, 2.0, 2.0))
                .fill(teksilo_tokens::Color::RED),
            Point::ZERO,
        );
        if let Some(t) = transform {
            scene.set_transform(id, t);
        }
        label.push((id, (cx, cy)));
    }
    let region = match transform {
        Some(t) => SceneRegion::lasso(lasso.transformed(&t)),
        None => SceneRegion::lasso(lasso),
    };
    scene
        .items_in_region(&region, ItemSelectionMode::IntersectsItemShape, 1.0)
        .into_iter()
        .map(|id| label.iter().find(|(i, _)| *i == id).unwrap().1)
        .collect()
}

#[test]
fn a_two_loop_lasso_picks_the_same_tiles_under_a_rotation() {
    let plain = lasso_picks(None);
    let rotated = lasso_picks(Some(Transform2D::rotate(30f32.to_radians())));

    // Sanity: the lasso does select something, so agreement is not two empty
    // sets agreeing.
    assert!(!plain.is_empty(), "the lasso picks nothing at all");

    let extra: Vec<_> = rotated.difference(&plain).copied().collect();
    let missing: Vec<_> = plain.difference(&rotated).copied().collect();
    assert!(
        extra.is_empty() && missing.is_empty(),
        "rotating the items changed the selection: gained {} {extra:?}, lost {} {missing:?}",
        extra.len(),
        missing.len()
    );

    // The witness, named: a tile in the empty ground between the two loops,
    // touching neither. The welded edge ran straight through it.
    assert!(
        !plain.contains(&(75, 0)) && !rotated.contains(&(75, 0)),
        "the tile at (75, 0) is in neither loop"
    );
}
