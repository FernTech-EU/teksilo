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

/// Tick the virtual clock until `view` stops flinging, up to a budget that no
/// shipped physics can outlast. Answers whether the coast ended.
///
/// The budget is six seconds and not a tuned number, because how long a coast
/// runs is a *platform* fact: `ScrollPhysics::Platform` selects the Android
/// spline off macOS, which runs out in well under two seconds, and the Flutter
/// exponential decay on macOS, which only crosses the 20 px/s settle tolerance
/// at `ln(20 / v) / ln(0.135)` — 3 s even at the 8000 px/s velocity cap. A
/// fixed tick count that clears one clears the other only by accident, and this
/// assertion is about a coast *ending*, not about when.
fn settles_within(tree: &mut WidgetTree, view: WidgetId) -> bool {
    (0..120).any(|_| {
        tree.advance_time(Duration::from_millis(50));
        !tree.is_flinging(view)
    })
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

    assert!(
        settles_within(&mut tree, view_id),
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
    assert!(
        settles_within(&mut tree, view_id),
        "a coast that can no longer move anything is stopped, not left spinning",
    );
    let pan_y = view_handle(&tree, view_id).pan().y;
    assert!(
        pan_y >= -20.0,
        "and the camera stayed inside the bounded scene, got pan.y = {pan_y}",
    );
}

/// The hold that arms the marquee, in the shape it is actually driven.
fn hold() -> Duration {
    teksilo_core::gesture::default_profile(teksilo_tokens::PointerKind::Touch).long_press
}

/// A finger pans a view that has selection on — the marquee waits for a hold.
///
/// The view carries a `PanClaim` **and** its own `on_drag`, and a node can hold
/// only one role in a `PointerSequence`. It is enrolled as the pan claimant
/// before any handler runs, so the drag half arrives at a taken slot;
/// `PointerSequence::defer_own_drag` attaches the drag's `DragActivation` to the
/// member the node already has, and `Auto` on a direct pointer with an eligible
/// pan resolves to `AfterLongPress`. So the marquee no longer latches at the
/// 18 dp touch drag slop — under which this surface could not pan under a finger
/// at all — and the pan wins at its own 36 dp.
///
/// The two numbers below are what pin that: nothing is running at 20 dp, where
/// the drag slop alone would already have taken the press, and the camera has
/// moved by 120 dp, where the pan became eligible.
#[test]
fn a_selecting_view_still_pans_under_a_finger() {
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
        view_handle(&tree, view_id).marquee.get().is_none(),
        "20 dp is past the drag slop and short of the pan slop: the deferred \
         marquee must not take the press there",
    );

    tree.dispatch_pointer(contact(
        id,
        PointerPhase::Move,
        Point::new(from.x, from.y + 120.0),
    ));
    assert!(
        view_handle(&tree, view_id).pan().y != 0.0,
        "and past the pan slop the camera moves, which is what this surface \
         could not do at all while the marquee latched first",
    );
    assert!(
        view_handle(&tree, view_id).marquee.get().is_none(),
        "the pan owns the press, so the marquee stays out for the rest of it",
    );
}

/// The other half of the same rule: hold first and the marquee is what the
/// finger gets.
///
/// The hold has to be a hold. A press that has already wandered past
/// `long_press_slop` when the deadline arrives is withdrawn rather than armed,
/// which is what stops a slow, deliberate pan from becoming a marquee merely by
/// outlasting the clock — so this test moves *after* the hold, not during it.
#[test]
fn a_hold_gives_the_selecting_view_its_marquee() {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::new(Scene::new()).selection_mode(crate::selection::SceneSelectionMode::Multi),
    );
    tree.layout(SizeProposal::exact(800.0, 600.0));

    let id = finger();
    let from = Point::new(400.0, 300.0);
    tree.dispatch_pointer(contact(id, PointerPhase::Down, from));
    tree.advance_time(hold() + Duration::from_millis(50));

    tree.dispatch_pointer(contact(
        id,
        PointerPhase::Move,
        Point::new(from.x, from.y + 20.0),
    ));
    assert!(
        view_handle(&tree, view_id).marquee.get().is_some(),
        "past the hold the marquee is armed and latches at the drag slop",
    );
    assert_eq!(
        view_handle(&tree, view_id).pan().y,
        0.0,
        "and the camera stays where it was",
    );
}

/// A two-fingered spread to twice the starting span leaves the zoom at exactly
/// twice — it does not compound.
///
/// This is the accumulation half of the pinch contract, driven end to end: two
/// real contacts through the router's `TouchPinchRecognizer`, through the one
/// pinch ingress, into `SceneView::on_pinch`.
///
/// *Un-`#[ignore]`d.* It was landed failing, because the two pinch producers
/// disagreed on what `scale` meant. The recognizer divided the live span by the
/// span at the *start* of the gesture, while the consumer — and the OS trackpad
/// arm, which reports a change per event and has no start baseline to divide by
/// — read each sample as the step since the one before. Folding a cumulative
/// value in with `zoom *= scale` multiplies the intermediate ratios together:
/// the five samples below left the zoom at 8.03 rather than 2.0, and a real
/// gesture delivering a sample per frame reached the default `max_zoom` of 10
/// and stopped there. `GestureEvent::PinchChanged` now states the per-sample
/// contract and `TouchPinchRecognizer` emits it.
#[test]
fn a_cumulative_pinch_reaches_the_spread_it_asked_for() {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    // Two fingers 100 dp apart, straddling the middle of the viewport.
    let (a, b) = (finger(), finger());
    let y = 300.0;
    let anchor = Point::new(350.0, y);
    tree.dispatch_pointer(contact(a, PointerPhase::Down, anchor));
    tree.dispatch_pointer(contact(b, PointerPhase::Down, Point::new(450.0, y)));
    assert!(
        tree.touch_pinch_active(),
        "two contacts on the view start a pinch",
    );

    // Spread the moving finger out to a 200 dp span — twice the start — over
    // five frames, the way samples really arrive.
    for span in [115.0f32, 132.0, 152.0, 174.0, 200.0] {
        tree.dispatch_pointer(contact(
            b,
            PointerPhase::Move,
            Point::new(anchor.x + span, y),
        ));
    }

    let zoom = view_handle(&tree, view_id).zoom.get();
    assert!(
        (zoom - 2.0).abs() < 0.05,
        "a two-fingered spread to twice the starting span must leave the zoom at \
         2.0, got {zoom}",
    );
}

/// One degree of trackpad twist rotates the scene by one degree.
///
/// This is the unit half of the pinch contract, at the consumer: the payload's
/// `rotation` is radians and reaches the view transform unscaled, and successive
/// samples add. The degrees-to-radians conversion itself happens at the platform
/// seam — `teksilo_platform::event_translation`'s `rotation_gesture`, asserted
/// by its own `rotation_gesture_translates` and, end to end from a winit event,
/// by `teksilo_app`'s `a_trackpad_twist_reaches_the_widget_in_radians`.
///
/// *Un-`#[ignore]`d.* It was landed failing because the seam passed winit's
/// degrees straight into a field the recognizer and every consumer read as
/// radians, so a one-degree twist turned the scene by a radian — about 57°. The
/// payload below is now what the seam produces for one degree.
#[test]
fn a_one_degree_trackpad_twist_rotates_the_scene_by_one_degree() {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    tree.pointer_move(Point::new(400.0, 300.0));

    // What the platform seam produces for one degree of twist, twice — which
    // also pins that samples are added rather than assigned.
    let one_degree = 1.0f32.to_radians();
    for _ in 0..2 {
        tree.dispatch_event(WidgetEvent::Gesture {
            gesture: teksilo_core::gesture::GestureEvent::PinchChanged {
                center: Point::new(400.0, 300.0),
                scale: 1.0,
                rotation: one_degree,
            },
        });
    }

    let view = view_handle(&tree, view_id);
    let radians = view.rotation.get();
    assert!(
        (radians - 2.0 * one_degree).abs() < 1e-4,
        "two degrees of twist is 0.03491 rad of scene rotation, got {radians}",
    );

    // …and it is the *transform* that turns by two degrees, so nothing between
    // the signal and the renderer re-interprets the unit. Zoom is 1, so the
    // angle of the image of a unit vector is the rotation itself.
    let t = view.view_transform();
    let o = t.apply_point(Point::new(0.0, 0.0));
    let x = t.apply_point(Point::new(1.0, 0.0));
    let turned = (x.y - o.y).atan2(x.x - o.x);
    assert!(
        (turned - 2.0 * one_degree).abs() < 1e-3,
        "the view transform must turn by the same two degrees, got {turned} rad",
    );
}
