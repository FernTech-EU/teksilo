// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The camera under a finger: the coast that follows a flick, and the
//! arbitration that decides whether a finger moves the camera at all.

use super::*;
use std::rc::Rc;
use std::time::Duration;
use teksilo_canvas::Point;
use teksilo_core::ManualClock;
use teksilo_core::pointer::{
    BackendDeviceKey, EventTime, PointerId, PointerIdAllocator, PointerInfo, PointerPhase,
    PointerSample,
};

fn finger() -> PointerId {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    PointerIdAllocator::global().begin(
        BackendDeviceKey::new(0x5CE2),
        NEXT.fetch_add(1, Ordering::Relaxed),
    )
}

fn contact(id: PointerId, phase: PointerPhase, at: Point) -> PointerSample {
    PointerSample {
        pointer: PointerInfo::touch(id, EventTime::ZERO),
        phase,
        position: at,
        button: None,
        modifiers: Default::default(),
        coalesced: Vec::new(),
    }
}

/// A flick and its release, timed on a `ManualClock` so the velocity comes off
/// the same axis the coast is later ticked on. Returns the release point.
fn flick_up(tree: &mut WidgetTree, clock: &Rc<ManualClock>, from: Point) -> Point {
    let id = finger();
    tree.dispatch_pointer(contact(id, PointerPhase::Down, from));
    let mut y = from.y;
    for step in 1..=5 {
        clock.set(EventTime::from_millis(step * 4));
        y -= 20.0;
        tree.dispatch_pointer(contact(id, PointerPhase::Move, Point::new(from.x, y)));
    }
    clock.set(EventTime::from_millis(24));
    let release = Point::new(from.x, y);
    tree.dispatch_pointer(contact(id, PointerPhase::Up, release));
    release
}

/// A flick that lifts keeps the camera moving, and the coast finishes.
///
/// The claim that makes this work is the view's `kinetic: true` `PanClaim`; the
/// coast arrives as further scrolls along the same claimant chain the pan used,
/// which is why it needs no scene-side code at all — but it needs a test, or a
/// claim that stopped being kinetic would go unnoticed.
#[test]
fn a_flicked_finger_keeps_the_camera_moving_after_the_lift() {
    let mut tree = WidgetTree::new();
    let clock = Rc::new(ManualClock::new(EventTime::ZERO));
    tree.set_input_clock(clock.clone());
    let view_id = tree.add(SceneView::new(Scene::new()));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    flick_up(&mut tree, &clock, Point::new(400.0, 400.0));
    assert!(
        tree.is_flinging(view_id),
        "a fast release hands the camera to a coast",
    );
    let at_release = view_handle(&tree, view_id).pan().y;

    tree.advance_time(Duration::from_millis(100));
    let coasting = view_handle(&tree, view_id).pan().y;
    assert!(
        coasting < at_release,
        "the coast keeps moving the camera in the flick's direction: \
         {at_release} -> {coasting}",
    );
    assert_eq!(
        view_handle(&tree, view_id).pan_y.animation_target(),
        None,
        "and it does it by setting the camera, not by aiming a tween at it",
    );

    for _ in 0..40 {
        tree.advance_time(Duration::from_millis(50));
    }
    assert!(
        !tree.is_flinging(view_id),
        "and it settles rather than coasting for ever",
    );
}

/// A coast stops at a bounded scene's edge instead of spinning against it.
#[test]
fn a_coast_stops_at_the_pan_bound() {
    let mut scene = Scene::new();
    // A scene barely taller than the viewport, so the reachable pan range is a
    // few pixels and the flick hits the far edge almost at once.
    scene.set_pan_bounds(Some(Rect::new(0.0, 0.0, 800.0, 610.0)));
    let mut tree = WidgetTree::new();
    let clock = Rc::new(ManualClock::new(EventTime::ZERO));
    tree.set_input_clock(clock.clone());
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    flick_up(&mut tree, &clock, Point::new(400.0, 400.0));
    assert!(
        tree.is_flinging(view_id),
        "the flick still starts a coast — without this the rest of the test would \
         pass on a view that never flings at all",
    );
    for _ in 0..40 {
        tree.advance_time(Duration::from_millis(50));
    }
    assert!(
        !tree.is_flinging(view_id),
        "a coast that can no longer move anything is stopped, not left spinning",
    );
    let pan_y = view_handle(&tree, view_id).pan().y;
    assert!(
        pan_y >= -20.0,
        "and the camera stayed inside the bounded scene, got pan.y = {pan_y}",
    );
}

/// Why a finger does not pan a view that has selection on, stated as the two
/// measurements that identify the mechanism.
///
/// The view carries a `PanClaim` **and** its own `on_drag`, and a node can hold
/// only one role in a `PointerSequence`. It is enrolled as the pan claimant
/// first, so its own drag is never enrolled as a competitor and its
/// `DragActivation` is never consulted — the deferral that makes a `GridView`'s
/// marquee wait for a hold cannot reach it. Its drag recognizer is then driven
/// through the ordinary capture dispatch, which runs *before* the arbitration
/// walk, and it latches at the touch **drag** slop of 18 dp — half the touch
/// **pan** slop of 36 dp. Recognition alone decides the sequence, so by the time
/// the pan is eligible the sequence has an owner and the pan never gets one.
///
/// The two numbers below are what pins that reading: the marquee is already
/// running at 20 dp of travel, where the pan is not yet eligible, and the camera
/// has still not moved at 120 dp, where it long since would have been. A slop
/// race alone would show the pan winning once it passed 36.
///
/// Fixing it means giving the press owner's own `DragActivation` a say when it
/// also holds an eligible pan claim, which is core arbitration and not the
/// scene's to change. Recorded here so the next reader has the measurement
/// rather than the inference.
#[test]
fn a_selecting_views_marquee_latches_before_its_pan_is_eligible() {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::new(Scene::new()).selection_mode(crate::selection::SceneSelectionMode::Multi),
    );
    tree.layout(SizeProposal::exact(800.0, 600.0));

    let id = finger();
    let from = Point::new(400.0, 300.0);
    tree.dispatch_pointer(contact(id, PointerPhase::Down, from));

    tree.dispatch_pointer(contact(
        id,
        PointerPhase::Move,
        Point::new(from.x, from.y + 20.0),
    ));
    assert!(
        view_handle(&tree, view_id).marquee.get().is_some(),
        "the marquee is already running at 20 dp — past the 18 dp drag slop and \
         short of the 36 dp pan slop",
    );

    tree.dispatch_pointer(contact(
        id,
        PointerPhase::Move,
        Point::new(from.x, from.y + 120.0),
    ));
    assert_eq!(
        view_handle(&tree, view_id).pan().y,
        0.0,
        "and the camera has still not moved at 120 dp, so the pan did not win \
         once it became eligible — the sequence was already decided",
    );
}

/// The scene reads a `PinchChanged`'s `scale` as the factor **since the previous
/// sample**, which is what the OS trackpad stream delivers and what the two
/// pre-existing single-event pinch tests assert.
///
/// The touchscreen recognizer delivers something else: its `scale` is cumulative
/// since `PinchStarted` (`TouchPinchRecognizer::contact_moved` divides the live
/// span by the *start* span). Multiplying the live zoom by a cumulative factor on
/// every sample compounds it, so a genuine ×2 spread delivered over several
/// samples multiplies the zoom by the product of every intermediate ratio and
/// slams into `max_zoom`.
///
/// This test states the contract the scene consumes. It is `#[ignore]`d because
/// it fails today for the touch producer, and the fix is not in this crate: the
/// producers have to agree, and both live outside `teksilo-scene` — either
/// `teksilo_core::gesture::pinch` emits a per-sample ratio (four lines, and its
/// own `two_contacts_start_and_the_scale_is_the_span_ratio` assertion changes),
/// or `teksilo_platform`'s `pinch_gesture` accumulates. Un-`ignore` it with
/// whichever lands; nothing here needs to change either way.
///
/// Measured on this fixture: five samples carrying the cumulative scales 1.15,
/// 1.32, 1.52, 1.74, 2.00 leave the zoom at 8.03 instead of 2.0. A real gesture
/// delivers samples every frame rather than five in total, so it reaches the
/// default `max_zoom` of 10 and stops there.
#[test]
#[ignore = "the two pinch producers disagree on what `scale` means; the fix is \
            in teksilo-core or teksilo-platform, not here — see the doc comment"]
fn a_cumulative_pinch_reaches_the_spread_it_asked_for() {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let centre = Point::new(400.0, 300.0);

    tree.pointer_move(centre);
    tree.dispatch_event(WidgetEvent::Gesture {
        gesture: teksilo_core::gesture::GestureEvent::PinchStarted { center: centre },
    });
    for cumulative in [1.15f32, 1.32, 1.52, 1.74, 2.00] {
        tree.dispatch_event(WidgetEvent::Gesture {
            gesture: teksilo_core::gesture::GestureEvent::PinchChanged {
                center: centre,
                scale: cumulative,
                rotation: 0.0,
            },
        });
    }
    let zoom = view_handle(&tree, view_id).zoom.get();
    assert!(
        (zoom - 2.0).abs() < 0.05,
        "a two-fingered spread to twice the starting span must leave the zoom at \
         2.0, got {zoom}",
    );
}

/// The pinch payload's `rotation` is read as radians, because that is what the
/// scene's `rotation` signal and its view transform are in.
///
/// The OS trackpad translator hands winit's `RotationGesture` delta straight
/// through, and winit reports that in **degrees** — so this test measures 1.0 rad
/// (about 57°) of scene rotation for one degree of twist. Same shape of defect as
/// the `scale` one above and the same reason it is not fixed here: the unit is
/// decided in `teksilo-platform`.
#[test]
#[ignore = "teksilo-platform's rotation_gesture passes winit's degrees into a \
            radian field; the fix is there, not here"]
fn a_one_degree_trackpad_twist_rotates_the_scene_by_one_degree() {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    tree.pointer_move(Point::new(400.0, 300.0));
    // What the platform layer produces for a one-degree twist today.
    tree.dispatch_event(WidgetEvent::Gesture {
        gesture: teksilo_core::gesture::GestureEvent::PinchChanged {
            center: Point::new(400.0, 300.0),
            scale: 1.0,
            rotation: 1.0,
        },
    });
    let radians = view_handle(&tree, view_id).rotation.get();
    assert!(
        (radians - std::f32::consts::PI / 180.0).abs() < 1e-4,
        "one degree of twist is 0.01745 rad of scene rotation, got {radians}",
    );
}
