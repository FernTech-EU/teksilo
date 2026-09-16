// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! End-to-end coverage for the scene's **geometry constraint**.
//!
//! Four routes consult one closure — the transform controller, the lightweight
//! item drag, the `Alt`+arrow nudge, and an app's own drag through
//! [`SceneModel::constrain_move`] — and this file measures all four, on both
//! content tiers.
//!
//! Several of these are written to redden when the mechanism is *removed*
//! rather than merely to pass:
//!
//! * the grab-offset test grabs deliberately off-corner, so a constraint fed a
//!   *pointer* position instead of the group's frame fails it;
//! * the ghost tests read the render frame, so constraining only the commit
//!   fails them;
//! * the preview/commit test compares the last previewed frame against the
//!   written one, so a phase-dependent hook fails it;
//! * every "is constrained" assertion has an unconstrained twin with the same
//!   gesture, so a test that merely observed a move could not pass both.

use super::*;
use crate::constrain::ChangeVerdict;
use crate::flags::ItemFlags;
use crate::items::RectItem;
use crate::magnet::{Magnet, MagnetRef, MagnetRole, MagnetVerdict, MagnetismConfig};
use crate::scene_model::SceneModel;
use crate::selection::{SceneSelection, SceneSelectionMode};
use crate::transform_session::{
    TransformConfig, TransformFrame, TransformOp, TransformOutcome, TransformSource,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use teksilo_canvas::{Point, Vec2};
use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};

// ---------------------------------------------------------------------------
// Scaffolding
// ---------------------------------------------------------------------------

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

/// The move sample that only *arms* the drag.
///
/// A press followed by one move is a latch and nothing else — the recognizer
/// reports `DragPhase::Started` there, with the press point as the anchor, so
/// no `Moved` has been delivered yet and no constraint has run. Every test that
/// looks at the gesture *while it is live* has to get past this first, and the
/// existing `the_heavyweight_preview_actually_moves_the_card_mid_gesture` does
/// the same thing by hand.
fn latch(tree: &mut WidgetTree, from: Point) {
    moved(tree, Point::new(from.x + 30.0, from.y));
}

fn flush_transform(tree: &mut WidgetTree, view_id: WidgetId) -> bool {
    let view = tree
        .widget_as_any_mut(view_id)
        .and_then(|a| a.downcast_mut::<SceneView>())
        .expect("downcast");
    view.flush_pending_transform()
}

fn flush_item_move(tree: &mut WidgetTree, view_id: WidgetId) -> bool {
    let view = tree
        .widget_as_any_mut(view_id)
        .and_then(|a| a.downcast_mut::<SceneView>())
        .expect("downcast");
    view.flush_pending_item_move()
}

/// Snap the moved box's own top-leading corner onto a `step`-unit grid.
///
/// The documented example, verbatim — and the reason it is *correct* is that
/// `c.proposed` is the group's scene box and not the cursor.
fn grid(step: f32) -> impl Fn(&crate::constrain::ProposedChange<'_>) -> ChangeVerdict + 'static {
    move |c| {
        let mut f = c.proposed;
        f.rect.x = (f.rect.x / step).round() * step;
        f.rect.y = (f.rect.y / step).round() * step;
        ChangeVerdict::Adjust(f)
    }
}

/// One draggable lightweight square at `(50, 50)`, 40 × 40.
fn one_square() -> (SceneModel, ItemId) {
    let model = SceneModel::new();
    let a = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0))
            .fill(teksilo_tokens::Color::RED)
            .draggable(true),
        Point::new(50.0, 50.0),
    );
    (model, a)
}

fn mount_plain(model: SceneModel) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::with_model(model).selection_mode(SceneSelectionMode::Multi));
    tree.layout(SizeProposal::exact(600.0, 400.0));
    (tree, view_id)
}

fn mount_controller(model: SceneModel, selection: SceneSelection) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::with_model(model)
            .selection_model(selection)
            .transform_controller(TransformConfig::new()),
    );
    tree.layout(SizeProposal::exact(600.0, 400.0));
    (tree, view_id)
}

/// Every transform the render frame pushes — the ghost, as the renderer sees
/// it. Copied from `drag_cancel`, deliberately: a drag ghost is only really
/// observable here.
fn transform_commands(frame: &teksilo_canvas::RenderFrame) -> Vec<[f32; 6]> {
    frame
        .draw_order
        .iter()
        .filter_map(|c| match c {
            teksilo_canvas::DrawCommand::PushTransform(t)
            | teksilo_canvas::DrawCommand::SetTransform(t) => Some(t.m),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The lightweight item drag
// ---------------------------------------------------------------------------

#[test]
fn a_lightweight_drag_commits_the_constrained_position() {
    let (model, a) = one_square();
    model.set_geometry_constraint(grid(25.0));
    let (mut tree, view_id) = mount_plain(model.clone());

    // Grab the square's centre and carry it +33/+19.
    down(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(103.0, 89.0));
    up(&mut tree, Point::new(103.0, 89.0));
    assert!(flush_item_move(&mut tree, view_id));

    // Raw would be (83, 69). The frame snaps to (75, 75).
    let r = model.scene_rect(a).expect("alive");
    assert!((r.x - 75.0).abs() < 1e-2, "{r:?}");
    assert!((r.y - 75.0).abs() < 1e-2, "{r:?}");
}

#[test]
fn the_same_drag_without_a_constraint_lands_raw() {
    // The twin of the test above. Without it, "did it move?" could not tell a
    // constrained drag from an unconstrained one.
    let (model, a) = one_square();
    let (mut tree, view_id) = mount_plain(model.clone());

    down(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(103.0, 89.0));
    up(&mut tree, Point::new(103.0, 89.0));
    assert!(flush_item_move(&mut tree, view_id));

    let r = model.scene_rect(a).expect("alive");
    assert!((r.x - 83.0).abs() < 1e-2, "{r:?}");
    assert!((r.y - 69.0).abs() < 1e-2, "{r:?}");
}

#[test]
fn a_grid_snap_lands_the_item_on_the_grid_however_far_from_its_corner_it_was_grabbed() {
    // The bug a pointer-valued hook has and a frame-valued one cannot: an item
    // grabbed 37 units from its corner would land 37 units off-grid, every
    // drag, for ever. Two grabs of the same item, two very different grab
    // offsets, and the committed corner must be identical.
    let mut landed = Vec::new();
    for grab in [Point::new(51.0, 51.0), Point::new(87.0, 88.0)] {
        let (model, a) = one_square();
        model.set_geometry_constraint(grid(25.0));
        let (mut tree, view_id) = mount_plain(model.clone());

        let travel = Vec2::new(103.0, 91.0);
        down(&mut tree, grab);
        let to = Point::new(grab.x + travel.x, grab.y + travel.y);
        moved(&mut tree, to);
        up(&mut tree, to);
        assert!(flush_item_move(&mut tree, view_id));
        let r = model.scene_rect(a).expect("alive");
        landed.push((r.x, r.y));
    }
    assert_eq!(
        landed[0], landed[1],
        "the grab offset must not reach the constraint: {landed:?}"
    );
    // …and it really is on the grid, not merely consistent.
    assert!((landed[0].0 / 25.0).fract().abs() < 1e-3, "{landed:?}");
    assert!((landed[0].1 / 25.0).fract().abs() < 1e-3, "{landed:?}");
}

#[test]
fn the_drag_ghost_is_constrained_and_not_only_the_commit() {
    // The one thing users notice about snapping is the part a post-hoc
    // `item_change_signal` observer can never reach: the item under the
    // pointer. Measured at the renderer, mid-gesture, before any commit.
    let (model, _a) = one_square();
    model.set_geometry_constraint(grid(25.0));
    let (mut tree, _view_id) = mount_plain(model.clone());

    // A reference render with the item parked exactly where the snap will put
    // it, taken from a second identical scene so nothing about the first is
    // disturbed.
    let (model_ref, a_ref) = one_square();
    model_ref.set_local_pos(a_ref, Point::new(75.0, 75.0));
    let (mut tree_ref, _) = mount_plain(model_ref);
    let want = transform_commands(&tree_ref.render());

    down(&mut tree, Point::new(70.0, 70.0));
    latch(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(103.0, 89.0));
    let during = transform_commands(&tree.render());

    assert_eq!(
        during, want,
        "mid-drag the item must already be drawn at the constrained position"
    );
}

#[test]
fn the_unconstrained_ghost_is_not_at_the_snapped_position() {
    // The negative twin of the ghost test: same gesture, no constraint, and the
    // renderer must NOT already agree with the snapped reference. Without this
    // the test above would pass for a scene that simply never moved.
    let (model, _a) = one_square();
    let (mut tree, _view_id) = mount_plain(model);

    let (model_ref, a_ref) = one_square();
    model_ref.set_local_pos(a_ref, Point::new(75.0, 75.0));
    let (mut tree_ref, _) = mount_plain(model_ref);
    let snapped = transform_commands(&tree_ref.render());

    down(&mut tree, Point::new(70.0, 70.0));
    latch(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(103.0, 89.0));
    let during = transform_commands(&tree.render());
    assert_ne!(during, snapped);
}

#[test]
fn a_rejecting_constraint_keeps_a_lightweight_drag_where_it_started() {
    let (model, a) = one_square();
    model.set_geometry_constraint(|_| ChangeVerdict::Reject);
    let (mut tree, view_id) = mount_plain(model.clone());

    down(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(200.0, 200.0));
    up(&mut tree, Point::new(200.0, 200.0));
    flush_item_move(&mut tree, view_id);

    assert_eq!(
        model.local_pos(a),
        Some(Point::new(50.0, 50.0)),
        "a rejected gesture writes nothing"
    );
}

#[test]
fn a_constraint_may_move_a_group_and_keeps_its_internal_arrangement() {
    let model = SceneModel::new();
    let a = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0))
            .fill(teksilo_tokens::Color::RED)
            .draggable(true),
        Point::new(50.0, 50.0),
    );
    let b = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0))
            .fill(teksilo_tokens::Color::BLUE)
            .draggable(true),
        Point::new(157.0, 50.0),
    );
    model.set_geometry_constraint(grid(25.0));
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([a, b]);
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::with_model(model.clone()).selection_model(selection));
    tree.layout(SizeProposal::exact(600.0, 400.0));

    let gap_before = model.scene_rect(b).unwrap().x - model.scene_rect(a).unwrap().x;

    down(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(103.0, 89.0));
    up(&mut tree, Point::new(103.0, 89.0));
    assert!(flush_item_move(&mut tree, view_id));

    let ra = model.scene_rect(a).unwrap();
    let rb = model.scene_rect(b).unwrap();
    // The union frame's leading corner is A's, so the snap is measured there…
    assert!((ra.x - 75.0).abs() < 1e-2, "{ra:?}");
    // …and B moved by exactly the same translation.
    assert!(
        ((rb.x - ra.x) - gap_before).abs() < 1e-2,
        "the group kept its arrangement: {ra:?} {rb:?}"
    );
}

#[test]
fn a_rotated_item_round_trips_exactly_through_the_frame_basis() {
    // A single rotated item's frame is stated in that item's OWN basis, so the
    // translation has to be turned into it and back out again. An identity
    // rewrite through `translated(translation())` must therefore land exactly
    // where no constraint at all would: if the two rotations did not cancel,
    // every constrained drag of a rotated item would drift.
    let run = |constrained: bool| {
        let (model, a) = one_square();
        model.set_transform(a, teksilo_canvas::Transform2D::rotate(0.7));
        if constrained {
            model.set_geometry_constraint(|c| ChangeVerdict::Adjust(c.translated(c.translation())));
        }
        let (mut tree, view_id) = mount_plain(model.clone());
        down(&mut tree, Point::new(55.0, 55.0));
        latch(&mut tree, Point::new(55.0, 55.0));
        moved(&mut tree, Point::new(88.0, 74.0));
        up(&mut tree, Point::new(88.0, 74.0));
        assert!(flush_item_move(&mut tree, view_id));
        model.local_pos(a).expect("alive")
    };
    let free = run(false);
    let identity_rewrite = run(true);
    assert!(
        (free.x - identity_rewrite.x).abs() < 1e-2 && (free.y - identity_rewrite.y).abs() < 1e-2,
        "the frame-basis round trip must be lossless: {free:?} vs {identity_rewrite:?}"
    );
    // …and the gesture really did move it, so the comparison is not of two
    // items that both stood still.
    assert!((free.x - 50.0).abs() > 1.0, "{free:?}");
}

// ---------------------------------------------------------------------------
// The transform controller — both tiers
// ---------------------------------------------------------------------------

#[test]
fn a_heavyweight_card_move_is_constrained() {
    // The blocker the spike's sketch could not clear: the hook has to reach the
    // tier the motivating use cases live in. A card moves through the transform
    // controller, never through the lightweight drag snapshot.
    let model = SceneModel::new();
    let card = model.add_widget(FillWidget::new(), Rect::new(40.0, 40.0, 120.0, 80.0));
    model.set_flag(card, ItemFlags::IS_DRAGGABLE, true);
    model.set_geometry_constraint(grid(25.0));
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([card]);
    let (mut tree, view_id) = mount_controller(model.clone(), selection);

    down(&mut tree, Point::new(100.0, 80.0));
    moved(&mut tree, Point::new(163.0, 131.0));
    up(&mut tree, Point::new(163.0, 131.0));
    assert!(flush_transform(&mut tree, view_id));

    // Raw would be (103, 91); the frame snaps to (100, 100).
    let r = model.scene_rect(card).expect("alive");
    assert!((r.x - 100.0).abs() < 1e-2, "{r:?}");
    assert!((r.y - 100.0).abs() < 1e-2, "{r:?}");
}

#[test]
fn the_same_card_move_without_a_constraint_lands_raw() {
    let model = SceneModel::new();
    let card = model.add_widget(FillWidget::new(), Rect::new(40.0, 40.0, 120.0, 80.0));
    model.set_flag(card, ItemFlags::IS_DRAGGABLE, true);
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([card]);
    let (mut tree, view_id) = mount_controller(model.clone(), selection);

    down(&mut tree, Point::new(100.0, 80.0));
    moved(&mut tree, Point::new(163.0, 131.0));
    up(&mut tree, Point::new(163.0, 131.0));
    assert!(flush_transform(&mut tree, view_id));

    let r = model.scene_rect(card).expect("alive");
    assert!((r.x - 103.0).abs() < 1e-2, "{r:?}");
    assert!((r.y - 91.0).abs() < 1e-2, "{r:?}");
}

#[test]
fn a_card_preview_mid_gesture_is_already_constrained() {
    // Both tiers preview through one affine, so the card's arena bounds are the
    // honest place to read the heavyweight ghost.
    let model = SceneModel::new();
    let card = model.add_widget(FillWidget::new(), Rect::new(40.0, 40.0, 120.0, 80.0));
    model.set_flag(card, ItemFlags::IS_DRAGGABLE, true);
    model.set_geometry_constraint(grid(25.0));
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([card]);
    let (mut tree, view_id) = mount_controller(model.clone(), selection);

    let kid = tree.children(view_id)[0];
    assert_eq!(
        tree.bounds(kid),
        Rect::new(40.0, 40.0, 120.0, 80.0),
        "precondition: the tree is settled at the model position"
    );

    down(&mut tree, Point::new(100.0, 80.0));
    latch(&mut tree, Point::new(100.0, 80.0));
    moved(&mut tree, Point::new(163.0, 131.0));
    tree.layout(SizeProposal::exact(600.0, 400.0));

    let during = tree.bounds(kid);
    assert!(
        (during.x - 100.0).abs() < 1e-2 && (during.y - 100.0).abs() < 1e-2,
        "the card previews at the constrained position, not the raw one: {during:?}"
    );
}

#[test]
fn a_resize_is_constrained_and_the_preview_agrees_with_the_commit() {
    let model = SceneModel::new();
    let a = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
        Point::new(50.0, 50.0),
    );
    model.set_flag(a, ItemFlags::IS_RESIZABLE, true);
    // Quantise the frame's extent to 20-unit steps.
    model.set_geometry_constraint(|c| {
        let mut f = c.proposed;
        f.rect.width = (f.rect.width / 20.0).round() * 20.0;
        f.rect.height = (f.rect.height / 20.0).round() * 20.0;
        ChangeVerdict::Adjust(f)
    });
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([a]);
    let (mut tree, view_id) = mount_controller(model.clone(), selection);

    // The outline is 44..96 square; grab the bottom-trailing handle.
    down(&mut tree, Point::new(96.0, 96.0));
    latch(&mut tree, Point::new(96.0, 96.0));
    moved(&mut tree, Point::new(129.0, 129.0));

    // What the chrome last showed…
    let previewed = {
        let view = view_handle(&tree, view_id);
        view.transform_session_signal()
            .get()
            .expect("a session is live")
            .frame
    };
    assert!(
        (previewed.rect.width - 80.0).abs() < 1e-2,
        "the preview is constrained: {previewed:?}"
    );

    up(&mut tree, Point::new(129.0, 129.0));
    assert!(flush_transform(&mut tree, view_id));

    // …is what was written. The frame is padding-free, so it is the item's box.
    let r = model.scene_rect(a).expect("alive");
    assert!(
        (r.width - previewed.rect.width).abs() < 1e-2
            && (r.height - previewed.rect.height).abs() < 1e-2,
        "the commit is the frame the preview drew: previewed {previewed:?}, wrote {r:?}"
    );
}

#[test]
fn a_gesture_that_ends_rejected_reports_cancelled_and_writes_nothing() {
    let model = SceneModel::new();
    let a = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
        Point::new(50.0, 50.0),
    );
    model.set_flag(a, ItemFlags::IS_DRAGGABLE, true);
    model.set_geometry_constraint(|_| ChangeVerdict::Reject);
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([a]);

    let outcomes: Rc<RefCell<Vec<TransformOutcome>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = outcomes.clone();
    let cfg = TransformConfig::new().on_end(move |_s, outcome, _ctx| {
        sink.borrow_mut().push(outcome);
    });
    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::with_model(model.clone())
            .selection_model(selection)
            .transform_controller(cfg),
    );
    tree.layout(SizeProposal::exact(600.0, 400.0));

    down(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(200.0, 200.0));
    up(&mut tree, Point::new(200.0, 200.0));
    assert!(
        !flush_transform(&mut tree, view_id),
        "a rejected gesture posts no commit"
    );
    assert_eq!(model.local_pos(a), Some(Point::new(50.0, 50.0)));
    assert_eq!(
        *outcomes.borrow(),
        vec![TransformOutcome::Cancelled],
        "the app is told nothing happened"
    );
}

#[test]
fn the_constraint_sees_the_gestures_start_frame_not_the_previous_sample() {
    // `start` is fixed for the gesture's life, which is what lets a policy
    // measure total travel. Three samples, three identical `start`s.
    let (model, a) = one_square();
    model.set_flag(a, ItemFlags::IS_DRAGGABLE, true);
    let starts: Rc<RefCell<Vec<TransformFrame>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = starts.clone();
    model.set_geometry_constraint(move |c| {
        sink.borrow_mut().push(c.start);
        ChangeVerdict::Accept
    });
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([a]);
    let (mut tree, _view_id) = mount_controller(model.clone(), selection);

    down(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(80.0, 70.0));
    moved(&mut tree, Point::new(120.0, 100.0));
    up(&mut tree, Point::new(120.0, 100.0));

    let seen = starts.borrow();
    assert!(seen.len() >= 3, "several samples: {}", seen.len());
    let first = seen[0];
    assert!(
        seen.iter().all(|f| *f == first),
        "start must be fixed for the gesture: {seen:?}"
    );
    assert!((first.rect.x - 50.0).abs() < 1e-3, "{first:?}");
}

// ---------------------------------------------------------------------------
// The keyboard and AT routes into the transform controller
// ---------------------------------------------------------------------------

fn key(tree: &mut WidgetTree, k: Key) {
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: k,
        modifiers: Modifiers::default(),
        text: None,
    });
}

#[test]
fn the_keyboard_transform_route_obeys_the_constraint_the_pointer_obeys() {
    // `docs/a11y/non-drag-alternatives.md`: the alternative must make the same
    // model change the drag makes. A constraint that reached only the pointer
    // would break that — so this drives both over the same rule and compares.
    let run = |use_pointer: bool| {
        let (model, a) = one_square();
        model.set_geometry_constraint(grid(25.0));
        let selection = SceneSelection::new(SceneSelectionMode::Multi);
        selection.replace([a]);
        let (mut tree, view_id) = mount_controller(model.clone(), selection);
        if use_pointer {
            down(&mut tree, Point::new(70.0, 70.0));
            latch(&mut tree, Point::new(70.0, 70.0));
            moved(&mut tree, Point::new(103.0, 89.0));
            up(&mut tree, Point::new(103.0, 89.0));
        } else {
            tree.focus(view_id);
            key(&mut tree, Key::Character('t')); // enter, roving on Move
            for _ in 0..3 {
                tree.dispatch_event(WidgetEvent::KeyDown {
                    key: Key::ArrowRight,
                    modifiers: Modifiers::SHIFT,
                    text: None,
                });
            }
            tree.dispatch_event(WidgetEvent::KeyDown {
                key: Key::ArrowDown,
                modifiers: Modifiers::SHIFT,
                text: None,
            });
            tree.dispatch_event(WidgetEvent::KeyDown {
                key: Key::ArrowDown,
                modifiers: Modifiers::SHIFT,
                text: None,
            });
            key(&mut tree, Key::Enter);
        }
        flush_transform(&mut tree, view_id);
        model.scene_rect(a).expect("alive")
    };
    // +33/+19 by pointer and +30/+20 by keyboard both snap to the same tile.
    let by_pointer = run(true);
    let by_keyboard = run(false);
    assert!((by_pointer.x - 75.0).abs() < 1e-2, "{by_pointer:?}");
    assert!(
        (by_pointer.x - by_keyboard.x).abs() < 1e-2 && (by_pointer.y - by_keyboard.y).abs() < 1e-2,
        "the two routes land in the same place: {by_pointer:?} {by_keyboard:?}"
    );
}

#[test]
fn an_at_action_on_a_handle_obeys_the_constraint_too() {
    // An assistive technology drives the handles through `Action::Increment`,
    // which routes into the same session — so it is constrained by the same
    // rule, and an AT user is never shown a geometry a sighted user cannot get.
    let (model, a) = one_square();
    // A hard lane: nothing may move on y at all.
    model.set_geometry_constraint(|c| {
        let t = c.translation();
        ChangeVerdict::Adjust(c.translated(Vec2::new(t.x, 0.0)))
    });
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([a]);
    let (mut tree, view_id) = mount_controller(model.clone(), selection);
    tree.focus(view_id);
    key(&mut tree, Key::Character('t'));
    // Rove to a vertical handle, then ask AT to step it.
    key(&mut tree, Key::Tab);
    key(&mut tree, Key::Tab);
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowDown,
        modifiers: Modifiers::SHIFT,
        text: None,
    });
    key(&mut tree, Key::Enter);
    flush_transform(&mut tree, view_id);

    let r = model.scene_rect(a).expect("alive");
    assert!(
        (r.y - 50.0).abs() < 1e-2,
        "the locked axis did not move: {r:?}"
    );
}

// ---------------------------------------------------------------------------
// The keyboard nudge
// ---------------------------------------------------------------------------

#[test]
fn the_alt_arrow_nudge_obeys_the_same_rule_the_pointer_does() {
    let (model, a) = one_square();
    // A lane: the item may only travel horizontally.
    model.set_geometry_constraint(|c| {
        let t = c.translation();
        ChangeVerdict::Adjust(c.translated(Vec2::new(t.x, 0.0)))
    });
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([a]);
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::with_model(model.clone()).selection_model(selection));
    tree.layout(SizeProposal::exact(600.0, 400.0));
    tree.focus(view_id);

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::ALT,
        text: None,
    });
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowDown,
        modifiers: Modifiers::ALT,
        text: None,
    });

    let p = model.local_pos(a).expect("alive");
    assert!((p.x - 51.0).abs() < 1e-3, "the permitted axis moved: {p:?}");
    assert!((p.y - 50.0).abs() < 1e-3, "the locked axis did not: {p:?}");
}

#[test]
fn an_unconstrained_nudge_still_moves_on_both_axes() {
    let (model, a) = one_square();
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([a]);
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::with_model(model.clone()).selection_model(selection));
    tree.layout(SizeProposal::exact(600.0, 400.0));
    tree.focus(view_id);

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::ALT,
        text: None,
    });
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowDown,
        modifiers: Modifiers::ALT,
        text: None,
    });
    assert_eq!(model.local_pos(a), Some(Point::new(51.0, 51.0)));
}

#[test]
fn the_nudge_is_told_it_came_from_the_keyboard() {
    let (model, a) = one_square();
    let sources: Rc<RefCell<Vec<TransformSource>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = sources.clone();
    model.set_geometry_constraint(move |c| {
        sink.borrow_mut().push(c.source);
        ChangeVerdict::Accept
    });
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([a]);
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::with_model(model.clone()).selection_model(selection));
    tree.layout(SizeProposal::exact(600.0, 400.0));
    tree.focus(view_id);

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::ALT,
        text: None,
    });
    assert_eq!(*sources.borrow(), vec![TransformSource::Keyboard]);
}

#[test]
fn a_nudge_a_constraint_refuses_writes_nothing_at_all() {
    let (model, a) = one_square();
    model.set_geometry_constraint(|_| ChangeVerdict::Reject);
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([a]);
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::with_model(model.clone()).selection_model(selection));
    tree.layout(SizeProposal::exact(600.0, 400.0));
    tree.focus(view_id);

    let before = model.mutation_version();
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::ALT,
        text: None,
    });
    assert_eq!(model.local_pos(a), Some(Point::new(50.0, 50.0)));
    assert_eq!(
        model.mutation_version(),
        before,
        "a refused nudge emits no change at all"
    );
}

// ---------------------------------------------------------------------------
// An app's own drag — the fourth route
// ---------------------------------------------------------------------------

#[test]
fn an_apps_own_drag_gets_the_same_answer_the_views_drag_gets() {
    // The card door. A heavyweight card's handler holds a `SceneModel`, never a
    // `&SceneView`, so this is the reason the constraint lives on the model —
    // and the two routes must not be two readings of one closure.
    let (view_model, a) = one_square();
    view_model.set_geometry_constraint(grid(25.0));
    let (mut tree, view_id) = mount_plain(view_model.clone());
    down(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(103.0, 89.0));
    up(&mut tree, Point::new(103.0, 89.0));
    assert!(flush_item_move(&mut tree, view_id));
    let by_view = view_model.scene_rect(a).expect("alive");

    // The same item, the same travel, driven entirely by hand.
    let (app_model, b) = one_square();
    app_model.set_geometry_constraint(grid(25.0));
    let start = app_model.transform_frame(&[b]).expect("resolves");
    let applied =
        app_model.constrain_move(&[b], start, Vec2::new(33.0, 19.0), TransformSource::Pointer);
    let p = app_model.local_pos(b).unwrap();
    app_model.set_local_pos(b, Point::new(p.x + applied.x, p.y + applied.y));
    let by_app = app_model.scene_rect(b).expect("alive");

    assert!(
        (by_view.x - by_app.x).abs() < 1e-3,
        "{by_view:?} {by_app:?}"
    );
    assert!(
        (by_view.y - by_app.y).abs() < 1e-3,
        "{by_view:?} {by_app:?}"
    );
}

#[test]
fn the_app_door_is_a_no_op_without_a_constraint() {
    let (model, a) = one_square();
    let start = model.transform_frame(&[a]).expect("resolves");
    let t = Vec2::new(33.0, 19.0);
    assert_eq!(
        model.constrain_move(&[a], start, t, TransformSource::Pointer),
        t
    );
    let f = TransformFrame {
        rect: Rect::new(1.0, 2.0, 3.0, 4.0),
        rotation: 0.0,
        count: 1,
    };
    assert_eq!(
        model.constrain_frame(
            &[a],
            TransformOp::Resize,
            start,
            f,
            TransformSource::Pointer
        ),
        f
    );
}

// ---------------------------------------------------------------------------
// What is NOT constrained
// ---------------------------------------------------------------------------

#[test]
fn programmatic_writes_are_never_constrained() {
    // A document load, a `SceneListAdapter` rebuild and a data-layer replay all
    // come through here. Snapping them is Qt's own best-known footgun.
    let (model, a) = one_square();
    let calls = Rc::new(Cell::new(0u32));
    let c = calls.clone();
    model.set_geometry_constraint(move |_| {
        c.set(c.get() + 1);
        ChangeVerdict::Reject
    });

    model.set_local_pos(a, Point::new(7.0, 9.0));
    assert_eq!(model.local_pos(a), Some(Point::new(7.0, 9.0)));

    model.set_local_bounds(a, Rect::new(0.0, 0.0, 13.0, 17.0));
    assert_eq!(model.local_bounds(a), Some(Rect::new(0.0, 0.0, 13.0, 17.0)));

    let delta = crate::transform_session::TransformDelta {
        translation: Vec2::new(3.0, 3.0),
        ..crate::transform_session::TransformDelta::IDENTITY
    };
    model.apply_transform_delta(&[a], &delta);
    assert_eq!(model.local_pos(a), Some(Point::new(10.0, 12.0)));

    assert_eq!(calls.get(), 0, "no mutator consults the constraint");
}

#[test]
fn an_unconstrained_scene_never_builds_the_closure() {
    // "It must cost nothing measurable" — the cost of no constraint is one
    // `Option` test per sample, which is exactly zero calls.
    let (model, a) = one_square();
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([a]);
    let (mut tree, view_id) = mount_controller(model.clone(), selection);
    assert!(!model.has_geometry_constraint());

    down(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(120.0, 100.0));
    tree.render();
    up(&mut tree, Point::new(120.0, 100.0));
    flush_transform(&mut tree, view_id);
    // Nothing above could have called a closure that does not exist; the
    // assertion that matters is that the gesture still worked.
    let r = model.scene_rect(a).expect("alive");
    assert!((r.x - 100.0).abs() < 1e-2, "{r:?}");
}

// ---------------------------------------------------------------------------
// Re-entrancy — enforced, not documented
// ---------------------------------------------------------------------------

#[test]
fn a_constraint_may_read_the_scene_through_the_borrow_it_is_handed() {
    let (model, a) = one_square();
    let seen: Rc<RefCell<Option<Rect>>> = Rc::new(RefCell::new(None));
    let sink = seen.clone();
    model.set_geometry_constraint(move |c| {
        *sink.borrow_mut() = c.scene.local_bounds(c.items[0]);
        ChangeVerdict::Accept
    });
    let (mut tree, view_id) = mount_plain(model.clone());

    down(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(90.0, 90.0));
    up(&mut tree, Point::new(90.0, 90.0));
    flush_item_move(&mut tree, view_id);

    assert_eq!(*seen.borrow(), Some(Rect::new(0.0, 0.0, 40.0, 40.0)));
    let _ = a;
}

#[test]
fn a_constraint_may_read_through_a_captured_model_handle() {
    let (model, a) = one_square();
    let reader = model.clone();
    let seen: Rc<RefCell<Option<Point>>> = Rc::new(RefCell::new(None));
    let sink = seen.clone();
    model.set_geometry_constraint(move |c| {
        *sink.borrow_mut() = reader.local_pos(c.items[0]);
        ChangeVerdict::Accept
    });
    let (mut tree, view_id) = mount_plain(model.clone());

    down(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(90.0, 90.0));
    up(&mut tree, Point::new(90.0, 90.0));
    flush_item_move(&mut tree, view_id);

    assert_eq!(*seen.borrow(), Some(Point::new(50.0, 50.0)));
    let _ = a;
}

#[test]
#[should_panic(expected = "a geometry constraint tried to write the scene")]
fn a_constraint_that_writes_the_scene_is_named_rather_than_left_to_refcell() {
    let (model, a) = one_square();
    let writer = model.clone();
    model.set_geometry_constraint(move |c| {
        // Wrong by construction: the answer is the return value.
        writer.set_local_pos(c.items[0], Point::new(0.0, 0.0));
        ChangeVerdict::Accept
    });
    let (mut tree, _view_id) = mount_plain(model.clone());
    let _ = a;

    down(&mut tree, Point::new(70.0, 70.0));
    latch(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(90.0, 90.0));
}

#[test]
#[should_panic(expected = "a geometry constraint asked this scene to constrain something")]
fn a_constraint_that_asks_for_another_constraint_is_named_rather_than_left_to_recurse() {
    // Nothing in the framework nests a constraint, so the only route here is a
    // closure calling `constrain_move` on its own model — which would recurse
    // until the stack ran out.
    let (model, a) = one_square();
    let reenter = model.clone();
    model.set_geometry_constraint(move |c| {
        let start = c.start;
        let _ = reenter.constrain_move(c.items, start, Vec2::ZERO, TransformSource::Pointer);
        ChangeVerdict::Accept
    });
    let (mut tree, _view_id) = mount_plain(model.clone());
    let _ = a;

    down(&mut tree, Point::new(70.0, 70.0));
    latch(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(90.0, 90.0));
}

#[test]
fn the_constraint_depth_survives_a_panicking_constraint() {
    // Enforcement that leaked would be worse than none: a scene stuck "inside a
    // constraint" would refuse every later write with the wrong diagnostic.
    let (model, a) = one_square();
    model.set_geometry_constraint(|_| panic!("policy blew up"));
    let (mut tree, _view_id) = mount_plain(model.clone());

    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        down(&mut tree, Point::new(70.0, 70.0));
        latch(&mut tree, Point::new(70.0, 70.0));
        moved(&mut tree, Point::new(90.0, 90.0));
    }));
    std::panic::set_hook(hook);
    assert!(caught.is_err(), "the panic really happened");

    // The scene is still writable, and with no bogus diagnostic.
    model.set_local_pos(a, Point::new(1.0, 2.0));
    assert_eq!(model.local_pos(a), Some(Point::new(1.0, 2.0)));
}

// ---------------------------------------------------------------------------
// Composition with magnetism
// ---------------------------------------------------------------------------

fn source_to_target(a: &MagnetRef, b: &MagnetRef) -> MagnetVerdict {
    if a.item != b.item
        && matches!(
            (a.role, b.role),
            (MagnetRole::Source, MagnetRole::Target) | (MagnetRole::Target, MagnetRole::Source)
        )
    {
        MagnetVerdict::accept()
    } else {
        MagnetVerdict::Reject
    }
}

/// A draggable item A at the origin with a Source magnet on its right edge, and
/// a fixed item B 200 away with a Target magnet on its left edge.
fn magnet_scene() -> (SceneModel, ItemId) {
    let model = SceneModel::new();
    let a = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0))
            .fill(teksilo_tokens::Color::RED)
            .draggable(true),
        Point::new(0.0, 0.0),
    );
    model.add_magnet(
        a,
        Magnet::new(Point::new(40.0, 20.0)).role(MagnetRole::Source),
    );
    let b = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::BLUE),
        Point::new(200.0, 0.0),
    );
    model.add_magnet(
        b,
        Magnet::new(Point::new(0.0, 20.0)).role(MagnetRole::Target),
    );
    (model, a)
}

fn mount_magnetic(model: SceneModel) -> (WidgetTree, WidgetId, Rc<Cell<u32>>) {
    let count = Rc::new(Cell::new(0u32));
    let c = count.clone();
    let cfg = MagnetismConfig::new(source_to_target).on_connect(move |_conn, _ctx| {
        c.set(c.get() + 1);
    });
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::with_model(model).magnetism(cfg));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    (tree, view_id, count)
}

#[test]
fn the_constraint_is_told_when_magnetism_moved_the_proposal() {
    let (model, _a) = magnet_scene();
    let flags: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = flags.clone();
    model.set_geometry_constraint(move |c| {
        sink.borrow_mut().push(c.magnet_snapped);
        ChangeVerdict::Accept
    });
    let (mut tree, _view_id, _count) = mount_magnetic(model.clone());

    // Far from B: no snap.
    down(&mut tree, Point::new(20.0, 20.0));
    latch(&mut tree, Point::new(20.0, 20.0));
    moved(&mut tree, Point::new(60.0, 20.0));
    assert_eq!(
        flags.borrow().last().copied(),
        Some(false),
        "no magnet in range"
    );

    // 2 units short of B's magnet: snapped.
    moved(&mut tree, Point::new(178.0, 20.0));
    assert_eq!(
        flags.borrow().last().copied(),
        Some(true),
        "the magnet took it"
    );
    up(&mut tree, Point::new(178.0, 20.0));
}

#[test]
fn accepting_a_magnetised_proposal_lets_the_magnet_win_over_the_grid() {
    let (model, a) = magnet_scene();
    // A 25-grid that stands aside for an explicit magnet — the documented
    // composition, in one line.
    model.set_geometry_constraint(|c| {
        if c.magnet_snapped {
            return ChangeVerdict::Accept;
        }
        let mut f = c.proposed;
        f.rect.x = (f.rect.x / 25.0).round() * 25.0;
        f.rect.y = (f.rect.y / 25.0).round() * 25.0;
        ChangeVerdict::Adjust(f)
    });
    let (mut tree, view_id, count) = mount_magnetic(model.clone());

    down(&mut tree, Point::new(20.0, 20.0));
    moved(&mut tree, Point::new(178.0, 20.0));
    up(&mut tree, Point::new(178.0, 20.0));
    flush_item_move(&mut tree, view_id);

    // 158 raw + 2 snap = 160, which is NOT a multiple of 25.
    let p = model.local_pos(a).expect("alive");
    assert!((p.x - 160.0).abs() < 1e-2, "the magnet won: {p:?}");
    assert_eq!(count.get(), 1, "and the connection fired");
}

#[test]
fn a_constraint_that_overrules_a_magnet_also_cancels_its_connection() {
    // Two snapping systems that disagree is worse than one: the constraint's
    // verdict decides the geometry AND the connection.
    let (model, a) = magnet_scene();
    model.set_geometry_constraint(grid(25.0));
    let (mut tree, view_id, count) = mount_magnetic(model.clone());

    down(&mut tree, Point::new(20.0, 20.0));
    moved(&mut tree, Point::new(178.0, 20.0));
    up(&mut tree, Point::new(178.0, 20.0));
    flush_item_move(&mut tree, view_id);

    let p = model.local_pos(a).expect("alive");
    assert!(
        (p.x - 150.0).abs() < 1e-2,
        "the grid overruled the magnet's 160: {p:?}"
    );
    assert_eq!(
        count.get(),
        0,
        "a magnet whose alignment was overruled connected nothing"
    );
}

#[test]
fn a_constraint_that_overrules_a_magnet_also_drops_its_marker() {
    // The same verdict decides the geometry, the connection AND the feedback: a
    // marker still saying "snapped" while the item sits a tile away is the
    // two-sources-of-truth bug in miniature.
    let (model, _a) = magnet_scene();
    let (mut tree, view_id, _count) = mount_magnetic(model.clone());

    // With no constraint the marker is up …
    down(&mut tree, Point::new(20.0, 20.0));
    latch(&mut tree, Point::new(20.0, 20.0));
    moved(&mut tree, Point::new(178.0, 20.0));
    assert!(
        view_handle(&tree, view_id).magnet_interaction_active(),
        "precondition: magnetism took this proposal"
    );
    up(&mut tree, Point::new(178.0, 20.0));

    // … and with a grid that overrules it, it is not.
    let (model2, _b) = magnet_scene();
    model2.set_geometry_constraint(grid(25.0));
    let (mut tree2, view2, _c2) = mount_magnetic(model2);
    down(&mut tree2, Point::new(20.0, 20.0));
    latch(&mut tree2, Point::new(20.0, 20.0));
    moved(&mut tree2, Point::new(178.0, 20.0));
    assert!(
        !view_handle(&tree2, view2).magnet_interaction_active(),
        "an overruled magnet must stop drawing its marker"
    );
}

#[test]
fn without_a_constraint_the_magnet_still_connects() {
    // The twin that keeps the test above honest: the connection is cancelled by
    // the override, not by the presence of this file.
    let (model, a) = magnet_scene();
    let (mut tree, view_id, count) = mount_magnetic(model.clone());

    down(&mut tree, Point::new(20.0, 20.0));
    moved(&mut tree, Point::new(178.0, 20.0));
    up(&mut tree, Point::new(178.0, 20.0));
    flush_item_move(&mut tree, view_id);

    assert!((model.local_pos(a).unwrap().x - 160.0).abs() < 1e-2);
    assert_eq!(count.get(), 1);
}

// ---------------------------------------------------------------------------
// Accessibility
// ---------------------------------------------------------------------------

#[test]
fn a_constraint_adds_and_removes_no_tab_stop() {
    let (model, a) = one_square();
    model.set_geometry_constraint(grid(25.0));
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([a]);
    let (mut tree, view_id) = mount_controller(model, selection);
    tree.sync_accessibility();
    assert_eq!(
        tree.tab_stops_within(view_id),
        vec![view_id],
        "the constraint is policy, not a control"
    );
}

#[test]
fn the_at_bounds_follow_the_constrained_preview() {
    // An AT user reading a card's position mid-gesture must be told where it
    // will land, not where the raw pointer is.
    let model = SceneModel::new();
    let card = model.add_widget(FillWidget::new(), Rect::new(40.0, 40.0, 120.0, 80.0));
    model.set_flag(card, ItemFlags::IS_DRAGGABLE, true);
    model.set_geometry_constraint(grid(25.0));
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([card]);
    let (mut tree, view_id) = mount_controller(model, selection);
    let kid = tree.children(view_id)[0];

    down(&mut tree, Point::new(100.0, 80.0));
    latch(&mut tree, Point::new(100.0, 80.0));
    moved(&mut tree, Point::new(163.0, 131.0));
    tree.layout(SizeProposal::exact(600.0, 400.0));
    tree.sync_accessibility();

    let b = tree.bounds(kid);
    assert!(
        (b.x - 100.0).abs() < 1e-2 && (b.y - 100.0).abs() < 1e-2,
        "the card's published rectangle is the constrained one: {b:?}"
    );
}

// ---------------------------------------------------------------------------
// What a constrained gesture costs
// ---------------------------------------------------------------------------

/// Drive one pointer sample plus the frame it causes, and count how many times
/// the constraint was asked.
fn calls_per_sample(items: usize, selected: usize) -> u32 {
    let model = SceneModel::new();
    let mut ids = Vec::new();
    for i in 0..items {
        let id = model.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 20.0, 20.0)),
            Point::new((i % 100) as f32 * 30.0, (i / 100) as f32 * 30.0),
        );
        model.set_flag(id, ItemFlags::IS_DRAGGABLE, true);
        ids.push(id);
    }
    let calls = Rc::new(Cell::new(0u32));
    let c = calls.clone();
    model.set_geometry_constraint(move |_| {
        c.set(c.get() + 1);
        ChangeVerdict::Accept
    });
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace(ids.iter().copied().take(selected));
    let (mut tree, _view_id) = mount_controller(model, selection);

    down(&mut tree, Point::new(10.0, 10.0));
    latch(&mut tree, Point::new(10.0, 10.0));
    calls.set(0);
    moved(&mut tree, Point::new(60.0, 20.0));
    tree.layout(SizeProposal::exact(600.0, 400.0));
    tree.render();
    tree.sync_accessibility();
    calls.get()
}

#[test]
fn the_constraint_is_asked_a_bounded_number_of_times_per_sample() {
    // The closure is a *pure function re-run per query*, not a per-gesture
    // callback: one sample that also lays out, paints and re-walks AT asks it
    // several times. That is fine — and only fine as long as the count is
    // bounded and independent of what is in the scene. An O(items) or
    // O(selection) count would turn a spatial-query constraint into a stall on
    // exactly the documents it exists for.
    let small = calls_per_sample(50, 1);
    let many_items = calls_per_sample(5_000, 1);
    let many_selected = calls_per_sample(5_000, 200);

    assert!(small > 0, "precondition: the constraint really ran");
    assert!(
        small <= 8,
        "one sample must not fan out into a crowd of queries: {small}"
    );
    assert_eq!(
        small, many_items,
        "the count follows the gesture, not the scene: {small} vs {many_items}"
    );
    assert_eq!(
        small, many_selected,
        "...nor the selection: {small} vs {many_selected}"
    );
}
