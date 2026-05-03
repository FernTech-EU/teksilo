//! Transform helpers for `SceneView` and `Scene`.
//!
//! Two flavours of transform live in fern-scene:
//!
//! * The **view transform** (this module's [`compose_view`]) maps a
//!   scene-coord point to its screen position. Composed from four
//!   `Signal<f32>` — `pan_x`, `pan_y`, `zoom`, `rotation` — kept
//!   separate so each animates independently with its own epsilon.
//! * The **item transform chain** ([`local_to_scene`]) walks a
//!   scene-graph item's parent chain composing per-item local→parent
//!   transforms into a single local→scene affine. Used by hit-test,
//!   paint and the spatial index to project an item's local-coord
//!   geometry into scene space.

pub use fern_canvas::Transform2D;
use fern_canvas::{Point, Vec2};

/// Compose `pan`, `zoom`, and `rotation` into a single 2D affine view
/// transform. The composition order — scale → rotate → translate —
/// means a scene-coord point `p` ends up at
/// `(R_rot ∘ S_zoom)(p) + pan`, i.e. zoom and rotation happen around
/// the *scene's* origin and pan is the post-rotation screen offset.
///
/// Pinch-to-zoom-around-pointer keeps a chosen scene point anchored
/// under the gesture center by adjusting `pan` accordingly — see
/// [`anchor_pan_for_pinch`].
///
/// The composition direction matches the renderer's stack semantic
/// (`device_t.then(prev_top)`, deepest-first) so the same single
/// transform fed into `BuildContext::set_transform` produces the
/// expected visual.
pub fn compose_view(pan: Vec2, zoom: f32, rotation_radians: f32) -> Transform2D {
    Transform2D::scale(zoom, zoom)
        .then(&Transform2D::rotate(rotation_radians))
        .then(&Transform2D::translate(pan.x, pan.y))
}

/// Compute a new `pan` such that the scene point currently displayed
/// under `screen_anchor` stays under `screen_anchor` after the new
/// `zoom` / `rotation` are applied. This is the math behind "pinch to
/// zoom around pointer" — the gesture provides a center on screen
/// and a new scale, and the user expects the content under that
/// center to stay put.
///
/// `bounds_origin` is the SceneView's screen-space position (the
/// `bounds.origin` value the parent layout chose for it). The view
/// transform that's actually on the renderer's stack folds this in
/// so a child at scene (sx, sy) lands at
/// `(bounds.x + zoom*sx + pan.x, bounds.y + zoom*sy + pan.y)`. The
/// pinch math has to use the same composition to map the screen-
/// space gesture center back to a scene-coord anchor; the returned
/// pan is the raw `pan` value to store in the signal — the
/// composition will add `bounds_origin` back in at draw time.
///
/// For root SceneView (`bounds_origin = (0, 0)`) the math reduces
/// to the simpler "pan + zoom around scene origin" form.
///
/// Returns `None` if the old view transform is degenerate (zoom 0,
/// for example). In that case the caller should leave pan unchanged
/// or reset it.
pub fn anchor_pan_for_pinch(
    screen_anchor: fern_canvas::Point,
    old_pan: Vec2,
    old_zoom: f32,
    old_rotation: f32,
    new_zoom: f32,
    new_rotation: f32,
    bounds_origin: Vec2,
) -> Option<Vec2> {
    let old_effective_pan = Vec2::new(old_pan.x + bounds_origin.x, old_pan.y + bounds_origin.y);
    let old_view = compose_view(old_effective_pan, old_zoom, old_rotation);
    let inverse = old_view.inverse()?;
    let scene_anchor = inverse.apply_point(screen_anchor);
    let projection_no_pan = compose_view(Vec2::ZERO, new_zoom, new_rotation);
    let projected = projection_no_pan.apply_point(scene_anchor);
    // Effective pan that places `scene_anchor` under
    // `screen_anchor` post-zoom/rotation, minus the bounds_origin
    // baked into the composition.
    Some(Vec2::new(
        screen_anchor.x - projected.x - bounds_origin.x,
        screen_anchor.y - projected.y - bounds_origin.y,
    ))
}

/// Compose a single per-item local→parent transform from `local_pos` and
/// an optional rotation/scale `transform`. The item's local origin maps
/// to `local_pos` in parent coords; rotation/scale apply around the
/// local origin, then the result is translated.
///
/// Equivalent to `Translate(local_pos) ∘ transform` under the
/// renderer's stack-top compose semantic.
pub fn local_to_parent(local_pos: Point, transform: &Transform2D) -> Transform2D {
    transform.then(&Transform2D::translate(local_pos.x, local_pos.y))
}

/// Compose a chain of per-item local→parent transforms into one
/// local→scene transform. `chain` is ordered from the **leaf** item up
/// to (but not including) the scene root — i.e. the leaf's transform
/// is at index 0 and its parent's is at index 1, etc. An empty chain
/// is the identity.
pub fn compose_chain(chain: &[Transform2D]) -> Transform2D {
    let mut acc = Transform2D::identity();
    for t in chain {
        acc = acc.then(t);
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::Point;

    #[test]
    fn compose_identity_when_at_rest() {
        let t = compose_view(Vec2::ZERO, 1.0, 0.0);
        assert!(t.is_identity());
    }

    #[test]
    fn compose_pan_only_translates() {
        let t = compose_view(Vec2::new(10.0, 20.0), 1.0, 0.0);
        let p = t.apply_point(Point::new(5.0, 5.0));
        assert_eq!(p, Point::new(15.0, 25.0));
    }

    #[test]
    fn compose_zoom_scales_then_translates() {
        let t = compose_view(Vec2::new(100.0, 0.0), 2.0, 0.0);
        // Scene (10, 5) → scale → (20, 10) → translate → (120, 10).
        let p = t.apply_point(Point::new(10.0, 5.0));
        assert!((p.x - 120.0).abs() < 1e-5);
        assert!((p.y - 10.0).abs() < 1e-5);
    }

    #[test]
    fn anchor_pan_keeps_pointer_invariant_under_zoom() {
        // Root SceneView (bounds_origin = 0). Initial state: identity
        // view. Zoom by 2× around the gesture center; the scene
        // point under that center must still project to the center
        // afterward.
        let center = Point::new(200.0, 100.0);
        let old_pan = Vec2::ZERO;
        let scene_under_center = Point::new(200.0, 100.0); // identity → same

        let new_pan =
            anchor_pan_for_pinch(center, old_pan, 1.0, 0.0, 2.0, 0.0, Vec2::ZERO).unwrap();
        let new_view = compose_view(new_pan, 2.0, 0.0);
        let new_screen = new_view.apply_point(scene_under_center);

        assert!((new_screen.x - center.x).abs() < 1e-3);
        assert!((new_screen.y - center.y).abs() < 1e-3);
    }

    #[test]
    fn anchor_pan_handles_non_origin_starting_pan() {
        // Start with pan = (50, 0), zoom = 1, gesture center = (200, 100).
        // Scene under center = (200 - 50, 100) = (150, 100).
        let center = Point::new(200.0, 100.0);
        let old_pan = Vec2::new(50.0, 0.0);

        let new_pan =
            anchor_pan_for_pinch(center, old_pan, 1.0, 0.0, 0.5, 0.0, Vec2::ZERO).unwrap();
        let new_view = compose_view(new_pan, 0.5, 0.0);
        let projected = new_view.apply_point(Point::new(150.0, 100.0));
        assert!((projected.x - center.x).abs() < 1e-3);
        assert!((projected.y - center.y).abs() < 1e-3);
    }

    #[test]
    fn anchor_pan_handles_non_zero_bounds_origin() {
        // SceneView positioned at parent-local (100, 50) — i.e.
        // bounds_origin = (100, 50). Pan = 0, zoom = 1. Identity-ish
        // view: scene-coord (sx, sy) lands at screen (100+sx, 50+sy).
        //
        // Pinch at screen (300, 150). Scene point under that center
        // = (300 - 100, 150 - 50) = (200, 100).
        //
        // Zoom to 2× around (300, 150). After zoom, scene (200, 100)
        // must still project to (300, 150).
        let center = Point::new(300.0, 150.0);
        let bounds_origin = Vec2::new(100.0, 50.0);
        let old_pan = Vec2::ZERO;
        let new_pan =
            anchor_pan_for_pinch(center, old_pan, 1.0, 0.0, 2.0, 0.0, bounds_origin).unwrap();

        let new_effective_pan =
            Vec2::new(new_pan.x + bounds_origin.x, new_pan.y + bounds_origin.y);
        let new_view = compose_view(new_effective_pan, 2.0, 0.0);
        let projected = new_view.apply_point(Point::new(200.0, 100.0));
        assert!(
            (projected.x - center.x).abs() < 1e-3,
            "projected x = {}, expected {}",
            projected.x,
            center.x
        );
        assert!(
            (projected.y - center.y).abs() < 1e-3,
            "projected y = {}, expected {}",
            projected.y,
            center.y
        );
    }
}
