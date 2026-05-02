//! View-transform helpers for `SceneView`.
//!
//! The view transform maps a scene-coordinate point to its visually-
//! displayed screen point. It's composed from four `Signal<f32>`s on
//! `SceneView` — `pan_x`, `pan_y`, `zoom`, `rotation` — kept separate so
//! each animates independently with its own epsilon (sub-pixel for
//! pan, sub-perceptual log-multiplier for zoom, sub-degree for
//! rotation).

pub use fern_canvas::Transform2D;
use fern_canvas::Vec2;

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
) -> Option<Vec2> {
    let old_view = compose_view(old_pan, old_zoom, old_rotation);
    let inverse = old_view.inverse()?;
    let scene_anchor = inverse.apply_point(screen_anchor);
    let projection_no_pan = compose_view(Vec2::ZERO, new_zoom, new_rotation);
    let projected = projection_no_pan.apply_point(scene_anchor);
    Some(Vec2::new(
        screen_anchor.x - projected.x,
        screen_anchor.y - projected.y,
    ))
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
        // Initial state: identity view.
        let center = Point::new(200.0, 100.0);
        let old_pan = Vec2::ZERO;
        let scene_under_center = Point::new(200.0, 100.0); // identity → same

        // Zoom by 2× around the gesture center.
        let new_pan = anchor_pan_for_pinch(center, old_pan, 1.0, 0.0, 2.0, 0.0).unwrap();
        let new_view = compose_view(new_pan, 2.0, 0.0);
        let new_screen = new_view.apply_point(scene_under_center);

        // The scene point under the gesture center must still project
        // to the gesture center after zoom.
        assert!((new_screen.x - center.x).abs() < 1e-3);
        assert!((new_screen.y - center.y).abs() < 1e-3);
    }

    #[test]
    fn anchor_pan_handles_non_origin_starting_pan() {
        // Start with pan = (50, 0), zoom = 1, gesture center = (200, 100).
        // Scene under center = (200 - 50, 100) = (150, 100).
        let center = Point::new(200.0, 100.0);
        let old_pan = Vec2::new(50.0, 0.0);

        // Zoom to 0.5×; the scene point at (150, 100) should still
        // project under (200, 100).
        let new_pan = anchor_pan_for_pinch(center, old_pan, 1.0, 0.0, 0.5, 0.0).unwrap();
        let new_view = compose_view(new_pan, 0.5, 0.0);
        let projected = new_view.apply_point(Point::new(150.0, 100.0));
        assert!((projected.x - center.x).abs() < 1e-3);
        assert!((projected.y - center.y).abs() < 1e-3);
    }
}
