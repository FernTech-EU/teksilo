// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a finger can reach in a scene, and what a mouse must keep.
//!
//! Every test here has a mouse arm and a touch arm against the same fixture,
//! because the whole claim of the kind-aware grab work is that the two answers
//! differ *and* the mouse's is the one it always gave. The mouse arms are the
//! load-bearing half: [`teksilo_core::pointer::hit_slop::HitSlop::for_pointer`]
//! returns nothing for a mouse at every density, so if a mouse arm ever moves,
//! the tolerance has leaked out of the miss-only pass it is supposed to live in.

use super::*;
use crate::items::RectItem;
use std::cell::Cell;
use std::rc::Rc;
use teksilo_canvas::Point;
use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
use teksilo_core::pointer::{
    BackendDeviceKey, EventTime, PointerId, PointerIdAllocator, PointerInfo, PointerPhase,
    PointerSample,
};

// ---------------------------------------------------------------- fixtures

fn finger() -> PointerId {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    PointerIdAllocator::global().begin(
        BackendDeviceKey::new(0x5CE1),
        NEXT.fetch_add(1, Ordering::Relaxed),
    )
}

fn contact(id: PointerId, phase: PointerPhase, at: Point, ms: u64) -> PointerSample {
    PointerSample {
        pointer: PointerInfo::touch(id, EventTime::from_millis(ms)),
        phase,
        position: at,
        button: None,
        modifiers: Modifiers::default(),
        coalesced: Vec::new(),
    }
}

/// A finger press, one move past the touch drag slop, and a lift.
fn finger_drag(tree: &mut WidgetTree, from: Point, to: Point) {
    let id = finger();
    tree.dispatch_pointer(contact(id, PointerPhase::Down, from, 0));
    tree.dispatch_pointer(contact(id, PointerPhase::Move, to, 16));
    tree.dispatch_pointer(contact(id, PointerPhase::Up, to, 32));
}

fn mouse_drag(tree: &mut WidgetTree, from: Point, to: Point) {
    tree.pointer_move(from);
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: from,
        button: PointerButton::Primary,
        modifiers: Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerMove { position: to });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: to,
        button: PointerButton::Primary,
        modifiers: Modifiers::default(),
    });
}

/// Press at `from` and release at `to` with no move in between — the shape that
/// exercises the tap-versus-drag tolerance without involving a recognizer.
fn mouse_press_release(tree: &mut WidgetTree, from: Point, to: Point) {
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: from,
        button: PointerButton::Primary,
        modifiers: Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: to,
        button: PointerButton::Primary,
        modifiers: Modifiers::default(),
    });
}

fn contact_press_release(tree: &mut WidgetTree, from: Point, to: Point) {
    let id = finger();
    tree.dispatch_pointer(contact(id, PointerPhase::Down, from, 0));
    tree.dispatch_pointer(contact(id, PointerPhase::Up, to, 16));
}

/// One 400×400 tappable item at the scene origin, plus its tap counter. Large
/// enough that both the press and the release land inside it at any zoom the
/// tests use, so the only thing under test is the movement tolerance.
fn wide_tappable_scene() -> (Scene, Rc<Cell<u32>>) {
    let mut scene = Scene::new();
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 400.0, 400.0)).fill(teksilo_tokens::Color::RED),
        Point::ZERO,
    );
    let taps = Rc::new(Cell::new(0u32));
    let c = taps.clone();
    scene.handlers_mut(id).unwrap().on_tap(move |_ev, _ctx| {
        c.set(c.get() + 1);
    });
    (scene, taps)
}

// ------------------------------------------------- the tap movement tolerance

/// The tap-versus-drag tolerance is a screen distance, so the same hand movement
/// means the same thing at every zoom.
///
/// The two zooms are chosen so that a scene-unit comparison against the same
/// number gets *both* answers wrong: 3 screen pixels at zoom 0.25 is 12 scene
/// units and would be rejected, and 6 screen pixels at zoom 4 is 1.5 scene units
/// and would be accepted. That is the defect stated as a measurement — zoomed
/// far enough out an ordinary click cannot land, and zoomed far enough in a
/// deliberate drag reads as a tap.
#[test]
fn the_tap_tolerance_is_the_same_screen_distance_at_every_zoom() {
    let (scene, taps) = wide_tappable_scene();
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    view_handle(&tree, view_id).set_zoom(0.25);
    mouse_press_release(&mut tree, Point::new(50.0, 50.0), Point::new(53.0, 50.0));
    assert_eq!(
        taps.get(),
        1,
        "3 screen px is a tap at zoom 0.25 (a scene-unit comparison would call \
         it 12 units of travel and refuse it)",
    );

    view_handle(&tree, view_id).set_zoom(4.0);
    mouse_press_release(&mut tree, Point::new(50.0, 50.0), Point::new(56.0, 50.0));
    assert_eq!(
        taps.get(),
        1,
        "6 screen px is not a tap at zoom 4 (a scene-unit comparison would call \
         it 1.5 units of travel and accept it)",
    );
}

/// A contact is allowed the travel its gesture profile already calls a tap; a
/// mouse is not.
#[test]
fn a_contact_taps_through_a_wobble_that_would_be_a_drag_for_a_mouse() {
    let (scene, touch_taps) = wide_tappable_scene();
    let mut tree = WidgetTree::new();
    tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    contact_press_release(&mut tree, Point::new(50.0, 50.0), Point::new(60.0, 50.0));
    assert_eq!(
        touch_taps.get(),
        1,
        "10 px of roll is one tap for a finger — the touch profile's tap slop \
         is 18 dp",
    );

    let (scene, mouse_taps) = wide_tappable_scene();
    let mut tree = WidgetTree::new();
    tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    mouse_press_release(&mut tree, Point::new(50.0, 50.0), Point::new(60.0, 50.0));
    assert_eq!(
        mouse_taps.get(),
        0,
        "the same 10 px is not a tap for a mouse — it keeps the view's own 4 px \
         floor",
    );
}

// -------------------------------------------------------- the item grab slop

/// A draggable stroke 2 scene units tall, and a press five pixels clear of it.
fn thin_draggable_scene() -> Scene {
    let mut scene = Scene::new();
    scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 120.0, 2.0))
            .fill(teksilo_tokens::Color::RED)
            .draggable(true),
        Point::new(50.0, 50.0),
    );
    scene
}

#[test]
fn a_finger_grabs_a_thin_item_a_mouse_has_to_hit_exactly() {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::new(thin_draggable_scene())
            .selection_mode(crate::selection::SceneSelectionMode::Multi),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    // The stroke occupies y 50..52; press at y 57, five pixels below it.
    finger_drag(&mut tree, Point::new(100.0, 57.0), Point::new(100.0, 90.0));
    let view = view_handle(&tree, view_id);
    assert!(
        view.drag_target.get().is_some() || view.pending_item_move.get().is_some(),
        "a finger five pixels off a two-pixel stroke must still grab it",
    );

    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::new(thin_draggable_scene())
            .selection_mode(crate::selection::SceneSelectionMode::Multi),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    mouse_drag(&mut tree, Point::new(100.0, 57.0), Point::new(100.0, 90.0));
    let view = view_handle(&tree, view_id);
    assert!(
        view.drag_target.get().is_none() && view.pending_item_move.get().is_none(),
        "a mouse five pixels off the stroke must miss it, exactly as before",
    );
}

/// The slop is a bounded top-up, not "a finger gets the bounding box".
///
/// A thin L's box is 100×100 with the stroke along two edges; a press at the
/// box's interior centre is ~35 px from any stroke, far outside the 8 dp a
/// coarse pointer can ever earn, so it must still fall through to the marquee.
#[test]
fn a_finger_does_not_get_a_thin_items_whole_bounding_box() {
    use crate::items::PathItem;
    use teksilo_canvas::Path;

    let mut path = Path::new();
    path.move_to(Point::new(0.0, 0.0));
    path.line_to(Point::new(0.0, 100.0));
    path.line_to(Point::new(100.0, 100.0));

    let mut scene = Scene::new();
    scene.add_item(
        PathItem::new(path, Rect::new(0.0, 0.0, 100.0, 100.0))
            .stroke(teksilo_tokens::Color::RED, 2.0)
            .draggable(true),
        Point::new(50.0, 50.0),
    );

    let mut tree = WidgetTree::new();
    let view_id =
        tree.add(SceneView::new(scene).selection_mode(crate::selection::SceneSelectionMode::Multi));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    // Box is scene (50,50)-(150,150); its far corner interior is ~35 px from
    // either stroke.
    finger_drag(&mut tree, Point::new(135.0, 65.0), Point::new(135.0, 100.0));
    let view = view_handle(&tree, view_id);
    assert!(
        view.drag_target.get().is_none() && view.pending_item_move.get().is_none(),
        "a press 35 px from the stroke is not a near miss and must not grab",
    );
    assert!(
        view.marquee.get().is_some() || view.pending_marquee_commit.get().is_some(),
        "it must fall through to the marquee, like a mouse there does",
    );
}

/// A target already at least the density's target size on its smaller axis earns
/// nothing, for any pointer. This is the arithmetic that keeps a scene full of
/// large cards from acquiring an eight-pixel halo each.
#[test]
fn a_large_item_earns_no_grab_slop_even_for_a_finger() {
    let mut scene = Scene::new();
    scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 200.0, 150.0))
            .fill(teksilo_tokens::Color::RED)
            .draggable(true),
        Point::new(50.0, 50.0),
    );
    let mut tree = WidgetTree::new();
    let view_id =
        tree.add(SceneView::new(scene).selection_mode(crate::selection::SceneSelectionMode::Multi));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    // Five pixels above the card's top edge.
    finger_drag(&mut tree, Point::new(120.0, 45.0), Point::new(120.0, 20.0));
    let view = view_handle(&tree, view_id);
    assert!(
        view.drag_target.get().is_none() && view.pending_item_move.get().is_none(),
        "a card that is already bigger than the target size earns no widening",
    );
}

// ------------------------------------------------------ the magnet grab disc

fn magnet_scene() -> Scene {
    use crate::magnet::{Magnet, MagnetRole};
    let mut scene = Scene::new();
    let a = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0))
            .fill(teksilo_tokens::Color::RED)
            .draggable(true),
        Point::ZERO,
    );
    scene.add_magnet(
        a,
        Magnet::new(Point::new(40.0, 20.0)).role(MagnetRole::Source),
    );
    scene
}

/// A magnet handle's declared radius is the whole reach for a mouse and a floor
/// for a finger.
///
/// `capture_px(6.0)` rather than the 14 dp default because a 28 dp grab disc
/// already clears the Compact target size, so the default earns nothing at this
/// density — the mechanism working, not a gap. A six-pixel handle is the case an
/// app with dense ports actually has.
///
/// Both arms are read **mid-gesture**: a port drag is consumed when the wire is
/// released, so a check after the lift reports `None` for every pointer and would
/// pass whatever the tolerance did.
#[test]
fn a_finger_earns_reach_on_a_magnet_handle_and_a_mouse_does_not() {
    use crate::magnet::{MagnetVerdict, MagnetismConfig};

    let cfg = || MagnetismConfig::new(|_a, _b| MagnetVerdict::accept()).capture_px(6.0);

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(magnet_scene()).magnetism(cfg()));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    // Nine pixels from the handle at (40, 20), and outside the item's own box.
    let f = finger();
    tree.dispatch_pointer(contact(f, PointerPhase::Down, Point::new(49.0, 20.0), 0));
    tree.dispatch_pointer(contact(f, PointerPhase::Move, Point::new(49.0, 45.0), 16));
    assert!(
        view_handle(&tree, view_id).port_drag.borrow().is_some(),
        "a finger nine pixels from a six-pixel handle grabs it",
    );

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(magnet_scene()).magnetism(cfg()));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.pointer_move(Point::new(49.0, 20.0));
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(49.0, 20.0),
        button: PointerButton::Primary,
        modifiers: Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(49.0, 45.0),
    });
    assert!(
        view_handle(&tree, view_id).port_drag.borrow().is_none(),
        "a mouse nine pixels away misses it, exactly as before",
    );

    // The mouse arm above is only worth anything if the same fixture *does*
    // answer a mouse inside the declared radius.
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(magnet_scene()).magnetism(cfg()));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.pointer_move(Point::new(44.0, 20.0));
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(44.0, 20.0),
        button: PointerButton::Primary,
        modifiers: Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(44.0, 45.0),
    });
    assert!(
        view_handle(&tree, view_id).port_drag.borrow().is_some(),
        "four pixels away is inside the declared six, for a mouse too",
    );
}

// ------------------------------------------------- the hover seam and a hold

/// The hover seam — item `on_hover`, the item tooltip, the cursor shape — belongs
/// to a pointer that hovers, and a contact is not one.
///
/// The contact travels past the touch tap slop so it is unambiguously a pan and
/// not a hold; a stationary contact on the same item *does* get the tip, by the
/// hold route, which is the next test.
#[test]
fn a_contact_move_fires_no_item_hover_and_schedules_no_tooltip() {
    fn fixture() -> (Scene, Rc<Cell<u32>>) {
        let mut scene = Scene::new();
        let id = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 200.0, 200.0)).fill(teksilo_tokens::Color::RED),
            Point::new(20.0, 20.0),
        );
        let hovers = Rc::new(Cell::new(0u32));
        let c = hovers.clone();
        let h = scene.handlers_mut(id).unwrap();
        h.on_hover(move |entered, _ctx| {
            if entered {
                c.set(c.get() + 1);
            }
        });
        h.tooltip(lit!("Item tip"));
        (scene, hovers)
    }

    let (scene, hovers) = fixture();
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let f = finger();
    tree.dispatch_pointer(contact(f, PointerPhase::Down, Point::new(60.0, 60.0), 0));
    tree.dispatch_pointer(contact(f, PointerPhase::Move, Point::new(60.0, 130.0), 16));
    tree.advance_time(
        teksilo_tokens::MotionTokens::default().tooltip_delay_heavy
            + std::time::Duration::from_millis(50),
    );
    assert_eq!(
        hovers.get(),
        0,
        "a contact must not fire an item's on_hover"
    );
    assert!(
        view_handle(&tree, view_id).hovered_item.get().is_none(),
        "and must not become the view's hovered item",
    );
    assert!(
        tree.active_overlays().is_empty(),
        "and a contact that panned across the item leaves no tooltip behind it",
    );

    // The mouse arm of the same fixture still hovers and still tips.
    let (scene, hovers) = fixture();
    let mut tree = WidgetTree::new();
    tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.pointer_move(Point::new(60.0, 60.0));
    assert_eq!(hovers.get(), 1, "a mouse over the same item still hovers");
    tree.advance_time(
        teksilo_tokens::MotionTokens::default().tooltip_delay_heavy
            + std::time::Duration::from_millis(50),
    );
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "and still gets the item's tooltip after the dwell",
    );
}

/// A hold on an item shows its tooltip, and leaves it up after the lift — a
/// finger cannot keep pointing at what it is reading.
#[test]
fn a_hold_on_an_item_shows_its_tooltip_and_keeps_it_after_the_lift() {
    let mut scene = Scene::new();
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 60.0, 60.0)).fill(teksilo_tokens::Color::RED),
        Point::new(20.0, 20.0),
    );
    scene.handlers_mut(id).unwrap().tooltip(lit!("Held tip"));

    let mut tree = WidgetTree::new();
    tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let f = finger();
    tree.dispatch_pointer(contact(f, PointerPhase::Down, Point::new(40.0, 40.0), 0));
    assert!(
        tree.active_overlays().is_empty(),
        "the press alone shows nothing — a tap must not leave a tip behind",
    );
    tree.advance_time(long_press() + std::time::Duration::from_millis(20));
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "holding for the profile's long press shows the held item's tooltip",
    );
    assert!(
        tree.find_by_label("Held tip").is_some(),
        "and it carries that item's own text",
    );
    tree.dispatch_pointer(contact(f, PointerPhase::Up, Point::new(40.0, 40.0), 600));
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "and it survives the lift, because the next press is what retracts it",
    );
}

/// A contact that travels disarms the hold: a pan across an item must not leave
/// a trail of tooltips behind it.
#[test]
fn a_contact_that_travels_disarms_the_hold() {
    let mut scene = Scene::new();
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 200.0, 200.0)).fill(teksilo_tokens::Color::RED),
        Point::new(20.0, 20.0),
    );
    scene.handlers_mut(id).unwrap().tooltip(lit!("Held tip"));

    let mut tree = WidgetTree::new();
    tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let f = finger();
    tree.dispatch_pointer(contact(f, PointerPhase::Down, Point::new(40.0, 40.0), 0));
    // Past the touch tap slop, so this is a pan and not a hold.
    tree.dispatch_pointer(contact(f, PointerPhase::Move, Point::new(40.0, 100.0), 16));
    tree.advance_time(long_press() + std::time::Duration::from_millis(20));
    assert!(
        tree.active_overlays().is_empty(),
        "a contact that has travelled 60 px is panning, not holding",
    );
}

/// A mouse gets no hold route at all: it reaches the same tip by resting, and a
/// second path would show a tooltip while the button is down.
#[test]
fn a_mouse_press_arms_no_hold_tooltip() {
    let mut scene = Scene::new();
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 60.0, 60.0)).fill(teksilo_tokens::Color::RED),
        Point::new(20.0, 20.0),
    );
    scene.handlers_mut(id).unwrap().tooltip(lit!("Held tip"));

    let mut tree = WidgetTree::new();
    tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(40.0, 40.0),
        button: PointerButton::Primary,
        modifiers: Modifiers::default(),
    });
    tree.advance_time(long_press() + std::time::Duration::from_millis(20));
    assert!(
        tree.active_overlays().is_empty(),
        "a held mouse button is not a request for a tooltip",
    );
}

/// The canary for the hold route: a mouse press held past the long-press deadline
/// and *then* dragged must still start a marquee.
///
/// This is a real trap, measured rather than imagined. Answering the hold with an
/// `on_long_press` handler on the view — the obvious implementation — installs a
/// `LongPressRecognizer` in the view's own gesture arena, where a recognizer that
/// wins **resets its peers** and long press outranks drag. Attaching one and
/// running this test was how that was found: it goes red, because the drag
/// recognizer loses the press position it was waiting to measure from. The hold
/// is therefore armed as a delayed overlay instead, and this test is what stops
/// anyone reaching for the handler again.
#[test]
fn a_mouse_press_held_past_the_hold_deadline_still_marquees() {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::new(Scene::new()).selection_mode(crate::selection::SceneSelectionMode::Multi),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));

    tree.pointer_move(Point::new(40.0, 40.0));
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(40.0, 40.0),
        button: PointerButton::Primary,
        modifiers: Modifiers::default(),
    });
    tree.advance_time(long_press() + std::time::Duration::from_millis(50));
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(90.0, 90.0),
    });
    assert!(
        view_handle(&tree, view_id).marquee.get().is_some(),
        "holding before dragging must not cost the mouse its marquee",
    );
}

fn long_press() -> std::time::Duration {
    teksilo_core::gesture::default_profile(teksilo_tokens::PointerKind::Touch).long_press
}
