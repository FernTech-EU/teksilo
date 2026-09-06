// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The boundary rule, the fling, the single pinch ingress, and the palm
//! fallback, against real trees driven through the real ingress doors.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use teksilo_canvas::{Point, Rect, Size, SizeProposal};

use super::*;
use crate::WidgetId;
use crate::event::{EventResponse, Modifiers, PointerButton, ScrollDelta, WidgetEvent};
use crate::gesture::{GestureEvent, PinchPhase};
use crate::pointer::clock::ManualClock;
use crate::pointer::touch_action::{PanAxes, TouchAction};
use crate::pointer::{
    BackendDeviceKey, EventTime, PointerAxes, PointerId, PointerIdAllocator, PointerInfo,
    PointerPhase, PointerSample,
};
use crate::test_widgets::{FillWidget, StackWidget};
use crate::widget_builder::WidgetBuilder;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A fresh contact identity, minted through the real allocator.
fn contact_id(raw: u64) -> PointerId {
    let alloc = PointerIdAllocator::global();
    let device = BackendDeviceKey::new(0x9A11);
    let id = alloc.begin(device, raw);
    alloc.end(device, raw);
    id
}

fn contact(id: PointerId, phase: PointerPhase, at: Point) -> PointerSample {
    PointerSample {
        pointer: PointerInfo::touch(id, EventTime::ZERO),
        phase,
        position: at,
        button: None,
        modifiers: Modifiers::NONE,
        coalesced: Vec::new(),
    }
}

/// What one test scrollable recorded.
#[derive(Default, Debug)]
struct Log {
    /// Scroll deltas the node **absorbed**, newest last.
    absorbed: Vec<f32>,
    /// Scroll deltas it was offered and declined.
    declined: Vec<f32>,
    /// Phases it was offered, in order.
    phases: Vec<ScrollPhase>,
    /// `PointerCancel`s it received.
    cancels: Vec<CancelReason>,
    /// Its scroll offset.
    offset: f32,
}

type Shared = Rc<RefCell<Log>>;

/// A test scrollable: a vertical [`PanClaim`], plus the exact `on_scroll`
/// contract `teksilo-widgets`' `common/scroll.rs` implements — clamp to
/// `[0, max]`, answer `Handled` when the axis absorbed anything and `Ignored`
/// at a hard boundary.
fn scrollable(
    log: Shared,
    max: f32,
    children: Vec<WidgetId>,
) -> impl crate::widget::Widget + 'static {
    scrollable_with(log, max, children, OverscrollBehavior::Chain)
}

/// [`scrollable`], declaring an explicit boundary policy.
///
/// Taken as a parameter rather than chained on afterwards: a second
/// `WidgetBuilder` call on an already-wrapped `impl Widget` re-wraps rather
/// than merging, and the inner wrapper's handlers would be dropped.
fn scrollable_with(
    log: Shared,
    max: f32,
    children: Vec<WidgetId>,
    overscroll: OverscrollBehavior,
) -> impl crate::widget::Widget + 'static {
    let scroll_log = log.clone();
    let cancel_log = log;
    let mut stack = StackWidget::new();
    for child in children {
        stack = stack.add_child(child);
    }
    stack
        .scroll_container(PanAxes::Y)
        .overscroll_behavior(overscroll)
        .on_scroll(move |event, _ctx| {
            let WidgetEvent::Scroll { delta, phase, .. } = event else {
                return EventResponse::Ignored;
            };
            let dy = match delta {
                ScrollDelta::Pixels { y, .. } => *y,
                ScrollDelta::Lines { y, .. } => *y * 16.0,
            };
            let mut log = scroll_log.borrow_mut();
            log.phases.push(*phase);
            let before = log.offset;
            log.offset = (before + dy).clamp(0.0, max);
            let moved = (log.offset - before).abs() > crate::overscroll::SCROLL_MOVE_EPSILON;
            if moved {
                log.absorbed.push(dy);
                EventResponse::Handled
            } else {
                log.declined.push(dy);
                EventResponse::Ignored
            }
        })
        .on_pointer_cancel(move |_info, reason, _ctx| {
            cancel_log.borrow_mut().cancels.push(reason);
        })
}

/// A tree of `outer { inner }`, both scrollables, filling 200 × 200.
struct Nested {
    tree: WidgetTree,
    inner: WidgetId,
    outer: WidgetId,
    inner_log: Shared,
    outer_log: Shared,
}

fn nested(inner_max: f32, outer_max: f32) -> Nested {
    let inner_log = Shared::default();
    let outer_log = Shared::default();
    let mut tree = WidgetTree::new();
    let inner = tree.add(scrollable(inner_log.clone(), inner_max, vec![]));
    let outer = tree.add(scrollable(outer_log.clone(), outer_max, vec![inner]));
    tree.layout(SizeProposal::exact(200.0, 200.0));
    Nested {
        tree,
        inner,
        outer,
        inner_log,
        outer_log,
    }
}

/// The pan slop for a contact, from the shipped touch profile.
fn pan_slop() -> f32 {
    crate::gesture::default_profile(teksilo_tokens::PointerKind::Touch)
        .pan_slop
        .expect("touch pans")
}

/// Press, then drag by `dy` in steps large enough to cross the pan slop.
///
/// Returns the finger's final position.
fn drag(tree: &mut WidgetTree, id: PointerId, from: Point, dy: f32) -> Point {
    tree.dispatch_pointer(contact(id, PointerPhase::Down, from));
    // One step past the slop to take the claim, then the movement itself.
    let arm = Point::new(from.x, from.y + pan_slop().copysign(dy) + dy.signum());
    tree.dispatch_pointer(contact(id, PointerPhase::Move, arm));
    let mut at = arm;
    let remaining = dy - (arm.y - from.y);
    if remaining.abs() > 0.0 {
        at = Point::new(from.x, arm.y + remaining);
        tree.dispatch_pointer(contact(id, PointerPhase::Move, at));
    }
    at
}

// ---------------------------------------------------------------------------
// The boundary rule
// ---------------------------------------------------------------------------

/// A finger on the inner list scrolls the **inner** list, and the outer one is
/// never offered the event while the inner can still absorb it.
#[test]
fn the_inner_container_keeps_the_claim_while_it_can_absorb() {
    let mut n = nested(1000.0, 1000.0);
    let finger = contact_id(1);
    drag(&mut n.tree, finger, Point::new(100.0, 100.0), -60.0);

    assert!(
        n.inner_log.borrow().offset > 0.0,
        "the inner list scrolled: {:?}",
        n.inner_log.borrow()
    );
    assert_eq!(
        n.outer_log.borrow().phases.len(),
        0,
        "the outer container was never offered an event the inner absorbed"
    );
}

/// At the inner list's boundary the **same whole event** goes to the outer one.
///
/// No residual and no back-channel: the delta the outer container receives is
/// bit-for-bit the delta the inner one declined, not a leftover.
#[test]
fn a_boundary_pan_hands_the_whole_event_outward_with_no_residual() {
    // The inner list has nothing to scroll, so it declines every sample.
    let mut n = nested(0.0, 1000.0);
    let finger = contact_id(2);
    drag(&mut n.tree, finger, Point::new(100.0, 100.0), -60.0);

    let inner = n.inner_log.borrow();
    let outer = n.outer_log.borrow();
    assert!(
        !inner.declined.is_empty(),
        "the inner list should have declined at its boundary"
    );
    assert!(
        !outer.absorbed.is_empty(),
        "the outer container should have taken what the inner declined"
    );
    assert_eq!(
        inner.declined, outer.absorbed,
        "the outer container receives the SAME whole deltas, never a residual"
    );
}

/// The claimant the chain moves past is told nothing at all — no
/// `PointerCancel`, and no terminal scroll phase. It did not lose the gesture.
#[test]
fn a_chained_past_claimant_gets_no_cancel_and_no_pan_ended() {
    let mut n = nested(0.0, 1000.0);
    let finger = contact_id(3);
    let at = drag(&mut n.tree, finger, Point::new(100.0, 100.0), -60.0);
    n.tree
        .dispatch_pointer(contact(finger, PointerPhase::Up, at));

    let inner = n.inner_log.borrow();
    assert!(
        inner.cancels.is_empty(),
        "the inner container must not be cancelled for declining an event: {:?}",
        inner.cancels
    );
    assert!(
        !inner.phases.contains(&ScrollPhase::Cancelled),
        "…and it must not be told the gesture was cancelled either"
    );
    // It DOES see the `Ended`, because the chain always offers it the event
    // first: the claim never left it.
    assert!(
        inner.phases.contains(&ScrollPhase::Ended),
        "the claimant is still first in line on the release"
    );
}

/// The inner container is offered the event first on **every** sample, even
/// after it declined one — the claim stays with it for the whole gesture.
#[test]
fn the_claim_does_not_move_outward_after_one_declined_event() {
    // A tiny inner range: it absorbs the first sample and then hits its end.
    let mut n = nested(4.0, 1000.0);
    let finger = contact_id(4);
    let from = Point::new(100.0, 100.0);
    n.tree
        .dispatch_pointer(contact(finger, PointerPhase::Down, from));
    let mut y = from.y;
    for _ in 0..4 {
        y -= pan_slop() + 10.0;
        n.tree
            .dispatch_pointer(contact(finger, PointerPhase::Move, Point::new(from.x, y)));
    }

    let inner = n.inner_log.borrow();
    let outer = n.outer_log.borrow();
    assert!(
        inner.phases.len() > outer.phases.len(),
        "the inner container saw every sample ({}) and the outer only the ones \
         the inner could not use ({})",
        inner.phases.len(),
        outer.phases.len()
    );
    assert!(
        !inner.absorbed.is_empty() && !inner.declined.is_empty(),
        "the fixture must actually reach the inner boundary mid-gesture"
    );
}

/// `OverscrollBehavior::Contain` stops the chain, even having absorbed nothing.
#[test]
fn contain_stops_the_chain() {
    let inner_log = Shared::default();
    let outer_log = Shared::default();
    let mut tree = WidgetTree::new();
    let inner = tree.add(scrollable_with(
        inner_log.clone(),
        0.0,
        vec![],
        OverscrollBehavior::Contain,
    ));
    let _outer = tree.add(scrollable(outer_log.clone(), 1000.0, vec![inner]));
    tree.layout(SizeProposal::exact(200.0, 200.0));

    let finger = contact_id(5);
    drag(&mut tree, finger, Point::new(100.0, 100.0), -60.0);

    assert!(
        !inner_log.borrow().declined.is_empty(),
        "the contained container still declined the event"
    );
    assert_eq!(
        outer_log.borrow().phases.len(),
        0,
        "…and `Contain` stopped the chain before the outer container saw it"
    );
}

/// The chain visits **only** pan claimants.
///
/// This node stands for `SpinBox`, which increments its value on `on_scroll`
/// and declares no `PanClaim` at all. It sits on the bubble path between the
/// inner list and the outer container; a boundary pan that reached it would
/// silently change a number because the user ran out of list.
#[test]
fn a_boundary_pan_never_reaches_a_spin_box_shaped_on_scroll_handler() {
    let inner_log = Shared::default();
    let outer_log = Shared::default();
    let spin_box_value = Rc::new(RefCell::new(0i32));
    let value = spin_box_value.clone();

    let mut tree = WidgetTree::new();
    let inner = tree.add(scrollable(inner_log.clone(), 0.0, vec![]));
    // No `.scroll_container(..)`: a SpinBox is not a pan surface.
    let spin_box = tree.add(
        StackWidget::new()
            .add_child(inner)
            .on_scroll(move |_event, _ctx| {
                *value.borrow_mut() += 1;
                EventResponse::Handled
            }),
    );
    let _outer = tree.add(scrollable(outer_log.clone(), 1000.0, vec![spin_box]));
    tree.layout(SizeProposal::exact(200.0, 200.0));

    let finger = contact_id(6);
    drag(&mut tree, finger, Point::new(100.0, 100.0), -60.0);

    assert_eq!(
        *spin_box_value.borrow(),
        0,
        "a boundary pan must never reach a non-claimant `on_scroll` handler"
    );
    assert!(
        !outer_log.borrow().absorbed.is_empty(),
        "…and it must reach the next real claimant outward"
    );
}

/// The same shape again with the node that stands for `TabBar`, which remaps a
/// wheel to horizontal tab scrolling and would switch tabs under a finger.
#[test]
fn a_boundary_pan_never_reaches_a_tab_bar_shaped_wheel_remap() {
    let inner_log = Shared::default();
    let outer_log = Shared::default();
    let selected_tab = Rc::new(RefCell::new(0i32));
    let tab = selected_tab.clone();

    let mut tree = WidgetTree::new();
    let inner = tree.add(scrollable(inner_log.clone(), 0.0, vec![]));
    let tab_bar = tree.add(
        StackWidget::new()
            .add_child(inner)
            .on_scroll(move |event, _ctx| {
                // A TabBar's remap: any vertical wheel becomes a tab step.
                if let WidgetEvent::Scroll { delta, .. } = event {
                    let dy = match delta {
                        ScrollDelta::Pixels { y, .. } => *y,
                        ScrollDelta::Lines { y, .. } => *y,
                    };
                    *tab.borrow_mut() += dy.signum() as i32;
                }
                EventResponse::Handled
            }),
    );
    let _outer = tree.add(scrollable(outer_log.clone(), 1000.0, vec![tab_bar]));
    tree.layout(SizeProposal::exact(200.0, 200.0));

    let finger = contact_id(7);
    drag(&mut tree, finger, Point::new(100.0, 100.0), -60.0);

    assert_eq!(
        *selected_tab.borrow(),
        0,
        "a boundary pan must never reach a TabBar's wheel remap"
    );
}

/// A **mouse wheel** still bubbles: it reaches the non-claimant handler exactly
/// as it always has. The claimant chain is for fingers, and nothing else.
#[test]
fn a_mouse_wheel_still_reaches_a_non_claimant_on_scroll_handler() {
    let value = Rc::new(RefCell::new(0i32));
    let counter = value.clone();
    let mut tree = WidgetTree::new();
    let spin_box = tree.add(FillWidget::new().on_scroll(move |_event, _ctx| {
        *counter.borrow_mut() += 1;
        EventResponse::Handled
    }));
    let _root = tree.add(StackWidget::new().add_child(spin_box));
    tree.layout(SizeProposal::exact(200.0, 200.0));

    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(100.0, 100.0),
    });
    tree.dispatch_event(WidgetEvent::scroll(
        ScrollDelta::Lines { x: 0.0, y: -1.0 },
        Modifiers::NONE,
    ));
    assert_eq!(*value.borrow(), 1, "the wheel bubbles, unchanged");
}

/// A mouse wheel at a boundary still chains through the *bubble*, ancestor by
/// ancestor — the route it has always taken.
#[test]
fn mouse_wheel_boundary_chaining_is_unchanged() {
    let mut n = nested(0.0, 1000.0);
    n.tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(100.0, 100.0),
    });
    n.tree.dispatch_event(WidgetEvent::scroll(
        ScrollDelta::Pixels { x: 0.0, y: 30.0 },
        Modifiers::NONE,
    ));

    assert_eq!(
        n.inner_log.borrow().declined,
        vec![30.0],
        "the inner container declined at its boundary"
    );
    assert_eq!(
        n.outer_log.borrow().absorbed,
        vec![30.0],
        "…and the wheel bubbled to the outer container unchanged"
    );
}

// ---------------------------------------------------------------------------
// Fling
// ---------------------------------------------------------------------------

/// A fast release starts a coast, `advance_time` moves it, and it stops at the
/// boundary rather than spinning.
#[test]
fn a_release_flings_and_advance_time_moves_it() {
    let mut n = nested(10_000.0, 10_000.0);
    let clock = Rc::new(ManualClock::new(EventTime::ZERO));
    n.tree.set_input_clock(clock.clone());

    let finger = contact_id(8);
    let from = Point::new(100.0, 180.0);
    n.tree
        .dispatch_pointer(contact(finger, PointerPhase::Down, from));
    // A fast flick: 20 dp every 4 ms = 5000 dp/s, well past
    // `min_fling_velocity`.
    let mut y = from.y;
    for step in 1..=5 {
        clock.set(EventTime::from_millis(step * 4));
        y -= 20.0;
        n.tree
            .dispatch_pointer(contact(finger, PointerPhase::Move, Point::new(from.x, y)));
    }
    clock.set(EventTime::from_millis(24));
    n.tree
        .dispatch_pointer(contact(finger, PointerPhase::Up, Point::new(from.x, y)));

    assert!(
        n.tree.is_flinging(n.inner),
        "a fast release hands off to a coast"
    );
    let after_release = n.inner_log.borrow().offset;

    n.tree.advance_time(Duration::from_millis(100));
    let coasting = n.inner_log.borrow().offset;
    assert!(
        coasting > after_release,
        "one `advance_time` moves the fling: {after_release} -> {coasting}"
    );

    // Let it run out. The clamped simulation settles well inside a second.
    for _ in 0..40 {
        n.tree.advance_time(Duration::from_millis(50));
    }
    assert!(
        !n.tree.is_flinging(n.inner),
        "the coast finishes rather than running for ever"
    );
}

/// A fling that runs out of inner list scrolls the outer one — the same chain,
/// the same rule, and the inner container is still told nothing terminal.
#[test]
fn a_fling_chains_at_a_boundary_exactly_as_a_pan_does() {
    // The inner list can take 30 dp and no more; the outer is deep.
    let mut n = nested(30.0, 10_000.0);
    let clock = Rc::new(ManualClock::new(EventTime::ZERO));
    n.tree.set_input_clock(clock.clone());

    let finger = contact_id(9);
    let from = Point::new(100.0, 180.0);
    n.tree
        .dispatch_pointer(contact(finger, PointerPhase::Down, from));
    let mut y = from.y;
    for step in 1..=5 {
        clock.set(EventTime::from_millis(step * 4));
        y -= 20.0;
        n.tree
            .dispatch_pointer(contact(finger, PointerPhase::Move, Point::new(from.x, y)));
    }
    clock.set(EventTime::from_millis(24));
    n.tree
        .dispatch_pointer(contact(finger, PointerPhase::Up, Point::new(from.x, y)));
    assert!(n.tree.is_flinging(n.inner));

    let outer_before = n.outer_log.borrow().offset;
    for _ in 0..20 {
        n.tree.advance_time(Duration::from_millis(16));
    }

    assert_eq!(
        n.inner_log.borrow().offset,
        30.0,
        "the inner list is pinned at its end"
    );
    assert!(
        n.outer_log.borrow().offset > outer_before,
        "…and the coast chained outward"
    );
    assert!(
        n.inner_log.borrow().cancels.is_empty(),
        "chaining a fling cancels nobody either"
    );
}

/// A press on a coasting surface catches it.
#[test]
fn a_pointer_down_on_a_flinging_target_stops_the_fling() {
    let mut n = nested(10_000.0, 10_000.0);
    let clock = Rc::new(ManualClock::new(EventTime::ZERO));
    n.tree.set_input_clock(clock.clone());

    let finger = contact_id(10);
    let from = Point::new(100.0, 180.0);
    n.tree
        .dispatch_pointer(contact(finger, PointerPhase::Down, from));
    let mut y = from.y;
    for step in 1..=5 {
        clock.set(EventTime::from_millis(step * 4));
        y -= 20.0;
        n.tree
            .dispatch_pointer(contact(finger, PointerPhase::Move, Point::new(from.x, y)));
    }
    clock.set(EventTime::from_millis(24));
    n.tree
        .dispatch_pointer(contact(finger, PointerPhase::Up, Point::new(from.x, y)));
    assert!(n.tree.is_flinging(n.inner));

    let catcher = contact_id(11);
    n.tree.dispatch_pointer(contact(
        catcher,
        PointerPhase::Down,
        Point::new(100.0, 100.0),
    ));
    assert!(
        !n.tree.is_flinging(n.inner),
        "pressing a coasting list catches it"
    );
}

/// macOS reports its own momentum. Starting a Teksilo fling on top of that is
/// the double-momentum bug, so a momentum-phase release must add nothing —
/// which is the rule [`KineticScroller::should_fling_for_phase`] states and
/// which the pan path inherits by never treating a momentum sample as a
/// release at all.
#[test]
fn macos_momentum_does_not_stack_a_second_fling() {
    let mut n = nested(10_000.0, 10_000.0);
    n.tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(100.0, 100.0),
    });
    // The OS momentum stream: pixel deltas with `phase: Momentum`, routed as
    // an ordinary (bubbling) trackpad scroll.
    for _ in 0..5 {
        n.tree.dispatch_scroll(crate::pointer::ScrollSample {
            delta: ScrollDelta::Pixels { x: 0.0, y: 20.0 },
            position: Some(Point::new(100.0, 100.0)),
            phase: ScrollPhase::Momentum,
            source: crate::pointer::ScrollSource::Trackpad,
            pointer: PointerInfo::mouse(EventTime::ZERO),
            modifiers: Modifiers::NONE,
        });
    }
    assert!(
        n.inner_log.borrow().offset > 0.0,
        "the OS momentum still scrolls the list"
    );
    assert!(
        !n.tree.is_flinging(n.inner) && !n.tree.is_flinging(n.outer),
        "…and no Teksilo coast is started on top of it"
    );
}

/// The fling is an input deadline like any other, and it is folded into the one
/// `WaitUntil`.
#[test]
fn next_input_deadline_folds_a_live_simulation() {
    let mut n = nested(10_000.0, 10_000.0);
    assert_eq!(
        n.tree.next_input_deadline(),
        None,
        "nothing pending, nothing to wake for"
    );

    n.tree.start_fling(
        n.inner,
        Vec2::new(0.0, 2000.0),
        vec![(n.inner, PanClaim::vertical())],
    );
    assert!(n.tree.is_flinging(n.inner));
    let deadline = n
        .tree
        .next_input_deadline()
        .expect("a live simulation wants the loop back");
    assert!(
        n.tree.next_timer_deadline() <= Some(deadline),
        "…and the tree's one `WaitUntil` is at or before it"
    );

    n.tree.stop_fling(n.inner);
    assert_eq!(n.tree.next_input_deadline(), None);
}

/// `prefers-reduced-motion` collapses the coast: the content stays where the
/// finger left it.
#[test]
fn reduced_motion_starts_no_coast() {
    let mut n = nested(10_000.0, 10_000.0);
    n.tree.set_accessibility_preferences(false, true, 1.0);
    n.tree.start_fling(
        n.inner,
        Vec2::new(0.0, 4000.0),
        vec![(n.inner, PanClaim::vertical())],
    );
    assert!(!n.tree.is_flinging(n.inner));
}

// ---------------------------------------------------------------------------
// Pinch — the two ingresses
// ---------------------------------------------------------------------------

/// Everything one `on_pinch` handler was told, in order, in a form two runs can
/// be compared by.
#[derive(Clone, Debug, PartialEq)]
enum PinchNote {
    Started,
    Changed { scale: i32, rotation: i32 },
    Ended,
}

fn pinch_recorder(notes: Rc<RefCell<Vec<PinchNote>>>) -> impl crate::widget::Widget + 'static {
    FillWidget::new()
        .touch_action(TouchAction::MANIPULATION)
        .on_pinch(move |phase, _ctx| {
            let note = match phase {
                PinchPhase::Started { .. } => PinchNote::Started,
                PinchPhase::Changed {
                    scale, rotation, ..
                } => PinchNote::Changed {
                    // Quantised so the two runs are compared on what a consumer
                    // acts on rather than on float noise.
                    scale: (scale * 1000.0).round() as i32,
                    rotation: (rotation * 1000.0).round() as i32,
                },
                PinchPhase::Ended { .. } => PinchNote::Ended,
                _ => return,
            };
            notes.borrow_mut().push(note);
        })
}

/// **The point of the package.** Two contacts and the OS trackpad stream
/// produce the *same* pinch stream, because both go through the one ingress.
#[test]
fn the_two_pinch_ingresses_produce_the_same_stream() {
    // The geometry both runs describe: two contacts 100 dp apart, spreading to
    // 200 dp in two steps, then one lifts.
    let spans = [140.0f32, 200.0f32];

    // --- ingress 1: two real contacts ---------------------------------
    let touch_notes = Rc::new(RefCell::new(Vec::new()));
    let mut tree = WidgetTree::new();
    tree.add(pinch_recorder(touch_notes.clone()));
    tree.layout(SizeProposal::exact(400.0, 400.0));

    let a = contact_id(20);
    let b = contact_id(21);
    let centre = 200.0f32;
    tree.dispatch_pointer(contact(
        a,
        PointerPhase::Down,
        Point::new(centre - 50.0, 200.0),
    ));
    tree.dispatch_pointer(contact(
        b,
        PointerPhase::Down,
        Point::new(centre + 50.0, 200.0),
    ));
    for span in spans {
        tree.dispatch_pointer(contact(
            b,
            PointerPhase::Move,
            Point::new(centre - 50.0 + span, 200.0),
        ));
    }
    tree.dispatch_pointer(contact(
        a,
        PointerPhase::Up,
        Point::new(centre - 50.0, 200.0),
    ));

    // --- ingress 2: the OS trackpad stream ----------------------------
    let os_notes = Rc::new(RefCell::new(Vec::new()));
    let mut os_tree = WidgetTree::new();
    os_tree.add(pinch_recorder(os_notes.clone()));
    os_tree.layout(SizeProposal::exact(400.0, 400.0));

    let mut ops = crate::window::NoopWindowOps;
    // The OS reports the same geometry: a start, then the same two scales, then
    // an end. Centres match because the first contact never moved.
    os_tree.dispatch_os_gesture(
        GestureEvent::PinchStarted {
            center: Point::new(centre, 200.0),
        },
        Some(Point::new(centre, 200.0)),
        &mut ops,
    );
    for span in spans {
        let center = Point::new(centre - 50.0 + span / 2.0, 200.0);
        os_tree.dispatch_os_gesture(
            GestureEvent::PinchChanged {
                center,
                scale: span / 100.0,
                rotation: 0.0,
            },
            Some(center),
            &mut ops,
        );
    }
    os_tree.dispatch_os_gesture(GestureEvent::PinchEnded, None, &mut ops);

    assert_eq!(
        *touch_notes.borrow(),
        vec![
            PinchNote::Started,
            PinchNote::Changed {
                scale: 1400,
                rotation: 0
            },
            PinchNote::Changed {
                scale: 2000,
                rotation: 0
            },
            PinchNote::Ended,
        ],
        "the touchscreen stream is Started / Changed × 2 / Ended"
    );
    assert_eq!(
        *touch_notes.borrow(),
        *os_notes.borrow(),
        "the two ingresses must produce the same stream — that is the whole point"
    );
}

/// A third contact never disturbs a running pinch.
#[test]
fn a_third_contact_is_ignored_by_the_tree() {
    let notes = Rc::new(RefCell::new(Vec::new()));
    let mut tree = WidgetTree::new();
    tree.add(pinch_recorder(notes.clone()));
    tree.layout(SizeProposal::exact(400.0, 400.0));

    let a = contact_id(22);
    let b = contact_id(23);
    let c = contact_id(24);
    tree.dispatch_pointer(contact(a, PointerPhase::Down, Point::new(150.0, 200.0)));
    tree.dispatch_pointer(contact(b, PointerPhase::Down, Point::new(250.0, 200.0)));
    let after_start = notes.borrow().len();

    tree.dispatch_pointer(contact(c, PointerPhase::Down, Point::new(200.0, 350.0)));
    tree.dispatch_pointer(contact(c, PointerPhase::Move, Point::new(200.0, 380.0)));
    assert_eq!(
        notes.borrow().len(),
        after_start,
        "the third contact produced no pinch phase at all"
    );
    assert!(tree.touch_pinch_active(), "…and the pinch is still running");
}

/// A contact leaving mid-pinch ends the gesture.
#[test]
fn a_contact_leaving_mid_pinch_ends_it() {
    let notes = Rc::new(RefCell::new(Vec::new()));
    let mut tree = WidgetTree::new();
    tree.add(pinch_recorder(notes.clone()));
    tree.layout(SizeProposal::exact(400.0, 400.0));

    let a = contact_id(25);
    let b = contact_id(26);
    tree.dispatch_pointer(contact(a, PointerPhase::Down, Point::new(150.0, 200.0)));
    tree.dispatch_pointer(contact(b, PointerPhase::Down, Point::new(250.0, 200.0)));
    tree.dispatch_pointer(contact(a, PointerPhase::Up, Point::new(150.0, 200.0)));

    assert_eq!(notes.borrow().last(), Some(&PinchNote::Ended));
    assert!(!tree.touch_pinch_active());
}

/// A subtree that forbids pinch-zoom gets none.
#[test]
fn touch_action_none_permits_no_pinch() {
    let notes = Rc::new(RefCell::new(Vec::new()));
    let mut tree = WidgetTree::new();
    let inner = tree.add(pinch_recorder(notes.clone()));
    // The ancestor narrows to NONE, which intersection makes absorbing.
    let _root = tree.add(
        StackWidget::new()
            .add_child(inner)
            .touch_action(TouchAction::NONE),
    );
    tree.layout(SizeProposal::exact(400.0, 400.0));

    let a = contact_id(27);
    let b = contact_id(28);
    tree.dispatch_pointer(contact(a, PointerPhase::Down, Point::new(150.0, 200.0)));
    tree.dispatch_pointer(contact(b, PointerPhase::Down, Point::new(250.0, 200.0)));
    assert!(notes.borrow().is_empty());
    assert!(!tree.touch_pinch_active());
}

// ---------------------------------------------------------------------------
// Palm
// ---------------------------------------------------------------------------

/// A large contact patch that never moves fires no tap and is cancelled
/// `PalmRejected`.
#[test]
fn a_large_stationary_contact_is_rejected_as_a_palm() {
    let taps = Rc::new(RefCell::new(0i32));
    let cancels = Rc::new(RefCell::new(Vec::new()));
    let tap_count = taps.clone();
    let cancel_log = cancels.clone();

    let mut tree = WidgetTree::new();
    tree.add(
        FillWidget::new()
            .on_tap(move |_e, _c| *tap_count.borrow_mut() += 1)
            .on_pointer_cancel(move |_info, reason, _c| cancel_log.borrow_mut().push(reason)),
    );
    tree.layout(SizeProposal::exact(200.0, 200.0));
    assert!(
        tree.palm_fallback_active(),
        "the fallback is on for a backend that reports no palms"
    );

    let palm = contact_id(30);
    let at = Point::new(100.0, 100.0);
    let mut down = contact(palm, PointerPhase::Down, at);
    down.pointer.axes = PointerAxes {
        contact: Some(Size::new(70.0, 70.0)),
        ..down.pointer.axes
    };
    let mut up = contact(palm, PointerPhase::Up, at);
    up.pointer.axes = down.pointer.axes;

    tree.dispatch_pointer(down);
    tree.dispatch_pointer(up);

    assert_eq!(*taps.borrow(), 0, "a palm fires no tap");
    assert_eq!(
        *cancels.borrow(),
        vec![CancelReason::PalmRejected],
        "…and is revoked with the reason that says why"
    );
}

/// A fingertip taps normally, and so does a large contact that actually
/// travelled — false-rejecting a real gesture is the worse failure, so the
/// heuristic needs *both* halves to be true.
#[test]
fn a_fingertip_and_a_travelling_large_contact_both_tap() {
    for (extent, travel) in [(12.0f32, 0.0f32), (70.0, 40.0)] {
        let taps = Rc::new(RefCell::new(0i32));
        let cancels = Rc::new(RefCell::new(Vec::new()));
        let counter = taps.clone();
        let cancel_log = cancels.clone();
        let mut tree = WidgetTree::new();
        tree.add(
            FillWidget::new()
                .on_tap(move |_e, _c| *counter.borrow_mut() += 1)
                .on_pointer_cancel(move |_i, reason, _c| cancel_log.borrow_mut().push(reason)),
        );
        tree.layout(SizeProposal::exact(200.0, 200.0));

        let finger = contact_id(31);
        let from = Point::new(100.0, 100.0);
        let axes = PointerAxes {
            contact: Some(Size::new(extent, extent)),
            ..PointerAxes::default()
        };
        let mut down = contact(finger, PointerPhase::Down, from);
        down.pointer.axes = axes;
        tree.dispatch_pointer(down);
        let to = Point::new(from.x + travel, from.y);
        if travel > 0.0 {
            let mut moved = contact(finger, PointerPhase::Move, to);
            moved.pointer.axes = axes;
            tree.dispatch_pointer(moved);
        }
        let mut up = contact(finger, PointerPhase::Up, to);
        up.pointer.axes = axes;
        tree.dispatch_pointer(up);

        assert_eq!(
            *taps.borrow(),
            1,
            "extent {extent}, travel {travel}: the tap must stand"
        );
        assert!(
            cancels.borrow().is_empty(),
            "extent {extent}, travel {travel}: and nothing may be revoked — \
             got {:?}",
            cancels.borrow()
        );
    }
}

/// A backend that classifies palms itself turns the heuristic off.
#[test]
fn a_palm_reporting_backend_disables_the_fallback() {
    let taps = Rc::new(RefCell::new(0i32));
    let counter = taps.clone();
    let mut tree = WidgetTree::new();
    tree.add(FillWidget::new().on_tap(move |_e, _c| *counter.borrow_mut() += 1));
    tree.layout(SizeProposal::exact(200.0, 200.0));
    tree.set_backend_reports_palm(true);
    assert!(!tree.palm_fallback_active());

    let big = contact_id(32);
    let at = Point::new(100.0, 100.0);
    let axes = PointerAxes {
        contact: Some(Size::new(70.0, 70.0)),
        ..PointerAxes::default()
    };
    let mut down = contact(big, PointerPhase::Down, at);
    down.pointer.axes = axes;
    let mut up = contact(big, PointerPhase::Up, at);
    up.pointer.axes = axes;
    tree.dispatch_pointer(down);
    tree.dispatch_pointer(up);

    assert_eq!(
        *taps.borrow(),
        1,
        "with the digitiser answering, the guess is off and the tap stands"
    );
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

/// The route is decided by the source, and only a synthesised pan chains.
#[test]
fn only_a_touch_pan_takes_the_claimant_chain() {
    use crate::pointer::ScrollSource;
    assert_eq!(
        ScrollDelivery::for_source(ScrollSource::TouchPan),
        ScrollDelivery::ClaimantChain
    );
    for source in [
        ScrollSource::Wheel,
        ScrollSource::Trackpad,
        ScrollSource::Programmatic,
    ] {
        assert_eq!(
            ScrollDelivery::for_source(source),
            ScrollDelivery::Bubble,
            "{source:?} keeps the route it has always had"
        );
    }
}

/// A `TouchPan` sample from a backend that recognises pans itself derives its
/// own chain from the sample's position — the door P18 uses.
#[test]
fn an_external_touch_pan_sample_derives_its_own_chain() {
    let mut n = nested(0.0, 1000.0);
    n.tree.dispatch_scroll(crate::pointer::ScrollSample {
        delta: ScrollDelta::Pixels { x: 0.0, y: 40.0 },
        position: Some(Point::new(100.0, 100.0)),
        phase: ScrollPhase::Changed,
        source: crate::pointer::ScrollSource::TouchPan,
        pointer: PointerInfo::touch(contact_id(40), EventTime::ZERO),
        modifiers: Modifiers::NONE,
    });
    assert_eq!(
        n.inner_log.borrow().declined,
        vec![40.0],
        "the innermost claimant was offered it first"
    );
    assert_eq!(
        n.outer_log.borrow().absorbed,
        vec![40.0],
        "…and it chained outward whole"
    );
}

/// A claimant destroyed mid-gesture is skipped, not treated as the end of the
/// chain: the containers outward of it are still entitled to the event.
#[test]
fn a_destroyed_claimant_is_skipped_rather_than_ending_the_chain() {
    let mut n = nested(0.0, 1000.0);
    let finger = contact_id(41);
    n.tree.dispatch_pointer(contact(
        finger,
        PointerPhase::Down,
        Point::new(100.0, 100.0),
    ));
    n.tree.destroy_subtree(n.inner);
    n.tree.dispatch_pointer(contact(
        finger,
        PointerPhase::Move,
        Point::new(100.0, 100.0 - pan_slop() - 20.0),
    ));

    assert!(
        !n.outer_log.borrow().absorbed.is_empty(),
        "the outer container still received the pan"
    );
}

/// A cancel takes the pan and the coast with it, and tells nobody a lie.
#[test]
fn a_cancel_abandons_the_pan_and_stops_the_coast() {
    let mut n = nested(10_000.0, 10_000.0);
    let finger = contact_id(42);
    let from = Point::new(100.0, 180.0);
    drag(&mut n.tree, finger, from, -60.0);
    n.tree.start_fling(
        n.inner,
        Vec2::new(0.0, 3000.0),
        vec![(n.inner, PanClaim::vertical())],
    );
    assert!(n.tree.is_flinging(n.inner));

    let mut ops = crate::window::NoopWindowOps;
    n.tree
        .cancel_pointer(finger, CancelReason::Platform, &mut ops);

    assert!(
        !n.tree.is_flinging(n.inner),
        "the coast the revoked gesture owned is stopped"
    );
    let before = n.inner_log.borrow().phases.len();
    n.tree.dispatch_pointer(contact(
        finger,
        PointerPhase::Move,
        Point::new(100.0, 100.0),
    ));
    assert_eq!(
        n.inner_log.borrow().phases.len(),
        before,
        "and the abandoned pan delivers nothing more"
    );
}

/// The synthesised scroll carries the wheel's sign convention: a finger
/// dragging **up** scrolls the content down, i.e. the offset increases.
#[test]
fn a_pan_carries_the_wheel_sign_convention() {
    let mut n = nested(1000.0, 1000.0);
    let finger = contact_id(43);
    drag(&mut n.tree, finger, Point::new(100.0, 150.0), -60.0);
    assert!(
        n.inner_log.borrow().offset > 0.0,
        "dragging up increases the offset, exactly as a wheel-down does"
    );

    let mut m = nested(1000.0, 1000.0);
    m.inner_log.borrow_mut().offset = 500.0;
    let other = contact_id(44);
    drag(&mut m.tree, other, Point::new(100.0, 50.0), 60.0);
    assert!(
        m.inner_log.borrow().offset < 500.0,
        "and dragging down decreases it"
    );
}

/// A pan reports phases in the order a continuous gesture has: one `Began`,
/// then `Changed`, then exactly one `Ended`.
#[test]
fn the_synthesised_phases_read_as_one_continuous_gesture() {
    let mut n = nested(10_000.0, 10_000.0);
    let finger = contact_id(45);
    let at = drag(&mut n.tree, finger, Point::new(100.0, 180.0), -60.0);
    n.tree
        .dispatch_pointer(contact(finger, PointerPhase::Up, at));

    let phases = n.inner_log.borrow().phases.clone();
    assert_eq!(phases.first(), Some(&ScrollPhase::Began));
    assert_eq!(phases.last(), Some(&ScrollPhase::Ended));
    assert_eq!(
        phases.iter().filter(|p| **p == ScrollPhase::Began).count(),
        1,
        "exactly one Began"
    );
    assert_eq!(
        phases.iter().filter(|p| **p == ScrollPhase::Ended).count(),
        1,
        "exactly one Ended"
    );
}

/// `Rect` is used only to keep the fixture honest about geometry; this asserts
/// the nested fixture really does put the inner container inside the outer one,
/// so every chaining test above is testing what it says it is.
#[test]
fn the_nested_fixture_really_nests() {
    let n = nested(100.0, 100.0);
    let inner = n.tree.bounds(n.inner);
    let outer = n.tree.bounds(n.outer);
    assert_eq!(outer, Rect::new(0.0, 0.0, 200.0, 200.0));
    assert_eq!(inner, outer, "the inner container fills the outer one");
    assert!(
        n.tree
            .pan_candidates(n.inner, TouchAction::AUTO)
            .iter()
            .map(|(id, _)| *id)
            .eq([n.inner, n.outer]),
        "…and the claimant chain is inner-then-outer"
    );
}

/// A press with the primary button is what a contact reports; the fixture would
/// be testing nothing if the tap owner never saw one.
#[test]
fn a_contact_presses_as_the_primary_button() {
    let seen = Rc::new(RefCell::new(None));
    let button = seen.clone();
    let mut tree = WidgetTree::new();
    tree.add(FillWidget::new().on_tap(move |event, _c| {
        *button.borrow_mut() = Some(event.button);
    }));
    tree.layout(SizeProposal::exact(100.0, 100.0));

    let finger = contact_id(46);
    let at = Point::new(50.0, 50.0);
    tree.dispatch_pointer(contact(finger, PointerPhase::Down, at));
    tree.dispatch_pointer(contact(finger, PointerPhase::Up, at));
    assert_eq!(*seen.borrow(), Some(PointerButton::Primary));
}
