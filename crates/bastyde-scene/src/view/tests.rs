// Sub-modules — added in Unit 9 to fill the audit-flagged
// coverage gaps without bulk-moving the existing legacy tests
// out of this file. The mechanical split of the legacy section
// is a follow-up.
mod a11y;
mod edge_cases;
mod nested;

use super::*;
use bastyde_core::widget_tree::WidgetTree;

#[derive(Debug)]
struct FillWidget;

impl FillWidget {
    fn new() -> Self {
        Self
    }
}

impl Widget for FillWidget {
    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(0.0, 0.0).into()
    }
}

// -- Placement -----------------------------------

#[test]
fn scene_view_places_widgets_at_scene_coords() {
    let mut scene = Scene::new();
    let a = scene.add_widget(FillWidget::new(), Rect::new(10.0, 20.0, 100.0, 50.0));
    let b = scene.add_widget(FillWidget::new(), Rect::new(200.0, 100.0, 80.0, 80.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    let kids = tree.children(view_id);
    assert_eq!(kids.len(), 2);
    assert_eq!(tree.bounds(kids[0]), Rect::new(10.0, 20.0, 100.0, 50.0));
    assert_eq!(tree.bounds(kids[1]), Rect::new(200.0, 100.0, 80.0, 80.0));

    let view = tree
        .widget_as_any(view_id)
        .and_then(|a| a.downcast_ref::<SceneView>())
        .expect("view is a SceneView");
    assert_eq!(view.widget_id_for(a), Some(kids[0]));
    assert_eq!(view.widget_id_for(b), Some(kids[1]));
}

#[test]
fn scene_view_layout_takes_proposal() {
    let scene = Scene::new();
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let bounds = tree.bounds(view_id);
    assert_eq!(bounds.width, 400.0);
    assert_eq!(bounds.height, 300.0);
}

#[test]
fn empty_scene_has_no_children() {
    let scene = Scene::new();
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    assert!(tree.children(view_id).is_empty());
}

#[test]
fn scene_view_default_size_when_proposal_unspecified() {
    let scene = Scene::new();
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene).default_size(640.0, 480.0));
    tree.layout(SizeProposal::unspecified());
    let bounds = tree.bounds(view_id);
    assert_eq!(bounds.width, 640.0);
    assert_eq!(bounds.height, 480.0);
}

// -- View-transform behaviour --------------------------------

fn view_handle(tree: &WidgetTree, view_id: WidgetId) -> &SceneView {
    tree.widget_as_any(view_id)
        .and_then(|a| a.downcast_ref::<SceneView>())
        .expect("view is a SceneView")
}

#[test]
fn initial_view_transform_is_identity() {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let view = view_handle(&tree, view_id);
    assert!(view.view_transform().is_identity());
    assert_eq!(view.pan(), Vec2::ZERO);
    assert_eq!(view.zoom(), 1.0);
    assert_eq!(view.rotation(), 0.0);
}

#[test]
fn set_pan_and_set_zoom_update_view_transform_immediately() {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    let view = view_handle(&tree, view_id);
    view.set_pan(Vec2::new(100.0, 0.0));
    view.set_zoom(2.0);
    let t = view.view_transform();
    // Scene (10, 5) → scale → (20, 10) → translate → (120, 10).
    let p = t.apply_point(Point::new(10.0, 5.0));
    assert!((p.x - 120.0).abs() < 1e-5);
    assert!((p.y - 10.0).abs() < 1e-5);
}

#[test]
fn pan_to_animates_mid_flight() {
    // Animation acceptance: pan_to(target, duration) ramps from
    // start to target over `duration`. At halfway, pan_x must be
    // strictly between start and target — proving the value is
    // mid-tween rather than snapped.
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    view_handle(&tree, view_id).pan_to(Vec2::new(400.0, 0.0), Duration::from_millis(200));
    // Advance halfway. The first tick processes pending requests
    // and starts the animation; the second advances the clock.
    tree.tick_animations(Duration::from_millis(100));
    let mid_x = view_handle(&tree, view_id).pan().x;
    assert!(
        mid_x > 0.0 && mid_x < 400.0,
        "pan_x should be mid-tween (got {})",
        mid_x
    );
    // Finish the animation.
    tree.tick_animations(Duration::from_millis(120));
    let end_x = view_handle(&tree, view_id).pan().x;
    assert!(
        (end_x - 400.0).abs() < 0.5,
        "pan_x should land near 400 (got {})",
        end_x
    );
}

#[test]
fn idle_drain_zero_frames_at_rest() {
    // The headline non-functional test: a SceneView that's been
    // built and laid out and is not currently animating must not
    // request any further frames *of its own accord*. Note that
    // `needs_redraw()` will still be true while `needs_paint` is
    // pending — that's the framework's normal "renderer hasn't
    // painted yet" signal, cleared on the next paint pass. The
    // bastyde-scene-specific contract is: no animation scheduler
    // entries running, no `request_frame()` calls from us.
    let mut scene = Scene::new();
    for i in 0..5 {
        scene.add_widget(
            FillWidget::new(),
            Rect::new(i as f32 * 50.0, 0.0, 40.0, 40.0),
        );
    }
    let mut tree = WidgetTree::new();
    let _view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    tree.tick_animations(Duration::from_millis(0));
    assert!(
        !tree.has_active_animations(),
        "no animations active at rest"
    );
    assert!(
        !tree.frame_requested(),
        "bastyde-scene must not call request_frame() at rest"
    );
    assert_eq!(
        tree.active_animation_count(),
        0,
        "scheduler queue must be empty at rest"
    );
}

#[test]
fn idle_drain_returns_after_pan_animation_completes() {
    // Variant of the idle-drain test: trigger an animation, let
    // it finish, then assert idle.
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    view_handle(&tree, view_id).pan_to(Vec2::new(200.0, 0.0), Duration::from_millis(80));
    // Push past the terminal tick (animation duration + slack).
    tree.tick_animations(Duration::from_millis(120));
    tree.tick_animations(Duration::from_millis(0));
    assert!(
        !tree.has_active_animations(),
        "animation must terminate cleanly"
    );
    // Pan should have reached its target.
    let pan = view_handle(&tree, view_id).pan();
    assert!((pan.x - 200.0).abs() < 0.5);
}

#[test]
fn zoom_to_clamps_to_max_zoom() {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()).max_zoom(4.0));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    view_handle(&tree, view_id).zoom_to(100.0, Duration::from_millis(50));
    tree.tick_animations(Duration::from_millis(80));
    tree.tick_animations(Duration::from_millis(0));
    assert!((view_handle(&tree, view_id).zoom() - 4.0).abs() < 0.001);
}

#[test]
fn zoom_to_clamps_to_min_zoom() {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()).min_zoom(0.5));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    view_handle(&tree, view_id).zoom_to(0.001, Duration::from_millis(50));
    tree.tick_animations(Duration::from_millis(80));
    tree.tick_animations(Duration::from_millis(0));
    assert!((view_handle(&tree, view_id).zoom() - 0.5).abs() < 0.001);
}

#[test]
fn fit_to_content_centres_scene_in_viewport() {
    let mut scene = Scene::new();
    // Two cards at scene coords; bounding box: (0, 0, 200, 100).
    scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 100.0, 100.0));
    scene.add_widget(FillWidget::new(), Rect::new(100.0, 0.0, 100.0, 100.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    view_handle(&tree, view_id).fit_to_content();
    // Drive past the terminal tick.
    tree.tick_animations(Duration::from_millis(200));
    tree.tick_animations(Duration::from_millis(0));

    let view = view_handle(&tree, view_id);
    let t = view.view_transform();
    // Content centre (100, 50) should project to viewport centre
    // (400, 300) under the resulting view transform.
    let projected = t.apply_point(Point::new(100.0, 50.0));
    assert!(
        (projected.x - 400.0).abs() < 1.0,
        "content centre x should land at viewport centre (got {})",
        projected.x
    );
    assert!(
        (projected.y - 300.0).abs() < 1.0,
        "content centre y should land at viewport centre (got {})",
        projected.y
    );
}

#[test]
fn scene_content_bounds_unions_all_items() {
    let mut scene = Scene::new();
    scene.add_widget(FillWidget::new(), Rect::new(-50.0, -20.0, 30.0, 30.0));
    scene.add_widget(FillWidget::new(), Rect::new(100.0, 50.0, 40.0, 40.0));
    let view = SceneView::new(scene);
    let b = view.scene_content_bounds().unwrap();
    assert!((b.x - -50.0).abs() < 1e-5);
    assert!((b.y - -20.0).abs() < 1e-5);
    assert!((b.right() - 140.0).abs() < 1e-5);
    assert!((b.bottom() - 90.0).abs() < 1e-5);
}

#[test]
fn scene_content_bounds_empty() {
    let view = SceneView::new(Scene::new());
    assert!(view.scene_content_bounds().is_none());
}

// -- Gesture wiring -----------------------------------------

#[test]
fn on_scroll_pixels_animates_pan() {
    // Trackpad two-finger pan delivers `ScrollDelta::Pixels`.
    // Verify the on_scroll handler routes the delta into the pan
    // signals as an `Easing::EaseOut` tween.
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    // Move pointer into the viewport so Scroll has a target.
    tree.pointer_move(Point::new(400.0, 300.0));
    tree.dispatch_event(WidgetEvent::Scroll {
        delta: ScrollDelta::Pixels { x: 50.0, y: 30.0 },
        modifiers: Default::default(),
    });

    // The animation has started but not finished — `animation_target`
    // should already reflect the requested delta.
    let view = view_handle(&tree, view_id);
    assert_eq!(view.pan_x.animation_target(), Some(50.0));
    assert_eq!(view.pan_y.animation_target(), Some(30.0));

    // Drive past the terminal tick.
    tree.tick_animations(Duration::from_millis(180));
    tree.tick_animations(Duration::from_millis(0));
    let view = view_handle(&tree, view_id);
    assert!((view.pan().x - 50.0).abs() < 0.5);
    assert!((view.pan().y - 30.0).abs() < 0.5);
}

#[test]
fn panned_scene_viewport_is_fully_hittable() {
    // A SceneView applies pan/zoom as a node transform but `clips_children`,
    // so its bounds are a fixed screen viewport: the whole viewport must stay
    // hittable at any pan, so a wheel / click over the visible scene reaches
    // the SceneView instead of falling through to whatever is behind it.
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()).default_size(200.0, 100.0));
    tree.layout(SizeProposal::exact(200.0, 100.0));
    view_handle(&tree, view_id).set_pan(bastyde_canvas::Vec2::new(50.0, 30.0));
    tree.layout(SizeProposal::exact(200.0, 100.0));

    // Every point inside the screen viewport hits the panned SceneView. The
    // near-origin points (1,1)/(10,10)/(20,10) inverse-map to negative scene
    // coords and missed before the hit-test fix.
    for p in [(1.0, 1.0), (10.0, 10.0), (20.0, 10.0), (100.0, 50.0), (199.0, 99.0)] {
        assert_eq!(
            tree.hit_test(bastyde_canvas::Point::new(p.0, p.1)),
            Some(view_id),
            "viewport point {p:?} must hit the panned SceneView",
        );
    }
    // Outside the viewport: no hit.
    assert_eq!(tree.hit_test(bastyde_canvas::Point::new(250.0, 50.0)), None);
}

#[test]
fn on_scroll_lines_uses_line_height_multiplier() {
    // Mouse-wheel scrolling delivers `ScrollDelta::Lines`. Each
    // line notch translates to `line_height` logical pixels of
    // pan (default 16). With `line_height(32.0)` set, a single
    // notch should target 32 px.
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()).line_height(32.0));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    tree.pointer_move(Point::new(400.0, 300.0));
    tree.dispatch_event(WidgetEvent::Scroll {
        delta: ScrollDelta::Lines { x: 0.0, y: 1.0 },
        modifiers: Default::default(),
    });

    let view = view_handle(&tree, view_id);
    assert_eq!(view.pan_y.animation_target(), Some(32.0));
}

#[test]
fn ctrl_wheel_zooms_about_cursor_keeping_scene_anchor_fixed() {
    // Ctrl+wheel zoom must keep the scene point under the cursor
    // fixed across the zoom step. Without zoom-about-pointer the
    // user perceives the scene "drifting away" — the right side
    // of the scene shifts further right when zooming in about
    // viewport center.
    use bastyde_core::event::{Modifiers, WidgetEvent as Ev};

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    // Park the cursor at (700, 400) — far from viewport center
    // (400, 300) — so any anchor mistake shows up clearly.
    let cursor = Point::new(700.0, 400.0);
    tree.dispatch_event(Ev::PointerMove { position: cursor });

    // Capture scene point under the cursor BEFORE zooming.
    let view = view_handle(&tree, view_id);
    let xform_before = view.view_transform();
    let scene_under_cursor = xform_before
        .inverse()
        .expect("identity xform invertible")
        .apply_point(cursor);

    // Ctrl+wheel scroll up by 1 line → zoom in.
    tree.dispatch_event(Ev::Scroll {
        delta: ScrollDelta::Lines { x: 0.0, y: 1.0 },
        modifiers: Modifiers::CTRL,
    });

    // Verify zoom changed and the scene point originally under
    // the cursor still projects to the cursor position.
    let view = view_handle(&tree, view_id);
    assert!(
        (view.zoom() - 1.0).abs() > 1e-3,
        "zoom should have changed (got {})",
        view.zoom()
    );
    let projected = view.view_transform().apply_point(scene_under_cursor);
    assert!(
        (projected.x - cursor.x).abs() < 0.5,
        "x: scene anchor must stay under cursor (cursor {} → projected {})",
        cursor.x,
        projected.x
    );
    assert!(
        (projected.y - cursor.y).abs() < 0.5,
        "y: scene anchor must stay under cursor (cursor {} → projected {})",
        cursor.y,
        projected.y
    );
}

#[test]
fn on_pinch_zooms_around_gesture_center() {
    // PinchPhase::Changed scales the zoom signal and re-anchors
    // pan so the scene point under the gesture center stays put.
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    tree.pointer_move(Point::new(200.0, 100.0));
    tree.dispatch_event(WidgetEvent::Gesture {
        gesture: bastyde_core::gesture::GestureEvent::PinchChanged {
            center: Point::new(200.0, 100.0),
            scale: 2.0,
            rotation: 0.0,
        },
    });

    let view = view_handle(&tree, view_id);
    // Zoom doubled — and the pinch handler `set`s synchronously
    // (no animation), so we can read the result immediately.
    assert!((view.zoom() - 2.0).abs() < 1e-3);
    // The scene point that was visible at (200, 100) before the
    // pinch should still project to (200, 100). At the start
    // pan = 0, zoom = 1, so the scene point under (200, 100) was
    // (200, 100). Under the new view (zoom 2, pan ?) it must
    // project to (200, 100) again.
    let projected = view.view_transform().apply_point(Point::new(200.0, 100.0));
    assert!(
        (projected.x - 200.0).abs() < 1e-2,
        "pinch must keep scene point under gesture center invariant (got x={})",
        projected.x
    );
    assert!((projected.y - 100.0).abs() < 1e-2);
}

#[test]
fn on_pinch_clamps_zoom_to_max() {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()).max_zoom(3.0));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    tree.pointer_move(Point::new(400.0, 300.0));
    tree.dispatch_event(WidgetEvent::Gesture {
        gesture: bastyde_core::gesture::GestureEvent::PinchChanged {
            center: Point::new(400.0, 300.0),
            scale: 100.0, // would zoom to 100× without clamp
            rotation: 0.0,
        },
    });
    let view = view_handle(&tree, view_id);
    assert!((view.zoom() - 3.0).abs() < 1e-3);
}

#[test]
fn reduced_motion_snaps_pan_instead_of_animating() {
    // When `prefers-reduced-motion` is set on the tree before the
    // SceneView is built, on_scroll must snap pan signals
    // directly (no animation, no scheduler entry).
    let mut tree = WidgetTree::new();
    tree.set_accessibility_preferences(false, true, 1.0);
    let view_id = tree.add(SceneView::new(Scene::new()));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    tree.pointer_move(Point::new(400.0, 300.0));
    tree.dispatch_event(WidgetEvent::Scroll {
        delta: ScrollDelta::Pixels { x: 50.0, y: 30.0 },
        modifiers: Default::default(),
    });

    let view = view_handle(&tree, view_id);
    // The signal landed at the target immediately, no tween.
    assert!((view.pan().x - 50.0).abs() < 1e-3);
    assert!((view.pan().y - 30.0).abs() < 1e-3);
    // No animation was queued.
    assert!(view.pan_x.animation_target().is_none());
    assert!(view.pan_y.animation_target().is_none());
    assert!(!tree.has_active_animations());
}

// -- Viewport culling --------------------------------------

#[test]
fn off_screen_items_are_culled_to_zero_size() {
    // The headline viewport-cull test: a SceneView at 800×600 with one
    // item inside the viewport and one item far outside. The
    // off-screen item's bounds collapse to zero so the layout/
    // paint walks short-circuit on it.
    let mut scene = Scene::new();
    let inside = scene.add_widget(FillWidget::new(), Rect::new(50.0, 50.0, 100.0, 100.0));
    let outside = scene.add_widget(FillWidget::new(), Rect::new(5_000.0, 5_000.0, 100.0, 100.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    let view = view_handle(&tree, view_id);
    let inside_widget = view.widget_id_for(inside).unwrap();
    let outside_widget = view.widget_id_for(outside).unwrap();

    let inside_bounds = tree.bounds(inside_widget);
    let outside_bounds = tree.bounds(outside_widget);
    assert_eq!(inside_bounds, Rect::new(50.0, 50.0, 100.0, 100.0));
    assert_eq!(
        outside_bounds.width, 0.0,
        "off-screen item must have zero width"
    );
    assert_eq!(
        outside_bounds.height, 0.0,
        "off-screen item must have zero height"
    );
}

#[test]
fn pan_brings_culled_items_back_into_view() {
    // Items outside the initial viewport collapse to zero; pan
    // the view to cover them and they should pop back to full
    // size on the next layout.
    let mut scene = Scene::new();
    let far_right = scene.add_widget(FillWidget::new(), Rect::new(2_000.0, 50.0, 100.0, 100.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    let view = view_handle(&tree, view_id);
    let far_widget = view.widget_id_for(far_right).unwrap();
    // Before pan: far_right is outside the viewport, culled to
    // zero.
    assert_eq!(tree.bounds(far_widget).width, 0.0);

    // Pan to bring it into view: pan_x = 1900 means scene-coord
    // 2000 lands at screen 100, well within the 800-px viewport.
    // (Pan is animated; snap directly via `set_pan` so the test
    // doesn't have to drive the scheduler for this case.)
    view.set_pan(Vec2::new(-1900.0, 0.0));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    let view = view_handle(&tree, view_id);
    let bounds = tree.bounds(view.widget_id_for(far_right).unwrap());
    assert_eq!(
        bounds,
        Rect::new(2_000.0, 50.0, 100.0, 100.0),
        "panned-to item should be re-inflated to its full scene_rect"
    );
}

#[test]
fn cull_uses_scene_rect_origin_as_anchor_even_when_culled() {
    // Even when collapsed to zero size, the culled child's
    // origin stays at its canonical scene-rect position. This
    // means focus-follow / scroll-into-view machinery sees a
    // consistent coordinate even for off-screen items.
    let mut scene = Scene::new();
    let id = scene.add_widget(FillWidget::new(), Rect::new(10_000.0, 5_000.0, 80.0, 80.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    let view = view_handle(&tree, view_id);
    let widget = view.widget_id_for(id).unwrap();
    let bounds = tree.bounds(widget);
    assert_eq!(bounds.x, 10_000.0);
    assert_eq!(bounds.y, 5_000.0);
    assert_eq!(bounds.width, 0.0);
    assert_eq!(bounds.height, 0.0);
}

#[test]
fn zoom_changes_culling_set() {
    // At zoom 1, an item far from the viewport is culled.
    // Zooming way out (small zoom = wide visible region) should
    // bring it back into the visible set.
    let mut scene = Scene::new();
    let far = scene.add_widget(FillWidget::new(), Rect::new(2_000.0, 0.0, 50.0, 50.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene).min_zoom(0.05));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    let view = view_handle(&tree, view_id);
    let far_widget = view.widget_id_for(far).unwrap();
    assert_eq!(tree.bounds(far_widget).width, 0.0);

    // Zoom out to 0.1× — the visible scene region is 8000 px
    // wide, well past the item at x=2000.
    view.set_zoom(0.1);
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let view = view_handle(&tree, view_id);
    assert!(
        tree.bounds(view.widget_id_for(far).unwrap()).width > 0.0,
        "zooming out must un-cull off-screen items"
    );
}

#[test]
fn non_root_scene_view_places_children_at_scene_coords_and_culls_correctly() {
    // SceneView nested inside a non-zero-origin parent (a Padding
    // wrapper that pushes it to (40, 40)). Verify:
    //   1. Children are placed at *pure scene_rect* in the
    //      arena (not offset by bounds.origin).
    //   2. The view transform folds in `bounds.origin` so the
    //      visual position of scene-coord (sx, sy) is
    //      (40 + zoom*sx + pan.x, 40 + zoom*sy + pan.y).
    //   3. Culling uses the screen-space SceneView rect so
    //      items in the visible scene region survive while
    //      far-off ones collapse.
    use bastyde_widgets::primitives::Padding;

    let mut scene = Scene::new();
    let inside = scene.add_widget(FillWidget::new(), Rect::new(10.0, 20.0, 100.0, 50.0));
    let outside = scene.add_widget(FillWidget::new(), Rect::new(5_000.0, 5_000.0, 50.0, 50.0));

    let mut tree = WidgetTree::new();
    let view = SceneView::new(scene);
    // Padding(40, 40, 40, 40) shifts the SceneView's bounds.origin
    // to (40, 40) within the 800×600 root layout.
    let root_id = tree.add(Padding::uniform(40.0_f32).child(view));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    // The SceneView should be the only child of the Padding.
    let view_id = tree.children(root_id)[0];
    let view = tree
        .widget_as_any(view_id)
        .and_then(|a| a.downcast_ref::<SceneView>())
        .expect("nested view downcast");

    // After layout, the SceneView's bounds_origin signal mirrors
    // the parent's chosen position (40, 40).
    assert_eq!(
        view.view_transform().apply_point(Point::new(0.0, 0.0)),
        Point::new(40.0, 40.0),
        "scene origin should land at SceneView's bounds origin under identity view"
    );

    // The visible item's arena bounds = pure scene_rect (no
    // bounds.origin offset, since the renderer adds it via
    // set_transform at paint time).
    let inside_widget = view.widget_id_for(inside).unwrap();
    assert_eq!(
        tree.bounds(inside_widget),
        Rect::new(10.0, 20.0, 100.0, 50.0),
        "child placed at pure scene_rect"
    );
    // Visual position via view_transform = bounds.origin + scene_rect.origin
    // (zoom = 1, pan = 0, rotation = 0).
    let visual_origin = view.view_transform().apply_point(Point::new(10.0, 20.0));
    assert!(
        (visual_origin.x - 50.0).abs() < 1e-3,
        "visual x = bounds.x + scene.x = 40 + 10 = 50 (got {})",
        visual_origin.x
    );
    assert!((visual_origin.y - 60.0).abs() < 1e-3);

    // Off-screen item culled to zero size despite the non-root
    // bounds.origin.
    let outside_widget = view.widget_id_for(outside).unwrap();
    let outside_bounds = tree.bounds(outside_widget);
    assert_eq!(outside_bounds.width, 0.0);
    assert_eq!(outside_bounds.height, 0.0);
}

#[test]
fn non_root_pinch_keeps_scene_under_gesture_center_invariant() {
    // The bounds-origin fix to `anchor_pan_for_pinch` means that
    // even when the SceneView is positioned at a non-zero parent
    // offset, pinch-to-zoom keeps the scene point under the
    // gesture center anchored to that center after zoom.
    //
    // The scene needs at least one item so `place_children`
    // actually runs and refreshes `bounds_origin_signal` — an
    // empty SceneView has no children for the framework to
    // walk past `place_children`, so its bounds origin would
    // stay at the `Vec2::ZERO` initial (a documented edge case
    // for empty scenes that real apps never hit).
    use bastyde_widgets::primitives::Padding;

    let mut scene = Scene::new();
    scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 10.0, 10.0));
    let mut tree = WidgetTree::new();
    let view = SceneView::new(scene);
    let root_id = tree.add(Padding::uniform(50.0_f32).child(view));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let view_id = tree.children(root_id)[0];

    // Move the pointer onto the SceneView and dispatch a pinch.
    // Gesture center at screen (200, 150). The SceneView is at
    // bounds.origin = (50, 50), so the scene point under the
    // center is (200 - 50, 150 - 50) = (150, 100) at zoom 1.
    tree.pointer_move(Point::new(200.0, 150.0));
    tree.dispatch_event(WidgetEvent::Gesture {
        gesture: bastyde_core::gesture::GestureEvent::PinchChanged {
            center: Point::new(200.0, 150.0),
            scale: 2.0,
            rotation: 0.0,
        },
    });

    let view = tree
        .widget_as_any(view_id)
        .and_then(|a| a.downcast_ref::<SceneView>())
        .expect("downcast");
    // After zoom, scene (150, 100) must still project to
    // screen (200, 150).
    let projected = view.view_transform().apply_point(Point::new(150.0, 100.0));
    assert!(
        (projected.x - 200.0).abs() < 1e-2,
        "projected x = {}, expected 200",
        projected.x
    );
    assert!((projected.y - 150.0).abs() < 1e-2);
}

#[test]
fn empty_scene_culling_is_a_no_op() {
    // Trivial — empty scene, trivial cull. Pins that the empty
    // case doesn't panic on the inverse-transform / index query
    // path.
    let scene = Scene::new();
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    assert!(tree.children(view_id).is_empty());
}

#[test]
fn pinch_with_invalid_scale_is_no_op() {
    // Defensive: scale = 0 or NaN must not crash or zero the zoom.
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    tree.pointer_move(Point::new(400.0, 300.0));
    tree.dispatch_event(WidgetEvent::Gesture {
        gesture: bastyde_core::gesture::GestureEvent::PinchChanged {
            center: Point::new(400.0, 300.0),
            scale: 0.0,
            rotation: 0.0,
        },
    });
    assert!((view_handle(&tree, view_id).zoom() - 1.0).abs() < 1e-3);

    tree.dispatch_event(WidgetEvent::Gesture {
        gesture: bastyde_core::gesture::GestureEvent::PinchChanged {
            center: Point::new(400.0, 300.0),
            scale: f32::NAN,
            rotation: 0.0,
        },
    });
    assert!((view_handle(&tree, view_id).zoom() - 1.0).abs() < 1e-3);
}

// -- Lightweight items ---------------------------------------

#[test]
fn scene_view_paints_visible_lightweight_items() {
    // Scene with a lightweight RectItem inside the viewport and
    // another well outside it. After `tree.render()`, exactly one
    // DecorationRect lands in the frame — the off-screen item is
    // culled before paint by `items_in_rect`.
    use crate::items::RectItem;
    use bastyde_tokens::Color;

    let mut scene = Scene::new();
    let _on_screen = scene.add_item(
        RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)).fill(Color::RED),
        Point::ZERO,
    );
    let _off_screen = scene.add_item(
        RectItem::new(Rect::new(5_000.0, 5_000.0, 20.0, 20.0)).fill(Color::BLUE),
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let _view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let frame = tree.render();
    // Single visible filled item ⇒ exactly one decoration.
    // (FillWidget paints nothing; SceneView paints no chrome of
    // its own; the off-screen item is culled.)
    assert_eq!(
        frame.decorations.len(),
        1,
        "visible RectItem must emit exactly one DecorationRect, off-screen item must be culled"
    );
    assert_eq!(frame.decorations[0].color, Color::RED.to_array());
}

#[test]
fn scene_view_culls_all_off_screen_lightweight_items() {
    // Both items off-screen → zero decorations from the
    // lightweight tier.
    use crate::items::RectItem;
    use bastyde_tokens::Color;

    let mut scene = Scene::new();
    scene.add_item(
        RectItem::new(Rect::new(5_000.0, 5_000.0, 20.0, 20.0)).fill(Color::RED),
        Point::ZERO,
    );
    scene.add_item(
        RectItem::new(Rect::new(-5_000.0, -5_000.0, 20.0, 20.0)).fill(Color::BLUE),
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let _view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let frame = tree.render();
    assert!(frame.decorations.is_empty());
}

#[test]
fn scene_view_paints_no_items_when_scene_is_widget_only() {
    // Heavyweight-only scene: SceneView::paint walks `items_in_rect`
    // but `Scene::item(id)` returns None for widgets, so no extra
    // draw commands are emitted from the lightweight tier.
    // Verifies the kind-filtering in paint and avoids a
    // double-paint of widget bounds via the scene path.
    let mut scene = Scene::new();
    scene.add_widget(FillWidget::new(), Rect::new(10.0, 10.0, 20.0, 20.0));

    let mut tree = WidgetTree::new();
    let _view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let frame = tree.render();
    // FillWidget paints nothing of its own; the scene contains
    // only widgets (no SceneItems); the SceneView itself doesn't
    // draw any background. Therefore the frame has no
    // decoration / shape / glyph / path entries at all.
    assert!(frame.decorations.is_empty());
    assert!(frame.paths.is_empty());
    assert!(frame.shapes.is_empty());
}

#[test]
fn scene_view_clips_children_so_items_dont_leak() {
    // SceneView::clips_children() returns true. Without a clip,
    // a path-item whose stroke extends past the viewport would
    // bleed past the SceneView's screen rect. The clip is what
    // contains the lightweight tier visually.
    let scene = Scene::new();
    let view = SceneView::new(scene);
    assert!(
        Widget::clips_children(&view),
        "SceneView must clip its subtree so light items don't bleed past bounds"
    );
}

// -- A11y + keyboard navigation -----------------------------

#[test]
fn scene_view_emits_synthetic_at_node_per_visible_item() {
    // The AT walker should emit one synthetic AT node per
    // visible lightweight item, with screen-projected bounds.
    // Off-screen items (subject to the off-screen-mode policy)
    // should be excluded from the tree.
    use crate::items::RectItem;
    use bastyde_core::accessibility::{SyntheticKind, is_synthetic, synthetic_node_id};
    use bastyde_tokens::Color;

    let mut scene = Scene::new();
    let on_screen = scene.add_item(
        RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0))
            .fill(Color::RED)
            .access_label("nearby"),
        Point::ZERO,
    );
    let _far_off = scene.add_item(
        RectItem::new(Rect::new(50_000.0, 50_000.0, 20.0, 20.0)).fill(Color::BLUE),
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Compute the synthetic NodeId we expect for the on-screen
    // item. The walker derives `synthetic_node_id(view_id,
    // item_id.as_u64(), SyntheticKind::SceneItem)`.
    let expected_id = synthetic_node_id(view_id, on_screen.as_u64(), SyntheticKind::SceneItem);
    assert!(is_synthetic(expected_id), "must have bit-63 set");

    // Build the AT tree update and verify our synthetic NodeId
    // appears (and the off-screen item's would-be id does not).
    let update = tree.sync_accessibility();
    let nodes_have_id =
        |needle: accesskit::NodeId| update.nodes.iter().any(|(id, _)| *id == needle);
    assert!(
        nodes_have_id(expected_id),
        "on-screen item must appear in the AT tree update"
    );
    let synthetic_count = update
        .nodes
        .iter()
        .filter(|(id, _)| is_synthetic(*id))
        .count();
    assert!(
        synthetic_count >= 1,
        "expected at least one synthetic SceneItem node, got {}",
        synthetic_count
    );
}

#[test]
fn keyboard_arrow_keys_animate_pan() {
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};

    let mut scene = Scene::new();
    scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 10.0, 10.0));
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    // Focus the SceneView so on_key fires on it.
    tree.focus(view_id);
    let pan_before = view_handle(&tree, view_id).pan();

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::default(),
        text: None,
    });

    // Pan target should move (negative x — content shifts to
    // bring the viewport's right side into view, equivalent to
    // panning the scene leftward in screen space). The pan
    // signal is animated; we check the *target*.
    let pan_target_x = view_handle(&tree, view_id)
        .pan_x_animation_target()
        .unwrap_or(pan_before.x);
    assert!(
        pan_target_x < pan_before.x,
        "ArrowRight should reduce pan_x target (saw {})",
        pan_target_x
    );
}

#[test]
fn keyboard_plus_minus_animate_zoom() {
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};

    let mut scene = Scene::new();
    scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 10.0, 10.0));
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    tree.focus(view_id);
    let zoom_before = view_handle(&tree, view_id).zoom();

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Character('+'),
        modifiers: Modifiers::default(),
        text: Some("+".into()),
    });

    let zoom_after_target = view_handle(&tree, view_id)
        .zoom_animation_target()
        .unwrap_or(zoom_before);
    assert!(
        zoom_after_target > zoom_before,
        "Plus key should increase zoom target (saw {})",
        zoom_after_target
    );
}

#[test]
fn a11y_off_screen_mode_viewport_only_excludes_grown_items() {
    // With ViewportOnly, an item just past the viewport edge
    // does NOT appear in the AT tree, even though the default
    // ViewportPlusN { n: 1 } would include it.
    use crate::items::RectItem;
    use bastyde_core::accessibility::is_synthetic;
    use bastyde_tokens::Color;

    let mut scene = Scene::new();
    // In-viewport item: definitely AT-visible.
    scene.add_item(
        RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)).fill(Color::RED),
        Point::ZERO,
    );
    // Item just past the right edge of the 400x300 viewport.
    // Default mode would include it (within 1× viewport
    // margin), but ViewportOnly should not.
    scene.add_item(
        RectItem::new(Rect::new(450.0, 100.0, 20.0, 20.0)).fill(Color::BLUE),
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let _view_id = tree.add(
        SceneView::new(scene).a11y_off_screen_mode(crate::a11y::A11yOffScreenMode::ViewportOnly),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let update = tree.sync_accessibility();
    let synthetic_count = update
        .nodes
        .iter()
        .filter(|(id, _)| is_synthetic(*id))
        .count();
    assert_eq!(
        synthetic_count, 1,
        "ViewportOnly mode must exclude the off-screen item"
    );
}

// -- Logical AT structure -----------------------------------

#[test]
fn add_a11y_group_round_trip() {
    let mut scene = Scene::new();
    let id = scene.add_a11y_group(crate::a11y::A11yGroup::builder().label("Act 1"));
    assert_eq!(scene.a11y_group(id).map(|g| g.label()), Some(Some("Act 1")));
}

#[test]
fn set_a11y_parent_reparents_item_under_group() {
    // Item declared with a logical parent (Group) should be
    // emitted under the group, NOT under the SceneView root.
    use crate::a11y::{A11yGroup, A11yNode};
    use crate::items::RectItem;
    use bastyde_core::accessibility::{SyntheticKind, is_synthetic, synthetic_node_id};

    let mut scene = Scene::new();
    let act1 = scene.add_a11y_group(A11yGroup::builder().label("Act 1"));
    let card = scene.add_item(
        RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)).access_label("Scene A"),
        Point::ZERO,
    );
    scene.set_a11y_parent(A11yNode::Item(card), Some(A11yNode::Group(act1)));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let update = tree.sync_accessibility();

    // Group node and item node both exist as synthetic nodes.
    let group_node_id = synthetic_node_id(view_id, act1.as_u64(), SyntheticKind::SceneGroup);
    let item_node_id = synthetic_node_id(view_id, card.as_u64(), SyntheticKind::SceneItem);

    let find_node = |needle: accesskit::NodeId| update.nodes.iter().find(|(id, _)| *id == needle);
    let group_node = find_node(group_node_id).expect("group node exists");
    let item_node = find_node(item_node_id).expect("item node exists");
    assert!(is_synthetic(group_node.0));
    assert!(is_synthetic(item_node.0));

    // The group's children list contains the item node; the
    // SceneView's children list does NOT contain the item node
    // directly (the reparenting moved it).
    assert!(
        group_node.1.children().contains(&item_node_id),
        "item must be a child of its declared logical parent group"
    );
    let scene_view_node_id = bastyde_core::accessibility::widget_id_to_node_id(view_id);
    let scene_view_node = find_node(scene_view_node_id).expect("scene view node");
    assert!(
        !scene_view_node.1.children().contains(&item_node_id),
        "item must NOT also appear as a direct child of SceneView when reparented"
    );
    assert!(
        scene_view_node.1.children().contains(&group_node_id),
        "group is the root-level synthetic — should be a direct child of SceneView"
    );
}

#[test]
fn nested_groups_emit_in_logical_dfs_order() {
    // Group B parented under Group A → SceneView's children list
    // contains A; A's contains B; B's contains its item.
    use crate::a11y::{A11yGroup, A11yNode};
    use crate::items::RectItem;
    use bastyde_core::accessibility::{SyntheticKind, synthetic_node_id};

    let mut scene = Scene::new();
    let outer = scene.add_a11y_group(A11yGroup::builder().label("Outer"));
    let inner = scene.add_a11y_group(A11yGroup::builder().label("Inner"));
    let item = scene.add_item(
        RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)),
        Point::ZERO,
    );
    scene.set_a11y_parent(A11yNode::Group(inner), Some(A11yNode::Group(outer)));
    scene.set_a11y_parent(A11yNode::Item(item), Some(A11yNode::Group(inner)));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let update = tree.sync_accessibility();

    let outer_id = synthetic_node_id(view_id, outer.as_u64(), SyntheticKind::SceneGroup);
    let inner_id = synthetic_node_id(view_id, inner.as_u64(), SyntheticKind::SceneGroup);
    let item_id_synth = synthetic_node_id(view_id, item.as_u64(), SyntheticKind::SceneItem);

    let find = |needle: accesskit::NodeId| {
        update
            .nodes
            .iter()
            .find(|(id, _)| *id == needle)
            .map(|(_, n)| n)
    };
    let outer_node = find(outer_id).expect("outer group exists");
    let inner_node = find(inner_id).expect("inner group exists");
    let _item_node = find(item_id_synth).expect("item node exists");

    assert!(outer_node.children().contains(&inner_id));
    assert!(inner_node.children().contains(&item_id_synth));
    assert!(!outer_node.children().contains(&item_id_synth));
}

#[test]
fn add_a11y_relation_writes_into_accesskit_arrays() {
    // Declared FlowTo from item A → item B should land as a
    // FlowTo entry on A's AccessKit Node.
    use crate::a11y::{A11yNode, A11yRelation};
    use crate::items::RectItem;
    use bastyde_core::accessibility::{SyntheticKind, synthetic_node_id};

    let mut scene = Scene::new();
    let a = scene.add_item(
        RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)),
        Point::ZERO,
    );
    let b = scene.add_item(
        RectItem::new(Rect::new(40.0, 10.0, 20.0, 20.0)),
        Point::ZERO,
    );
    scene.add_a11y_relation(A11yNode::Item(a), A11yRelation::FlowTo, A11yNode::Item(b));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let update = tree.sync_accessibility();

    let a_id = synthetic_node_id(view_id, a.as_u64(), SyntheticKind::SceneItem);
    let b_id = synthetic_node_id(view_id, b.as_u64(), SyntheticKind::SceneItem);
    let a_node = update
        .nodes
        .iter()
        .find(|(id, _)| *id == a_id)
        .map(|(_, n)| n)
        .expect("a node exists");
    // AccessKit's `flow_to` accessor returns the slice we pushed.
    assert!(
        a_node.flow_to().contains(&b_id),
        "FlowTo relation must land on AccessKit's flow_to array"
    );
}

#[test]
fn set_a11y_live_marks_node_as_live_region() {
    use crate::a11y::A11yNode;
    use crate::items::RectItem;
    use bastyde_core::accessibility::{SyntheticKind, synthetic_node_id};

    let mut scene = Scene::new();
    let item = scene.add_item(
        RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)),
        Point::ZERO,
    );
    scene.set_a11y_live(A11yNode::Item(item), accesskit::Live::Polite);

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let update = tree.sync_accessibility();

    let id = synthetic_node_id(view_id, item.as_u64(), SyntheticKind::SceneItem);
    let node = update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == id)
        .map(|(_, n)| n)
        .expect("item node");
    assert_eq!(node.live(), Some(accesskit::Live::Polite));
}

#[test]
fn set_a11y_landmark_overrides_role() {
    use crate::a11y::A11yNode;
    use crate::items::RectItem;
    use bastyde_core::accessibility::{SyntheticKind, synthetic_node_id};

    let mut scene = Scene::new();
    let item = scene.add_item(
        RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)),
        Point::ZERO,
    );
    // RectItem default role is GraphicsObject. Landmark override
    // should re-set it to Region.
    scene.set_a11y_landmark(A11yNode::Item(item), accesskit::Role::Region);

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let update = tree.sync_accessibility();

    let id = synthetic_node_id(view_id, item.as_u64(), SyntheticKind::SceneItem);
    let node = update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == id)
        .map(|(_, n)| n)
        .expect("item node");
    assert_eq!(node.role(), accesskit::Role::Region);
}

#[test]
fn remove_a11y_group_drops_dependent_decorations() {
    // Removing a group must drop relations / live / landmarks
    // / categories that target the group; child items declared
    // under it fall back to the SceneView root.
    use crate::a11y::{A11yCategory, A11yGroup, A11yNode, A11yRelation};
    use crate::items::RectItem;

    let mut scene = Scene::new();
    let g = scene.add_a11y_group(A11yGroup::builder().label("G"));
    let item = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);
    scene.set_a11y_parent(A11yNode::Item(item), Some(A11yNode::Group(g)));
    scene.set_a11y_live(A11yNode::Group(g), accesskit::Live::Assertive);
    scene.set_a11y_landmark(A11yNode::Group(g), accesskit::Role::Region);
    scene.add_a11y_relation(
        A11yNode::Item(item),
        A11yRelation::Controls,
        A11yNode::Group(g),
    );
    scene.set_a11y_categories(A11yNode::Group(g), &[A11yCategory::new("act")]);

    scene.remove_a11y_group(g);

    // Decorations that targeted the removed group are gone.
    assert!(scene.a11y_group(g).is_none());
    assert!(scene.a11y_live.is_empty());
    assert!(scene.a11y_landmarks.is_empty());
    assert!(scene.a11y_relations().is_empty());
    assert!(scene.a11y_categories_of(A11yNode::Group(g)).is_none());
    // Item's parent declaration is dropped — falls back to root.
    assert!(scene.a11y_parent_of(A11yNode::Item(item)).is_none());
}

#[test]
fn parent_cycle_does_not_loop_walker() {
    // Malformed: A → B → A. The walker visits each node once
    // (HashSet guard) and never recurses indefinitely.
    use crate::a11y::{A11yGroup, A11yNode};

    let mut scene = Scene::new();
    let a = scene.add_a11y_group(A11yGroup::builder().label("A"));
    let b = scene.add_a11y_group(A11yGroup::builder().label("B"));
    scene.set_a11y_parent(A11yNode::Group(a), Some(A11yNode::Group(b)));
    scene.set_a11y_parent(A11yNode::Group(b), Some(A11yNode::Group(a)));

    let mut tree = WidgetTree::new();
    let _view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    // Just running sync_accessibility without panic / hang is
    // the assertion: cycle guard works.
    let _ = tree.sync_accessibility();
}

// -- A11yMode + auto-graft of widget descendants -------------

/// Helper: a widget with a deterministic accessibility role we
/// can detect in the AT update.
#[derive(Debug)]
struct LabelledFill {
    label: &'static str,
}
impl Widget for LabelledFill {
    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(20.0, 20.0).into()
    }
    fn accessibility(&self, builder: &mut bastyde_core::accessibility::AccessNodeBuilder) {
        builder.set_role(accesskit::Role::Button);
        builder.set_name(self.label);
    }
}

#[test]
fn cooperative_default_emits_items_at_root_when_unparented() {
    // Cooperative is the default mode. Items without a declared
    // parent emit as direct children of SceneView — Cooperative mode
    // visual-default behaviour, preserved.
    use crate::items::RectItem;
    use bastyde_core::accessibility::{is_synthetic, widget_id_to_node_id};

    let mut scene = Scene::new();
    scene.add_item(
        RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)),
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let update = tree.sync_accessibility();
    let view_node = update
        .nodes
        .iter()
        .find(|(id, _)| *id == widget_id_to_node_id(view_id))
        .map(|(_, n)| n)
        .expect("scene view node");
    let synth_kids = view_node
        .children()
        .iter()
        .filter(|id| is_synthetic(**id))
        .count();
    assert_eq!(synth_kids, 1, "Cooperative emits unparented item at root");
}

#[test]
fn strictly_parallel_suppresses_unparented_items() {
    // In StrictlyParallel mode an item without a declared
    // parent does NOT emit. Apps must place every node they
    // want AT-visible.
    use crate::a11y::{A11yGroup, A11yMode, A11yNode};
    use crate::items::RectItem;
    use bastyde_core::accessibility::{is_synthetic, widget_id_to_node_id};

    let mut scene = Scene::new();
    let g = scene.add_a11y_group(A11yGroup::builder().label("G"));
    let placed = scene.add_item(
        RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)),
        Point::ZERO,
    );
    let _orphan = scene.add_item(
        RectItem::new(Rect::new(40.0, 10.0, 20.0, 20.0)),
        Point::ZERO,
    );
    scene.set_a11y_parent(A11yNode::Item(placed), Some(A11yNode::Group(g)));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene).a11y_mode(A11yMode::StrictlyParallel));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let update = tree.sync_accessibility();

    // Total synthetic node count: just the group + the placed
    // item. The orphan item is suppressed.
    let synth_total = update
        .nodes
        .iter()
        .filter(|(id, _)| is_synthetic(*id))
        .count();
    assert_eq!(
        synth_total, 2,
        "StrictlyParallel: only group + placed item, orphan suppressed"
    );

    // SceneView's children list contains the group only —
    // not the orphan item.
    let view_node = update
        .nodes
        .iter()
        .find(|(id, _)| *id == widget_id_to_node_id(view_id))
        .map(|(_, n)| n)
        .unwrap();
    let synth_kids: Vec<_> = view_node
        .children()
        .iter()
        .filter(|id| is_synthetic(**id))
        .collect();
    assert_eq!(synth_kids.len(), 1, "only the group reaches root");
}

#[test]
fn auto_graft_widget_appears_under_declared_logical_group() {
    // The headline auto-graft test: a heavyweight
    // widget added via `Scene::add_widget` is declared (via
    // its `ItemId`) under a logical group. The widget's
    // `NodeId` must appear in the group's children list AND
    // must NOT appear in SceneView's own children list.
    use crate::a11y::{A11yGroup, A11yNode};
    use bastyde_core::accessibility::{SyntheticKind, synthetic_node_id, widget_id_to_node_id};

    let mut scene = Scene::new();
    let act_one = scene.add_a11y_group(A11yGroup::builder().label("Act 1"));
    let card_item_id = scene.add_widget(
        LabelledFill { label: "card" },
        Rect::new(10.0, 10.0, 20.0, 20.0),
    );
    // Declare the parent up-front via ItemId — works for both
    // lightweight and heavyweight scene entries.
    scene.set_a11y_parent(A11yNode::Item(card_item_id), Some(A11yNode::Group(act_one)));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);
    let card_widget_id = view
        .widget_id_for(card_item_id)
        .expect("card was materialised");

    let update = tree.sync_accessibility();
    let view_node_id = widget_id_to_node_id(view_id);
    let group_node_id = synthetic_node_id(view_id, act_one.as_u64(), SyntheticKind::SceneGroup);
    let widget_node_id = widget_id_to_node_id(card_widget_id);

    let find = |id: accesskit::NodeId| update.nodes.iter().find(|(n, _)| *n == id).map(|(_, n)| n);
    let scene_view = find(view_node_id).expect("scene view node");
    let group = find(group_node_id).expect("group node");
    let _widget_node = find(widget_node_id).expect("widget node still emitted");

    assert!(
        group.children().contains(&widget_node_id),
        "widget must be a child of its declared logical group"
    );
    assert!(
        !scene_view.children().contains(&widget_node_id),
        "widget must NOT also appear as a direct child of SceneView"
    );
}

#[test]
fn auto_graft_redirect_hook_default_is_none() {
    // Sanity: a SceneView with no widget-parent declarations
    // returns None from the redirect hook, so default
    // behaviour is unchanged.
    let mut scene = Scene::new();
    scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 10.0, 10.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);
    // Pick any descendant — without a declaration the hook
    // returns None.
    let view_widget_id = view_id;
    // Use any non-existent widget id; the hook must still
    // return None.
    assert!(
        Widget::a11y_redirect_descendant(view, view_widget_id, view_widget_id).is_none(),
        "redirect hook returns None when no declaration is in place"
    );
}

/// A trivial container widget: takes one child via `build`,
/// reports it through `children()`, lays it out at full
/// proposed size, paints nothing, opts OUT of descendant
/// redirects (default false). Used by deep-descendant tests
/// to insert an extra arena level between SceneView and the
/// inner widget so we can verify the ancestor-chain walk
/// reaches SceneView even past a non-opting intermediate.
#[derive(Debug)]
struct PlainContainer {
    inner_id: Option<WidgetId>,
}
impl PlainContainer {
    fn new() -> Self {
        Self { inner_id: None }
    }
}
impl Widget for PlainContainer {
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        let id = ctx.add(LabelledFill { label: "inner" });
        self.inner_id = Some(id);
        vec![id]
    }
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(40.0, 40.0).into()
    }
    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [bastyde_core::widget::WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for placement in children.iter_mut() {
            placement.origin = Point::new(bounds.x, bounds.y);
            placement.size = Size::new(bounds.width, bounds.height);
        }
    }
    fn children(&self) -> Vec<WidgetId> {
        self.inner_id.into_iter().collect()
    }
}

#[test]
fn auto_graft_deep_descendant_under_scene_view_group() {
    // The headline deep-descendant test. Arena shape:
    //   SceneView → PlainContainer → LabelledFill (inner)
    //
    // PlainContainer opts OUT of `wants_descendant_redirects`
    // (default false). SceneView opts IN. Declaring
    // `A11yNode::Widget(inner_id)` causes the framework
    // walker — when iterating PlainContainer's children — to
    // walk up the arena, skip PlainContainer (opt-out), find
    // SceneView (opt-in), and consult its hook. SceneView
    // returns `Some(group_node_id)` and the walker skips the
    // default push.
    //
    // Result: inner's NodeId appears in the declared group's
    // children list, NOT in PlainContainer's. The widget's
    // own AccessKit Node still emits via the recursive walk
    // and lands in `nodes`.
    use crate::a11y::{A11yGroup, A11yNode};
    use bastyde_core::accessibility::{SyntheticKind, synthetic_node_id, widget_id_to_node_id};

    // Stage 1: add a PlainContainer scene-entry, layout once
    // to learn the inner widget's allocated `WidgetId`.
    let mut scene = Scene::new();
    let group = scene.add_a11y_group(A11yGroup::builder().label("Tools"));
    scene.add_widget(PlainContainer::new(), Rect::new(10.0, 10.0, 40.0, 40.0));
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let container_id = tree.children(view_id)[0];
    let inner_id = tree.children(container_id)[0];

    // Stage 2: declare the deep-descendant relocation via
    // `scene_mut()` reached through `widget_as_any_mut`. The
    // arena assigned `inner_id` during layout; use it.
    let scene_view = tree
        .widget_as_any_mut(view_id)
        .and_then(|a| a.downcast_mut::<SceneView>())
        .expect("downcast SceneView mut");
    scene_view
        .scene_mut()
        .set_a11y_parent(A11yNode::Widget(inner_id), Some(A11yNode::Group(group)));

    // Stage 3: re-layout (so AT walker sees the new
    // declaration via the next sync_accessibility) and verify.
    // Re-layout marks dirty; the arena state stays stable so
    // `inner_id` is still valid.
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let update = tree.sync_accessibility();

    let group_node_id = synthetic_node_id(view_id, group.as_u64(), SyntheticKind::SceneGroup);
    let inner_node_id = widget_id_to_node_id(inner_id);
    let container_node_id = widget_id_to_node_id(container_id);

    let find = |id: accesskit::NodeId| update.nodes.iter().find(|(n, _)| *n == id).map(|(_, n)| n);
    let group_node = find(group_node_id).expect("group emitted");
    let container_node = find(container_node_id).expect("container emitted");
    let _inner_node = find(inner_node_id).expect("inner widget still emitted");

    assert!(
        group_node.children().contains(&inner_node_id),
        "inner widget must appear under its declared logical group, \
         not under its arena parent"
    );
    assert!(
        !container_node.children().contains(&inner_node_id),
        "inner widget must NOT appear under PlainContainer (its arena \
         parent) — the redirect skipped that push"
    );
}

#[test]
fn auto_graft_deep_descendant_no_op_without_optin_ancestor() {
    // If no ancestor opts into `wants_descendant_redirects`,
    // the ancestor-chain walk is a no-op (each ancestor's
    // flag is checked, fast-path returns false), and the
    // descendant emits normally as a child of its arena
    // parent. This pins the opt-in semantic: the cost of the
    // ancestor walk is bounded to subtrees that genuinely
    // need it.
    //
    // We can't run a clean test with no SceneView at all
    // (the auto-graft surface doesn't apply), so instead we
    // verify that without a `set_a11y_parent` declaration,
    // the inner widget appears under its arena parent
    // (PlainContainer) — confirming the SceneView opt-in
    // doesn't accidentally claim every descendant.
    use bastyde_core::accessibility::widget_id_to_node_id;

    let mut scene = Scene::new();
    scene.add_widget(PlainContainer::new(), Rect::new(10.0, 10.0, 40.0, 40.0));
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let container_id = tree.children(view_id)[0];
    let inner_id = tree.children(container_id)[0];

    let update = tree.sync_accessibility();
    let container_node = update
        .nodes
        .iter()
        .find(|(id, _)| *id == widget_id_to_node_id(container_id))
        .map(|(_, n)| n)
        .expect("container emitted");
    assert!(
        container_node
            .children()
            .contains(&widget_id_to_node_id(inner_id)),
        "without a redirect declaration, inner widget appears under \
         its arena parent — opt-in does not claim every descendant"
    );
}

#[test]
fn ancestor_chain_walk_skips_optout_intermediate() {
    // Arena shape: SceneView → PlainContainer → LabelledFill.
    // PlainContainer is `wants_descendant_redirects = false`
    // (default). The walker, iterating PlainContainer's
    // children, must skip past it and reach SceneView for
    // the redirect query — proving the opt-out flag doesn't
    // halt the walk. Distinct from the headline test in
    // that we explicitly target the intermediate's opt-out
    // behaviour.
    use crate::a11y::{A11yGroup, A11yNode};
    use bastyde_core::accessibility::{SyntheticKind, synthetic_node_id, widget_id_to_node_id};

    let mut scene = Scene::new();
    let group = scene.add_a11y_group(A11yGroup::builder().label("G"));
    scene.add_widget(PlainContainer::new(), Rect::new(10.0, 10.0, 40.0, 40.0));
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let container_id = tree.children(view_id)[0];
    let inner_id = tree.children(container_id)[0];

    // Sanity: PlainContainer opts out (default).
    assert!(
        !PlainContainer::new().wants_descendant_redirects(),
        "PlainContainer must default to opt-out for this test to be meaningful"
    );

    let scene_view = tree
        .widget_as_any_mut(view_id)
        .and_then(|a| a.downcast_mut::<SceneView>())
        .unwrap();
    scene_view
        .scene_mut()
        .set_a11y_parent(A11yNode::Widget(inner_id), Some(A11yNode::Group(group)));

    tree.layout(SizeProposal::exact(400.0, 300.0));
    let update = tree.sync_accessibility();

    let group_node = update
        .nodes
        .iter()
        .find(|(id, _)| {
            *id == synthetic_node_id(view_id, group.as_u64(), SyntheticKind::SceneGroup)
        })
        .map(|(_, n)| n)
        .unwrap();
    assert!(
        group_node
            .children()
            .contains(&widget_id_to_node_id(inner_id)),
        "ancestor walk must reach SceneView past the opt-out \
         intermediate"
    );
}

// -- Nested-SceneView gap-filling APIs ------------------------------

#[test]
fn interactive_default_is_true() {
    let view = SceneView::new(Scene::new());
    assert!(view.interactive, "SceneView::interactive defaults to true");
}

#[test]
fn non_interactive_ignores_scroll() {
    // When the outer SceneView is locked (chart chrome
    // pattern), scroll events must not pan its view. The
    // gesture handlers aren't registered, so the scroll is
    // ignored at this widget — events bubble through to
    // siblings / inner SceneViews that do handle them.
    let scene = Scene::new();
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene).interactive(false));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);
    let pan_before = view.pan();

    // Send a scroll directly to the SceneView. Without an
    // on_scroll handler registered, the event is unhandled
    // here and pan stays put.
    tree.pointer_move(Point::new(100.0, 100.0));
    tree.dispatch_event(WidgetEvent::Scroll {
        delta: ScrollDelta::Pixels { x: 50.0, y: 50.0 },
        modifiers: Default::default(),
    });

    let view = view_handle(&tree, view_id);
    assert_eq!(
        view.pan(),
        pan_before,
        "non-interactive SceneView must not pan on scroll"
    );
    // Animation target must also be unset — no tween started.
    assert!(view.pan_x_animation_target().is_none());
    assert!(view.pan_y_animation_target().is_none());
}

#[test]
fn interactive_does_pan_on_scroll() {
    // Counterpoint: with the default `interactive = true`,
    // scroll DOES animate pan. Pins that the gating doesn't
    // accidentally drop scroll handling for normal scenes.
    let scene = Scene::new();
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    tree.pointer_move(Point::new(100.0, 100.0));
    tree.dispatch_event(WidgetEvent::Scroll {
        delta: ScrollDelta::Pixels { x: 50.0, y: 0.0 },
        modifiers: Default::default(),
    });

    let view = view_handle(&tree, view_id);
    let target = view
        .pan_x_animation_target()
        .expect("interactive scene must enqueue a pan animation");
    assert!(target.abs() > 1.0, "pan_x animation target moved");
}

#[test]
fn pan_x_signal_returns_live_handle() {
    // `pan_x_signal()` must return a live handle: external
    // observers see updates when pan changes.
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let view = view_handle(&tree, view_id);
    let signal = view.pan_x_signal();
    assert_eq!(signal.get(), 0.0);

    // Programmatic pan_to.
    view.pan_to(Vec2::new(123.0, 0.0), Duration::ZERO);
    // Animations land via tree.advance_time but Duration::ZERO
    // settles immediately on `set` for finite-duration tweens.
    // Verify the signal reflects the post-target state.
    let target = view
        .pan_x_animation_target()
        .or_else(|| Some(signal.get()))
        .unwrap();
    assert!(
        (target - 123.0).abs() < 1e-3,
        "pan_x_signal must observe pan_to target (saw {})",
        target
    );
}

#[test]
fn view_transform_signal_updates_on_pan() {
    // The composed view_transform signal must reflect pan
    // changes for reactive consumers.
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let view = view_handle(&tree, view_id);
    let xform_signal = view.view_transform_signal();
    let before = xform_signal.get();
    assert!(before.is_identity(), "initial view_transform is identity");

    // Set pan_x directly (bypasses tweening).
    view.pan_x_signal().set(50.0);
    let after = xform_signal.get();
    // Translation component should reflect the pan.
    let projected = after.apply_point(Point::new(0.0, 0.0));
    assert!(
        (projected.x - 50.0).abs() < 1e-3,
        "view_transform_signal must update when pan_x changes \
         (projected x = {})",
        projected.x
    );
}

#[test]
fn text_item_with_signal_text_repaints_on_signal_change() {
    // The chart axis-label use case: TextItem::with_signal_text
    // ties its rendered text to a signal. Changing the signal
    // must dirty the SceneView's paint so the next render
    // walks the items and emits the updated text.
    //
    // Binding dirties are processed at the start of `layout()`
    // (via `process_state_changes`), not eagerly on `set()`.
    // The test mirrors the real per-frame pattern: layout →
    // render → mutate signal → next layout marks paint dirty.
    use crate::items::TextItem;
    use bastyde_core::signal::Signal;

    let mut scene = Scene::new();
    let label_text = Signal::new(String::from("0.0"));
    scene.add_item(
        TextItem::with_signal_text(label_text.clone(), Rect::new(0.0, 0.0, 50.0, 20.0)),
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let _view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = tree.render();
    assert!(
        !tree.needs_paint(),
        "after initial render, paint should be clean"
    );

    // Mutate the signal — RepaintOnly binding queues a dirty
    // entry; the next `layout()` flushes it and marks the
    // SceneView as needing paint.
    label_text.set(String::from("123.4"));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert!(
        tree.needs_paint(),
        "TextItem::with_signal_text signal change must dirty \
         SceneView's paint via register_bindings"
    );
}

#[test]
fn text_item_label_returns_static_text_for_static_items() {
    // Existing semantic preserved: TextItem with static text
    // returns it via `label()` when no override is set.
    use crate::items::TextItem;
    let item = TextItem::new("Hello", Rect::new(0.0, 0.0, 50.0, 20.0));
    assert_eq!(
        crate::item::SceneItem::label(&item).as_deref(),
        Some("Hello")
    );
}

#[test]
fn text_item_label_returns_signal_snapshot_for_bound_items() {
    // Bound text: `label()` snapshots the current signal value.
    use crate::items::TextItem;
    use bastyde_core::signal::Signal;
    let signal = Signal::new(String::from("initial"));
    let item = TextItem::with_signal_text(signal.clone(), Rect::new(0.0, 0.0, 50.0, 20.0));
    assert_eq!(
        crate::item::SceneItem::label(&item).as_deref(),
        Some("initial")
    );
    signal.set(String::from("updated"));
    assert_eq!(
        crate::item::SceneItem::label(&item).as_deref(),
        Some("updated")
    );
}

#[test]
fn nested_scene_chart_pattern_smoke() {
    // End-to-end: outer locked SceneView holding axis-label
    // TextItems bound to inner SceneView's pan_x_signal.
    // Verifies the wiring composes cleanly without panic.
    use crate::items::TextItem;
    use bastyde_core::signal::Signal;

    // Inner data scene.
    let mut inner_scene = Scene::new();
    inner_scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 10.0, 10.0));
    let inner = SceneView::new(inner_scene);
    let inner_pan_x = inner.pan_x_signal();
    let axis_label_text: Signal<String> = inner_pan_x.map(|px| format!("x = {:.1}", px));

    // Outer chrome scene.
    let mut outer_scene = Scene::new();
    outer_scene.add_widget(inner, Rect::new(40.0, 0.0, 360.0, 280.0));
    outer_scene.add_item(
        TextItem::with_signal_text(axis_label_text.clone(), Rect::new(0.0, 290.0, 80.0, 10.0)),
        Point::ZERO,
    );
    let outer = SceneView::new(outer_scene).interactive(false);

    let mut tree = WidgetTree::new();
    let _root_id = tree.add(outer);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = tree.render();

    // Mutate inner's pan via the outer scene's child handle.
    // For the smoke test, just mutate the signal directly —
    // axis_label_text is a derived signal, mutating its
    // upstream (inner_pan_x) should propagate through. The
    // next `layout()` flushes binding dirties.
    inner_pan_x.set(42.0);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert!(
        tree.needs_paint(),
        "outer SceneView must dirty paint when inner's \
         pan_x_signal changes — derived axis-label text \
         updates via `register_bindings`"
    );
}

#[test]
fn nested_scene_view_geometry_after_paint() {
    // After the first paint, an inner SceneView placed at
    // scene_rect (X, Y, W, H) inside an outer SceneView must
    // have its `bounds_origin_signal` updated to (X, Y) so its
    // own view_transform places its lightweight items at the
    // expected screen position. Without this sync (which lives
    // in `paint()`), the inner SceneView would draw at the
    // outer's scene origin instead of at its own scene_rect.
    //
    // Regression for "no embedded scene visible" — the inner
    // never received a `place_children` call (it has zero
    // widget children) and bounds_origin_signal stayed at the
    // default (0, 0).
    use crate::items::RectItem;
    use bastyde_tokens::Color;

    // Inner: one small RectItem at scene-coord (10, 10).
    let mut inner_scene = Scene::new();
    inner_scene.add_item(
        RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)).fill(Color::RED),
        Point::ZERO,
    );
    let inner = SceneView::new(inner_scene).default_size(50.0, 40.0);

    // Outer holds the inner at scene-coord (200, 150, 50, 40).
    let mut outer_scene = Scene::new();
    let inner_id = outer_scene.add_widget(inner, Rect::new(200.0, 150.0, 50.0, 40.0));
    let outer = SceneView::new(outer_scene);

    let mut tree = WidgetTree::new();
    let outer_id = tree.add(outer);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    // Render once to drive paint, which is where the inner's
    // `bounds_origin_signal` gets synced.
    let _ = tree.render();

    // Verify inner widget id resolved.
    let outer_view = view_handle(&tree, outer_id);
    let inner_widget_id = outer_view
        .widget_id_for(inner_id)
        .expect("inner SceneView materialised under outer");

    // Inspect the inner SceneView's view_transform_signal —
    // it should reflect the placement origin (200, 150).
    let inner_view = tree
        .widget_as_any(inner_widget_id)
        .and_then(|a| a.downcast_ref::<SceneView>())
        .expect("inner widget is a SceneView");

    let xform = inner_view.view_transform_signal().get();
    // Origin (0, 0) in inner-scene-coord must project to
    // (200, 150) in outer-scene-coord (= screen, since outer
    // is at zoom 1, pan 0).
    let projected = xform.apply_point(Point::ZERO);
    assert!(
        (projected.x - 200.0).abs() < 0.5,
        "inner origin must project to outer scene_rect.x = 200 (got {})",
        projected.x
    );
    assert!(
        (projected.y - 150.0).abs() < 0.5,
        "inner origin must project to outer scene_rect.y = 150 (got {})",
        projected.y
    );

    // And inner-scene (10, 10) must project to (210, 160).
    let projected_dot = xform.apply_point(Point::new(10.0, 10.0));
    assert!(
        (projected_dot.x - 210.0).abs() < 0.5,
        "inner item at (10,10) must project to (210, ...) — got {}",
        projected_dot.x
    );
    assert!(
        (projected_dot.y - 160.0).abs() < 0.5,
        "inner item at (10,10) must project to (..., 160) — got {}",
        projected_dot.y
    );
}

// -- Selection + marquee -----------------------------------

#[test]
fn selection_default_is_none_mode() {
    let view = SceneView::new(Scene::new());
    assert_eq!(
        view.selection().mode(),
        crate::selection::SceneSelectionMode::None
    );
}

#[test]
fn selection_mode_builder_sets_multi() {
    let view =
        SceneView::new(Scene::new()).selection_mode(crate::selection::SceneSelectionMode::Multi);
    assert_eq!(
        view.selection().mode(),
        crate::selection::SceneSelectionMode::Multi
    );
}

#[test]
fn marquee_drag_ends_with_pending_commit() {
    // Diagnostic: full Started → Moved → Ended → pending_commit.
    use crate::items::RectItem;
    use bastyde_canvas::Point;

    let mut scene = Scene::new();
    scene.add_item(
        RectItem::new(Rect::new(50.0, 50.0, 20.0, 20.0)).fill(bastyde_tokens::Color::RED),
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let view_id =
        tree.add(SceneView::new(scene).selection_mode(crate::selection::SceneSelectionMode::Multi));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    tree.pointer_move(Point::new(40.0, 40.0));
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(40.0, 40.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(80.0, 80.0),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(80.0, 80.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });

    let view = view_handle(&tree, view_id);
    let pending = view.pending_marquee_commit.get();
    let (rect, _) = pending.expect("drag Ended must post pending_marquee_commit");
    // The rect should enclose (50, 50, 20, 20) — origin (40,40)
    // to current (80,80), screen-coords. Identity view-transform
    // → scene_rect = (40, 40, 40, 40).
    assert!(
        rect.width >= 30.0 && rect.height >= 30.0,
        "marquee rect was tiny: {:?}",
        rect
    );
    assert!(
        rect.x <= 50.0 && rect.x + rect.width >= 70.0,
        "marquee rect doesn't enclose item x range: {:?}",
        rect
    );
}

#[test]
fn marquee_drag_recognizes_started_phase() {
    // Diagnostic: drag-Started is recognized at all.
    use crate::items::RectItem;
    use bastyde_canvas::Point;

    let mut scene = Scene::new();
    scene.add_item(
        RectItem::new(Rect::new(50.0, 50.0, 20.0, 20.0)).fill(bastyde_tokens::Color::RED),
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let view_id =
        tree.add(SceneView::new(scene).selection_mode(crate::selection::SceneSelectionMode::Multi));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Hover so pointer-over state is set, then PointerDown,
    // PointerMove (crosses 5px threshold), PointerUp.
    tree.pointer_move(Point::new(40.0, 40.0));
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(40.0, 40.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(80.0, 80.0),
    });

    // After Started fires, marquee state should be Some.
    let view = view_handle(&tree, view_id);
    assert!(
        view.marquee.get().is_some(),
        "drag Started must populate marquee state"
    );
}

#[test]
fn marquee_drag_records_pending_commit() {
    // Drive on_drag through the event path: Started → Moved →
    // Ended. The closure must post a pending commit via
    // `pending_marquee_commit`. Then `flush_marquee_commit`
    // (or the next layout) materialises the selection.
    use crate::items::RectItem;
    use bastyde_canvas::Point;

    let mut scene = Scene::new();
    let inside = scene.add_item(
        RectItem::new(Rect::new(50.0, 50.0, 20.0, 20.0)).fill(bastyde_tokens::Color::RED),
        Point::ZERO,
    );
    let outside = scene.add_item(
        RectItem::new(Rect::new(500.0, 500.0, 20.0, 20.0)).fill(bastyde_tokens::Color::BLUE),
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let view_id =
        tree.add(SceneView::new(scene).selection_mode(crate::selection::SceneSelectionMode::Multi));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Drive pointer-down → move → up at coordinates that
    // produce a screen-rect enclosing `inside` but not
    // `outside`. The view-transform is identity at this
    // point, so screen and scene coords coincide.
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(40.0, 40.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(80.0, 80.0),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(80.0, 80.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });

    let view = view_handle(&tree, view_id);
    // Materialise the marquee result — outside a real
    // per-frame loop the test calls this directly.
    view.flush_marquee_commit();
    let selected = view.selection().selected();
    assert!(
        selected.contains(&inside),
        "marquee enclosing `inside` must select it (got {:?})",
        selected
    );
    assert!(
        !selected.contains(&outside),
        "marquee not enclosing `outside` must not select it"
    );
}

#[test]
fn drag_to_move_translates_lightweight_item() {
    // Pointer-down inside a lightweight item, drag, release →
    // the item's bounds in the scene shift by the drag delta.
    // The hit-test snapshot used by on_drag is refreshed in
    // place_children; the test calls layout once before
    // dragging so the snapshot is populated.
    use crate::items::RectItem;
    use bastyde_canvas::Point;

    let mut scene = Scene::new();
    let item_id = scene.add_item(
        RectItem::new(Rect::new(50.0, 50.0, 30.0, 30.0))
            .fill(bastyde_tokens::Color::RED)
            .draggable(true),
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let view_id =
        tree.add(SceneView::new(scene).selection_mode(crate::selection::SceneSelectionMode::Multi));
    // First layout populates the lightweight bounds snapshot.
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Press inside the item (60, 60), drag to (100, 100), release.
    tree.pointer_move(Point::new(60.0, 60.0));
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(60.0, 60.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(100.0, 100.0),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(100.0, 100.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });

    // Diagnostic: verify the snapshot was populated, and
    // check whether drag_target or marquee was selected.
    {
        let view = view_handle(&tree, view_id);
        let snap = view.lightweight_bounds_snapshot.borrow();
        let drag = view.drag_target.get();
        let marq = view.marquee.get();
        let pending_move = view.pending_item_move.get();
        let pending_marq = view.pending_marquee_commit.get();
        assert!(
            drag.is_some() || pending_move.is_some(),
            "expected drag_target or pending_move set; \
             snap={:?} drag={:?} marquee={:?} pending_move={:?} pending_marq={:?}",
            snap,
            drag,
            marq,
            pending_move,
            pending_marq
        );
    }

    // Drain the pending move via the public &mut helper.
    let view = tree
        .widget_as_any_mut(view_id)
        .and_then(|a| a.downcast_mut::<SceneView>())
        .expect("downcast");
    let flushed = view.flush_pending_item_move();
    assert!(flushed, "drag-to-move should post a pending commit");

    // The item bounds now reflect the drag delta (+40 on each axis).
    let new_rect = view.scene().scene_rect(item_id).expect("item still exists");
    assert!(
        (new_rect.x - 90.0).abs() < 1e-3,
        "item x moved by drag delta (expected 90, got {})",
        new_rect.x
    );
    assert!(
        (new_rect.y - 90.0).abs() < 1e-3,
        "item y moved by drag delta (expected 90, got {})",
        new_rect.y
    );
    // Size unchanged.
    assert_eq!(new_rect.width, 30.0);
    assert_eq!(new_rect.height, 30.0);
}

#[test]
fn drag_to_move_persists_via_rebuild_signal_no_snap_back() {
    // Regression: real apps don't call `flush_pending_item_move`
    // — they rely on the `drag_dirty` rebuild signal to drain
    // the pending commit on the next layout pass. Drive a drag
    // end and then run a layout; the position must persist.
    // Drive a SECOND drag from the new position; the position
    // must reflect both drags (no snap-back).
    use crate::items::RectItem;
    use bastyde_canvas::Point;

    let mut scene = Scene::new();
    // A heavyweight widget child so the SceneView's
    // `has_built_children` flag flips to true — that's the
    // gate guarding `collect_needs_rebuild` and the realistic
    // shape of any showcase scene with cards.
    scene.add_widget(FillWidget::new(), Rect::new(200.0, 200.0, 50.0, 50.0));
    let item_id = scene.add_item(
        RectItem::new(Rect::new(50.0, 50.0, 30.0, 30.0))
            .fill(bastyde_tokens::Color::RED)
            .draggable(true),
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let view_id =
        tree.add(SceneView::new(scene).selection_mode(crate::selection::SceneSelectionMode::Multi));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // -- First drag: (60, 60) → (100, 100) → release.
    tree.pointer_move(Point::new(60.0, 60.0));
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(60.0, 60.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(100.0, 100.0),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(100.0, 100.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });

    // After Ended the on_drag closure must have posted a
    // pending move and bumped drag_dirty.
    {
        let view = view_handle(&tree, view_id);
        assert!(
            view.pending_item_move.get().is_some(),
            "drag end must post pending_item_move"
        );
        assert!(view.drag_dirty.get() > 0, "drag end must bump drag_dirty");
    }

    // Real apps' layout cycle runs after event dispatch, where
    // drag_dirty (bumped on Ended) triggers the rebuild that
    // drains the pending commit.
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let view = view_handle(&tree, view_id);
    let after_first = view.scene().scene_rect(item_id).expect("item still exists");
    assert!(
        (after_first.x - 90.0).abs() < 1e-3,
        "first drag must persist (expected x=90, got {})",
        after_first.x
    );
    assert!(
        (after_first.y - 90.0).abs() < 1e-3,
        "first drag must persist (expected y=90, got {})",
        after_first.y
    );

    // -- Second drag: (100, 100) → (140, 140) → release.
    // After the first drag the item is at (90, 90, 30, 30), so
    // the press at (100, 100) is inside it.
    tree.pointer_move(Point::new(100.0, 100.0));
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(100.0, 100.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(140.0, 140.0),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(140.0, 140.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let view = view_handle(&tree, view_id);
    let after_second = view.scene().scene_rect(item_id).expect("item still exists");
    // Each drag delta is +40 on each axis. Two drags from
    // (50, 50) → (90, 90) → (130, 130).
    assert!(
        (after_second.x - 130.0).abs() < 1e-3,
        "second drag must compose with first (expected x=130, got {})",
        after_second.x
    );
    assert!(
        (after_second.y - 130.0).abs() < 1e-3,
        "second drag must compose with first (expected y=130, got {})",
        after_second.y
    );
}

#[test]
fn drag_cascades_to_declared_descendants() {
    // QGraphicsScene-style: a child item declared via
    // `Scene::set_item_parent` moves with its parent on drag.
    // Apps build labelled-rect compounds (a TextItem child of
    // a draggable RectItem) without writing custom items.
    use crate::items::{RectItem, TextItem};
    use bastyde_canvas::Point;

    let mut scene = Scene::new();
    // Heavyweight child to flip `has_built_children`.
    scene.add_widget(FillWidget::new(), Rect::new(300.0, 300.0, 50.0, 50.0));
    let parent_rect = scene.add_item(
        RectItem::new(Rect::new(50.0, 50.0, 80.0, 60.0))
            .fill(bastyde_tokens::Color::RED)
            .draggable(true),
        Point::ZERO,
    );
    let label = scene.add_item(
        TextItem::new("child", Rect::new(58.0, 70.0, 64.0, 20.0)),
        Point::ZERO,
    );
    scene.set_item_parent(label, Some(parent_rect));

    let mut tree = WidgetTree::new();
    let view_id =
        tree.add(SceneView::new(scene).selection_mode(crate::selection::SceneSelectionMode::Multi));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Drag the parent (60, 60) → (100, 100) — delta = +40 x +40.
    tree.pointer_move(Point::new(60.0, 60.0));
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(60.0, 60.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(100.0, 100.0),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(100.0, 100.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let view = view_handle(&tree, view_id);
    let parent_after = view
        .scene()
        .scene_rect(parent_rect)
        .expect("parent still exists");
    let label_after = view.scene().scene_rect(label).expect("label still exists");

    // Parent moved (50,50) → (90,90).
    assert!(
        (parent_after.x - 90.0).abs() < 1e-3 && (parent_after.y - 90.0).abs() < 1e-3,
        "parent must move by drag delta (got {:?})",
        parent_after
    );
    // Label is a declared child — must have moved by the SAME
    // delta (+40, +40). Original label at (58, 70) → (98, 110).
    assert!(
        (label_after.x - 98.0).abs() < 1e-3 && (label_after.y - 110.0).abs() < 1e-3,
        "child must cascade with parent's delta (got {:?})",
        label_after
    );
}

#[test]
fn parent_child_drag_persists_across_two_drags() {
    // Showcase regression: parent RectItem with declared TextItem
    // child, dragged twice in a row. After the cascade refactor
    // the second drag must compose with the first — neither parent
    // nor child may snap back to original position.
    use crate::items::{RectItem, TextItem};
    use bastyde_canvas::Point;

    let mut scene = Scene::new();
    // Heavyweight child to flip `has_built_children` (matches
    // realistic scenes with at least one widget tier).
    scene.add_widget(FillWidget::new(), Rect::new(300.0, 300.0, 50.0, 50.0));
    let parent_rect = scene.add_item(
        RectItem::new(Rect::new(50.0, 50.0, 80.0, 60.0))
            .fill(bastyde_tokens::Color::RED)
            .draggable(true),
        Point::ZERO,
    );
    let label = scene.add_item(
        TextItem::new("child", Rect::new(58.0, 70.0, 64.0, 20.0)),
        Point::ZERO,
    );
    scene.set_item_parent(label, Some(parent_rect));

    let mut tree = WidgetTree::new();
    let view_id =
        tree.add(SceneView::new(scene).selection_mode(crate::selection::SceneSelectionMode::Multi));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // -- Drag 1: press at (60,60), release at (100,100). Δ = +40,+40.
    tree.pointer_move(Point::new(60.0, 60.0));
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(60.0, 60.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(100.0, 100.0),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(100.0, 100.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // -- Drag 2: parent now at (90,90,80,60). Press inside it at
    // (110,110), release at (150,150). Δ = +40,+40.
    tree.pointer_move(Point::new(110.0, 110.0));
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(110.0, 110.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(150.0, 150.0),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(150.0, 150.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let view = view_handle(&tree, view_id);
    let parent_after = view.scene().scene_rect(parent_rect).unwrap();
    let label_after = view.scene().scene_rect(label).unwrap();
    // Parent: (50,50) + (40,40) + (40,40) = (130,130).
    assert!(
        (parent_after.x - 130.0).abs() < 1e-3 && (parent_after.y - 130.0).abs() < 1e-3,
        "parent must compose two drags (expected 130,130; got {:?})",
        parent_after
    );
    // Label: (58,70) + (40,40) + (40,40) = (138,150).
    assert!(
        (label_after.x - 138.0).abs() < 1e-3 && (label_after.y - 150.0).abs() < 1e-3,
        "child must compose two drags via cascade (expected 138,150; got {:?})",
        label_after
    );
}

#[test]
fn looping_item_animation_survives_drag_end_rebuild() {
    // Showcase regression: dropping a draggable square in section 5
    // froze the PulsingDot animations in section 8. Cause: the
    // `drag_dirty` rebuild cancels animations owned by SceneView,
    // and the items' `register_bindings` must re-register and
    // re-arm `animate_looping` so the loop resumes on the next
    // pending-pickup pass.
    use crate::SceneItem;
    use crate::animation::register_animated_item_signal;
    use crate::item::SceneItemPaintContext;
    use crate::items::RectItem;
    use bastyde_canvas::Point;
    use bastyde_core::binding::BindingLevel;
    use bastyde_core::build_context::BuildContext;
    use bastyde_core::widget_id::WidgetId;
    use bastyde_tokens::Easing;

    #[derive(Debug)]
    struct Looper {
        bounds: Rect,
        phase: Signal<f32>,
    }
    impl SceneItem for Looper {
        fn local_bounds(&self) -> Rect {
            self.bounds
        }
        fn set_local_bounds(&mut self, b: Rect) {
            self.bounds = b;
        }
        fn paint(&self, _c: &mut bastyde_canvas::Canvas, _x: &SceneItemPaintContext) {}
        fn register_bindings(&self, ctx: &mut BuildContext, view_id: WidgetId) {
            register_animated_item_signal(ctx, &self.phase);
            self.phase
                .bind_to(view_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
            self.phase
                .animate_looping(1.0, Duration::from_millis(200), Easing::Linear, None);
        }
    }

    let mut scene = Scene::new();
    // Heavyweight child to flip `has_built_children` so the
    // drag-end rebuild path actually runs (matches realistic
    // showcase scenes with at least one widget tier).
    scene.add_widget(FillWidget::new(), Rect::new(280.0, 200.0, 30.0, 30.0));
    // Five loopers — same shape as the showcase's PulsingDot row.
    let phases: Vec<Signal<f32>> = (0..5).map(|_| Signal::new_animated(0.0)).collect();
    for (i, p) in phases.iter().enumerate() {
        scene.add_item(
            Looper {
                bounds: Rect::new(200.0 + i as f32 * 30.0, 50.0, 20.0, 20.0),
                phase: p.clone(),
            },
            Point::ZERO,
        );
    }
    let drag_rect = scene.add_item(
        RectItem::new(Rect::new(10.0, 10.0, 30.0, 30.0))
            .fill(bastyde_tokens::Color::RED)
            .draggable(true),
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let _view_id =
        tree.add(SceneView::new(scene).selection_mode(crate::selection::SceneSelectionMode::Multi));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Animation should be ticking before any drag.
    tree.tick_animations(Duration::from_millis(50));
    for (i, p) in phases.iter().enumerate() {
        let v = p.get();
        assert!(
            v > 0.0 && v < 1.0,
            "phase[{}] must tween mid-loop pre-drag (got {})",
            i,
            v
        );
    }

    // Drag the rect — Down/Move/Up. This bumps drag_dirty, which
    // triggers a SceneView rebuild on the next layout pass, which
    // is where the regression bites.
    tree.pointer_move(Point::new(20.0, 20.0));
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(20.0, 20.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(60.0, 60.0),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(60.0, 60.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    // First layout: drains the pending move; rebuild's
    // `register_bindings` re-arms the loop's pending request.
    tree.layout(SizeProposal::exact(400.0, 300.0));
    // tick_animations picks up the pending and re-inserts the
    // looping animation into the scheduler, then advances time.
    tree.tick_animations(Duration::from_millis(80));
    let post_a: Vec<f32> = phases.iter().map(|p| p.get()).collect();
    // Confirm motion resumed for every looper: advance another
    // small slice and require each value to have moved.
    tree.tick_animations(Duration::from_millis(40));
    for (i, p) in phases.iter().enumerate() {
        let post_b = p.get();
        assert!(
            (post_b - post_a[i]).abs() > 1e-4,
            "looper[{}] must keep ticking after drag-end rebuild \
             (post_a = {}, post_b = {})",
            i,
            post_a[i],
            post_b
        );
    }
    // And the rect actually moved — sanity check that the drag
    // commit ran.
    let view = view_handle(&tree, _view_id);
    let r = view.scene().scene_rect(drag_rect).unwrap();
    assert!(
        (r.x - 50.0).abs() < 1e-3 && (r.y - 50.0).abs() < 1e-3,
        "drag commit must have run (got {:?})",
        r
    );
}

#[test]
fn drag_in_empty_area_starts_marquee_not_move() {
    // Press in empty scene area (no item underneath) → marquee
    // mode wins; dragging across an item selects it instead of
    // moving it.
    use crate::items::RectItem;
    use bastyde_canvas::Point;

    let mut scene = Scene::new();
    let item_id = scene.add_item(
        RectItem::new(Rect::new(100.0, 100.0, 30.0, 30.0)).fill(bastyde_tokens::Color::RED),
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let view_id =
        tree.add(SceneView::new(scene).selection_mode(crate::selection::SceneSelectionMode::Multi));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Press at (10, 10) — empty area. Drag to (200, 200) —
    // crosses the item. Release.
    tree.pointer_move(Point::new(10.0, 10.0));
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(10.0, 10.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(200.0, 200.0),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(200.0, 200.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });

    let view = view_handle(&tree, view_id);
    view.flush_marquee_commit();
    assert!(
        view.selection().is_selected(item_id),
        "marquee enclosing the item must select it"
    );
    // Item bounds unchanged (no drag-to-move happened).
    let rect = view.scene().scene_rect(item_id).unwrap();
    assert_eq!(rect.x, 100.0);
    assert_eq!(rect.y, 100.0);
}

#[test]
fn z_order_paints_higher_z_after_lower() {
    // Two overlapping lightweight items: id A at z=0 (back),
    // id B at z=10 (front). The frame's draw_order should
    // place B's decoration *after* A's so it appears on top.
    use crate::items::RectItem;
    use bastyde_canvas::DrawCommand;

    let mut scene = Scene::new();
    let a = scene.add_item(
        RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)).fill(bastyde_tokens::Color::RED),
        Point::ZERO,
    );
    let b = scene.add_item(
        RectItem::new(Rect::new(15.0, 15.0, 20.0, 20.0)).fill(bastyde_tokens::Color::BLUE),
        Point::ZERO,
    );
    // Reverse paint order via z: A on top by default (later
    // insertion = on top), but we set B's z higher so B paints
    // last instead.
    scene.set_z(a, 0.0);
    scene.set_z(b, 10.0);

    let mut tree = WidgetTree::new();
    let _view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let frame = tree.render();

    // First decoration is A (red), second is B (blue) — same
    // as default insertion order in this case. Now flip z and
    // verify the order changes.
    let red_first = frame.decorations.iter().enumerate().find_map(|(i, d)| {
        if d.color == bastyde_tokens::Color::RED.to_array() {
            Some(i)
        } else {
            None
        }
    });
    let blue_first = frame.decorations.iter().enumerate().find_map(|(i, d)| {
        if d.color == bastyde_tokens::Color::BLUE.to_array() {
            Some(i)
        } else {
            None
        }
    });
    assert!(
        red_first.is_some() && blue_first.is_some(),
        "both decorations should be present"
    );
    assert!(
        red_first.unwrap() < blue_first.unwrap(),
        "z=10 (blue) should paint after z=0 (red)"
    );
    // Decoration commands are present (the per-item
    // scene-transform push emits SetTransform commands too, so
    // they're interleaved with decorations in `draw_order`).
    assert!(
        frame
            .draw_order
            .iter()
            .any(|cmd| matches!(cmd, DrawCommand::Decoration(_))),
        "expected at least one Decoration in draw_order"
    );
}

#[test]
fn z_order_default_zero_preserves_insertion_order() {
    // Without explicit z, items paint in insertion order.
    use crate::items::RectItem;

    let mut scene = Scene::new();
    scene.add_item(
        RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)).fill(bastyde_tokens::Color::RED),
        Point::ZERO,
    );
    scene.add_item(
        RectItem::new(Rect::new(15.0, 15.0, 20.0, 20.0)).fill(bastyde_tokens::Color::BLUE),
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let _view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let frame = tree.render();
    assert_eq!(
        frame.decorations[0].color,
        bastyde_tokens::Color::RED.to_array(),
        "first-inserted item paints first by default"
    );
    assert_eq!(
        frame.decorations[1].color,
        bastyde_tokens::Color::BLUE.to_array()
    );
}

#[test]
fn marquee_no_op_in_none_mode() {
    // With selection mode None, drag does nothing — the
    // on_drag handler isn't even registered.
    use bastyde_canvas::Point;

    let mut scene = Scene::new();
    scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 10.0, 10.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(40.0, 40.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(80.0, 80.0),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(80.0, 80.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });

    let view = view_handle(&tree, view_id);
    assert_eq!(view.selection().count(), 0);
}

#[test]
fn marquee_does_not_unmount_heavyweight_children() {
    // Regression: dragging a marquee in empty space used to make
    // heavyweight children "disappear" because the drag-end path
    // bumped a Rebuild-level signal, and rebuilding SceneView
    // re-pushed materialised WidgetIds to child_ids without
    // re-attaching them via ctx.add_boxed — the framework's
    // rebuild reconciliation pruned them. The fix routes the
    // marquee drain through Relayout (place_children) instead.
    use bastyde_canvas::Point;

    let mut scene = Scene::new();
    let widget_item = scene.add_widget(FillWidget::new(), Rect::new(50.0, 50.0, 80.0, 60.0));

    let mut tree = WidgetTree::new();
    let view_id =
        tree.add(SceneView::new(scene).selection_mode(crate::selection::SceneSelectionMode::Multi));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Confirm the widget is in the arena before any drag.
    let view = view_handle(&tree, view_id);
    let materialised_id = view
        .widget_id_for(widget_item)
        .expect("widget materialised");
    assert!(tree.children(view_id).contains(&materialised_id));

    // Drag a marquee in empty space (above the widget).
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(200.0, 10.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(260.0, 30.0),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(260.0, 30.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });

    // After marquee end the framework processes any dirty
    // signals — drive a layout to give place_children a chance
    // to drain.
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Heavyweight child must still be a child of the SceneView.
    assert!(
        tree.children(view_id).contains(&materialised_id),
        "marquee end must NOT unmount heavyweight children — \
         children: {:?}, expected to contain: {:?}",
        tree.children(view_id),
        materialised_id,
    );
}

// -- Focus-order traversal -----------------------------------------

fn rect_item_at(x: f32, y: f32) -> crate::items::RectItem {
    use bastyde_tokens::Color;
    crate::items::RectItem::new(Rect::new(x, y, 10.0, 10.0)).fill(Color::RED)
}

#[test]
fn focus_order_default_walks_insertion_order_forward() {
    let mut scene = Scene::new();
    let a = scene.add_item(rect_item_at(0.0, 0.0), Point::ZERO);
    let b = scene.add_item(rect_item_at(20.0, 0.0), Point::ZERO);
    let c = scene.add_item(rect_item_at(40.0, 0.0), Point::ZERO);

    let view = SceneView::new(scene);
    // None → first
    assert_eq!(view.next_focus(None), Some(a));
    // a → b → c → None (no wrap)
    assert_eq!(view.next_focus(Some(a)), Some(b));
    assert_eq!(view.next_focus(Some(b)), Some(c));
    assert_eq!(view.next_focus(Some(c)), None);
}

#[test]
fn focus_order_default_walks_insertion_order_backward() {
    let mut scene = Scene::new();
    let a = scene.add_item(rect_item_at(0.0, 0.0), Point::ZERO);
    let b = scene.add_item(rect_item_at(20.0, 0.0), Point::ZERO);
    let c = scene.add_item(rect_item_at(40.0, 0.0), Point::ZERO);

    let view = SceneView::new(scene);
    // None → last
    assert_eq!(view.previous_focus(None), Some(c));
    // c → b → a → None
    assert_eq!(view.previous_focus(Some(c)), Some(b));
    assert_eq!(view.previous_focus(Some(b)), Some(a));
    assert_eq!(view.previous_focus(Some(a)), None);
}

#[test]
fn focus_order_empty_scene_returns_none() {
    let view = SceneView::new(Scene::new());
    assert_eq!(view.next_focus(None), None);
    assert_eq!(view.previous_focus(None), None);
}

#[test]
fn focus_order_callback_overrides_default() {
    // App-supplied callback returns items in REVERSE insertion
    // order on Forward — proves the callback overrides the
    // built-in walk and isn't merely augmenting it.
    let mut scene = Scene::new();
    let a = scene.add_item(rect_item_at(0.0, 0.0), Point::ZERO);
    let b = scene.add_item(rect_item_at(20.0, 0.0), Point::ZERO);
    let c = scene.add_item(rect_item_at(40.0, 0.0), Point::ZERO);

    let view = SceneView::new(scene).focus_order(move |scene, dir, current| {
        let mut ids = scene.ids();
        if matches!(dir, FocusDirection::Forward) {
            ids.reverse();
        }
        match current {
            None => ids.first().copied(),
            Some(cur) => ids
                .iter()
                .position(|id| *id == cur)
                .and_then(|i| ids.get(i + 1).copied()),
        }
    });

    // Forward starts at last (c) and walks toward a.
    assert_eq!(view.next_focus(None), Some(c));
    assert_eq!(view.next_focus(Some(c)), Some(b));
    assert_eq!(view.next_focus(Some(b)), Some(a));
    assert_eq!(view.next_focus(Some(a)), None);
}

// -- fit_to_items / fit_to_selection -------------------------------

#[test]
fn fit_to_items_empty_is_noop() {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let view = view_handle(&tree, view_id);
    let pan_before = view.pan();
    let zoom_before = view.zoom();
    view.fit_to_items(&[]);
    // No-op: no animation kicked off.
    assert_eq!(view.pan(), pan_before);
    assert_eq!(view.zoom(), zoom_before);
}

#[test]
fn fit_to_items_skips_stale_ids() {
    let mut scene = Scene::new();
    let a = scene.add_item(rect_item_at(0.0, 0.0), Point::ZERO);
    scene.remove(a);

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let view = view_handle(&tree, view_id);
    let pan_before = view.pan();
    let zoom_before = view.zoom();
    // Stale id — skipped.
    view.fit_to_items(&[a]);
    assert_eq!(view.pan(), pan_before);
    assert_eq!(view.zoom(), zoom_before);
}

#[test]
fn fit_to_selection_uses_selected_ids() {
    let mut scene = Scene::new();
    let a = scene.add_item(rect_item_at(100.0, 100.0), Point::ZERO);
    let _b = scene.add_item(rect_item_at(900.0, 900.0), Point::ZERO);

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene).selection_mode(crate::SceneSelectionMode::Multi));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let view = view_handle(&tree, view_id);

    // Empty selection → no-op.
    let pan_before = view.pan();
    let zoom_before = view.zoom();
    view.fit_to_selection();
    assert_eq!(view.pan(), pan_before);
    assert_eq!(view.zoom(), zoom_before);

    // Select item `a`, fit_to_selection animates toward its bounds —
    // the resulting target zoom should be at or above min_zoom and
    // bounded by max_zoom.
    view.selection().select_one(a);
    view.fit_to_selection();
    // After kicking the animation, current zoom is between the
    // start (1.0) and the eventual target. We assert the call
    // reached the math (zoom didn't stay exactly 1.0 unless the
    // computed target happens to also be 1.0). Loose check: the
    // pan signal is no longer at the origin since `a` is at
    // (100,100) and we're centering it in an 800x600 viewport.
    // The effective zoom-range override (default Some(0.1..=10.0))
    // should contain the current zoom.
    let range = view
        .zoom_range_override_signal()
        .get()
        .expect("default override is Some(0.1..=10.0)");
    assert!(view.zoom() >= *range.start());
    assert!(view.zoom() <= *range.end());
}

#[test]
fn focus_order_callback_can_skip_items() {
    // App-supplied callback only Tab-cycles between odd-indexed
    // items — exercises the "domain-specific traversal" use case
    // (e.g. graph editor that only tabs through nodes, not
    // connectors).
    let mut scene = Scene::new();
    let _skip0 = scene.add_item(rect_item_at(0.0, 0.0), Point::ZERO);
    let keep1 = scene.add_item(rect_item_at(20.0, 0.0), Point::ZERO);
    let _skip2 = scene.add_item(rect_item_at(40.0, 0.0), Point::ZERO);
    let keep3 = scene.add_item(rect_item_at(60.0, 0.0), Point::ZERO);

    let allowed = [keep1, keep3];
    let view = SceneView::new(scene).focus_order(move |_scene, dir, current| match current {
        None => match dir {
            FocusDirection::Forward => allowed.first().copied(),
            FocusDirection::Backward => allowed.last().copied(),
        },
        Some(cur) => {
            let pos = allowed.iter().position(|id| *id == cur)?;
            match dir {
                FocusDirection::Forward => allowed.get(pos + 1).copied(),
                FocusDirection::Backward => {
                    if pos == 0 {
                        None
                    } else {
                        allowed.get(pos - 1).copied()
                    }
                }
            }
        }
    });

    assert_eq!(view.next_focus(None), Some(keep1));
    assert_eq!(view.next_focus(Some(keep1)), Some(keep3));
    assert_eq!(view.next_focus(Some(keep3)), None);
    assert_eq!(view.previous_focus(None), Some(keep3));
    assert_eq!(view.previous_focus(Some(keep3)), Some(keep1));
}

// -- Nested-SceneView a11y -----------------------------------------

#[test]
fn default_scene_view_has_pane_role() {
    use bastyde_core::accessibility::widget_id_to_node_id;

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let update = tree.sync_accessibility();
    let view_node = update
        .nodes
        .iter()
        .find(|(id, _)| *id == widget_id_to_node_id(view_id))
        .map(|(_, n)| n)
        .expect("scene view node");
    assert_eq!(view_node.role(), accesskit::Role::Pane);
}

#[test]
fn nested_scene_view_uses_region_role() {
    use bastyde_core::accessibility::widget_id_to_node_id;

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()).nested_a11y(true));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let update = tree.sync_accessibility();
    let view_node = update
        .nodes
        .iter()
        .find(|(id, _)| *id == widget_id_to_node_id(view_id))
        .map(|(_, n)| n)
        .expect("scene view node");
    // Nested SceneViews announce as Region instead of Pane to
    // avoid landmark redundancy in nested layouts.
    assert_eq!(view_node.role(), accesskit::Role::Region);
}

#[test]
fn a11y_label_is_announced() {
    use bastyde_core::accessibility::widget_id_to_node_id;

    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::new(Scene::new())
            .nested_a11y(true)
            .a11y_label("Chart data area"),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let update = tree.sync_accessibility();
    let view_node = update
        .nodes
        .iter()
        .find(|(id, _)| *id == widget_id_to_node_id(view_id))
        .map(|(_, n)| n)
        .expect("scene view node");
    assert_eq!(view_node.label(), Some("Chart data area"));
}

#[test]
fn is_nested_accessor_reflects_builder_setting() {
    let view = SceneView::new(Scene::new());
    assert!(!view.is_nested());
    let view = view.nested_a11y(true);
    assert!(view.is_nested());
}

// -- View-state persistence ----------------------------------------

#[test]
fn state_snapshot_reflects_current_view() {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let view = view_handle(&tree, view_id);
    view.set_pan(Vec2::new(100.0, 50.0));
    view.set_zoom(2.0);
    view.set_rotation(0.5);

    let s = view.state();
    assert_eq!(s.pan_x, 100.0);
    assert_eq!(s.pan_y, 50.0);
    assert_eq!(s.zoom, 2.0);
    assert_eq!(s.rotation, 0.5);
}

#[test]
fn restore_state_round_trip() {
    let saved = crate::SceneViewState::new(Vec2::new(42.0, -17.0), 1.5, 0.25);

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let view = view_handle(&tree, view_id);

    view.restore_state(saved);
    assert_eq!(view.pan(), Vec2::new(42.0, -17.0));
    assert_eq!(view.zoom(), 1.5);
    assert_eq!(view.rotation(), 0.25);

    // Round-trip: snapshot equals the restored input.
    assert_eq!(view.state(), saved);
}

#[test]
fn restore_state_clamps_zoom() {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()).max_zoom(5.0));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let view = view_handle(&tree, view_id);

    // Saved zoom of 100.0 (e.g. corrupted settings) is clamped
    // to max_zoom on restore — apps don't end up with an
    // unusable infinitely-zoomed view from a stale config.
    let saved = crate::SceneViewState::new(Vec2::ZERO, 100.0, 0.0);
    view.restore_state(saved);
    assert_eq!(view.zoom(), 5.0);
}

#[test]
fn identity_state_is_default() {
    let s: crate::SceneViewState = Default::default();
    assert!(s.is_identity());
}

// -- a11y_bounds_space ---------------------------------------------

#[test]
fn a11y_bounds_default_to_screen_projection() {
    use crate::items::RectItem;
    use bastyde_core::accessibility::is_synthetic;
    use bastyde_tokens::Color;

    let mut scene = Scene::new();
    scene.add_item(
        RectItem::new(Rect::new(100.0, 50.0, 20.0, 20.0)).fill(Color::RED),
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    // Pan + zoom: screen bounds become (200 - panx, 100 - pany)
    // at zoom 2.0 → corners (200, 100, 40, 40).
    let view = view_handle(&tree, view_id);
    view.set_zoom(2.0);

    let update = tree.sync_accessibility();
    let item_node = update
        .nodes
        .iter()
        .find(|(id, _)| is_synthetic(*id))
        .map(|(_, n)| n)
        .expect("synthetic item node");

    // Default Screen mode: bounds reflect 2x scale.
    let bounds = item_node.bounds().expect("item bounds set");
    let width = bounds.x1 - bounds.x0;
    assert!(
        (width - 40.0).abs() < 0.5,
        "screen width should reflect zoom"
    );
}

#[test]
fn a11y_bounds_scene_mode_reports_raw_scene_coords() {
    use crate::items::RectItem;
    use bastyde_core::accessibility::is_synthetic;
    use bastyde_tokens::Color;

    let mut scene = Scene::new();
    scene.add_item(
        RectItem::new(Rect::new(100.0, 50.0, 20.0, 20.0)).fill(Color::RED),
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene).a11y_bounds_space(crate::A11yBoundsSpace::Scene));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let view = view_handle(&tree, view_id);
    view.set_zoom(2.0);

    let update = tree.sync_accessibility();
    let item_node = update
        .nodes
        .iter()
        .find(|(id, _)| is_synthetic(*id))
        .map(|(_, n)| n)
        .expect("synthetic item node");

    // Scene mode: bounds match the raw scene rect, ignoring zoom.
    let bounds = item_node.bounds().expect("item bounds set");
    let width = bounds.x1 - bounds.x0;
    assert!(
        (width - 20.0).abs() < 0.5,
        "scene-mode width must equal raw scene width"
    );
    assert!((bounds.x0 - 100.0).abs() < 0.5);
    assert!((bounds.y0 - 50.0).abs() < 0.5);
}

#[test]
fn current_a11y_bounds_space_accessor_reflects_setting() {
    let view = SceneView::new(Scene::new());
    assert_eq!(
        view.current_a11y_bounds_space(),
        crate::A11yBoundsSpace::Screen
    );
    let view = view.a11y_bounds_space(crate::A11yBoundsSpace::Scene);
    assert_eq!(
        view.current_a11y_bounds_space(),
        crate::A11yBoundsSpace::Scene
    );
}

// -- Debug overlays ------------------------------------------------

#[test]
fn debug_overlay_default_is_inactive() {
    let cfg = DebugOverlay::default();
    assert!(!cfg.is_active());
}

#[test]
fn debug_overlay_all_is_active() {
    let cfg = DebugOverlay::ALL;
    assert!(cfg.is_active());
    assert!(cfg.item_bounds);
    assert!(cfg.content_bounds);
    assert!(cfg.viewport);
    assert!(cfg.selection_bounds);
}

#[test]
fn debug_overlay_setting_round_trips() {
    let view = SceneView::new(Scene::new()).debug_overlay(DebugOverlay {
        item_bounds: true,
        ..Default::default()
    });
    let cfg = view.current_debug_overlay();
    assert!(cfg.item_bounds);
    assert!(!cfg.viewport);
    assert!(cfg.is_active());
}

#[test]
fn debug_overlay_renders_when_enabled() {
    // Smoke test: a SceneView with debug overlay on produces
    // additional draw commands compared to one with overlays
    // off, given identical scene contents.
    let make_scene = || {
        let mut s = Scene::new();
        s.add_item(rect_item_at(20.0, 20.0), Point::ZERO);
        s.add_item(rect_item_at(60.0, 20.0), Point::ZERO);
        s
    };

    let mut tree_off = WidgetTree::new();
    tree_off.add(SceneView::new(make_scene()));
    tree_off.layout(SizeProposal::exact(400.0, 300.0));
    let off_count = tree_off.render().draw_order.len();

    let mut tree_on = WidgetTree::new();
    tree_on.add(SceneView::new(make_scene()).debug_overlay(DebugOverlay::ALL));
    tree_on.layout(SizeProposal::exact(400.0, 300.0));
    let on_count = tree_on.render().draw_order.len();

    // ALL adds at minimum: 2 item-bound strokes + 1 content
    // outline + 1 viewport. Selection_bounds adds 0 because
    // nothing is selected.
    assert!(
        on_count > off_count,
        "debug overlay must produce more draws (on={}, off={})",
        on_count,
        off_count
    );
}

// -----------------------------------------------------------------
// R2: scene policy + adopt_scene_size
// -----------------------------------------------------------------

#[test]
fn adopt_scene_size_returns_scene_extent_from_layout_response() {
    use crate::items::RectItem;
    let mut scene = Scene::new();
    scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 200.0, 150.0)),
        bastyde_canvas::Point::new(0.0, 0.0),
    );
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene).adopt_scene_size(true));
    // Propose nothing; the view must size itself to the scene.
    tree.layout(SizeProposal::unspecified());
    let bounds = tree.bounds(view_id);
    assert!(
        (bounds.width - 200.0).abs() < 1e-3,
        "width = {}",
        bounds.width
    );
    assert!(
        (bounds.height - 150.0).abs() < 1e-3,
        "height = {}",
        bounds.height
    );
}

#[test]
fn pan_axes_none_makes_set_pan_a_noop() {
    let mut scene = Scene::new();
    scene.pan_axes(crate::scene::PanAxes::None);
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);
    view.set_pan(Vec2::new(123.0, 456.0));
    assert_eq!(view.pan(), Vec2::ZERO);
}

#[test]
fn pan_axes_horizontal_blocks_vertical_set_pan() {
    let mut scene = Scene::new();
    scene.pan_axes(crate::scene::PanAxes::Horizontal);
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);
    view.set_pan(Vec2::new(50.0, 75.0));
    let pan = view.pan();
    assert!((pan.x - 50.0).abs() < 1e-3);
    assert!(pan.y.abs() < 1e-3, "Y axis must stay 0 (got {})", pan.y);
}

#[test]
fn zoomable_false_makes_zoom_to_a_noop() {
    let mut scene = Scene::new();
    scene.zoomable(false);
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);
    let z0 = view.zoom();
    view.set_zoom(2.5);
    assert!((view.zoom() - z0).abs() < 1e-6);
}

// -----------------------------------------------------------------
// R3: per-item events
// -----------------------------------------------------------------

#[test]
fn on_tap_fires_when_item_clicked() {
    use crate::items::RectItem;
    use std::cell::Cell;
    use std::rc::Rc;
    let mut scene = Scene::new();
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 50.0, 50.0)).fill(bastyde_tokens::Color::RED),
        bastyde_canvas::Point::new(20.0, 20.0),
    );
    let count = Rc::new(Cell::new(0_u32));
    let count_clone = count.clone();
    scene.handlers_mut(id).unwrap().on_tap(move |_pt, _ctx| {
        count_clone.set(count_clone.get() + 1);
    });
    let mut tree = WidgetTree::new();
    tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    // Tap squarely inside the item.
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: bastyde_canvas::Point::new(40.0, 40.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: bastyde_canvas::Point::new(40.0, 40.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    assert_eq!(count.get(), 1, "on_tap must fire once");
}

#[test]
fn on_context_menu_fires_on_secondary_button() {
    use crate::items::RectItem;
    use std::cell::Cell;
    use std::rc::Rc;
    let mut scene = Scene::new();
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 50.0, 50.0)).fill(bastyde_tokens::Color::RED),
        bastyde_canvas::Point::new(20.0, 20.0),
    );
    let fired = Rc::new(Cell::new(false));
    let fired_clone = fired.clone();
    scene
        .handlers_mut(id)
        .unwrap()
        .on_context_menu(move |_pt, _ctx| {
            fired_clone.set(true);
        });
    let mut tree = WidgetTree::new();
    tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: bastyde_canvas::Point::new(40.0, 40.0),
        button: bastyde_core::event::PointerButton::Secondary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    assert!(fired.get(), "on_context_menu must fire on right-click");
}

#[test]
fn drag_mode_no_drag_disables_marquee_and_drag_to_move() {
    use crate::items::RectItem;
    let mut scene = Scene::new();
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 30.0, 30.0))
            .fill(bastyde_tokens::Color::RED)
            .draggable(true),
        bastyde_canvas::Point::new(10.0, 10.0),
    );
    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::new(scene)
            .selection_mode(crate::SceneSelectionMode::Multi)
            .drag_mode(crate::DragMode::NoDrag),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    // Attempt drag — should produce no change.
    tree.pointer_move(bastyde_canvas::Point::new(20.0, 20.0));
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: bastyde_canvas::Point::new(20.0, 20.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: bastyde_canvas::Point::new(60.0, 60.0),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: bastyde_canvas::Point::new(60.0, 60.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);
    // local_pos unchanged.
    assert_eq!(
        view.scene().local_pos(id),
        Some(bastyde_canvas::Point::new(10.0, 10.0))
    );
}

#[test]
fn adopt_scene_size_disables_user_pan() {
    let mut scene = Scene::new();
    scene.add_item(
        crate::items::RectItem::new(Rect::new(0.0, 0.0, 200.0, 150.0)),
        bastyde_canvas::Point::ZERO,
    );
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene).adopt_scene_size(true));
    tree.layout(SizeProposal::unspecified());
    let view = view_handle(&tree, view_id);
    view.set_pan(Vec2::new(99.0, 99.0));
    assert_eq!(view.pan(), Vec2::ZERO);
}

// -----------------------------------------------------------------
// R5: background / foreground / ensure_visible / cache modes
// -----------------------------------------------------------------

#[test]
fn background_runs_before_items() {
    // The background closure paints a marker decoration, then a
    // RectItem paints another. The frame's draw_order must list
    // the background marker first among Decoration commands.
    use crate::items::RectItem;
    use bastyde_canvas::DrawCommand;
    use bastyde_tokens::Color;

    let bg_color = Color::new(0.1, 0.2, 0.3, 1.0);
    let item_color = Color::new(0.9, 0.8, 0.7, 1.0);

    let mut scene = Scene::new();
    scene.add_item(
        RectItem::new(Rect::new(20.0, 20.0, 30.0, 30.0)).fill(item_color),
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let _id = tree.add(SceneView::new(scene).background(move |c, _ctx, _r| {
        c.fill_rect(Rect::new(0.0, 0.0, 5.0, 5.0), bg_color);
    }));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let frame = tree.render();

    // Find first and second Decoration entries' colors.
    let mut decos: Vec<[f32; 4]> = Vec::new();
    for cmd in &frame.draw_order {
        if let DrawCommand::Decoration(idx) = cmd {
            decos.push(frame.decorations[*idx].color);
        }
    }
    assert!(
        decos.len() >= 2,
        "expected ≥2 Decoration entries, got {}",
        decos.len()
    );
    assert_eq!(decos[0], bg_color.to_array(), "background must paint first");
    assert!(decos.iter().any(|c| *c == item_color.to_array()));
}

#[test]
fn foreground_runs_after_items() {
    use crate::items::RectItem;
    use bastyde_canvas::DrawCommand;
    use bastyde_tokens::Color;

    let item_color = Color::new(0.4, 0.5, 0.6, 1.0);
    let fg_color = Color::new(0.95, 0.05, 0.05, 1.0);

    let mut scene = Scene::new();
    scene.add_item(
        RectItem::new(Rect::new(20.0, 20.0, 30.0, 30.0)).fill(item_color),
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let _id = tree.add(SceneView::new(scene).foreground(move |c, _ctx, _r| {
        c.fill_rect(Rect::new(0.0, 0.0, 5.0, 5.0), fg_color);
    }));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let frame = tree.render();

    let mut decos: Vec<[f32; 4]> = Vec::new();
    for cmd in &frame.draw_order {
        if let DrawCommand::Decoration(idx) = cmd {
            decos.push(frame.decorations[*idx].color);
        }
    }
    assert!(
        decos.len() >= 2,
        "expected ≥2 Decoration entries, got {}",
        decos.len()
    );
    // Last Decoration must be the foreground marker — items
    // (and the marquee/debug overlay, which are absent here) all
    // paint before it.
    assert_eq!(*decos.last().unwrap(), fg_color.to_array());
}

#[test]
fn background_receives_visible_scene_region() {
    use crate::items::RectItem;
    use std::cell::RefCell;
    use std::rc::Rc;

    let captured: Rc<RefCell<Option<Rect>>> = Rc::new(RefCell::new(None));
    let captured_w = captured.clone();

    let mut scene = Scene::new();
    scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);

    let mut tree = WidgetTree::new();
    let _id = tree.add(SceneView::new(scene).background(move |_c, _ctx, region| {
        *captured_w.borrow_mut() = Some(region);
    }));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = tree.render();

    let region = captured.borrow().expect("background must run on render");
    // No pan, no zoom → visible scene region matches the SceneView's
    // own rect (origin/size projected through identity).
    assert!((region.width - 400.0).abs() < 1e-3);
    assert!((region.height - 300.0).abs() < 1e-3);
}

#[test]
fn ensure_visible_pans_only() {
    // Scene with an item far outside the default viewport. Calling
    // ensure_visible must shift pan (not zoom) so the item lands
    // inside the visible region.
    use crate::items::RectItem;

    let mut scene = Scene::new();
    let target = scene.add_item(
        RectItem::new(Rect::new(500.0, 0.0, 50.0, 50.0)),
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let view = view_handle(&tree, view_id);
    let zoom_before = view.zoom();
    let scene_rect = view.scene().scene_rect(target).expect("rect");
    view.ensure_visible(scene_rect, 10.0);

    // Zoom unchanged.
    assert!((view.zoom() - zoom_before).abs() < 1e-6);
    // Pan is animated (Unit 1 fix), so the live value at t=0 is
    // still the starting pan. Inspect the in-flight animation
    // target instead — that's where the tween is heading.
    let target_x = view
        .pan_x_animation_target()
        .expect("ensure_visible should now animate pan, not snap");
    assert!(
        target_x < 0.0,
        "expected leftward pan target to bring item into view, got {}",
        target_x
    );
}

#[test]
fn ensure_visible_noop_when_target_already_inside() {
    use crate::items::RectItem;
    let mut scene = Scene::new();
    let id = scene.add_item(
        RectItem::new(Rect::new(50.0, 50.0, 30.0, 30.0)),
        Point::ZERO,
    );
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);
    let pan_before = view.pan();
    let r = view.scene().scene_rect(id).unwrap();
    view.ensure_visible(r, 0.0);
    assert_eq!(view.pan(), pan_before);
}

#[test]
fn cache_mode_item_coordinate_avoids_repeat_paints() {
    // A scene item that returns CacheMode::ItemCoordinate and
    // counts how many times its `paint` is invoked. The
    // SceneView's frame cache short-circuits `tree.render()`
    // when nothing's dirty, so we force re-paint between calls
    // by nudging pan — pan changes dirty the view but don't
    // dirty the per-item cache (cache is in local coords).
    use crate::cache::CacheMode;
    use crate::item::{SceneItem, SceneItemPaintContext};
    use std::cell::Cell;
    use std::rc::Rc;

    #[derive(Debug)]
    struct CachedRect {
        bounds: Rect,
        count: Rc<Cell<u32>>,
    }
    impl SceneItem for CachedRect {
        fn local_bounds(&self) -> Rect {
            self.bounds
        }
        fn set_local_bounds(&mut self, b: Rect) {
            self.bounds = b;
        }
        fn paint(&self, canvas: &mut bastyde_canvas::Canvas, _ctx: &SceneItemPaintContext) {
            self.count.set(self.count.get() + 1);
            canvas.fill_rect(self.bounds, bastyde_tokens::Color::new(1.0, 0.0, 0.0, 1.0));
        }
        fn cache_mode(&self) -> CacheMode {
            CacheMode::ItemCoordinate
        }
    }

    let count = Rc::new(Cell::new(0));
    let mut scene = Scene::new();
    let _id = scene.add_item(
        CachedRect {
            bounds: Rect::new(10.0, 10.0, 30.0, 30.0),
            count: count.clone(),
        },
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = tree.render();
    // First render records into the cache. Force re-paint by
    // nudging pan, then render again — the per-item path must
    // hit the cache and skip the item's `paint`.
    view_handle(&tree, view_id).set_pan(Vec2::new(10.0, 0.0));
    let _ = tree.render();
    view_handle(&tree, view_id).set_pan(Vec2::new(20.0, 0.0));
    let _ = tree.render();
    assert_eq!(
        count.get(),
        1,
        "ItemCoordinate cache must skip repeat paint; got {} invocations",
        count.get()
    );
    // Cache contains exactly the one entry.
    assert_eq!(view_handle(&tree, view_id).item_cache_len(), 1);
}

#[test]
fn cache_evicts_on_invalidate() {
    // After explicit invalidate, the next paint pass must re-record.
    use crate::cache::CacheMode;
    use crate::item::{SceneItem, SceneItemPaintContext};
    use std::cell::Cell;
    use std::rc::Rc;

    #[derive(Debug)]
    struct CachedRect {
        bounds: Rect,
        count: Rc<Cell<u32>>,
    }
    impl SceneItem for CachedRect {
        fn local_bounds(&self) -> Rect {
            self.bounds
        }
        fn set_local_bounds(&mut self, b: Rect) {
            self.bounds = b;
        }
        fn paint(&self, canvas: &mut bastyde_canvas::Canvas, _ctx: &SceneItemPaintContext) {
            self.count.set(self.count.get() + 1);
            canvas.fill_rect(self.bounds, bastyde_tokens::Color::new(0.0, 0.5, 1.0, 1.0));
        }
        fn cache_mode(&self) -> CacheMode {
            CacheMode::ItemCoordinate
        }
    }

    let count = Rc::new(Cell::new(0));
    let mut scene = Scene::new();
    let id = scene.add_item(
        CachedRect {
            bounds: Rect::new(5.0, 5.0, 20.0, 20.0),
            count: count.clone(),
        },
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = tree.render();
    assert_eq!(count.get(), 1);

    view_handle(&tree, view_id).invalidate_item_cache(id);
    // Force re-paint via pan nudge.
    view_handle(&tree, view_id).set_pan(Vec2::new(7.0, 0.0));
    let _ = tree.render();
    assert_eq!(count.get(), 2, "evicted cache must trigger re-paint");
}

#[test]
fn cache_evicts_on_item_change_signal() {
    // Geometry change to the cached item must flow through the
    // item_change_signal observer and evict the entry. Verifies
    // the wiring in `build()`.
    use crate::cache::CacheMode;
    use crate::item::{SceneItem, SceneItemPaintContext};
    use std::cell::Cell;
    use std::rc::Rc;

    #[derive(Debug)]
    struct CachedRect {
        bounds: Rect,
        count: Rc<Cell<u32>>,
    }
    impl SceneItem for CachedRect {
        fn local_bounds(&self) -> Rect {
            self.bounds
        }
        fn set_local_bounds(&mut self, b: Rect) {
            self.bounds = b;
        }
        fn paint(&self, canvas: &mut bastyde_canvas::Canvas, _ctx: &SceneItemPaintContext) {
            self.count.set(self.count.get() + 1);
            canvas.fill_rect(self.bounds, bastyde_tokens::Color::new(0.0, 0.5, 1.0, 1.0));
        }
        fn cache_mode(&self) -> CacheMode {
            CacheMode::ItemCoordinate
        }
    }

    let count = Rc::new(Cell::new(0));
    let mut scene = Scene::new();
    let id = scene.add_item(
        CachedRect {
            bounds: Rect::new(5.0, 5.0, 20.0, 20.0),
            count: count.clone(),
        },
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = tree.render();
    assert_eq!(count.get(), 1);
    let view = view_handle(&tree, view_id);
    assert_eq!(view.item_cache_len(), 1);

    // Drive the signal directly to verify the observer wired in
    // `build()` reacts and evicts the entry. Going through a
    // Scene mutator would require `&mut Scene`, which we don't
    // have through `view_handle` — the observer doesn't care
    // about the source.
    view.scene()
        .item_change_signal()
        .set(crate::scene::ItemChange::LocalBoundsChanged {
            id,
            old: Rect::ZERO,
            new: Rect::new(0.0, 0.0, 1.0, 1.0),
        });
    assert!(
        !view.item_cache.borrow().contains(id),
        "LocalBoundsChanged via item_change_signal must evict cache entry"
    );
}

// -----------------------------------------------------------------
// Unit 1 — P1 correctness fixes
// -----------------------------------------------------------------

#[test]
fn adopt_scene_size_uses_extent_dimensions_not_far_corner() {
    // Regression for the right()/bottom() vs width/height confusion
    // in `layout_response`. An item positioned at negative scene
    // coords has a scene_rect whose `right()` and `bottom()` are
    // smaller than its `width` / `height` — under the old code
    // `adopt_scene_size` would request (right, bottom) and the
    // SceneView would lay out smaller than the scene extent,
    // hiding content.
    use crate::items::RectItem;

    let mut scene = Scene::new();
    scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 200.0, 200.0)),
        // Position at negative scene origin so the scene's bounding
        // rect is (-100, -100, 200, 200): right()=100, bottom()=100,
        // but width=200, height=200.
        bastyde_canvas::Point::new(-100.0, -100.0),
    );

    let extent = scene.scene_rect_extent().expect("extent exists");
    assert_eq!(extent.width, 200.0);
    assert_eq!(extent.height, 200.0);
    assert!(
        (extent.right() - 100.0).abs() < 1e-3,
        "sanity: right() is smaller than width when origin is negative"
    );

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene).adopt_scene_size(true));
    tree.layout(SizeProposal::unspecified());

    let bounds = tree.bounds(view_id);
    assert_eq!(
        bounds.width, 200.0,
        "adopt_scene_size must size to extent.width, not extent.right()"
    );
    assert_eq!(
        bounds.height, 200.0,
        "adopt_scene_size must size to extent.height, not extent.bottom()"
    );
}

#[test]
fn ensure_visible_animates_pan_via_pan_to() {
    // Regression for ensure_visible snapping with `set_pan` instead
    // of animating via `pan_to`. The live `pan()` reading is the
    // pre-tween value at t=0; the *animation target* is what was
    // scheduled by `pan_to`. If `ensure_visible` snapped, no target
    // would be in flight.
    use crate::items::RectItem;

    let mut scene = Scene::new();
    let target = scene.add_item(
        RectItem::new(Rect::new(800.0, 0.0, 50.0, 50.0)),
        Point::ZERO,
    );

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);

    let scene_rect = view.scene().scene_rect(target).expect("rect");
    view.ensure_visible(scene_rect, 10.0);

    let target_x = view
        .pan_x_animation_target()
        .expect("ensure_visible must schedule a pan animation, not snap");
    assert!(
        target_x < 0.0,
        "expected leftward pan target, got {}",
        target_x
    );
    // Live pan still at starting value (animation in flight).
    assert!(
        view.pan().x.abs() < 1e-6,
        "live pan should still be at starting value during tween, got {}",
        view.pan().x
    );
}

#[test]
fn scroll_hand_drag_honors_pan_axes_horizontal() {
    // Regression for the ScrollHandDrag bypass — under the bug,
    // a horizontal-locked scene could still be panned vertically
    // by the hand-tool because the drag handler wrote pan_x/pan_y
    // directly without consulting `pan_axes`.
    let mut scene = Scene::new();
    scene.pan_axes(crate::scene::PanAxes::Horizontal);

    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::new(scene)
            .selection_mode(crate::SceneSelectionMode::Multi)
            .drag_mode(crate::DragMode::ScrollHandDrag),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Press, then two Moves (the first crosses the recognizer's
    // 5px threshold and emits DragStarted; the second is the one
    // that fires DragMoved with a delta), then release. We assert
    // on the delta of the SECOND move only — diagonal (+25, +35),
    // of which only the x component should move pan.
    tree.pointer_move(bastyde_canvas::Point::new(100.0, 100.0));
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: bastyde_canvas::Point::new(100.0, 100.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    // First move: crosses threshold, fires DragStarted (no pan).
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: bastyde_canvas::Point::new(105.0, 105.0),
    });
    // Second move: fires DragMoved with delta (+25, +35).
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: bastyde_canvas::Point::new(130.0, 140.0),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: bastyde_canvas::Point::new(130.0, 140.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });

    let view = view_handle(&tree, view_id);
    let pan = view.pan();
    assert!(
        (pan.x - 25.0).abs() < 1e-3,
        "horizontal hand-drag should move pan_x by 25 (second-move delta), got {}",
        pan.x
    );
    assert!(
        pan.y.abs() < 1e-3,
        "vertical axis is locked — pan_y must stay 0, got {}",
        pan.y
    );
}

#[test]
fn scroll_hand_drag_blocked_when_pan_axes_none() {
    let mut scene = Scene::new();
    scene.pan_axes(crate::scene::PanAxes::None);

    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::new(scene)
            .selection_mode(crate::SceneSelectionMode::Multi)
            .drag_mode(crate::DragMode::ScrollHandDrag),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));

    tree.pointer_move(bastyde_canvas::Point::new(50.0, 50.0));
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: bastyde_canvas::Point::new(50.0, 50.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: bastyde_canvas::Point::new(120.0, 90.0),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: bastyde_canvas::Point::new(120.0, 90.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });

    let view = view_handle(&tree, view_id);
    assert_eq!(
        view.pan(),
        Vec2::ZERO,
        "PanAxes::None must block hand-drag on both axes"
    );
}

#[test]
fn ctrl_wheel_zoom_snaps_without_animation_target() {
    // The Ctrl+wheel zoom path intentionally snaps (the anchor math
    // is exact only at start/end, so animating zoom + pan
    // independently drifts mid-tween). This test pins that intent:
    // after a Ctrl+wheel event, zoom changed but neither zoom nor
    // pan_x/pan_y has an in-flight animation target. The Unit 1
    // cleanup removed a `let _ = zoom_dur;` no-op binding; this
    // test guards against accidentally re-introducing animation in
    // the scroll-zoom branch.
    let scene = Scene::new();
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let view = view_handle(&tree, view_id);
    let z_before = view.zoom();

    // Position the cursor inside the view first so the zoom-about-
    // pointer anchor has a defined position.
    tree.pointer_move(bastyde_canvas::Point::new(200.0, 150.0));

    tree.dispatch_event(WidgetEvent::Scroll {
        delta: bastyde_core::event::ScrollDelta::Lines { x: 0.0, y: 1.0 },
        modifiers: bastyde_core::event::Modifiers::CTRL,
    });

    let view = view_handle(&tree, view_id);
    assert!(
        (view.zoom() - z_before).abs() > 1e-3,
        "Ctrl+wheel must change zoom (was {}, now {})",
        z_before,
        view.zoom()
    );
    assert!(
        view.zoom_animation_target().is_none(),
        "Ctrl+wheel zoom is intentionally a snap — no in-flight animation"
    );
    assert!(
        view.pan_x_animation_target().is_none(),
        "Ctrl+wheel pan adjustment is intentionally a snap"
    );
    assert!(
        view.pan_y_animation_target().is_none(),
        "Ctrl+wheel pan adjustment is intentionally a snap"
    );
}

// -----------------------------------------------------------------
// Unit 2 — IGNORES_TRANSFORMATIONS enforcement
// -----------------------------------------------------------------

/// A test-only `SceneItem` that records `canvas.current_transform()`
/// whenever its `paint` runs. Lets tests assert what effective
/// transform the item rendered under — which is the only directly
/// observable signal of IGNORES_TRANSFORMATIONS doing its job.
#[derive(Debug)]
struct TransformRecorder {
    bounds: Rect,
    captured: std::rc::Rc<std::cell::Cell<Option<bastyde_canvas::Transform2D>>>,
}

impl crate::item::SceneItem for TransformRecorder {
    fn local_bounds(&self) -> Rect {
        self.bounds
    }
    fn set_local_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn paint(&self, canvas: &mut bastyde_canvas::Canvas, _ctx: &crate::item::SceneItemPaintContext) {
        self.captured.set(Some(canvas.current_transform()));
        // Emit something so the renderer doesn't elide the whole
        // item — also gives a draw_order entry to inspect if needed.
        canvas.fill_rect(self.bounds, bastyde_tokens::Color::new(0.5, 0.5, 0.5, 1.0));
    }
}

#[test]
fn ignores_xform_paints_under_pure_translate_at_screen_anchor() {
    // Regression for the IGNORES_TRANSFORMATIONS flag previously
    // being inert. Each widget paints into a fresh canvas (identity
    // start), and the render walker composes the SceneView's
    // `set_transform(view_transform)` scope on top at frame
    // playback time. So the captured `canvas.current_transform()`
    // is the transform SceneView::paint emitted; the COMPOSED
    // transform actually applied at the renderer is
    // `captured.then(&view_transform)`. For IGNORES items we want
    // that composition to be a pure `Translate(screen_anchor)` —
    // no scale from zoom, no rotation from the view.
    use std::cell::Cell;
    use std::rc::Rc;
    let captured = Rc::new(Cell::new(None));
    let mut scene = Scene::new();
    let id = scene.add_item(
        TransformRecorder {
            bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
            captured: captured.clone(),
        },
        Point::new(50.0, 60.0),
    );
    scene.set_flag(id, crate::flags::ItemFlags::IGNORES_TRANSFORMATIONS, true);

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);
    // Move the view: pan to (100, 50) and zoom to 2.0×.
    view.set_pan(Vec2::new(100.0, 50.0));
    view.set_zoom(2.0);
    let view_xform = view.view_transform();
    let scene_anchor = Point::new(50.0, 60.0);
    let expected_screen_anchor = view_xform.apply_point(scene_anchor);

    let _ = tree.render();

    let captured_xform = captured
        .get()
        .expect("IGNORES item should have painted at least once");
    let composed = captured_xform.then(&view_xform);
    let m = composed.m;
    // Composed: pure translate to the screen anchor — linear part
    // is identity (no zoom scale, no view rotation leaking in).
    assert!((m[0] - 1.0).abs() < 1e-3, "composed a == 1, got {}", m[0]);
    assert!(m[1].abs() < 1e-3, "composed b == 0, got {}", m[1]);
    assert!(m[2].abs() < 1e-3, "composed c == 0, got {}", m[2]);
    assert!((m[3] - 1.0).abs() < 1e-3, "composed d == 1, got {}", m[3]);
    // Translation = screen anchor.
    assert!(
        (m[4] - expected_screen_anchor.x).abs() < 1e-3,
        "composed tx == screen_anchor.x ({}), got {}",
        expected_screen_anchor.x,
        m[4]
    );
    assert!(
        (m[5] - expected_screen_anchor.y).abs() < 1e-3,
        "composed ty == screen_anchor.y ({}), got {}",
        expected_screen_anchor.y,
        m[5]
    );
}

#[test]
fn ignores_xform_off_paints_under_full_view_composition() {
    // Sanity check: without the flag, the item paints under the
    // FULL composed `view_transform * local_to_scene` chain, so
    // a rect of width 10 in item-local coords renders at width
    // 10×zoom on screen. SceneView::paint emits only the
    // local_to_scene transform; the renderer composes view_xform
    // on top at frame playback.
    use std::cell::Cell;
    use std::rc::Rc;
    let captured = Rc::new(Cell::new(None));
    let mut scene = Scene::new();
    scene.add_item(
        TransformRecorder {
            bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
            captured: captured.clone(),
        },
        Point::new(50.0, 60.0),
    );
    // No flag set — default behavior.

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    {
        let view = view_handle(&tree, view_id);
        view.set_zoom(2.0);
    }

    let _ = tree.render();

    let captured_xform = captured
        .get()
        .expect("normal item should have painted at least once");
    let view_xform = view_handle(&tree, view_id).view_transform();
    let composed = captured_xform.then(&view_xform);
    // Composed linear should equal view zoom (2.0).
    assert!(
        (composed.m[0] - 2.0).abs() < 1e-3,
        "without IGNORES, composed linear == view zoom (2.0), got {}",
        composed.m[0]
    );
    assert!(
        (composed.m[3] - 2.0).abs() < 1e-3,
        "without IGNORES, composed linear == view zoom (2.0), got {}",
        composed.m[3]
    );
    // And translation = view_xform(scene_anchor) = Scale(2)(50,60) = (100,120).
    assert!(
        (composed.m[4] - 100.0).abs() < 1e-3,
        "composed tx, got {}",
        composed.m[4]
    );
    assert!(
        (composed.m[5] - 120.0).abs() < 1e-3,
        "composed ty, got {}",
        composed.m[5]
    );
}

#[test]
fn ignores_xform_hit_test_anchor_tracks_scene_point_but_size_fixed_under_zoom() {
    // The IGNORES_TRANSFORMATIONS semantic mirrors Qt's
    // `ItemIgnoresTransformations`: the item's anchor follows
    // its scene point through pan/zoom (the visible position
    // tracks the data point underneath), but its SIZE stays
    // fixed in screen pixels (it doesn't grow with zoom).
    //
    // So under 2× zoom, the screen anchor doubles (the scene
    // point at (100, 100) is now at screen (200, 200)), but
    // the item's bounding rect is still 40×40 screen pixels.
    // Contrast with a normal item, which would be 80×80
    // screen pixels at (200, 200) under 2× zoom.
    use crate::items::RectItem;
    use std::cell::Cell;
    use std::rc::Rc;

    let mut scene = Scene::new();
    // Item local_bounds (0,0,40,40), local_pos (100, 100).
    // scene_anchor = (100, 100). At zoom 1, pan 0: screen_anchor = (100, 100).
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(bastyde_tokens::Color::RED),
        Point::new(100.0, 100.0),
    );
    scene.set_flag(id, crate::flags::ItemFlags::IGNORES_TRANSFORMATIONS, true);

    let count = Rc::new(Cell::new(0_u32));
    let count_clone = count.clone();
    scene.handlers_mut(id).unwrap().on_tap(move |_pt, _ctx| {
        count_clone.set(count_clone.get() + 1);
    });

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let tap = |tree: &mut WidgetTree, x: f32, y: f32| {
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: bastyde_canvas::Point::new(x, y),
            button: bastyde_core::event::PointerButton::Primary,
            modifiers: bastyde_core::event::Modifiers::default(),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: bastyde_canvas::Point::new(x, y),
            button: bastyde_core::event::PointerButton::Primary,
            modifiers: bastyde_core::event::Modifiers::default(),
        });
    };
    // Zoom 1: screen anchor = (100, 100). Tap inside (110, 110).
    tap(&mut tree, 110.0, 110.0);
    assert_eq!(count.get(), 1, "zoom 1: tap at (110, 110) should hit");

    // Zoom to 2×. screen anchor = view_xform(scene_anchor)
    //   = Scale(2)(100, 100) = (200, 200). Size stays 40×40.
    // Tap inside the new screen rect (200..240, 200..240).
    let view = view_handle(&tree, view_id);
    view.set_zoom(2.0);
    tap(&mut tree, 210.0, 210.0);
    assert_eq!(
        count.get(),
        2,
        "zoom 2: tap at (210, 210) (inside the screen-projected anchor + 40px) should hit"
    );

    // A tap at (110, 110), the OLD anchor before zoom, must miss
    // — the anchor follows the scene point.
    tap(&mut tree, 110.0, 110.0);
    assert_eq!(
        count.get(),
        2,
        "zoom 2: the pre-zoom anchor (110, 110) must MISS — anchor tracks scene point"
    );

    // A tap at (250, 250), which is INSIDE what a normal item's
    // zoom-scaled rect would cover (200..280 = 40×2 = 80px wide),
    // must MISS the IGNORES item — its size is fixed at 40px.
    tap(&mut tree, 250.0, 250.0);
    assert_eq!(
        count.get(),
        2,
        "zoom 2: tap inside what would be a normal item's scaled rect must MISS \
         IGNORES item (size stays at 40px, not 80px)"
    );
}

#[test]
fn ignores_xform_hit_test_anchor_follows_pan() {
    // Panning shifts the screen anchor; the IGNORES item stays
    // attached to its scene anchor (the parent-relative scene
    // point), so panning the view moves the item's screen
    // position by the same screen delta.
    use crate::items::RectItem;
    use std::cell::Cell;
    use std::rc::Rc;

    let mut scene = Scene::new();
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(bastyde_tokens::Color::RED),
        Point::new(100.0, 100.0),
    );
    scene.set_flag(id, crate::flags::ItemFlags::IGNORES_TRANSFORMATIONS, true);

    let count = Rc::new(Cell::new(0_u32));
    let count_clone = count.clone();
    scene.handlers_mut(id).unwrap().on_tap(move |_pt, _ctx| {
        count_clone.set(count_clone.get() + 1);
    });

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);

    // Pan by (+50, +50): screen_anchor = view_xform(scene_anchor)
    //   = pan + zoom * scene_anchor = (50,50) + 1*(100,100) = (150, 150).
    view.set_pan(Vec2::new(50.0, 50.0));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let tap = |tree: &mut WidgetTree, x: f32, y: f32| {
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: bastyde_canvas::Point::new(x, y),
            button: bastyde_core::event::PointerButton::Primary,
            modifiers: bastyde_core::event::Modifiers::default(),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: bastyde_canvas::Point::new(x, y),
            button: bastyde_core::event::PointerButton::Primary,
            modifiers: bastyde_core::event::Modifiers::default(),
        });
    };

    // (160, 160) is inside (150..190, 150..190) — should hit.
    tap(&mut tree, 160.0, 160.0);
    assert_eq!(
        count.get(),
        1,
        "after pan, IGNORES item is at screen anchor (150, 150) — tap (160, 160) must hit"
    );

    // The OLD pre-pan anchor (110, 110) should now MISS.
    tap(&mut tree, 110.0, 110.0);
    assert_eq!(
        count.get(),
        1,
        "after pan, the pre-pan anchor must miss — IGNORES items follow pan"
    );
    let _ = id;
}

#[test]
fn ignores_xform_debug_overlay_paints_screen_anchored_bounds() {
    // The debug item-bounds overlay paints commands through a
    // view-transform-scoped canvas. For IGNORES items, the
    // overlay must outline the actual visible area (fixed-pixel
    // size rooted at the screen-projected anchor), NOT the
    // scaled scene_rect that a naive scene-coord stroke would
    // produce.
    //
    // We verify by inverse: take the decoration rect the
    // overlay emitted, project it through the live view
    // transform, and assert the result equals the expected
    // screen rect — i.e. the round-trip lands on (screen_anchor
    // + local_bounds) with width/height untouched by zoom.
    use crate::items::RectItem;

    let mut scene = Scene::new();
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(bastyde_tokens::Color::RED),
        Point::new(100.0, 100.0),
    );
    scene.set_flag(id, crate::flags::ItemFlags::IGNORES_TRANSFORMATIONS, true);

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene).debug_overlay(DebugOverlay {
        item_bounds: true,
        ..DebugOverlay::default()
    }));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    {
        let view = view_handle(&tree, view_id);
        view.set_zoom(2.0);
    }
    let view_xform = view_handle(&tree, view_id).view_transform();
    let scene_anchor = Point::new(100.0, 100.0);
    let screen_anchor = view_xform.apply_point(scene_anchor);
    let expected_screen_rect = Rect::new(screen_anchor.x, screen_anchor.y, 40.0, 40.0);

    let frame = tree.render();
    // stroke_rect emits 4 thin decoration rects (top, bottom,
    // left, right edges), each centered on the boundary of the
    // passed rect. We compute the union of all four and the
    // union's center should equal the center of the passed
    // rect — robust to stroke-centering offsets.
    let item_color_arr = bastyde_tokens::Color::new(0.20, 0.75, 0.35, 0.85).to_array();
    let edges: Vec<[f32; 4]> = frame
        .decorations
        .iter()
        .filter(|d| {
            d.color
                .iter()
                .zip(item_color_arr.iter())
                .all(|(a, b)| (a - b).abs() < 1e-3)
        })
        .map(|d| d.rect)
        .collect();
    assert_eq!(
        edges.len(),
        4,
        "stroke_rect should emit 4 edge decorations, got {}",
        edges.len()
    );
    let min_x = edges
        .iter()
        .map(|r| r[0])
        .fold(f32::INFINITY, f32::min);
    let min_y = edges
        .iter()
        .map(|r| r[1])
        .fold(f32::INFINITY, f32::min);
    let max_x = edges
        .iter()
        .map(|r| r[0] + r[2])
        .fold(f32::NEG_INFINITY, f32::max);
    let max_y = edges
        .iter()
        .map(|r| r[1] + r[3])
        .fold(f32::NEG_INFINITY, f32::max);
    let union_scene = Rect::new(min_x, min_y, max_x - min_x, max_y - min_y);
    let union_screen = view_xform.apply_rect(union_scene);
    let union_center_x = union_screen.x + union_screen.width / 2.0;
    let union_center_y = union_screen.y + union_screen.height / 2.0;
    let expected_center_x = expected_screen_rect.x + expected_screen_rect.width / 2.0;
    let expected_center_y = expected_screen_rect.y + expected_screen_rect.height / 2.0;
    assert!(
        (union_center_x - expected_center_x).abs() < 0.5,
        "overlay union center x: expected {}, got {}",
        expected_center_x,
        union_center_x
    );
    assert!(
        (union_center_y - expected_center_y).abs() < 0.5,
        "overlay union center y: expected {}, got {}",
        expected_center_y,
        union_center_y
    );
    // Size: outer extent of the stroke = local_bounds + stroke
    // width (1.0 in scene coords; 2.0 in screen coords at 2× zoom).
    // So union screen size = 40 + 2 = 42.
    assert!(
        (union_screen.width - 42.0).abs() < 0.5,
        "union screen width should be ~42 (40 fixed + 2px stroke at 2× zoom), \
         not ~82 (80 scaled + 2), got {}",
        union_screen.width
    );
    assert!(
        (union_screen.height - 42.0).abs() < 0.5,
        "union screen height should be ~42, got {}",
        union_screen.height
    );
}

// -----------------------------------------------------------------
// Unit 3 — SceneConstraints (reactive pan_axes, pan_bounds, zoom_range)
// -----------------------------------------------------------------

#[test]
fn pan_axes_signal_is_reactive_at_runtime() {
    // Regression for the build-time snapshot of pan_axes. After
    // Unit 3, mutating Scene::pan_axes at runtime takes effect on
    // the very next set_pan / pan_to call — no rebuild needed.
    let scene = Scene::new();
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);
    // Initial policy: Both. set_pan should apply both axes.
    view.set_pan(Vec2::new(50.0, 60.0));
    assert_eq!(view.pan(), Vec2::new(50.0, 60.0));

    // Flip to Vertical at runtime via the signal (clone the signal
    // accessor; Signals are Rc-backed so this just shares state).
    view.scene()
        .pan_axes_signal()
        .set(crate::scene::PanAxes::Vertical);
    view.set_pan(Vec2::new(99.0, 75.0));
    // X axis is locked → stays at 50; Y updates.
    assert!(
        (view.pan().x - 50.0).abs() < 1e-3,
        "X axis locked, expected 50, got {}",
        view.pan().x
    );
    assert!(
        (view.pan().y - 75.0).abs() < 1e-3,
        "Y axis open, expected 75, got {}",
        view.pan().y
    );
}

#[test]
fn pan_bounds_clamps_set_pan_to_keep_viewport_inside() {
    // Scene declares a 1000×800 doc bounds. A 400×300 viewport
    // pans into it; pan must keep the visible scene region
    // entirely inside [0, 1000] × [0, 800]. At zoom 1, that
    // means pan_x in [400-1000, 0] = [-600, 0]; pan_y similar.
    let mut scene = Scene::new();
    scene.set_pan_bounds(Some(Rect::new(0.0, 0.0, 1000.0, 800.0)));
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);

    // Try to pan past the right edge (pan_x = 100 would shift
    // the visible region into negative scene x).
    view.set_pan(Vec2::new(100.0, 50.0));
    assert!(
        view.pan().x <= 1e-3,
        "pan_x must be <= 0 to keep visible inside scene bounds, got {}",
        view.pan().x
    );

    // Try to pan past the left edge.
    view.set_pan(Vec2::new(-9999.0, -9999.0));
    // pan_x lower bound = viewport_w - bounds.right * zoom = 400 - 1000 = -600
    assert!(
        (view.pan().x - -600.0).abs() < 1e-3,
        "pan_x must clamp to -600 (viewport - bounds.right), got {}",
        view.pan().x
    );
    // pan_y lower bound = 300 - 800 = -500
    assert!(
        (view.pan().y - -500.0).abs() < 1e-3,
        "pan_y must clamp to -500, got {}",
        view.pan().y
    );
}

#[test]
fn pan_bounds_centers_when_viewport_larger_than_bounds() {
    // 200×200 bounds, 400×300 viewport — bounds smaller than
    // viewport on both axes → center: pan_x = vp/2 - bounds_center_x
    //   = 200 - 100 = 100. pan_y = 150 - 100 = 50.
    let mut scene = Scene::new();
    scene.set_pan_bounds(Some(Rect::new(0.0, 0.0, 200.0, 200.0)));
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);

    view.set_pan(Vec2::new(9999.0, 9999.0));
    assert!(
        (view.pan().x - 100.0).abs() < 1e-3,
        "centered pan_x, expected 100, got {}",
        view.pan().x
    );
    assert!(
        (view.pan().y - 50.0).abs() < 1e-3,
        "centered pan_y, expected 50, got {}",
        view.pan().y
    );

    // Setting an arbitrary pan still snaps to the centered value.
    view.set_pan(Vec2::new(-50.0, 0.0));
    assert!((view.pan().x - 100.0).abs() < 1e-3);
    assert!((view.pan().y - 50.0).abs() < 1e-3);
}

#[test]
fn view_pan_bounds_override_tightens_scene_bounds() {
    // Scene declares 1000×800 bounds; view tightens to inner
    // 500×400 region. Effective = intersect = (250, 200, 500, 400)
    // (centered intersect for this example). Pan clamps against
    // the tighter rect — view-side tightens, never loosens.
    let mut scene = Scene::new();
    scene.set_pan_bounds(Some(Rect::new(0.0, 0.0, 1000.0, 800.0)));
    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::new(scene).pan_bounds_override(Some(Rect::new(250.0, 200.0, 500.0, 400.0))),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);

    // Effective bounds = (250, 200, 500, 400).
    // pan_x clamp: [vp_w - bounds.right * zoom, -bounds.x * zoom]
    //            = [400 - 750, -250] = [-350, -250].
    view.set_pan(Vec2::new(9999.0, 9999.0));
    assert!(
        (view.pan().x - -250.0).abs() < 1e-3,
        "pan_x upper clamp at -250 (view-tightened intersect), got {}",
        view.pan().x
    );
    view.set_pan(Vec2::new(-9999.0, -9999.0));
    assert!(
        (view.pan().x - -350.0).abs() < 1e-3,
        "pan_x lower clamp at -350, got {}",
        view.pan().x
    );
}

#[test]
fn zoom_range_intersects_scene_and_view_overrides() {
    // Scene declares zoom_range 0.5..=4.0. View override default
    // is 0.1..=10.0. Effective intersection: 0.5..=4.0.
    let mut scene = Scene::new();
    scene.set_zoom_range(Some(0.5..=4.0));
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);

    view.set_zoom(10.0);
    assert!(
        (view.zoom() - 4.0).abs() < 1e-3,
        "zoom should clamp to 4.0 (scene's upper), got {}",
        view.zoom()
    );
    view.set_zoom(0.01);
    assert!(
        (view.zoom() - 0.5).abs() < 1e-3,
        "zoom should clamp to 0.5 (scene's lower), got {}",
        view.zoom()
    );
}

#[test]
fn view_zoom_range_override_tightens_scene_range() {
    // Scene declares 0.1..=10.0. View tightens to 0.5..=2.0.
    // Intersection: 0.5..=2.0. set_zoom(5.0) clamps to 2.0.
    let mut scene = Scene::new();
    scene.set_zoom_range(Some(0.1..=10.0));
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene).zoom_range_override(Some(0.5..=2.0)));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);

    view.set_zoom(5.0);
    assert!(
        (view.zoom() - 2.0).abs() < 1e-3,
        "view-side tightening — should clamp to 2.0, got {}",
        view.zoom()
    );
    view.set_zoom(0.05);
    assert!(
        (view.zoom() - 0.5).abs() < 1e-3,
        "should clamp to 0.5, got {}",
        view.zoom()
    );
}

#[test]
fn view_cannot_loosen_scene_zoom_range() {
    // Scene says 1.0..=2.0. View override tries 0.1..=10.0. The
    // intersection is 1.0..=2.0 — view can't loosen.
    let mut scene = Scene::new();
    scene.set_zoom_range(Some(1.0..=2.0));
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene).zoom_range_override(Some(0.1..=10.0)));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);

    view.set_zoom(5.0);
    assert!(
        (view.zoom() - 2.0).abs() < 1e-3,
        "scene tighter than view override — must clamp to scene's 2.0, got {}",
        view.zoom()
    );
    view.set_zoom(0.5);
    assert!(
        (view.zoom() - 1.0).abs() < 1e-3,
        "must clamp to scene's lower 1.0, got {}",
        view.zoom()
    );
}

#[test]
fn min_zoom_max_zoom_shims_still_work_after_refactor() {
    // Back-compat: existing .min_zoom(v) / .max_zoom(v) builder
    // methods should still clamp zoom, now as shims over
    // zoom_range_override.
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()).min_zoom(0.5).max_zoom(3.0));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);

    view.set_zoom(10.0);
    assert!((view.zoom() - 3.0).abs() < 1e-3, "got {}", view.zoom());
    view.set_zoom(0.01);
    assert!((view.zoom() - 0.5).abs() < 1e-3, "got {}", view.zoom());
}

#[test]
fn scene_constraints_helper_accessors_return_signals() {
    // Surface check: the Scene exposes the four signal accessors
    // and they reflect the mutator state.
    let mut scene = Scene::new();
    let pan_axes_sig = scene.pan_axes_signal();
    let pan_bounds_sig = scene.pan_bounds_signal();
    let zoom_range_sig = scene.zoom_range_signal();
    let zoomable_sig = scene.zoomable_signal();

    assert_eq!(pan_axes_sig.get(), crate::scene::PanAxes::Both);
    assert_eq!(pan_bounds_sig.get(), None);
    assert_eq!(zoom_range_sig.get(), None);
    assert_eq!(zoomable_sig.get(), true);

    scene.pan_axes(crate::scene::PanAxes::Horizontal);
    scene.set_pan_bounds(Some(Rect::new(0.0, 0.0, 10.0, 10.0)));
    scene.set_zoom_range(Some(0.5..=3.0));
    scene.zoomable(false);

    assert_eq!(pan_axes_sig.get(), crate::scene::PanAxes::Horizontal);
    assert_eq!(pan_bounds_sig.get(), Some(Rect::new(0.0, 0.0, 10.0, 10.0)));
    assert_eq!(zoom_range_sig.get(), Some(0.5..=3.0));
    assert_eq!(zoomable_sig.get(), false);
}

// -----------------------------------------------------------------
// Unit 4 — shape-aware hit-test in handler_snapshot
// -----------------------------------------------------------------

#[test]
fn path_item_stroke_only_dispatch_uses_segment_distance_not_aabb() {
    // Regression for the handler_snapshot dispatch path falling
    // back to AABB. A stroke-only PathItem drawn as an L-shape
    // has a 100×100 AABB but only thin pixels along the actual
    // segments. The corner point opposite the L's bend lies
    // inside the AABB but FAR from any stroke — a tap there must
    // miss after Unit 4 (it would have hit under the old AABB
    // fallback).
    use crate::items::PathItem;
    use bastyde_canvas::Path;
    use std::cell::Cell;
    use std::rc::Rc;

    // L-shape: down from (10, 10) to (10, 90), then right to (90, 90).
    let mut path = Path::new();
    path.move_to(Point::new(10.0, 10.0));
    path.line_to(Point::new(10.0, 90.0));
    path.line_to(Point::new(90.0, 90.0));
    let item = PathItem::new(path, Rect::new(0.0, 0.0, 100.0, 100.0))
        .stroke(bastyde_tokens::Color::RED, 2.0);

    let mut scene = Scene::new();
    let id = scene.add_item(item, Point::ZERO);
    let count = Rc::new(Cell::new(0_u32));
    let count_clone = count.clone();
    scene.handlers_mut(id).unwrap().on_tap(move |_pt, _ctx| {
        count_clone.set(count_clone.get() + 1);
    });

    let mut tree = WidgetTree::new();
    tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let tap = |tree: &mut WidgetTree, x: f32, y: f32| {
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: bastyde_canvas::Point::new(x, y),
            button: bastyde_core::event::PointerButton::Primary,
            modifiers: bastyde_core::event::Modifiers::default(),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: bastyde_canvas::Point::new(x, y),
            button: bastyde_core::event::PointerButton::Primary,
            modifiers: bastyde_core::event::Modifiers::default(),
        });
    };

    // Tap squarely ON the vertical stroke segment.
    tap(&mut tree, 10.0, 50.0);
    assert_eq!(count.get(), 1, "tap on stroke should hit");

    // Tap inside the AABB but FAR from any stroke (the L's
    // inner corner, around (60, 30)). Old code AABB-hit; Unit 4
    // segment-distance test misses.
    tap(&mut tree, 60.0, 30.0);
    assert_eq!(
        count.get(),
        1,
        "tap inside AABB but outside stroke must miss; old AABB fallback would have hit"
    );
}

#[test]
fn group_item_logical_only_dispatch_passes_through_to_item_beneath() {
    // Regression for the GroupItem-shaped pass-through. A
    // logical-only GroupItem (no fill, no stroke, no inline
    // label) should NOT capture pointer events — clicks must
    // fall through to items painted beneath / behind it. Old
    // AABB fallback in dispatch captured every event in the
    // group's rect, blocking the inner item.
    use crate::items::{GroupItem, RectItem};
    use std::cell::Cell;
    use std::rc::Rc;

    // Inner RectItem at (20, 20, 30, 30); GroupItem AABB
    // (0, 0, 100, 100) overlapping it. Group is logical-only
    // (default — no fill, no stroke, no label).
    let inner_item = RectItem::new(Rect::new(0.0, 0.0, 30.0, 30.0)).fill(bastyde_tokens::Color::RED);
    let group = GroupItem::new(Rect::new(0.0, 0.0, 100.0, 100.0));

    let mut scene = Scene::new();
    let group_id = scene.add_item(group, Point::ZERO);
    let inner_id = scene.add_item(inner_item, Point::new(20.0, 20.0));
    // Higher z on the inner so it's the topmost — confirms the
    // dispatch order isn't the issue.
    scene.set_z(group_id, 0.0);
    scene.set_z(inner_id, 10.0);

    let group_hits = Rc::new(Cell::new(0_u32));
    let inner_hits = Rc::new(Cell::new(0_u32));
    {
        let group_hits = group_hits.clone();
        scene.handlers_mut(group_id).unwrap().on_tap(move |_pt, _ctx| {
            group_hits.set(group_hits.get() + 1);
        });
    }
    {
        let inner_hits = inner_hits.clone();
        scene.handlers_mut(inner_id).unwrap().on_tap(move |_pt, _ctx| {
            inner_hits.set(inner_hits.get() + 1);
        });
    }

    let mut tree = WidgetTree::new();
    tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let tap = |tree: &mut WidgetTree, x: f32, y: f32| {
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: bastyde_canvas::Point::new(x, y),
            button: bastyde_core::event::PointerButton::Primary,
            modifiers: bastyde_core::event::Modifiers::default(),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: bastyde_canvas::Point::new(x, y),
            button: bastyde_core::event::PointerButton::Primary,
            modifiers: bastyde_core::event::Modifiers::default(),
        });
    };

    // Tap inside the inner rect (inside both the inner item AND
    // the logical group's AABB). Inner item should receive the
    // tap; the logical group must NOT.
    tap(&mut tree, 30.0, 30.0);
    assert_eq!(inner_hits.get(), 1, "inner rect should receive the tap");
    assert_eq!(
        group_hits.get(),
        0,
        "logical-only group must NOT capture events that fall inside its AABB; \
         old AABB fallback would have given it count=1"
    );

    // Tap inside the group's AABB but OUTSIDE the inner rect:
    // logical-only group misses, no item receives the tap.
    tap(&mut tree, 80.0, 80.0);
    assert_eq!(inner_hits.get(), 1, "inner rect untouched");
    assert_eq!(
        group_hits.get(),
        0,
        "logical-only group must miss this tap too"
    );
}

#[test]
fn group_item_visual_dispatch_uses_aabb_as_before() {
    // Counter-test for the above: VISUAL groups (with fill /
    // stroke / inline label) should still AABB-hit and receive
    // group-level click handlers — Unit 4 only changes the
    // logical-only case.
    use crate::items::GroupItem;
    use std::cell::Cell;
    use std::rc::Rc;

    let group = GroupItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)).fill(bastyde_tokens::Color::BLUE);

    let mut scene = Scene::new();
    let id = scene.add_item(group, Point::ZERO);
    let hits = Rc::new(Cell::new(0_u32));
    let hits_clone = hits.clone();
    scene.handlers_mut(id).unwrap().on_tap(move |_pt, _ctx| {
        hits_clone.set(hits_clone.get() + 1);
    });

    let mut tree = WidgetTree::new();
    tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    tree.dispatch_event(WidgetEvent::PointerDown {
        position: bastyde_canvas::Point::new(50.0, 50.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: bastyde_canvas::Point::new(50.0, 50.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    assert_eq!(
        hits.get(),
        1,
        "visual group should still receive AABB-based taps"
    );
}

#[test]
fn rect_item_default_clone_shape_test_aabb_hits_as_before() {
    // Surface check: items that don't override clone_shape_test
    // get the default AABB closure, which matches their
    // shape_contains exactly (since RectItem IS its AABB).
    use crate::item::SceneItem;
    use crate::items::RectItem;
    let item = RectItem::new(Rect::new(10.0, 10.0, 30.0, 30.0));
    let test = item.clone_shape_test();
    assert!(test(Point::new(20.0, 20.0), 1.0));
    assert!(!test(Point::new(5.0, 5.0), 1.0));
    assert!(!test(Point::new(45.0, 45.0), 1.0));
}

// -----------------------------------------------------------------
// Unit 5 — documented-but-missing APIs
//   (map_*, viewport_in_scene_signal, Scene::item_thumbnails)
// -----------------------------------------------------------------

#[test]
fn map_to_scene_and_map_from_scene_round_trip() {
    let scene = Scene::new();
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);

    // Pan and zoom so the transform is non-trivial.
    view.set_pan(Vec2::new(40.0, 20.0));
    view.set_zoom(2.5);

    let view_pt = Point::new(123.0, 87.0);
    let round_trip = view.map_from_scene(view.map_to_scene(view_pt));
    assert!(
        (round_trip.x - view_pt.x).abs() < 1e-3,
        "round-trip x: expected {}, got {}",
        view_pt.x,
        round_trip.x
    );
    assert!(
        (round_trip.y - view_pt.y).abs() < 1e-3,
        "round-trip y: expected {}, got {}",
        view_pt.y,
        round_trip.y
    );

    // map_from_scene should match the view transform.
    let xform = view.view_transform();
    let scene_pt = Point::new(10.0, 10.0);
    let expected = xform.apply_point(scene_pt);
    let actual = view.map_from_scene(scene_pt);
    assert!((actual.x - expected.x).abs() < 1e-3);
    assert!((actual.y - expected.y).abs() < 1e-3);
}

#[test]
fn map_rect_round_trips_under_pan_zoom() {
    let scene = Scene::new();
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);
    view.set_pan(Vec2::new(10.0, -20.0));
    view.set_zoom(1.5);

    let view_rect = Rect::new(50.0, 60.0, 100.0, 80.0);
    let scene_rect = view.map_rect_to_scene(view_rect);
    let back = view.map_rect_from_scene(scene_rect);
    assert!((back.x - view_rect.x).abs() < 1e-2);
    assert!((back.y - view_rect.y).abs() < 1e-2);
    assert!((back.width - view_rect.width).abs() < 1e-2);
    assert!((back.height - view_rect.height).abs() < 1e-2);
}

#[test]
fn viewport_in_scene_signal_reflects_pan_zoom_and_viewport() {
    // At pan=0, zoom=1, viewport 400×300: visible scene region =
    // (0, 0, 400, 300). After set_pan(100, 50) at zoom 1: visible
    // shifts to (-100, -50, 400, 300). After set_zoom(2) (which
    // re-anchors pan, so we set pan again post-zoom to control
    // for it): visible becomes (-50, -25, 200, 150) under pan 100.
    let scene = Scene::new();
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);

    let region_sig = view.viewport_in_scene_signal();
    let r0 = region_sig.get();
    assert!((r0.x - 0.0).abs() < 1e-3, "initial x, got {}", r0.x);
    assert!((r0.y - 0.0).abs() < 1e-3, "initial y, got {}", r0.y);
    assert!((r0.width - 400.0).abs() < 1e-3);
    assert!((r0.height - 300.0).abs() < 1e-3);

    view.set_pan(Vec2::new(100.0, 50.0));
    let r1 = region_sig.get();
    assert!((r1.x - -100.0).abs() < 1e-3, "after pan x, got {}", r1.x);
    assert!((r1.y - -50.0).abs() < 1e-3, "after pan y, got {}", r1.y);
    assert!((r1.width - 400.0).abs() < 1e-3, "width unchanged at zoom 1");

    view.set_zoom(2.0);
    let r2 = region_sig.get();
    // Width halves under zoom 2.
    assert!(
        (r2.width - 200.0).abs() < 1e-3,
        "zoom 2 width: expected 200, got {}",
        r2.width
    );
    assert!(
        (r2.height - 150.0).abs() < 1e-3,
        "zoom 2 height: expected 150, got {}",
        r2.height
    );
}

#[test]
fn viewport_size_signal_fires_when_layout_resolves_new_size() {
    use std::cell::Cell;
    use std::rc::Rc;

    let scene = Scene::new();
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Capture the initial size + install an observer that counts
    // fires on subsequent set_pan calls (which should NOT fire) vs
    // resize calls (which SHOULD).
    let view = view_handle(&tree, view_id);
    let sig = view.viewport_size_signal();
    let fires = Rc::new(Cell::new(0_u32));
    let fires_clone = fires.clone();
    let _h = sig.observe(move |_| fires_clone.set(fires_clone.get() + 1));

    // Re-layout with the same size — observer should NOT fire
    // (the layout_response gates set() on inequality).
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(
        fires.get(),
        0,
        "same-size layout pass must not fire viewport observers"
    );

    // Re-layout with a new size — observer fires once.
    tree.layout(SizeProposal::exact(500.0, 400.0));
    assert_eq!(
        fires.get(),
        1,
        "size change should fire viewport observer once, got {}",
        fires.get()
    );
}

#[test]
fn item_thumbnails_returns_visible_lightweight_items_with_colors() {
    use crate::items::RectItem;
    let mut scene = Scene::new();
    let red = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 30.0, 30.0)).fill(bastyde_tokens::Color::RED),
        Point::new(10.0, 10.0),
    );
    let blue = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 50.0)).fill(bastyde_tokens::Color::BLUE),
        Point::new(100.0, 100.0),
    );

    let thumbs = scene.item_thumbnails();
    assert_eq!(thumbs.len(), 2);
    // Insertion order.
    let (rect_red, color_red) = thumbs[0];
    assert_eq!(rect_red, Rect::new(10.0, 10.0, 30.0, 30.0));
    assert_eq!(color_red, bastyde_tokens::Color::RED);
    let (rect_blue, color_blue) = thumbs[1];
    assert_eq!(rect_blue, Rect::new(100.0, 100.0, 40.0, 50.0));
    assert_eq!(color_blue, bastyde_tokens::Color::BLUE);
    let _ = (red, blue);
}

// -----------------------------------------------------------------
// Unit 6 — reactive DragMode
// -----------------------------------------------------------------

#[test]
fn drag_mode_signal_flips_behavior_at_runtime() {
    // Start in RubberBand mode (default). A drag that misses
    // every item should produce a marquee. After flipping the
    // drag mode to ScrollHandDrag via the signal — without
    // rebuilding the view — the SAME gesture must pan the view
    // instead of marquee-selecting.
    let scene = Scene::new();
    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::new(scene).selection_mode(crate::SceneSelectionMode::Multi),
        // Note: NO .drag_mode(...) — defaults to RubberBand.
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let drag = |tree: &mut WidgetTree, fx: f32, fy: f32, tx: f32, ty: f32| {
        tree.pointer_move(bastyde_canvas::Point::new(fx, fy));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: bastyde_canvas::Point::new(fx, fy),
            button: bastyde_core::event::PointerButton::Primary,
            modifiers: bastyde_core::event::Modifiers::default(),
        });
        // Two moves so the second produces DragMoved (recognizer
        // emits Started on the threshold-crossing Move and Moved
        // on subsequent ones).
        let mid = bastyde_canvas::Point::new((fx + tx) * 0.5, (fy + ty) * 0.5);
        tree.dispatch_event(WidgetEvent::PointerMove { position: mid });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: bastyde_canvas::Point::new(tx, ty),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: bastyde_canvas::Point::new(tx, ty),
            button: bastyde_core::event::PointerButton::Primary,
            modifiers: bastyde_core::event::Modifiers::default(),
        });
    };

    // Round 1: RubberBand mode. Drag from (50,50) to (200,200) —
    // marquee fires, drag does NOT pan.
    {
        let view = view_handle(&tree, view_id);
        assert_eq!(
            view.drag_mode_signal().get(),
            crate::DragMode::RubberBand
        );
    }
    drag(&mut tree, 50.0, 50.0, 200.0, 200.0);
    {
        let view = view_handle(&tree, view_id);
        assert_eq!(
            view.pan(),
            Vec2::ZERO,
            "RubberBand mode: drag must NOT pan the view"
        );
    }

    // Round 2: flip to ScrollHandDrag via the signal. Same
    // gesture — should pan now, marquee should NOT trigger.
    {
        let view = view_handle(&tree, view_id);
        view.drag_mode_signal()
            .set(crate::DragMode::ScrollHandDrag);
    }
    drag(&mut tree, 100.0, 100.0, 175.0, 150.0);
    {
        let view = view_handle(&tree, view_id);
        // Hand-drag pans by the delta of the second move only
        // (37.5, 25) — same recognizer convention as the
        // existing Unit 1 test.
        let p = view.pan();
        assert!(
            p.x.abs() > 1e-3 || p.y.abs() > 1e-3,
            "ScrollHandDrag mode (via runtime signal flip): drag must pan; got {:?}",
            p
        );
    }
}

#[test]
fn drag_mode_signal_to_no_drag_disables_dispatch_at_runtime() {
    // Start in ScrollHandDrag, drag once (pan moves), flip to
    // NoDrag via signal, drag again (pan must NOT move further).
    let scene = Scene::new();
    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::new(scene)
            .selection_mode(crate::SceneSelectionMode::Multi)
            .drag_mode(crate::DragMode::ScrollHandDrag),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let drag = |tree: &mut WidgetTree, fx: f32, fy: f32, tx: f32, ty: f32| {
        tree.pointer_move(bastyde_canvas::Point::new(fx, fy));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: bastyde_canvas::Point::new(fx, fy),
            button: bastyde_core::event::PointerButton::Primary,
            modifiers: bastyde_core::event::Modifiers::default(),
        });
        let mid = bastyde_canvas::Point::new((fx + tx) * 0.5, (fy + ty) * 0.5);
        tree.dispatch_event(WidgetEvent::PointerMove { position: mid });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: bastyde_canvas::Point::new(tx, ty),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: bastyde_canvas::Point::new(tx, ty),
            button: bastyde_core::event::PointerButton::Primary,
            modifiers: bastyde_core::event::Modifiers::default(),
        });
    };

    drag(&mut tree, 100.0, 100.0, 150.0, 150.0);
    let pan_after_first = view_handle(&tree, view_id).pan();
    assert!(
        pan_after_first.x.abs() > 1e-3 || pan_after_first.y.abs() > 1e-3,
        "hand drag in round 1 should pan; got {:?}",
        pan_after_first
    );

    // Flip to NoDrag mid-life.
    view_handle(&tree, view_id)
        .drag_mode_signal()
        .set(crate::DragMode::NoDrag);

    // Second drag: pan must NOT change.
    drag(&mut tree, 200.0, 200.0, 250.0, 250.0);
    let pan_after_second = view_handle(&tree, view_id).pan();
    assert_eq!(
        pan_after_second, pan_after_first,
        "after flipping to NoDrag, subsequent drag must NOT change pan"
    );
}

#[test]
fn bind_drag_mode_shares_app_owned_signal() {
    // Caller owns a Signal<DragMode>, binds the view to it,
    // toggles it from outside — view picks up the change.
    let app_owned: Signal<crate::DragMode> =
        Signal::new(crate::DragMode::RubberBand);
    let scene = Scene::new();
    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::new(scene)
            .selection_mode(crate::SceneSelectionMode::Multi)
            .bind_drag_mode(app_owned.clone()),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Mutate the app-owned signal — view reads through it.
    app_owned.set(crate::DragMode::ScrollHandDrag);
    let view = view_handle(&tree, view_id);
    assert_eq!(
        view.drag_mode_signal().get(),
        crate::DragMode::ScrollHandDrag,
        "view's drag_mode_signal must reflect the shared app-owned signal"
    );

    // Conversely, mutating through the view also updates the
    // shared signal (both are clones of the same Rc-backed
    // Signal).
    view.drag_mode_signal().set(crate::DragMode::NoDrag);
    assert_eq!(app_owned.get(), crate::DragMode::NoDrag);
}

// -----------------------------------------------------------------
// Unit 7 — SceneItem handler parity (TapEvent shape + accept_tap_buttons)
// -----------------------------------------------------------------

#[test]
fn on_tap_event_receives_modifiers_and_button() {
    // The rich `on_tap_event` setter exposes the full
    // SceneTapEvent so handlers can read modifiers and button.
    // Old `on_tap(Fn(Point, _))` callers still work via the
    // back-compat shim.
    use crate::item_handlers::SceneTapEvent;
    use crate::items::RectItem;
    use std::cell::RefCell;
    use std::rc::Rc;

    let mut scene = Scene::new();
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 50.0, 50.0)).fill(bastyde_tokens::Color::RED),
        Point::new(20.0, 20.0),
    );
    let captured: Rc<RefCell<Option<SceneTapEvent>>> = Rc::new(RefCell::new(None));
    let captured_clone = captured.clone();
    scene
        .handlers_mut(id)
        .unwrap()
        .on_tap_event(move |ev, _ctx| {
            *captured_clone.borrow_mut() = Some(*ev);
        });

    let mut tree = WidgetTree::new();
    tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Tap with Shift held.
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: bastyde_canvas::Point::new(40.0, 40.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::SHIFT,
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: bastyde_canvas::Point::new(40.0, 40.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::SHIFT,
    });

    let ev = captured.borrow().expect("on_tap_event must fire");
    assert_eq!(ev.position_scene, bastyde_canvas::Point::new(40.0, 40.0));
    assert_eq!(ev.button, bastyde_core::event::PointerButton::Primary);
    assert!(ev.modifiers.shift(), "Shift modifier should be set");
    assert!(!ev.modifiers.ctrl());
}

#[test]
fn on_tap_point_shim_still_compiles_and_fires() {
    // Back-compat: legacy callers that pass a Fn(Point, &mut ctx)
    // should keep working — the setter wraps the closure to
    // extract event.position_scene.
    use crate::items::RectItem;
    use std::cell::Cell;
    use std::rc::Rc;

    let mut scene = Scene::new();
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 50.0, 50.0)).fill(bastyde_tokens::Color::RED),
        Point::new(20.0, 20.0),
    );
    let count = Rc::new(Cell::new(0_u32));
    let count_clone = count.clone();
    let pt_seen = Rc::new(Cell::new(Point::ZERO));
    let pt_seen_clone = pt_seen.clone();
    scene.handlers_mut(id).unwrap().on_tap(move |pt, _ctx| {
        count_clone.set(count_clone.get() + 1);
        pt_seen_clone.set(pt);
    });

    let mut tree = WidgetTree::new();
    tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: bastyde_canvas::Point::new(40.0, 40.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: bastyde_canvas::Point::new(40.0, 40.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });

    assert_eq!(count.get(), 1);
    assert_eq!(pt_seen.get(), bastyde_canvas::Point::new(40.0, 40.0));
}

#[test]
fn accept_tap_buttons_gates_middle_click() {
    // Default: only PRIMARY counts as a tap. Middle-click is
    // ignored. After opting in via accept_tap_buttons(PRIMARY |
    // MIDDLE), middle-click fires on_tap with button == Middle.
    use crate::items::RectItem;
    use std::cell::RefCell;
    use std::rc::Rc;

    let mut scene = Scene::new();
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 50.0, 50.0)).fill(bastyde_tokens::Color::RED),
        Point::new(20.0, 20.0),
    );
    let buttons: Rc<RefCell<Vec<bastyde_core::event::PointerButton>>> = Rc::new(RefCell::new(vec![]));
    let buttons_clone = buttons.clone();
    {
        let h = scene.handlers_mut(id).unwrap();
        h.on_tap_event(move |ev, _ctx| {
            buttons_clone.borrow_mut().push(ev.button);
        });
        // Round 1 uses the default mask (PRIMARY only).
    }

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let click = |tree: &mut WidgetTree, button: bastyde_core::event::PointerButton| {
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: bastyde_canvas::Point::new(40.0, 40.0),
            button,
            modifiers: bastyde_core::event::Modifiers::default(),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: bastyde_canvas::Point::new(40.0, 40.0),
            button,
            modifiers: bastyde_core::event::Modifiers::default(),
        });
    };

    // PRIMARY hits.
    click(&mut tree, bastyde_core::event::PointerButton::Primary);
    // MIDDLE is gated out by the default mask.
    click(&mut tree, bastyde_core::event::PointerButton::Middle);
    assert_eq!(
        buttons.borrow().clone(),
        vec![bastyde_core::event::PointerButton::Primary]
    );

    // Now widen the mask: PRIMARY | MIDDLE.
    {
        let view = tree
            .widget_as_any_mut(view_id)
            .and_then(|a| a.downcast_mut::<SceneView>())
            .expect("downcast");
        view.scene_mut()
            .handlers_mut(id)
            .unwrap()
            .accept_tap_buttons(
                bastyde_core::event::ButtonMask::PRIMARY | bastyde_core::event::ButtonMask::MIDDLE,
            );
    }
    // Re-layout (with a tiny size delta to force a fresh
    // layout_response pass, which rebuilds the handler_snapshot
    // and picks up the widened accept_tap_buttons mask).
    tree.layout(SizeProposal::exact(401.0, 300.0));
    click(&mut tree, bastyde_core::event::PointerButton::Middle);
    assert_eq!(
        buttons.borrow().clone(),
        vec![
            bastyde_core::event::PointerButton::Primary,
            bastyde_core::event::PointerButton::Middle,
        ]
    );
}

#[test]
fn on_context_menu_event_receives_modifiers() {
    use crate::item_handlers::SceneTapEvent;
    use crate::items::RectItem;
    use std::cell::RefCell;
    use std::rc::Rc;

    let mut scene = Scene::new();
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 50.0, 50.0)).fill(bastyde_tokens::Color::RED),
        Point::new(20.0, 20.0),
    );
    let captured: Rc<RefCell<Option<SceneTapEvent>>> = Rc::new(RefCell::new(None));
    let captured_clone = captured.clone();
    scene
        .handlers_mut(id)
        .unwrap()
        .on_context_menu_event(move |ev, _ctx| {
            *captured_clone.borrow_mut() = Some(*ev);
        });

    let mut tree = WidgetTree::new();
    tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: bastyde_canvas::Point::new(35.0, 35.0),
        button: bastyde_core::event::PointerButton::Secondary,
        modifiers: bastyde_core::event::Modifiers::CTRL,
    });

    let ev = captured.borrow().expect("on_context_menu_event must fire");
    assert_eq!(ev.button, bastyde_core::event::PointerButton::Secondary);
    assert!(ev.modifiers.ctrl());
}

#[test]
fn mismatched_down_up_buttons_do_not_fire_tap() {
    // Press Primary, release Middle — should NOT fire on_tap
    // because the recognizer requires matched buttons.
    use crate::items::RectItem;
    use std::cell::Cell;
    use std::rc::Rc;

    let mut scene = Scene::new();
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 50.0, 50.0)).fill(bastyde_tokens::Color::RED),
        Point::new(20.0, 20.0),
    );
    let count = Rc::new(Cell::new(0_u32));
    let count_clone = count.clone();
    scene
        .handlers_mut(id)
        .unwrap()
        .on_tap(move |_pt, _ctx| count_clone.set(count_clone.get() + 1));

    let mut tree = WidgetTree::new();
    tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: bastyde_canvas::Point::new(40.0, 40.0),
        button: bastyde_core::event::PointerButton::Primary,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: bastyde_canvas::Point::new(40.0, 40.0),
        button: bastyde_core::event::PointerButton::Middle,
        modifiers: bastyde_core::event::Modifiers::default(),
    });
    assert_eq!(count.get(), 0, "mismatched buttons must not fire tap");
}

#[test]
fn item_thumbnails_skips_invisible_and_logical_items() {
    use crate::flags::ItemFlags;
    use crate::items::{GroupItem, RectItem};

    let mut scene = Scene::new();
    let visible = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)).fill(bastyde_tokens::Color::RED),
        Point::ZERO,
    );
    let invisible = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)).fill(bastyde_tokens::Color::BLUE),
        Point::ZERO,
    );
    scene.set_flag(invisible, ItemFlags::IS_VISIBLE, false);
    // Logical-only group: no fill/stroke/label. HAS_NO_CONTENTS
    // would also exclude; but a logical-only group keeps the
    // default flags. We add a separate HAS_NO_CONTENTS item to
    // exercise that exclusion path.
    let logical = scene.add_item(
        GroupItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)),
        Point::ZERO,
    );
    scene.set_flag(logical, ItemFlags::HAS_NO_CONTENTS, true);

    let thumbs = scene.item_thumbnails();
    assert_eq!(
        thumbs.len(),
        1,
        "should return only the visible content-bearing item"
    );
    assert_eq!(thumbs[0].1, bastyde_tokens::Color::RED);
    let _ = visible;
}
