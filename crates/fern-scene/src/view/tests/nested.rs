//! Coverage for nested `SceneView`s — the chart-style layout
//! where an outer view holds fixed axis chrome and an inner
//! view holds the free-pan / free-zoom data area. Per Unit 3,
//! per-(sub-)scene constraint independence falls out naturally
//! because each nested view carries its own `Scene` with its
//! own `SceneConstraints`. These tests pin that property.
//!
//! Note: we test by building two side-by-side SceneViews with
//! distinct constraints and asserting they don't share state.
//! True embedded-nesting (an inner SceneView added as a
//! heavyweight item inside an outer Scene) requires the full
//! heavyweight-item materialise path, which is tested indirectly
//! through the existing arena machinery. Pure constraint
//! independence is the property we care about here.

use crate::items::RectItem;
use crate::scene::{PanAxes, Scene};
use crate::view::SceneView;
use fern_canvas::{Point, Rect, Vec2};
use fern_canvas::SizeProposal;
use fern_core::widget_id::WidgetId;
use fern_core::widget_tree::WidgetTree;

fn view_handle(tree: &WidgetTree, view_id: WidgetId) -> &SceneView {
    tree.widget_as_any(view_id)
        .and_then(|a| a.downcast_ref::<SceneView>())
        .expect("view is a SceneView")
}

// -----------------------------------------------------------------
// Per-scene constraint independence
// -----------------------------------------------------------------

#[test]
fn two_scenes_have_independent_pan_axes() {
    // Two separate Scenes — one locked vertically, one locked
    // horizontally. Their pan_axes signals don't share state.
    let mut outer_scene = Scene::new();
    outer_scene.pan_axes(PanAxes::Vertical);
    let mut inner_scene = Scene::new();
    inner_scene.pan_axes(PanAxes::Horizontal);

    let mut tree = WidgetTree::new();
    let outer_id = tree.add(SceneView::new(outer_scene));
    let inner_id = tree.add(SceneView::new(inner_scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let outer = view_handle(&tree, outer_id);
    let inner = view_handle(&tree, inner_id);
    assert_eq!(outer.scene().current_pan_axes(), PanAxes::Vertical);
    assert_eq!(inner.scene().current_pan_axes(), PanAxes::Horizontal);

    // Mutating one doesn't ripple to the other.
    outer.scene().pan_axes_signal().set(PanAxes::Both);
    assert_eq!(outer.scene().current_pan_axes(), PanAxes::Both);
    assert_eq!(
        inner.scene().current_pan_axes(),
        PanAxes::Horizontal,
        "inner scene's pan_axes must NOT be affected by outer's change"
    );
}

#[test]
fn two_scenes_have_independent_zoom_state() {
    // Each SceneView has its own zoom signal; setting zoom on
    // one must not affect the other (even at the same default
    // zoom_range_override of Some(0.1..=10.0)).
    let scene_a = Scene::new();
    let scene_b = Scene::new();

    let mut tree = WidgetTree::new();
    let a_id = tree.add(SceneView::new(scene_a));
    let b_id = tree.add(SceneView::new(scene_b));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    {
        let a = view_handle(&tree, a_id);
        a.set_zoom(2.5);
    }
    let a = view_handle(&tree, a_id);
    let b = view_handle(&tree, b_id);
    assert!((a.zoom() - 2.5).abs() < 1e-3);
    assert!(
        (b.zoom() - 1.0).abs() < 1e-3,
        "b's zoom must stay at default 1.0; got {}",
        b.zoom()
    );
}

#[test]
fn two_scenes_have_independent_pan_bounds() {
    // Different pan_bounds rects on two scenes — each view
    // clamps against its own bounds, not the other's.
    let mut scene_a = Scene::new();
    scene_a.set_pan_bounds(Some(Rect::new(0.0, 0.0, 1000.0, 800.0)));
    let mut scene_b = Scene::new();
    scene_b.set_pan_bounds(Some(Rect::new(0.0, 0.0, 200.0, 200.0)));

    let mut tree = WidgetTree::new();
    let a_id = tree.add(SceneView::new(scene_a));
    let b_id = tree.add(SceneView::new(scene_b));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // a tries to pan -1000, -1000. Clamps at the larger bounds.
    {
        let a = view_handle(&tree, a_id);
        a.set_pan(Vec2::new(-9999.0, -9999.0));
    }
    {
        let b = view_handle(&tree, b_id);
        b.set_pan(Vec2::new(-9999.0, -9999.0));
    }

    let a = view_handle(&tree, a_id);
    let b = view_handle(&tree, b_id);
    // a's clamp: pan_x = vp - bounds.right * zoom = 400 - 1000 = -600.
    assert!(
        (a.pan().x - -600.0).abs() < 1e-3,
        "a pan_x clamp, got {}",
        a.pan().x
    );
    // b's bounds 200×200 < viewport 400×300 on both axes:
    // centered. pan_x = vp/2 - center * zoom = 200 - 100 = 100.
    assert!(
        (b.pan().x - 100.0).abs() < 1e-3,
        "b pan_x centered, got {}",
        b.pan().x
    );
}

// -----------------------------------------------------------------
// Chart-shaped: outer fixed, inner free-pan
// -----------------------------------------------------------------

#[test]
fn chart_shaped_outer_fixed_axis_inner_free_pan_data() {
    // Outer view: axis chrome — pan locked entirely, zoom
    // locked. Inner view: free pan + zoom — the data area.
    // Different constraints; mutating the inner shouldn't
    // accidentally unlock the outer.
    let mut outer_scene = Scene::new();
    outer_scene.pan_axes(PanAxes::None); // no user pan
    outer_scene.zoomable(false);
    let mut inner_scene = Scene::new();
    // Inner: default — Both axes, zoomable.
    inner_scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)).fill(fern_tokens::Color::BLUE),
        Point::new(50.0, 50.0),
    );

    let mut tree = WidgetTree::new();
    let outer_id = tree.add(SceneView::new(outer_scene));
    let inner_id = tree.add(SceneView::new(inner_scene));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    let outer = view_handle(&tree, outer_id);
    let inner = view_handle(&tree, inner_id);

    // Try to pan / zoom the OUTER — both should be no-ops.
    let outer_pan_before = outer.pan();
    let outer_zoom_before = outer.zoom();
    outer.set_pan(Vec2::new(100.0, 200.0));
    outer.set_zoom(2.0);
    assert_eq!(outer.pan(), outer_pan_before, "outer pan must be locked");
    assert!(
        (outer.zoom() - outer_zoom_before).abs() < 1e-6,
        "outer zoom must be locked"
    );

    // Pan / zoom INNER — both apply.
    inner.set_pan(Vec2::new(50.0, 75.0));
    inner.set_zoom(2.0);
    assert!((inner.pan().x - 50.0).abs() < 1e-3);
    assert!((inner.pan().y - 75.0).abs() < 1e-3);
    assert!((inner.zoom() - 2.0).abs() < 1e-3);

    // Sanity: outer is still locked after inner moved.
    assert_eq!(outer.pan(), outer_pan_before);
}

// -----------------------------------------------------------------
// Shared Scene via view-level overrides (the converse case)
// -----------------------------------------------------------------

#[test]
fn view_pan_bounds_override_is_per_view_even_with_same_scene() {
    // Conceptually: if two views could share a Scene, each
    // could apply its own pan_bounds_override and they'd pan
    // independently within the same scene-declared range. The
    // Scene type isn't Clone right now, so this test exercises
    // the "two Scenes, same shape, different overrides" proxy
    // — but the property under test is that pan_bounds_override
    // is genuinely view-local and doesn't leak through the
    // Scene's constraint signals.
    let scene_a = Scene::new();
    let scene_b = Scene::new();

    let mut tree = WidgetTree::new();
    let a_id = tree.add(
        SceneView::new(scene_a)
            .pan_bounds_override(Some(Rect::new(0.0, 0.0, 1000.0, 800.0))),
    );
    let b_id = tree.add(
        SceneView::new(scene_b)
            .pan_bounds_override(Some(Rect::new(0.0, 0.0, 200.0, 200.0))),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));

    {
        let a = view_handle(&tree, a_id);
        a.set_pan(Vec2::new(-9999.0, -9999.0));
    }
    {
        let b = view_handle(&tree, b_id);
        b.set_pan(Vec2::new(-9999.0, -9999.0));
    }

    let a = view_handle(&tree, a_id);
    let b = view_handle(&tree, b_id);
    assert!((a.pan().x - -600.0).abs() < 1e-3, "a clamp via override");
    assert!((b.pan().x - 100.0).abs() < 1e-3, "b centered via override");
}
