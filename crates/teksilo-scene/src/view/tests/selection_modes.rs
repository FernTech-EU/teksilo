// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The four Qt selection modes driven through the real marquee: the rubber
//! band picks items by the same geometry a click does, a rotated view uses the
//! band's exact quadrilateral instead of its enlarged hull, and the
//! accessibility tree is untouched by any of it.

use std::collections::BTreeSet;

use teksilo_canvas::{Path, Point, Rect, SizeProposal, Transform2D};
use teksilo_core::event::WidgetEvent;
use teksilo_core::event::{Modifiers, PointerButton};
use teksilo_core::widget_tree::WidgetTree;

use crate::item::ItemId;
use crate::items::{PathItem, RectItem};
use crate::scene::Scene;
use crate::selection::{SceneSelection, SceneSelectionMode};
use crate::shape::{ItemSelectionMode, SceneRegion};
use crate::view::SceneView;

fn diagonal_wire(scene: &mut Scene, from: Point, to: Point) -> ItemId {
    let mut path = Path::new();
    path.move_to(from).line_to(to);
    scene.add_item(
        PathItem::new(path).stroke(teksilo_tokens::Color::BLACK, 2.0),
        Point::ZERO,
    )
}

fn selection_of(scene: &Scene, region: &SceneRegion, mode: ItemSelectionMode) -> BTreeSet<ItemId> {
    let sel = SceneSelection::new(SceneSelectionMode::Multi);
    sel.commit_marquee_region(scene, region, mode, 1.0, false);
    sel.selection_signal().get()
}

// ---------------------------------------------------------------------------
// The default mode, and what it changed
// ---------------------------------------------------------------------------

#[test]
fn a_band_that_only_grazes_a_wires_box_no_longer_selects_the_wire() {
    // The incoherence this track exists to remove: clicking the empty corner
    // of a connector's bounding box misses it (per-segment distance), while a
    // marquee grazing the same corner used to select it (AABB).
    let mut scene = Scene::new();
    let wire = diagonal_wire(&mut scene, Point::new(0.0, 0.0), Point::new(200.0, 200.0));

    // A small band on the wire's bottom-left corner: inside its box, 100 units
    // from the stroke.
    let corner = SceneRegion::rect(Rect::new(0.0, 180.0, 20.0, 20.0));
    assert!(
        !selection_of(&scene, &corner, ItemSelectionMode::IntersectsItemShape).contains(&wire),
        "the corner of the box is not the wire"
    );
    assert!(
        selection_of(
            &scene,
            &corner,
            ItemSelectionMode::IntersectsItemBoundingRect
        )
        .contains(&wire),
        "the pre-ItemShape rule is still reachable by name"
    );

    // Crossing the stroke selects it under every mode that asks about overlap.
    let across = SceneRegion::rect(Rect::new(90.0, 90.0, 20.0, 20.0));
    assert!(selection_of(&scene, &across, ItemSelectionMode::IntersectsItemShape).contains(&wire));
}

#[test]
fn contains_modes_demand_the_whole_item() {
    let mut scene = Scene::new();
    let tile = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
        Point::new(50.0, 50.0),
    );
    let half = SceneRegion::rect(Rect::new(0.0, 0.0, 70.0, 200.0));
    let whole = SceneRegion::rect(Rect::new(0.0, 0.0, 200.0, 200.0));
    for mode in [
        ItemSelectionMode::ContainsItemShape,
        ItemSelectionMode::ContainsItemBoundingRect,
    ] {
        assert!(
            !selection_of(&scene, &half, mode).contains(&tile),
            "{mode:?}"
        );
        assert!(
            selection_of(&scene, &whole, mode).contains(&tile),
            "{mode:?}"
        );
    }
    for mode in [
        ItemSelectionMode::IntersectsItemShape,
        ItemSelectionMode::IntersectsItemBoundingRect,
    ] {
        assert!(
            selection_of(&scene, &half, mode).contains(&tile),
            "{mode:?}"
        );
    }
}

#[test]
fn an_untransformed_default_shape_item_selects_exactly_as_it_always_did() {
    // The compatibility claim, stated as a test: for the overwhelmingly common
    // case the two modes are the same answer, edge-exclusivity included.
    let mut scene = Scene::new();
    let tile = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)).fill(teksilo_tokens::Color::RED),
        Point::new(10.0, 10.0),
    );
    for band in [
        Rect::new(0.0, 0.0, 100.0, 100.0),
        Rect::new(15.0, 15.0, 2.0, 2.0),
        Rect::new(0.0, 0.0, 10.0, 10.0), // edge-touching: excluded, as before
        Rect::new(500.0, 500.0, 10.0, 10.0),
    ] {
        let region = SceneRegion::rect(band);
        let by_shape = selection_of(&scene, &region, ItemSelectionMode::IntersectsItemShape);
        let by_box = selection_of(
            &scene,
            &region,
            ItemSelectionMode::IntersectsItemBoundingRect,
        );
        let legacy = scene.items_in_rect(band).contains(&tile);
        assert_eq!(by_shape.contains(&tile), legacy, "shape mode vs {band:?}");
        assert_eq!(by_box.contains(&tile), legacy, "box mode vs {band:?}");
    }
}

#[test]
fn a_lasso_selects_what_it_encircles() {
    let mut scene = Scene::new();
    let inside = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)).fill(teksilo_tokens::Color::RED),
        Point::new(10.0, 10.0),
    );
    let merely_boxed = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)).fill(teksilo_tokens::Color::BLUE),
        Point::new(85.0, 85.0),
    );
    let lasso = SceneRegion::lasso(Path::polygon(&[
        Point::new(0.0, 0.0),
        Point::new(100.0, 0.0),
        Point::new(0.0, 100.0),
    ]));
    let picked = selection_of(&scene, &lasso, ItemSelectionMode::IntersectsItemShape);
    assert!(picked.contains(&inside));
    assert!(
        !picked.contains(&merely_boxed),
        "the loop's bounding box is not the loop"
    );
}

// ---------------------------------------------------------------------------
// The rotated view
// ---------------------------------------------------------------------------

#[test]
fn a_marquee_under_a_rotated_view_uses_its_exact_quadrilateral() {
    // `inv.apply_rect(screen_rect)` gives the AABB of the rotated band, which
    // over-selects everything in the enlarged hull. `SceneRegion` keeps the
    // corners.
    let screen_band = Rect::new(0.0, 0.0, 100.0, 100.0);
    let rotate = Transform2D::rotate(std::f32::consts::FRAC_PI_4);
    let inv = rotate.inverse().expect("a rotation is invertible");

    let hull = inv.apply_rect(screen_band);
    let exact = SceneRegion::from_screen_rect(screen_band, &inv);
    assert!(
        exact.as_rect().is_none(),
        "a rotated band is not a rectangle"
    );

    let mut scene = Scene::new();
    // A tile parked in the hull's corner, outside the rotated band.
    let in_the_hull_only = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 4.0, 4.0)).fill(teksilo_tokens::Color::RED),
        Point::new(hull.x + 1.0, hull.y + 1.0),
    );
    // And one at the band's centre.
    let centred = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 4.0, 4.0)).fill(teksilo_tokens::Color::BLUE),
        inv.apply_point(Point::new(50.0, 50.0)),
    );

    let picked = selection_of(&scene, &exact, ItemSelectionMode::IntersectsItemShape);
    assert!(picked.contains(&centred));
    assert!(
        !picked.contains(&in_the_hull_only),
        "the hull's corner is outside the band the user actually drew"
    );
    // The old behaviour, for contrast.
    let hull_region = SceneRegion::rect(hull);
    assert!(
        selection_of(&scene, &hull_region, ItemSelectionMode::IntersectsItemShape)
            .contains(&in_the_hull_only)
    );
}

// ---------------------------------------------------------------------------
// Through the live view
// ---------------------------------------------------------------------------

#[test]
fn the_views_marquee_mode_reaches_the_commit() {
    fn drag_marquee(mode: ItemSelectionMode) -> bool {
        let mut scene = Scene::new();
        let wire = diagonal_wire(&mut scene, Point::new(0.0, 0.0), Point::new(200.0, 200.0));
        let mut tree = WidgetTree::new();
        let view_id = tree.add(
            SceneView::new(scene)
                .selection_mode(SceneSelectionMode::Multi)
                .marquee_selection_mode(mode),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Drag a small band over the wire's bottom-left box corner.
        tree.pointer_move(Point::new(2.0, 180.0));
        tree.dispatch_event(WidgetEvent::pointer_down(
            Point::new(2.0, 180.0),
            PointerButton::Primary,
            Modifiers::default(),
        ));
        tree.dispatch_event(WidgetEvent::pointer_move(Point::new(20.0, 198.0)));
        tree.dispatch_event(WidgetEvent::pointer_up(
            Point::new(20.0, 198.0),
            PointerButton::Primary,
            Modifiers::default(),
        ));
        let view = super::view_handle(&tree, view_id);
        view.flush_marquee_commit();
        view.selection().selection_signal().get().contains(&wire)
    }

    assert!(
        !drag_marquee(ItemSelectionMode::IntersectsItemShape),
        "the default mode asks about the stroke"
    );
    assert!(
        drag_marquee(ItemSelectionMode::IntersectsItemBoundingRect),
        "the opt-out asks about the box"
    );
}

// ---------------------------------------------------------------------------
// Accessibility is untouched
// ---------------------------------------------------------------------------

#[test]
fn a_narrower_shape_changes_no_at_node_and_no_advertised_bounds() {
    // Hit geometry and the accessibility tree are separate walks, and this
    // pins that: a rounded tile whose transparent corners are no longer
    // clickable keeps its AT node, its label and its full advertised box.
    // Nothing in `a11y.rs` or `view/a11y_impl.rs` reads a shape, and this is
    // what would redden if that changed.
    let mut scene = Scene::new();
    let rounded = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 60.0, 60.0))
            .fill(teksilo_tokens::Color::RED)
            .corner_radius(20.0)
            .access_label(teksilo_i18n::lit!("rounded tile")),
        Point::new(10.0, 10.0),
    );
    let wire = diagonal_wire(&mut scene, Point::new(0.0, 0.0), Point::new(120.0, 120.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let update = tree.accessibility_tree_snapshot();

    let tile_node = teksilo_core::accessibility::synthetic_node_id(
        view_id,
        rounded.as_u64(),
        teksilo_core::accessibility::SyntheticKind::SceneItem,
    );
    let wire_node = teksilo_core::accessibility::synthetic_node_id(
        view_id,
        wire.as_u64(),
        teksilo_core::accessibility::SyntheticKind::SceneItem,
    );
    let find = |id| {
        update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == id)
            .map(|(_, n)| n)
    };
    let tile = find(tile_node).expect("the rounded tile is still an AT node");
    assert_eq!(tile.label(), Some("rounded tile"));
    assert!(
        find(wire_node).is_some(),
        "a stroke-only wire is still an AT node too"
    );

    // Its advertised box is the whole rectangle: a hit shape that excludes the
    // transparent corners must not shrink what an AT client is told the
    // element covers.
    let bounds = tile.bounds().expect("an AT node has bounds");
    assert!((bounds.x1 - bounds.x0 - 60.0).abs() < 0.01, "{bounds:?}");
    assert!((bounds.y1 - bounds.y0 - 60.0).abs() < 0.01, "{bounds:?}");

    let view = super::view_handle(&tree, view_id);
    let shape = view.scene().item_shape(rounded).expect("has a shape");
    assert_eq!(shape.bounding_rect(), Rect::new(0.0, 0.0, 60.0, 60.0));
    assert!(!shape.contains(Point::new(0.5, 0.5), 1.0), "and yet");

    // Reachability is the thing `WidgetTree::focus(id)` cannot prove.
    let stops = tree.tab_stops_within(view_id);
    assert!(
        stops.contains(&view_id),
        "the SceneView is still a tab stop; stops = {stops:?}"
    );
}
