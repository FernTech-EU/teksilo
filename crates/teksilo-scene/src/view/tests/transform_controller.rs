// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! End-to-end coverage for the selection transform controller: the group move
//! (both tiers), the frame band and handle grabs, resize and rotate, the
//! preview lockstep between the two tiers, cancellation, the keyboard route,
//! the flags it honours, and the accessibility nodes it publishes.
//!
//! Several of these are written to redden when the mechanism is *removed*
//! rather than merely to pass — the flag tests and the preview-lockstep test in
//! particular assert the negative case beside the positive one, because a test
//! that only ever asks "did it move?" cannot tell a working controller from one
//! that moves everything.

use super::*;
use crate::flags::ItemFlags;
use crate::items::RectItem;
use crate::scene_model::SceneModel;
use crate::selection::{SceneSelection, SceneSelectionMode};
use crate::transform_session::{
    LivePreview, TransformConfig, TransformDelta, TransformHandle, TransformHandleSet, TransformOp,
    TransformOutcome,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use teksilo_canvas::Point;
use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};

fn down(tree: &mut WidgetTree, p: Point) {
    tree.pointer_move(p);
    tree.dispatch_event(WidgetEvent::pointer_down(
        p,
        PointerButton::Primary,
        Modifiers::default(),
    ));
}
fn moved(tree: &mut WidgetTree, p: Point) {
    tree.dispatch_event(WidgetEvent::pointer_move(p));
}
fn up(tree: &mut WidgetTree, p: Point) {
    tree.dispatch_event(WidgetEvent::pointer_up(
        p,
        PointerButton::Primary,
        Modifiers::default(),
    ));
}
fn key(tree: &mut WidgetTree, k: Key) {
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: k,
        modifiers: Modifiers::default(),
        text: None,
    });
}
fn key_mod(tree: &mut WidgetTree, k: Key, modifiers: Modifiers) {
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: k,
        modifiers,
        text: None,
    });
}

/// Apply whatever the gesture committed — the drain `build()` runs, reached
/// from a headless driver.
fn flush(tree: &mut WidgetTree, view_id: WidgetId) -> bool {
    let view = tree
        .widget_as_any_mut(view_id)
        .and_then(|a| a.downcast_mut::<SceneView>())
        .expect("downcast");
    view.flush_pending_transform()
}

/// Two draggable, resizable, rotatable lightweight squares 100 apart, plus a
/// shared selection model so a test can select without a pointer.
fn two_square_scene() -> (SceneModel, ItemId, ItemId, SceneSelection) {
    let model = SceneModel::new();
    let a = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
        Point::new(50.0, 50.0),
    );
    let b = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::BLUE),
        Point::new(150.0, 50.0),
    );
    for id in [a, b] {
        model.set_flag(id, ItemFlags::IS_DRAGGABLE, true);
        model.set_flag(id, ItemFlags::IS_RESIZABLE, true);
        model.set_flag(id, ItemFlags::IS_ROTATABLE, true);
    }
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    (model, a, b, selection)
}

fn mount(
    model: SceneModel,
    selection: SceneSelection,
    cfg: TransformConfig,
) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::with_model(model)
            .selection_model(selection)
            .transform_controller(cfg),
    );
    tree.layout(SizeProposal::exact(600.0, 400.0));
    (tree, view_id)
}

// ---------------------------------------------------------------------------
// Group move
// ---------------------------------------------------------------------------

#[test]
fn a_body_drag_on_one_selected_item_moves_the_whole_selection() {
    let (model, a, b, selection) = two_square_scene();
    selection.replace([a, b]);
    let (mut tree, view_id) = mount(model.clone(), selection, TransformConfig::new());

    // Press inside A (its box is 50,50..90,90), drag +30/+20.
    down(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(100.0, 90.0));
    up(&mut tree, Point::new(100.0, 90.0));
    assert!(flush(&mut tree, view_id), "a group move must post a commit");

    let ra = model.scene_rect(a).unwrap();
    let rb = model.scene_rect(b).unwrap();
    assert!((ra.x - 80.0).abs() < 1e-3, "{ra:?}");
    assert!((ra.y - 70.0).abs() < 1e-3, "{ra:?}");
    assert!((rb.x - 180.0).abs() < 1e-3, "b followed: {rb:?}");
    assert!((rb.y - 70.0).abs() < 1e-3, "b followed: {rb:?}");
}

#[test]
fn a_heavyweight_card_moves_for_the_first_time() {
    // The gap this whole stage exists to close: the draggable snapshot filters
    // by tier before it filters by flag, so before the controller a
    // `SceneView` could not move a card with a pointer at all.
    let model = SceneModel::new();
    let card = model.add_widget(FillWidget::new(), Rect::new(40.0, 40.0, 120.0, 80.0));
    model.set_flag(card, ItemFlags::IS_DRAGGABLE, true);
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([card]);
    let (mut tree, view_id) = mount(model.clone(), selection, TransformConfig::new());

    let before = model.scene_rect(card).unwrap();
    // Grab the card's body. A bare `FillWidget` claims no press, so the view
    // sees the drag — which is exactly the best-effort case the doc describes.
    down(&mut tree, Point::new(100.0, 80.0));
    moved(&mut tree, Point::new(160.0, 130.0));
    up(&mut tree, Point::new(160.0, 130.0));
    assert!(flush(&mut tree, view_id));

    let after = model.scene_rect(card).unwrap();
    assert!(
        (after.x - before.x - 60.0).abs() < 1e-3,
        "{before:?} {after:?}"
    );
    assert!(
        (after.y - before.y - 50.0).abs() < 1e-3,
        "{before:?} {after:?}"
    );
    assert!((after.width - before.width).abs() < 1e-3, "size untouched");
}

#[test]
fn the_frame_band_moves_a_selection_whose_card_claims_its_own_press() {
    // The route that always works. The band is drawn `padding` OUTSIDE the
    // selection, so it is on pixels no card owns — which is what makes the
    // flagship gesture reachable over a card that takes its own press.
    let model = SceneModel::new();
    let card = model.add_widget(FillWidget::new(), Rect::new(100.0, 100.0, 80.0, 60.0));
    model.set_flag(card, ItemFlags::IS_DRAGGABLE, true);
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([card]);
    let cfg = TransformConfig::new().padding(6.0).handle_px(9.0);
    let (mut tree, view_id) = mount(model.clone(), selection, cfg);

    // The outline sits at 94..186 x 94..166. Grab the middle of the top edge
    // band — x is the frame centre, so no corner handle is within reach.
    down(&mut tree, Point::new(140.0, 94.0));
    moved(&mut tree, Point::new(150.0, 104.0));
    up(&mut tree, Point::new(150.0, 104.0));
    assert!(flush(&mut tree, view_id), "the band must start a move");

    let after = model.scene_rect(card).unwrap();
    assert!((after.x - 110.0).abs() < 1e-3, "{after:?}");
    assert!((after.y - 110.0).abs() < 1e-3, "{after:?}");
}

#[test]
fn a_selected_child_of_a_selected_parent_is_not_moved_twice() {
    let model = SceneModel::new();
    let parent = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 80.0, 80.0)).fill(teksilo_tokens::Color::RED),
        Point::new(50.0, 50.0),
    );
    let child = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 20.0, 20.0)).fill(teksilo_tokens::Color::BLUE),
        Point::new(10.0, 10.0),
    );
    model.set_item_parent(child, Some(parent));
    model.set_flag(parent, ItemFlags::IS_DRAGGABLE, true);
    model.set_flag(child, ItemFlags::IS_DRAGGABLE, true);
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([parent, child]);
    let (mut tree, view_id) = mount(model.clone(), selection, TransformConfig::new());

    let child_before = model.scene_rect(child).unwrap();
    down(&mut tree, Point::new(100.0, 100.0));
    moved(&mut tree, Point::new(130.0, 100.0));
    up(&mut tree, Point::new(130.0, 100.0));
    assert!(flush(&mut tree, view_id));

    let child_after = model.scene_rect(child).unwrap();
    assert!(
        (child_after.x - child_before.x - 30.0).abs() < 1e-3,
        "the child must move once, not twice: {child_before:?} -> {child_after:?}"
    );
}

// ---------------------------------------------------------------------------
// The flags are honoured, not bypassed
// ---------------------------------------------------------------------------

#[test]
fn a_selected_item_without_is_draggable_does_not_become_movable() {
    // `ItemFlags::default()` carries IS_SELECTABLE and not IS_DRAGGABLE, so
    // routing by selection membership alone would make every decorative item
    // pointer-movable the moment a marquee touched it.
    let model = SceneModel::new();
    let locked = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 60.0, 60.0)).fill(teksilo_tokens::Color::RED),
        Point::new(50.0, 50.0),
    );
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([locked]);
    let (mut tree, view_id) = mount(model.clone(), selection, TransformConfig::new());

    let before = model.scene_rect(locked).unwrap();
    down(&mut tree, Point::new(80.0, 80.0));
    moved(&mut tree, Point::new(140.0, 140.0));
    up(&mut tree, Point::new(140.0, 140.0));
    flush(&mut tree, view_id);

    let after = model.scene_rect(locked).unwrap();
    assert!(
        (after.x - before.x).abs() < 1e-3 && (after.y - before.y).abs() < 1e-3,
        "a select-only item must not move: {before:?} -> {after:?}"
    );
    // …and the press fell through to the marquee, which is the documented
    // behaviour this must not have taken away.
    let view = view_handle(&tree, view_id);
    assert!(
        view.marquee.get().is_some() || view.pending_marquee_commit.borrow().is_some(),
        "the press must still rubber-band"
    );
}

#[test]
fn a_mixed_selection_moves_only_its_movable_members() {
    let model = SceneModel::new();
    let movable = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
        Point::new(50.0, 50.0),
    );
    let locked = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::BLUE),
        Point::new(150.0, 50.0),
    );
    model.set_flag(movable, ItemFlags::IS_DRAGGABLE, true);
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([movable, locked]);
    let (mut tree, view_id) = mount(model.clone(), selection, TransformConfig::new());

    let locked_before = model.scene_rect(locked).unwrap();
    down(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(100.0, 70.0));
    up(&mut tree, Point::new(100.0, 70.0));
    assert!(flush(&mut tree, view_id));

    assert!((model.scene_rect(movable).unwrap().x - 80.0).abs() < 1e-3);
    assert!(
        (model.scene_rect(locked).unwrap().x - locked_before.x).abs() < 1e-3,
        "the locked member stays where it is"
    );
}

#[test]
fn resize_and_rotate_need_every_root_and_move_needs_only_one() {
    let model = SceneModel::new();
    let a = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
        Point::new(0.0, 0.0),
    );
    let b = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::BLUE),
        Point::new(100.0, 0.0),
    );
    model.set_flag(a, ItemFlags::IS_DRAGGABLE, true);
    model.set_flag(a, ItemFlags::IS_RESIZABLE, true);
    model.set_flag(a, ItemFlags::IS_ROTATABLE, true);
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([a, b]);
    let (tree, view_id) = mount(model, selection, TransformConfig::new());
    let view = view_handle(&tree, view_id);
    let d = view.transform_enabled().expect("controller installed");

    assert!(d.supports(TransformOp::Move), "one movable root is enough");
    assert!(
        !d.supports(TransformOp::Resize),
        "a partial resize would tear the selection apart"
    );
    assert!(!d.supports(TransformOp::Rotate));
    let handles = d.visible_handles();
    assert!(handles.contains(&TransformHandle::Move));
    assert!(
        !handles.contains(&TransformHandle::BottomTrailing),
        "an unavailable operation shows no handle: {handles:?}"
    );
    assert!(!handles.contains(&TransformHandle::Rotate));
}

#[test]
fn a_heavyweight_root_is_never_rotated() {
    // Not because the arena cannot hit-test a rotated widget — it can — but
    // because a card's layout box is the AABB of its transformed bounds, so a
    // rotation there inflates the box and turns nothing.
    let model = SceneModel::new();
    let card = model.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 80.0, 40.0));
    model.set_flags(
        card,
        ItemFlags::default()
            .with(ItemFlags::IS_DRAGGABLE)
            .with(ItemFlags::IS_ROTATABLE),
    );
    assert!(
        model
            .transformable_roots(&[card], TransformOp::Rotate)
            .is_empty(),
        "a widget entry is refused a rotation even carrying IS_ROTATABLE"
    );
    // …and the low-level apply agrees, so the refusal is not only in the UI.
    let before = model.transform(card).unwrap();
    model.apply_transform_delta(
        &[card],
        &TransformDelta {
            rotation: 0.4,
            ..TransformDelta::IDENTITY
        },
    );
    assert_eq!(
        model.transform(card).unwrap(),
        before,
        "apply_transform_delta must not rotate a widget entry"
    );
}

// ---------------------------------------------------------------------------
// Resize and rotate
// ---------------------------------------------------------------------------

#[test]
fn dragging_a_corner_reflows_the_item_rather_than_scaling_it() {
    let model = SceneModel::new();
    let a = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)).fill(teksilo_tokens::Color::RED),
        Point::new(100.0, 100.0),
    );
    model.set_flag(a, ItemFlags::IS_RESIZABLE, true);
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([a]);
    let cfg = TransformConfig::new().padding(6.0);
    let (mut tree, view_id) = mount(model.clone(), selection, cfg);

    // The bottom-trailing handle sits on the padded outline: (206, 206).
    down(&mut tree, Point::new(206.0, 206.0));
    moved(&mut tree, Point::new(256.0, 256.0));
    up(&mut tree, Point::new(256.0, 256.0));
    assert!(flush(&mut tree, view_id), "a corner drag must commit");

    let r = model.scene_rect(a).unwrap();
    assert!((r.x - 100.0).abs() < 1e-2, "top-leading pinned: {r:?}");
    assert!((r.y - 100.0).abs() < 1e-2, "top-leading pinned: {r:?}");
    assert!((r.width - 150.0).abs() < 1e-2, "{r:?}");
    assert!((r.height - 150.0).abs() < 1e-2, "{r:?}");
    // The item's own box grew — it reflowed, it was not left at 100 and drawn
    // through a scale.
    let local = model.local_bounds(a).unwrap();
    assert!((local.width - 150.0).abs() < 1e-2, "{local:?}");
}

#[test]
fn the_rotate_puck_turns_a_single_item_about_its_centre() {
    let model = SceneModel::new();
    let a = model.add_item(
        RectItem::new(Rect::new(-50.0, -50.0, 100.0, 100.0)).fill(teksilo_tokens::Color::RED),
        Point::new(200.0, 200.0),
    );
    model.set_flag(a, ItemFlags::IS_ROTATABLE, true);
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([a]);
    let cfg = TransformConfig::new().handles(TransformHandleSet::ROTATE);
    let (mut tree, view_id) = mount(model.clone(), selection, cfg);

    // The puck floats ROTATE_OFFSET_PX above the padded top edge: the frame is
    // centred on (200,200) with a 100-unit box, so the puck is near
    // (200, 200 - 50 - 6 - 22) = (200, 122).
    down(&mut tree, Point::new(200.0, 122.0));
    // Swing a quarter turn clockwise: to the trailing side of the centre.
    moved(&mut tree, Point::new(300.0, 200.0));
    up(&mut tree, Point::new(300.0, 200.0));
    assert!(flush(&mut tree, view_id), "the puck must commit a rotation");

    let rot = model.scene_rotation(a).unwrap();
    assert!(
        (rot.to_degrees() - 90.0).abs() < 1.0,
        "expected ~90°, got {}",
        rot.to_degrees()
    );
    // A symmetric square about its own centre: the scene box is unmoved.
    let r = model.scene_rect(a).unwrap();
    assert!(
        (r.x - 150.0).abs() < 1e-2 && (r.y - 150.0).abs() < 1e-2,
        "{r:?}"
    );
}

// ---------------------------------------------------------------------------
// The preview, and its lockstep between the two tiers
// ---------------------------------------------------------------------------

#[test]
fn the_two_tiers_preview_one_geometry() {
    // The design's single lockstep constraint. The lightweight tier composes
    // the preview affine into each item's `local -> scene`; the heavyweight
    // tier applies it to the placement rect. One function, so they cannot
    // disagree — and this compares the two answers directly.
    let model = SceneModel::new();
    let light = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
        Point::new(50.0, 50.0),
    );
    let card = model.add_widget(FillWidget::new(), Rect::new(150.0, 50.0, 60.0, 40.0));
    for id in [light, card] {
        model.set_flag(id, ItemFlags::IS_DRAGGABLE, true);
    }
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([light, card]);
    let (mut tree, view_id) = mount(model.clone(), selection, TransformConfig::new());

    down(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(90.0, 70.0)); // Started: the anchor
    moved(&mut tree, Point::new(120.0, 100.0)); // the first carrying sample
    tree.layout(SizeProposal::exact(600.0, 400.0));

    let view = view_handle(&tree, view_id);
    let (roots, xform) = view
        .transform_preview()
        .expect("a live gesture must publish a preview");
    assert!(roots.contains(&light) && roots.contains(&card));
    // The heavyweight tier's answer: the placement the arena was given.
    let card_widget = *view
        .widget_to_item
        .iter()
        .find(|(_, item)| **item == card)
        .map(|(w, _)| w)
        .expect("the card is materialised");
    let placed = tree.bounds(card_widget);
    // …against the lightweight tier's: the same affine over the model rect.
    let expected = xform.apply_rect(model.scene_rect(card).unwrap());
    assert!(
        (placed.width - expected.width).abs() < 1e-2
            && (placed.height - expected.height).abs() < 1e-2,
        "the tiers must agree on size: placed={placed:?} expected={expected:?}"
    );

    up(&mut tree, Point::new(120.0, 100.0));
    flush(&mut tree, view_id);
}

#[test]
fn the_heavyweight_preview_actually_moves_the_card_mid_gesture() {
    // Blocker B1: the preview needs a Relayout-level trigger. A plain `Cell`
    // gives none, and the cards would sit still until the commit while the
    // frame and the lightweight items followed the pointer.
    //
    // Deleting the `Relayout` binding on `TransformRuntime::tick` (or
    // downgrading it to `RepaintOnly`) reddens this.
    let model = SceneModel::new();
    let card = model.add_widget(FillWidget::new(), Rect::new(50.0, 50.0, 80.0, 60.0));
    model.set_flag(card, ItemFlags::IS_DRAGGABLE, true);
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([card]);
    let (mut tree, view_id) = mount(model.clone(), selection, TransformConfig::new());

    let widget = {
        let view = view_handle(&tree, view_id);
        *view
            .widget_to_item
            .iter()
            .find(|(_, item)| **item == card)
            .map(|(w, _)| w)
            .expect("materialised")
    };
    let before = tree.bounds(widget);

    down(&mut tree, Point::new(90.0, 80.0));
    moved(&mut tree, Point::new(120.0, 80.0)); // past the latch slop: Started
    tree.layout(SizeProposal::exact(600.0, 400.0));
    assert!(
        !tree.needs_layout() && !tree.needs_reconcile(),
        "precondition: the tree is settled before the sample under test"
    );

    moved(&mut tree, Point::new(190.0, 130.0));
    assert!(
        tree.needs_reconcile(),
        "the sample must at least have dirtied a binding"
    );
    // …and the layout pass is what turns that binding into placement. It
    // early-returns when nothing needs *laying out*, so this call re-places the
    // card only if the controller's tick is bound at `Relayout`. Downgrade that
    // binding to `RepaintOnly` — which is all a plain `Cell` can ever amount to
    // — and the pass flushes the binding, marks paint, returns, and the
    // assertion below reddens with the card still at its old bounds.
    tree.layout(SizeProposal::exact(600.0, 400.0));

    let during = tree.bounds(widget);
    assert!(
        (during.x - before.x - 100.0).abs() < 1.0,
        "the card must follow the pointer during the gesture, \
         not jump at the commit: {before:?} -> {during:?}"
    );

    up(&mut tree, Point::new(190.0, 130.0));
    flush(&mut tree, view_id);
}

#[test]
fn ghost_preview_leaves_both_tiers_alone_until_the_commit() {
    let (model, a, b, selection) = two_square_scene();
    selection.replace([a, b]);
    let cfg = TransformConfig::new().live_preview(LivePreview::Ghost);
    let (mut tree, view_id) = mount(model.clone(), selection, cfg);

    down(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(140.0, 70.0));
    {
        let view = view_handle(&tree, view_id);
        assert!(
            view.transform_preview().is_none(),
            "Ghost previews nothing, for either tier"
        );
        assert!(
            view.transform_session_signal().get().is_some(),
            "…but the session is still live, so the frame still moves"
        );
    }
    up(&mut tree, Point::new(140.0, 70.0));
    assert!(flush(&mut tree, view_id));
    assert!((model.scene_rect(a).unwrap().x - 120.0).abs() < 1e-3);
}

#[test]
fn nothing_is_written_until_the_release() {
    let (model, a, b, selection) = two_square_scene();
    selection.replace([a, b]);
    let changes = Rc::new(Cell::new(0u32));
    let c = changes.clone();
    let _obs = model.item_change_signal().observe(move |_| {
        c.set(c.get() + 1);
    });
    let (mut tree, view_id) = mount(model.clone(), selection, TransformConfig::new());
    changes.set(0);

    down(&mut tree, Point::new(70.0, 70.0));
    for x in [80.0, 90.0, 100.0, 110.0, 120.0] {
        moved(&mut tree, Point::new(x, 70.0));
    }
    assert_eq!(
        changes.get(),
        0,
        "a model-free session emits no ItemChange while the pointer moves"
    );
    up(&mut tree, Point::new(120.0, 70.0));
    assert_eq!(changes.get(), 0, "…nor at the release, which only posts");
    assert!(flush(&mut tree, view_id));
    assert!(changes.get() > 0, "the drain is what writes");
}

// ---------------------------------------------------------------------------
// Cancellation
// ---------------------------------------------------------------------------

/// The hold a scene grab is armed by under a finger, plus a margin — a
/// `SceneView` holds a `PanClaim` *and* its own `on_drag`, so a press that
/// simply travels pans the camera and only a *held* press grabs. See
/// `view::tests::drag_cancel`, which explains the same shape at length.
fn hold_ms() -> std::time::Duration {
    teksilo_core::gesture::default_profile(teksilo_tokens::PointerKind::Touch).long_press
        + std::time::Duration::from_millis(50)
}

#[test]
fn a_finger_transforms_after_a_hold_and_pans_without_one() {
    // The controller is not mouse-only. A `SceneView` with a controller carries
    // more `on_drag` on a node that also claims the pan, and core arbitration
    // is what keeps both reachable: travel pans, hold-then-travel transforms.
    //
    // One big square, pressed dead centre: a finger earns a much wider grab
    // radius than a mouse, and the point has to be clear of every handle for
    // the test to be about the body drag it says it is about.
    let model = SceneModel::new();
    let a = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 200.0, 200.0)).fill(teksilo_tokens::Color::RED),
        Point::new(100.0, 100.0),
    );
    model.set_flag(a, ItemFlags::IS_DRAGGABLE, true);
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([a]);
    let (mut tree, view_id) = mount(model.clone(), selection, TransformConfig::new());

    // No hold: the camera pans and nothing is grabbed.
    let pan_before = view_handle(&tree, view_id).pan_x.get();
    let f = tree.new_contact();
    tree.touch_down(f, Point::new(200.0, 200.0));
    tree.touch_move(f, Point::new(270.0, 200.0));
    tree.touch_up(f, Point::new(270.0, 200.0));
    assert!(!flush(&mut tree, view_id), "a plain finger travel is a pan");
    assert!(
        (view_handle(&tree, view_id).pan_x.get() - pan_before).abs() > 1.0,
        "…and it actually panned"
    );
    assert!((model.scene_rect(a).unwrap().x - 100.0).abs() < 1e-3);

    // Held: the same finger transforms.
    let scene_x_before = model.scene_rect(a).unwrap().x;
    let f2 = tree.new_contact();
    tree.touch_down(f2, Point::new(200.0, 200.0));
    tree.advance_time(hold_ms());
    tree.touch_move(f2, Point::new(200.0, 200.0));
    tree.touch_move(f2, Point::new(220.0, 200.0));
    tree.touch_move(f2, Point::new(270.0, 200.0));
    tree.touch_up(f2, Point::new(270.0, 200.0));
    assert!(flush(&mut tree, view_id), "a held finger transforms");
    assert!(
        model.scene_rect(a).unwrap().x > scene_x_before + 10.0,
        "the selection moved under the finger"
    );
    tree.assert_no_leaked_pointer_state();
}

#[test]
fn a_cancelled_gesture_writes_nothing_and_leaves_nothing_behind() {
    let (model, a, b, selection) = two_square_scene();
    selection.replace([a, b]);
    let (mut tree, view_id) = mount(model.clone(), selection, TransformConfig::new());
    let before = model.scene_rect(a).unwrap();

    let f = tree.new_contact();
    tree.touch_down(f, Point::new(70.0, 70.0));
    tree.advance_time(hold_ms());
    tree.touch_move(f, Point::new(90.0, 70.0));
    tree.touch_move(f, Point::new(140.0, 140.0));
    {
        let view = view_handle(&tree, view_id);
        assert!(
            view.transform_preview().is_some(),
            "precondition: the gesture is live and previewing"
        );
    }
    tree.touch_cancel(f, Point::new(140.0, 140.0));
    tree.layout(SizeProposal::exact(600.0, 400.0));

    {
        let view = view_handle(&tree, view_id);
        assert!(
            view.transform_preview().is_none(),
            "a revoked contact must not keep previewing the transform"
        );
        assert!(view.transform_session_signal().get().is_none());
    }
    assert!(!flush(&mut tree, view_id), "nothing may be pending");
    let after = model.scene_rect(a).unwrap();
    assert!((after.x - before.x).abs() < 1e-3 && (after.y - before.y).abs() < 1e-3);
    tree.assert_no_leaked_pointer_state();
}

#[test]
fn escape_during_a_keyboard_gesture_drops_it() {
    let (model, a, _b, selection) = two_square_scene();
    selection.replace([a]);
    let (mut tree, view_id) = mount(model.clone(), selection, TransformConfig::new());
    tree.focus(view_id);
    let before = model.scene_rect(a).unwrap();

    key(&mut tree, Key::Character('t'));
    key(&mut tree, Key::ArrowRight);
    key(&mut tree, Key::ArrowRight);
    {
        let view = view_handle(&tree, view_id);
        assert!(view.transform_session_signal().get().is_some());
    }
    key(&mut tree, Key::Escape);
    assert!(!flush(&mut tree, view_id), "Esc commits nothing");
    let after = model.scene_rect(a).unwrap();
    assert!((after.x - before.x).abs() < 1e-3, "{before:?} -> {after:?}");
}

// ---------------------------------------------------------------------------
// The keyboard route, and its agreement with the pointer
// ---------------------------------------------------------------------------

#[test]
fn the_keyboard_route_makes_the_same_model_change_the_drag_makes() {
    // `docs/a11y/non-drag-alternatives.md`: "the alternative must make the same
    // model change the drag makes", asserted by driving both over one model.
    let run_pointer = || {
        let (model, a, b, selection) = two_square_scene();
        selection.replace([a, b]);
        let (mut tree, view_id) = mount(model.clone(), selection, TransformConfig::new());
        down(&mut tree, Point::new(70.0, 70.0));
        moved(&mut tree, Point::new(80.0, 75.0));
        up(&mut tree, Point::new(80.0, 75.0));
        flush(&mut tree, view_id);
        (model.scene_rect(a).unwrap(), model.scene_rect(b).unwrap())
    };
    let run_keyboard = || {
        let (model, a, b, selection) = two_square_scene();
        selection.replace([a, b]);
        let (mut tree, view_id) = mount(model.clone(), selection, TransformConfig::new());
        tree.focus(view_id);
        key(&mut tree, Key::Character('t')); // enter, roving on Move
        key_mod(&mut tree, Key::ArrowRight, Modifiers::SHIFT); // +10
        key(&mut tree, Key::ArrowDown);
        key(&mut tree, Key::ArrowDown);
        key(&mut tree, Key::ArrowDown);
        key(&mut tree, Key::ArrowDown);
        key(&mut tree, Key::ArrowDown);
        key(&mut tree, Key::Enter);
        flush(&mut tree, view_id);
        (model.scene_rect(a).unwrap(), model.scene_rect(b).unwrap())
    };
    let (pa, pb) = run_pointer();
    let (ka, kb) = run_keyboard();
    assert!(
        (pa.x - ka.x).abs() < 1e-3 && (pa.y - ka.y).abs() < 1e-3,
        "{pa:?} {ka:?}"
    );
    assert!(
        (pb.x - kb.x).abs() < 1e-3 && (pb.y - kb.y).abs() < 1e-3,
        "{pb:?} {kb:?}"
    );
}

#[test]
fn tab_roves_the_handles_and_the_mode_key_leaves() {
    let (model, a, _b, selection) = two_square_scene();
    selection.replace([a]);
    let (mut tree, view_id) = mount(model, selection, TransformConfig::new());
    tree.focus(view_id);

    key(&mut tree, Key::Character('t'));
    {
        let view = view_handle(&tree, view_id);
        assert_eq!(
            view.transform_rt.keyboard_focus.get(),
            Some(TransformHandle::Move)
        );
    }
    key(&mut tree, Key::Tab);
    {
        let view = view_handle(&tree, view_id);
        assert_eq!(
            view.transform_rt.keyboard_focus.get(),
            Some(TransformHandle::TopLeading)
        );
    }
    key_mod(&mut tree, Key::Tab, Modifiers::SHIFT);
    {
        let view = view_handle(&tree, view_id);
        assert_eq!(
            view.transform_rt.keyboard_focus.get(),
            Some(TransformHandle::Move)
        );
    }
    key(&mut tree, Key::Character('t'));
    let view = view_handle(&tree, view_id);
    assert!(!view.transform_rt.keyboard_mode.get());
    assert_eq!(view.transform_rt.keyboard_focus.get(), None);
}

#[test]
fn alt_arrow_still_nudges_and_no_longer_moves_a_child_twice() {
    // The pre-existing keyboard nudge kept its meaning, and its doc comment
    // ("matching pointer group-drag") became true: it prunes roots now, so a
    // selected child of a selected parent is translated once.
    let model = SceneModel::new();
    let parent = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 80.0, 80.0)).fill(teksilo_tokens::Color::RED),
        Point::new(50.0, 50.0),
    );
    let child = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 20.0, 20.0)).fill(teksilo_tokens::Color::BLUE),
        Point::new(10.0, 10.0),
    );
    model.set_item_parent(child, Some(parent));
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([parent, child]);
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::with_model(model.clone()).selection_model(selection));
    tree.layout(SizeProposal::exact(600.0, 400.0));
    tree.focus(view_id);

    let before = model.scene_rect(child).unwrap();
    key_mod(&mut tree, Key::ArrowRight, Modifiers::ALT);
    let after = model.scene_rect(child).unwrap();
    assert!(
        (after.x - before.x - 1.0).abs() < 1e-3,
        "one step, not two: {before:?} -> {after:?}"
    );
}

// ---------------------------------------------------------------------------
// Hooks
// ---------------------------------------------------------------------------

#[test]
fn the_three_hooks_fire_once_each_and_report_the_outcome() {
    let (model, a, b, selection) = two_square_scene();
    selection.replace([a, b]);
    let log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let (l1, l2, l3) = (log.clone(), log.clone(), log.clone());
    let cfg = TransformConfig::new()
        .on_start(move |s, _| l1.borrow_mut().push(format!("start:{}", s.items.len())))
        .on_change(move |_, _| l2.borrow_mut().push("change".into()))
        .on_end(move |_, outcome, _| {
            l3.borrow_mut().push(match outcome {
                TransformOutcome::Committed => "committed".into(),
                TransformOutcome::Cancelled => "cancelled".into(),
            })
        });
    let (mut tree, view_id) = mount(model, selection, cfg);

    down(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(90.0, 70.0)); // Started — not a change
    moved(&mut tree, Point::new(110.0, 70.0));
    moved(&mut tree, Point::new(130.0, 70.0));
    up(&mut tree, Point::new(130.0, 70.0));
    flush(&mut tree, view_id);

    let entries = log.borrow().clone();
    assert_eq!(entries.first().map(String::as_str), Some("start:2"));
    assert_eq!(entries.last().map(String::as_str), Some("committed"));
    assert_eq!(
        entries.iter().filter(|e| *e == "change").count(),
        3,
        "one per sample, including the release's: {entries:?}"
    );
}

#[test]
fn the_geometry_constraint_constrains_the_commit_not_only_the_drawing() {
    let model = SceneModel::new();
    let a = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
        Point::new(50.0, 50.0),
    );
    model.set_flag(a, ItemFlags::IS_DRAGGABLE, true);
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([a]);
    // Snap the frame's origin to a 25-unit grid.
    model.set_geometry_constraint(|c| {
        let snap = |v: f32| (v / 25.0).round() * 25.0;
        let mut f = c.proposed;
        f.rect.x = snap(f.rect.x);
        f.rect.y = snap(f.rect.y);
        crate::constrain::ChangeVerdict::Adjust(f)
    });
    let cfg = TransformConfig::new();
    let (mut tree, view_id) = mount(model.clone(), selection, cfg);

    down(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(93.0, 70.0));
    up(&mut tree, Point::new(93.0, 70.0));
    assert!(flush(&mut tree, view_id));

    let r = model.scene_rect(a).unwrap();
    assert!(
        (r.x - 75.0).abs() < 1e-2,
        "the committed position is the constrained one: {r:?}"
    );
}

// ---------------------------------------------------------------------------
// Edge auto-pan
// ---------------------------------------------------------------------------

#[test]
fn holding_at_the_viewport_edge_pans_the_view_and_carries_the_selection() {
    // Auto-pan without a frame tick of its own: the pan signals are animated
    // and already bound at `Relayout`, and the session stores a **screen**
    // point it re-projects, so the scene slides under a stationary pointer and
    // the selection keeps following the point beneath it.
    let model = SceneModel::new();
    let a = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 60.0, 60.0)).fill(teksilo_tokens::Color::RED),
        Point::new(400.0, 200.0),
    );
    model.set_flag(a, ItemFlags::IS_DRAGGABLE, true);
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([a]);
    let (mut tree, view_id) = mount(
        model.clone(),
        selection,
        TransformConfig::new()
            .edge_pan_px(30.0)
            .edge_pan_speed(400.0),
    );

    let pan_before = view_handle(&tree, view_id).pan_x.get();
    down(&mut tree, Point::new(430.0, 230.0));
    moved(&mut tree, Point::new(500.0, 230.0)); // Started
    moved(&mut tree, Point::new(585.0, 230.0)); // inside the trailing band
    tree.advance_time(std::time::Duration::from_millis(250));
    tree.layout(SizeProposal::exact(600.0, 400.0));

    let pan_after = view_handle(&tree, view_id).pan_x.get();
    assert!(
        pan_after < pan_before - 20.0,
        "a pointer held at the trailing edge must pan the content leading-ward: \
         {pan_before} -> {pan_after}"
    );
    // Read the *live* resolution, not the published snapshot: the snapshot is
    // republished on input samples, and the whole point of auto-pan is that the
    // frame keeps moving while the pointer stands still. See
    // `SceneView::transform_session_signal`.
    let (_, resolved) = view_handle(&tree, view_id)
        .transform_enabled()
        .and_then(|d| d.resolved())
        .expect("still gesturing");
    assert!(
        resolved.delta.translation.x > 200.0,
        "the selection keeps following the pointer's scene position as the view \
         slides under it: {:?}",
        resolved.delta
    );

    up(&mut tree, Point::new(585.0, 230.0));
    assert!(flush(&mut tree, view_id));
    assert!(model.scene_rect(a).unwrap().x > 600.0);
}

#[test]
fn a_gesture_clear_of_the_edges_arms_no_pan() {
    let (model, a, b, selection) = two_square_scene();
    selection.replace([a, b]);
    let (mut tree, view_id) = mount(model, selection, TransformConfig::new());

    let pan_before = view_handle(&tree, view_id).pan_x.get();
    down(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(90.0, 70.0));
    moved(&mut tree, Point::new(200.0, 200.0));
    tree.advance_time(std::time::Duration::from_millis(400));
    assert_eq!(
        view_handle(&tree, view_id).pan_x.get(),
        pan_before,
        "nothing near an edge, nothing panned"
    );
    up(&mut tree, Point::new(200.0, 200.0));
    flush(&mut tree, view_id);
}

// ---------------------------------------------------------------------------
// The chrome sits outside the content — the B2 mechanism, measured
// ---------------------------------------------------------------------------

#[test]
fn every_handle_clears_the_selections_own_bounds() {
    // This is *why* the frame band and the handles are reachable over a card
    // that claims its own press: with the default padding, a handle's whole
    // disc lies outside the rectangle the arena laid the card out at. Shrink
    // `padding` below `handle_px / 2` and this stops being true, which is what
    // the setter's doc warns about.
    let model = SceneModel::new();
    let card = model.add_widget(FillWidget::new(), Rect::new(100.0, 100.0, 120.0, 80.0));
    model.set_flags(
        card,
        ItemFlags::default()
            .with(ItemFlags::IS_DRAGGABLE)
            .with(ItemFlags::IS_RESIZABLE)
            .with(ItemFlags::IS_ROTATABLE),
    );
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([card]);
    let (tree, view_id) = mount(model.clone(), selection, TransformConfig::new());

    let view = view_handle(&tree, view_id);
    let d = view.transform_enabled().unwrap();
    let frame = d.selection_frame().unwrap();
    let content = model.scene_rect(card).unwrap();
    let half = crate::transform_session::DEFAULT_HANDLE_PX * 0.5;
    let points = d.handle_points(&frame, 1.0);
    assert!(!points.is_empty(), "the card offers handles");
    for (handle, p) in points {
        let disc = Rect::new(p.x - half, p.y - half, half * 2.0, half * 2.0);
        let overlaps = crate::scene::rects_intersect(disc, content);
        assert!(
            !overlaps,
            "{handle:?} at {p:?} overlaps the card's own bounds {content:?} — \
             the chrome would be unreachable over a card that claims its press"
        );
    }
}

// ---------------------------------------------------------------------------
// Accessibility
// ---------------------------------------------------------------------------

#[test]
fn the_frame_and_its_handles_are_published_to_assistive_technology() {
    use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id, widget_id_to_node_id};

    let (model, a, _b, selection) = two_square_scene();
    selection.replace([a]);
    let (mut tree, view_id) = mount(model, selection, TransformConfig::new());

    let update = tree.sync_accessibility();
    let frame_id = synthetic_node_id(view_id, 0, SyntheticKind::SceneHandle);
    let (_, frame) = update
        .nodes
        .iter()
        .find(|(id, _)| *id == frame_id)
        .expect("the selection frame must be published");
    assert_eq!(frame.role(), accesskit::Role::Group);
    assert_eq!(frame.label(), Some("Selection"));

    let view_node = update
        .nodes
        .iter()
        .find(|(id, _)| *id == widget_id_to_node_id(view_id))
        .map(|(_, n)| n)
        .expect("the view's own node");
    assert!(
        view_node.children().contains(&frame_id),
        "the frame hangs off the view, not off an item"
    );

    // Every offered handle has a node, a name and both step actions.
    for handle in [
        TransformHandle::Move,
        TransformHandle::TopLeading,
        TransformHandle::Trailing,
        TransformHandle::Rotate,
    ] {
        let node_id = synthetic_node_id(
            view_id,
            handle.index() as u64 + 1,
            SyntheticKind::SceneHandle,
        );
        let (_, node) = update
            .nodes
            .iter()
            .find(|(id, _)| *id == node_id)
            .unwrap_or_else(|| panic!("{handle:?} must be published"));
        assert!(node.label().is_some(), "{handle:?} must be named");
        assert!(
            node.supports_action(accesskit::Action::Increment)
                && node.supports_action(accesskit::Action::Decrement),
            "{handle:?} must offer a non-drag route"
        );
        assert!(frame.children().contains(&node_id));
    }
}

#[test]
fn an_at_increment_on_a_handle_makes_the_same_change_the_keyboard_makes() {
    use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id};

    let (model, a, _b, selection) = two_square_scene();
    selection.replace([a]);
    let (mut tree, view_id) = mount(model.clone(), selection, TransformConfig::new());
    tree.sync_accessibility();

    let node = synthetic_node_id(
        view_id,
        TransformHandle::Move.index() as u64 + 1,
        SyntheticKind::SceneHandle,
    );
    let before = model.scene_rect(a).unwrap();
    let mut ops = teksilo_core::window::NoopWindowOps;
    let handled = tree.dispatch_access_action(node, accesskit::Action::Increment, None, &mut ops);
    assert!(handled, "an advertised action must be a live one");
    flush(&mut tree, view_id);
    let after = model.scene_rect(a).unwrap();
    // The band is the one handle that has no single axis, so its `Increment` is
    // both of the selection's coordinates at once — one step of the keyboard's
    // trailing arrow plus one of its down arrow. See `TransformHandle::at_steps`.
    assert!(
        (after.x - before.x - 1.0).abs() < 1e-3 && (after.y - before.y - 1.0).abs() < 1e-3,
        "Increment on the move handle steps the selection on both axes: {before:?} -> {after:?}"
    );
}

#[test]
fn the_keyboard_mode_points_active_descendant_at_the_roved_handle() {
    use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id, widget_id_to_node_id};

    let (model, a, _b, selection) = two_square_scene();
    selection.replace([a]);
    let (mut tree, view_id) = mount(model, selection, TransformConfig::new());
    tree.focus(view_id);
    key(&mut tree, Key::Character('t'));
    key(&mut tree, Key::Tab);

    let update = tree.sync_accessibility();
    let view_node = update
        .nodes
        .iter()
        .find(|(id, _)| *id == widget_id_to_node_id(view_id))
        .map(|(_, n)| n)
        .expect("view node");
    assert_eq!(
        view_node.active_descendant(),
        Some(synthetic_node_id(
            view_id,
            TransformHandle::TopLeading.index() as u64 + 1,
            SyntheticKind::SceneHandle,
        ))
    );
}

#[test]
fn a_view_without_a_controller_publishes_no_transform_nodes() {
    use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id};

    let (model, a, _b, selection) = two_square_scene();
    selection.replace([a]);
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::with_model(model).selection_model(selection));
    tree.layout(SizeProposal::exact(600.0, 400.0));

    let update = tree.sync_accessibility();
    let frame_id = synthetic_node_id(view_id, 0, SyntheticKind::SceneHandle);
    assert!(
        !update.nodes.iter().any(|(id, _)| *id == frame_id),
        "no controller, no chrome, no nodes"
    );
}

#[test]
fn the_view_stays_the_only_tab_stop() {
    // The chrome is a paint pass, so it adds no focusable arena node — the
    // handles are reachable through the view's own roving mode, not by Tab.
    let (model, a, _b, selection) = two_square_scene();
    selection.replace([a]);
    let mut tree = WidgetTree::new();
    let root = tree.add(
        SceneView::with_model(model)
            .selection_model(selection)
            .transform_controller(TransformConfig::new()),
    );
    tree.layout(SizeProposal::exact(600.0, 400.0));
    let stops = tree.tab_stops_within(root);
    assert_eq!(
        stops,
        vec![root],
        "the controller must not add or remove a tab stop"
    );
}

// ---------------------------------------------------------------------------
// Accessibility — every handle's own verb
// ---------------------------------------------------------------------------

/// One 100 x 80 lightweight rectangle at scene (50, 50), draggable, resizable
/// and rotatable, already selected.
///
/// Deliberately **not** square: a handle that confuses its axes passes on a
/// square and fails here.
fn one_rect_scene() -> (SceneModel, ItemId, SceneSelection) {
    let model = SceneModel::new();
    let a = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 100.0, 80.0)).fill(teksilo_tokens::Color::RED),
        Point::new(50.0, 50.0),
    );
    model.set_flag(a, ItemFlags::IS_DRAGGABLE, true);
    model.set_flag(a, ItemFlags::IS_RESIZABLE, true);
    model.set_flag(a, ItemFlags::IS_ROTATABLE, true);
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([a]);
    (model, a, selection)
}

fn frame_of(tree: &WidgetTree, view_id: WidgetId) -> crate::transform_session::TransformFrame {
    view_handle(tree, view_id)
        .transform_enabled()
        .expect("a controller is installed and enabled")
        .selection_frame()
        .expect("a selection with a frame")
}

/// What one `Increment` on `handle` must do to the frame, as the change in
/// each of the four edge coordinates its name points at plus the change in its
/// angle in degrees: `(leading, top, trailing, bottom, degrees)`.
///
/// **One rule, written out**: the verb moves every coordinate the handle drives
/// by `+1` and leaves the rest alone. For a slider that coordinate is the
/// `numeric_value` it announces; for a corner it is both of the corner's own
/// coordinates; for the frame band it is the whole rectangle.
fn advertised_increment(handle: TransformHandle) -> (f32, f32, f32, f32, f32) {
    match handle {
        TransformHandle::Move => (1.0, 1.0, 1.0, 1.0, 0.0),
        TransformHandle::TopLeading => (1.0, 1.0, 0.0, 0.0, 0.0),
        TransformHandle::Top => (0.0, 1.0, 0.0, 0.0, 0.0),
        TransformHandle::TopTrailing => (0.0, 1.0, 1.0, 0.0, 0.0),
        TransformHandle::Leading => (1.0, 0.0, 0.0, 0.0, 0.0),
        TransformHandle::Trailing => (0.0, 0.0, 1.0, 0.0, 0.0),
        TransformHandle::BottomLeading => (1.0, 0.0, 0.0, 1.0, 0.0),
        TransformHandle::Bottom => (0.0, 0.0, 0.0, 1.0, 0.0),
        TransformHandle::BottomTrailing => (0.0, 0.0, 1.0, 1.0, 0.0),
        TransformHandle::Rotate => (0.0, 0.0, 0.0, 0.0, 1.0),
    }
}

/// The value a slider handle announces, read straight off a **fresh** AT walk.
fn announced_value(tree: &WidgetTree, view_id: WidgetId, handle: TransformHandle) -> Option<f64> {
    use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id};
    let node_id = synthetic_node_id(
        view_id,
        handle.index() as u64 + 1,
        SyntheticKind::SceneHandle,
    );
    tree.accessibility_tree_snapshot()
        .nodes
        .iter()
        .find(|(id, _)| *id == node_id)
        .and_then(|(_, n)| n.numeric_value())
}

/// The names of the per-axis custom actions a handle publishes, read off a
/// fresh AT walk.
fn published_custom_actions(
    tree: &WidgetTree,
    view_id: WidgetId,
    handle: TransformHandle,
) -> Vec<String> {
    use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id};
    let node_id = synthetic_node_id(
        view_id,
        handle.index() as u64 + 1,
        SyntheticKind::SceneHandle,
    );
    tree.accessibility_tree_snapshot()
        .nodes
        .iter()
        .find(|(id, _)| *id == node_id)
        .map(|(_, n)| {
            n.custom_actions()
                .iter()
                .map(|a| a.description.to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// One handle's node, as the **published** tree serves it.
fn published_handle_node(
    tree: &mut WidgetTree,
    view_id: WidgetId,
    handle: TransformHandle,
) -> Option<accesskit::Node> {
    use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id};
    let node_id = synthetic_node_id(
        view_id,
        handle.index() as u64 + 1,
        SyntheticKind::SceneHandle,
    );
    tree.sync_accessibility()
        .nodes
        .iter()
        .find(|(id, _)| *id == node_id)
        .map(|(_, n)| n.clone())
}

/// Drive one AccessKit action at one handle and settle the model.
///
/// The `sync_accessibility` first is not decoration: routing an action aimed at
/// a synthetic node back to its owner widget goes through the map the walk
/// builds, so a tree that has never published cannot receive one. A real app
/// walks every frame.
fn at_action(
    tree: &mut WidgetTree,
    view_id: WidgetId,
    handle: TransformHandle,
    action: accesskit::Action,
    data: Option<accesskit::ActionData>,
) -> bool {
    use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id};
    tree.sync_accessibility();
    let node_id = synthetic_node_id(
        view_id,
        handle.index() as u64 + 1,
        SyntheticKind::SceneHandle,
    );
    let mut ops = teksilo_core::window::NoopWindowOps;
    let handled = tree.dispatch_access_action(node_id, action, data, &mut ops);
    flush(tree, view_id);
    tree.layout(SizeProposal::exact(600.0, 400.0));
    handled
}

/// **The** assistive-technology contract: every handle the frame publishes,
/// driven through `dispatch_access_action` exactly as a screen reader drives
/// it, must move the model the way that handle's own name and value advertise
/// — in *both* directions.
///
/// It exists because a "handled = true" that changes nothing is invisible to
/// every other test here. The route used to map every verb to one horizontal
/// arrow, so "Resize top" and "Resize bottom" answered *handled*, announced an
/// unchanged number, and left the item exactly where it was: the height could
/// not be changed from assistive technology at all, by any route.
#[test]
fn every_published_handle_moves_the_model_the_way_its_name_advertises() {
    for handle in crate::transform_session::HANDLE_ORDER {
        for (action, sign) in [
            (accesskit::Action::Increment, 1.0_f32),
            (accesskit::Action::Decrement, -1.0_f32),
        ] {
            let (model, _a, selection) = one_rect_scene();
            let (mut tree, view_id) = mount(model, selection, TransformConfig::new());
            let before = frame_of(&tree, view_id);
            assert!(
                at_action(&mut tree, view_id, handle, action, None),
                "{handle:?} publishes {action:?}, so it must handle it"
            );
            let after = frame_of(&tree, view_id);

            let (dx0, dy0, dx1, dy1, ddeg) = advertised_increment(handle);
            let mut want = vec![(
                "degrees",
                before.rotation.to_degrees() + ddeg * sign,
                after.rotation.to_degrees(),
            )];
            if ddeg == 0.0 {
                want.extend([
                    ("leading", before.rect.x + dx0 * sign, after.rect.x),
                    ("top", before.rect.y + dy0 * sign, after.rect.y),
                    (
                        "trailing",
                        before.rect.right() + dx1 * sign,
                        after.rect.right(),
                    ),
                    (
                        "bottom",
                        before.rect.bottom() + dy1 * sign,
                        after.rect.bottom(),
                    ),
                ]);
            } else {
                // `rect` is stated in the frame's **own** basis, so turning the
                // frame restates every edge coordinate even though the box has
                // not moved. What a rotation must leave alone is the centre and
                // the extent, in scene space.
                let (c0, c1) = (before.centre_scene(), after.centre_scene());
                want.extend([
                    ("centre x", c0.x, c1.x),
                    ("centre y", c0.y, c1.y),
                    ("width", before.rect.width, after.rect.width),
                    ("height", before.rect.height, after.rect.height),
                ]);
            }
            for (edge, expected, got) in want {
                assert!(
                    (got - expected).abs() < 1e-3,
                    "{handle:?} {action:?}: {edge} must read {expected}, reads {got} \
                     (frame {before:?} -> {after:?})"
                );
            }
        }
    }
}

/// The other half of the same contract: a slider that advertises a number must
/// *announce* the number it moved. Read off a fresh AT walk, so this is what an
/// assistive technology would be told after the step.
#[test]
fn a_slider_handles_announced_value_follows_its_own_verb() {
    for handle in crate::transform_session::HANDLE_ORDER {
        if !handle.is_scalar() {
            let (model, _a, selection) = one_rect_scene();
            let (tree, view_id) = mount(model, selection, TransformConfig::new());
            assert!(
                announced_value(&tree, view_id, handle).is_none(),
                "{handle:?} drives two numbers, so it must announce none of them"
            );
            continue;
        }
        for (action, sign) in [
            (accesskit::Action::Increment, 1.0_f64),
            (accesskit::Action::Decrement, -1.0_f64),
        ] {
            let (model, _a, selection) = one_rect_scene();
            let (mut tree, view_id) = mount(model, selection, TransformConfig::new());
            let before = announced_value(&tree, view_id, handle)
                .unwrap_or_else(|| panic!("{handle:?} is a slider and must announce a value"));
            assert!(at_action(&mut tree, view_id, handle, action, None));
            let after = announced_value(&tree, view_id, handle).expect("still a slider");
            assert!(
                (after - (before + sign)).abs() < 1e-3,
                "{handle:?} {action:?}: announced value must go {before} -> {}, went to {after}",
                before + sign
            );
        }
    }
}

/// A two-dimensional handle's `Increment` is its two edge neighbours' at once,
/// and its four custom actions are those neighbours one at a time. Stated as an
/// equality between two routes rather than as two copies of one table, so a
/// change to either that does not reach the other reddens here.
#[test]
fn a_corner_custom_action_drives_one_axis_and_its_increment_drives_both() {
    use crate::transform_session::TransformStep;

    for handle in crate::transform_session::HANDLE_ORDER {
        let (model, _a, selection) = one_rect_scene();
        let (tree, view_id) = mount(model, selection, TransformConfig::new());
        let published = published_custom_actions(&tree, view_id, handle);
        if handle.is_scalar() {
            assert!(
                published.is_empty(),
                "{handle:?} drives one axis already — per-axis actions would be noise"
            );
            continue;
        }
        assert_eq!(
            published,
            TransformStep::ALL
                .iter()
                .map(|s| s.default_label().to_string())
                .collect::<Vec<_>>(),
            "{handle:?} must publish one action per direction, in TransformStep::ALL order"
        );

        // Each custom action moves exactly the coordinates its own direction
        // names, and the two that `Increment` is made of reproduce it.
        let (dx0, dy0, dx1, dy1, _) = advertised_increment(handle);
        for step in TransformStep::ALL {
            let (model, _a, selection) = one_rect_scene();
            let (mut tree, view_id) = mount(model, selection, TransformConfig::new());
            let before = frame_of(&tree, view_id);
            assert!(
                at_action(
                    &mut tree,
                    view_id,
                    handle,
                    accesskit::Action::CustomAction,
                    Some(accesskit::ActionData::CustomAction(step.index() as i32)),
                ),
                "{handle:?} advertises {step:?}, so it must handle it"
            );
            let after = frame_of(&tree, view_id);
            let (want_x, want_y) = match step {
                TransformStep::Trailing => (1.0, 0.0),
                TransformStep::Leading => (-1.0, 0.0),
                TransformStep::Down => (0.0, 1.0),
                TransformStep::Up => (0.0, -1.0),
            };
            let checks = [
                ("leading", before.rect.x + dx0 * want_x, after.rect.x),
                ("top", before.rect.y + dy0 * want_y, after.rect.y),
                (
                    "trailing",
                    before.rect.right() + dx1 * want_x,
                    after.rect.right(),
                ),
                (
                    "bottom",
                    before.rect.bottom() + dy1 * want_y,
                    after.rect.bottom(),
                ),
            ];
            for (edge, expected, got) in checks {
                assert!(
                    (got - expected).abs() < 1e-3,
                    "{handle:?} {step:?}: {edge} must read {expected}, reads {got}"
                );
            }
        }

        // The equality this test's name claims, actually driven. `Increment` is
        // the handle's own `at_steps(true)` applied together, so replaying
        // those same steps one at a time through the custom-action route must
        // land on the identical frame. Without this the two routes are two
        // copies of one table and a change to either can miss the other.
        let via_increment = {
            let (model, _a, selection) = one_rect_scene();
            let (mut tree, view_id) = mount(model, selection, TransformConfig::new());
            assert!(at_action(
                &mut tree,
                view_id,
                handle,
                accesskit::Action::Increment,
                None,
            ));
            frame_of(&tree, view_id)
        };
        let via_steps = {
            let (model, _a, selection) = one_rect_scene();
            let (mut tree, view_id) = mount(model, selection, TransformConfig::new());
            for step in handle.at_steps(true) {
                assert!(at_action(
                    &mut tree,
                    view_id,
                    handle,
                    accesskit::Action::CustomAction,
                    Some(accesskit::ActionData::CustomAction(step.index() as i32)),
                ));
            }
            frame_of(&tree, view_id)
        };
        assert!(
            (via_increment.rect.x - via_steps.rect.x).abs() < 1e-3
                && (via_increment.rect.y - via_steps.rect.y).abs() < 1e-3
                && (via_increment.rect.width - via_steps.rect.width).abs() < 1e-3
                && (via_increment.rect.height - via_steps.rect.height).abs() < 1e-3,
            "{handle:?}: Increment must be exactly its own at_steps applied together \
             — {via_increment:?} via the verb, {via_steps:?} via the steps"
        );
    }
}

/// Every action a handle's node *advertises* must actually be in its supported
/// set, because that set is what an AT-SPI or UIA adapter reads to decide the
/// action interface exists at all.
///
/// `set_custom_actions` publishing four descriptions is not the same claim:
/// the router dispatches a `CustomAction` without consulting the supported
/// list, so every test here stays green with the `add_action` deleted and a
/// real screen reader is simply never offered the four directions — the whole
/// change-the-width-without-the-height route, gone silently.
#[test]
fn a_handle_supports_every_action_it_advertises() {
    for handle in crate::transform_session::HANDLE_ORDER {
        let (model, _a, selection) = one_rect_scene();
        let (mut tree, view_id) = mount(model, selection, TransformConfig::new());
        let node = published_handle_node(&mut tree, view_id, handle)
            .unwrap_or_else(|| panic!("{handle:?} must be published"));

        assert!(
            node.supports_action(accesskit::Action::Increment)
                && node.supports_action(accesskit::Action::Decrement),
            "{handle:?} answers Increment/Decrement, so its node must say so"
        );
        let two_dimensional = !handle.is_scalar();
        assert_eq!(
            node.supports_action(accesskit::Action::CustomAction),
            two_dimensional,
            "{handle:?} publishes {} per-axis actions, so CustomAction support must match",
            if two_dimensional { "four" } else { "no" }
        );
    }
}

// ---------------------------------------------------------------------------
// What the controller reads, and what invalidates it
// ---------------------------------------------------------------------------
//
// The controller's memo keys on the three things its answer reads — the model,
// the selection and the enabled flag — and says so. Being right-keyed only
// helps when something asks, and for two of the three nothing did: no binding
// on this view carried a selection change or an `enabled` flip into the
// relayout and accessibility passes. These tests ask the question the way a
// frame asks it, through `render` and through `sync_accessibility` (the
// **cached** door every platform adapter is fed), not through the uncached
// snapshot that was right all along.

/// One frame: flush bindings, lay out, and count every draw command.
fn frame_draws(tree: &mut WidgetTree) -> usize {
    tree.layout(SizeProposal::exact(600.0, 400.0));
    tree.render().draw_order.len()
}

/// Every node the controller publishes — the frame and its handles — as the
/// **published** tree serves them.
fn published_transform_nodes(tree: &mut WidgetTree, view_id: WidgetId) -> Vec<accesskit::NodeId> {
    use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id};
    let ours: Vec<accesskit::NodeId> = (0..=crate::transform_session::HANDLE_ORDER.len() as u64)
        .map(|i| synthetic_node_id(view_id, i, SyntheticKind::SceneHandle))
        .collect();
    tree.sync_accessibility()
        .nodes
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| ours.contains(id))
        .collect()
}

/// What the selection frame's node announces, in the **published** tree.
fn published_frame_value(tree: &mut WidgetTree, view_id: WidgetId) -> Option<String> {
    use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id};
    let frame_id = synthetic_node_id(view_id, 0, SyntheticKind::SceneHandle);
    tree.sync_accessibility()
        .nodes
        .iter()
        .find(|(id, _)| *id == frame_id)
        .and_then(|(_, n)| n.value().map(str::to_owned))
}

/// The same, from a fresh uncached walk — what the tree *would* say.
fn fresh_frame_value(tree: &WidgetTree, view_id: WidgetId) -> Option<String> {
    use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id};
    let frame_id = synthetic_node_id(view_id, 0, SyntheticKind::SceneHandle);
    tree.accessibility_tree_snapshot()
        .nodes
        .iter()
        .find(|(id, _)| *id == frame_id)
        .and_then(|(_, n)| n.value().map(str::to_owned))
}

/// `TransformConfig::enabled` is documented as reactive — the setter takes a
/// `Prop`, and `enabled_signal()` exists "for a toolbar to read or bind". A
/// toolbar toggle wired to it has to take the chrome off the screen *and* out
/// of the tree an assistive technology holds; otherwise a screen reader goes on
/// offering a handle whose actions the flag has just made refuse.
///
/// Calibrated against a controller that was off from the start, so it cannot
/// pass by the chrome having been invisible all along.
#[test]
fn turning_the_controller_off_takes_its_chrome_off_the_screen_and_out_of_the_tree() {
    let off_from_the_start = {
        let (model, _a, selection) = one_rect_scene();
        let cfg = TransformConfig::new();
        cfg.enabled_signal().set(false);
        let (mut tree, view_id) = mount(model, selection, cfg);
        let draws = frame_draws(&mut tree);
        assert!(published_transform_nodes(&mut tree, view_id).is_empty());
        draws
    };

    let (model, _a, selection) = one_rect_scene();
    let cfg = TransformConfig::new();
    let enabled = cfg.enabled_signal();
    let (mut tree, view_id) = mount(model, selection, cfg);

    let on = frame_draws(&mut tree);
    assert!(
        on > off_from_the_start,
        "the chrome must draw something to begin with ({on} vs {off_from_the_start})"
    );
    assert!(
        !published_transform_nodes(&mut tree, view_id).is_empty(),
        "…and publish something"
    );

    enabled.set(false);
    let after = frame_draws(&mut tree);
    assert_eq!(
        after, off_from_the_start,
        "a live `enabled` flip must leave exactly the frame a controller that was \
         never on would have drawn"
    );
    assert_eq!(
        published_transform_nodes(&mut tree, view_id),
        Vec::new(),
        "…and must stop publishing handles whose actions the flag now refuses"
    );

    // Back on again: the flag is a toggle, not a one-way door.
    enabled.set(true);
    assert_eq!(frame_draws(&mut tree), on);
    assert!(!published_transform_nodes(&mut tree, view_id).is_empty());
}

/// A selection change the pointer did not make — "select all", a search result,
/// or the other pane of a shared `SceneSelection` — must reach the published
/// tree. Stated as "the cached door agrees with a fresh walk", with the fresh
/// walk asserted first so the test cannot pass by both being wrong.
#[test]
fn a_programmatic_selection_change_reaches_the_published_tree() {
    let (model, a, b, selection) = two_square_scene();
    selection.replace([a, b]);
    let (mut tree, view_id) = mount(model, selection.clone(), TransformConfig::new());
    tree.layout(SizeProposal::exact(600.0, 400.0));
    assert_eq!(
        published_frame_value(&mut tree, view_id).as_deref(),
        Some("140 by 40"),
        "two 40-wide squares 100 apart"
    );

    selection.replace([a]);
    tree.layout(SizeProposal::exact(600.0, 400.0));
    assert_eq!(
        fresh_frame_value(&tree, view_id).as_deref(),
        Some("40 by 40"),
        "the walk itself follows the selection"
    );
    assert_eq!(
        published_frame_value(&mut tree, view_id),
        fresh_frame_value(&tree, view_id),
        "…and so must the tree an assistive technology actually holds"
    );

    selection.clear();
    tree.layout(SizeProposal::exact(600.0, 400.0));
    assert_eq!(
        published_transform_nodes(&mut tree, view_id),
        Vec::new(),
        "an emptied selection publishes no frame at all"
    );
}

/// The documented multi-view case: two panes over one model, sharing one
/// `SceneSelection`. Selecting from anywhere — including the other pane — must
/// move *both* panes' chrome and both panes' published nodes, or a hit test or
/// a braille route in the quiet pane lands on empty scene.
#[test]
fn a_shared_selection_moves_both_panes_chrome_and_both_published_trees() {
    use teksilo_widgets::{Expand, HStack};

    let (model, a, b, selection) = two_square_scene();
    selection.replace([a, b]);
    let mut tree = WidgetTree::new();
    let pane_a = tree.add(
        SceneView::with_model(model.clone())
            .selection_model(selection.clone())
            .transform_controller(TransformConfig::new()),
    );
    let pane_b = tree.add(
        SceneView::with_model(model)
            .selection_model(selection.clone())
            .transform_controller(TransformConfig::new()),
    );
    tree.add(
        HStack::new()
            .child(Expand::new().child(pane_a))
            .child(Expand::new().child(pane_b)),
    );
    tree.layout(SizeProposal::exact(800.0, 400.0));

    for pane in [pane_a, pane_b] {
        assert_eq!(
            published_frame_value(&mut tree, pane).as_deref(),
            Some("140 by 40")
        );
    }

    // Settle: consume any accessibility dirt the mount left, so what follows
    // can only come from the selection write.
    tree.layout(SizeProposal::exact(800.0, 400.0));
    tree.sync_accessibility();
    tree.sync_accessibility();

    selection.replace([b]);
    tree.layout(SizeProposal::exact(800.0, 400.0));
    for pane in [pane_a, pane_b] {
        assert_eq!(
            published_frame_value(&mut tree, pane).as_deref(),
            Some("40 by 40"),
            "pane {pane:?} kept describing the previous selection"
        );
    }
}

/// The name a handle announces is resolved at **walk** time, which is what
/// keeps a `tr!` label locale-reactive with no rebuild — but only because a
/// locale switch dirties the accessibility tree by itself. A plain
/// `Signal<String>` has nothing doing that for it, so the controller binds its
/// own labels. Same shape as `enabled` and the selection: read at use time,
/// bound nowhere.
#[test]
fn a_reactive_handle_name_reaches_the_published_tree() {
    use crate::transform_session::TransformLabels;
    use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id};

    fn published_label(tree: &mut WidgetTree, view_id: WidgetId) -> Option<String> {
        let node_id = synthetic_node_id(
            view_id,
            TransformHandle::Move.index() as u64 + 1,
            SyntheticKind::SceneHandle,
        );
        tree.sync_accessibility()
            .nodes
            .iter()
            .find(|(id, _)| *id == node_id)
            .and_then(|(_, n)| n.label().map(str::to_owned))
    }

    let name = teksilo_core::signal::Signal::new("Grab".to_string());
    let (model, _a, selection) = one_rect_scene();
    let cfg = TransformConfig::new()
        .labels(TransformLabels::new().handle(TransformHandle::Move, name.clone()));
    let (mut tree, view_id) = mount(model, selection, cfg);
    tree.layout(SizeProposal::exact(600.0, 400.0));
    assert_eq!(published_label(&mut tree, view_id).as_deref(), Some("Grab"));

    name.set("Saisir".to_string());
    tree.layout(SizeProposal::exact(600.0, 400.0));
    assert_eq!(
        published_label(&mut tree, view_id).as_deref(),
        Some("Saisir"),
        "a renamed handle must stop announcing the old name"
    );
}
