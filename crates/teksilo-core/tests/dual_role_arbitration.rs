// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A node that declares a [`PanClaim`] **and** carries its own `on_drag`.
//!
//! One node holds exactly one `SequenceMember`, and `begin_sequence` enrols a
//! pan claimant before any handler runs — so a node that is both a pan surface
//! and a drag source used to have its drag half silently refused
//! (`PointerSequence::enrol` declines an already-enrolled id) and its
//! [`DragActivation`] never consulted. Its `DragRecognizer` then ran through
//! the ordinary capture dispatch, which *precedes* the arbitration walk,
//! latched at `drag_slop` and decided the sequence at half the travel the pan
//! needed. A surface shaped like that could not pan under a finger at all.
//!
//! The fix attaches the self-drag's deferral to the member the node already
//! has — `PointerSequence::defer_own_drag` — so the member goes on competing as
//! a pan at `pan_slop` while its own recognizers are held off until its
//! `DragActivation` allows them.
//!
//! Two fixtures, because the defect has two doors:
//!
//! * [`dual_role_captor`] — the dual-role node takes the press itself (the
//!   lightweight tier: a scene view whose content is paint-only items). The
//!   refusal happened in `enrol_sequence_members`' `widget_has_drag(captured)`
//!   arm.
//! * [`dual_role_ancestor`] — an interactive child takes the press and the
//!   dual-role node is a strict ancestor (the heavyweight tier: scene content
//!   that is a real widget). The refusal happened in the ancestor walk, through
//!   `enrol_drag` → `enrol`.
//!
//! Thresholds throughout are the Compact touch profile: `drag_slop` 18,
//! `pan_slop` 36, `long_press` 500 ms, `long_press_slop` 18.
//!
//! [`PanClaim`]: teksilo_core::pointer::touch_action::PanClaim
//! [`DragActivation`]: teksilo_tokens::DragActivation

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use common::{Leaf, Stack};
use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::event::{EventResponse, PointerButton, WidgetEvent};
use teksilo_core::gesture::{DragPhase, MemberRole, MemberState};
use teksilo_core::pointer::touch_action::PanClaim;
use teksilo_core::pointer::{CancelReason, PointerId};
use teksilo_core::widget::Widget;
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_tokens::{DragActivation, TargetDensity};

const LONG_PRESS: Duration = Duration::from_millis(500);

/// Everything a fixture's dual-role node observed during one press.
#[derive(Clone, Default)]
struct Log {
    drag_started: Rc<Cell<usize>>,
    scrolls: Rc<Cell<usize>>,
    taps: Rc<Cell<usize>>,
    long_presses: Rc<Cell<usize>>,
    cancels: Rc<RefCell<Vec<CancelReason>>>,
}

struct Fixture {
    tree: WidgetTree,
    /// The node holding both roles: a pan claim and its own drag.
    view: WidgetId,
    log: Log,
    press: Point,
}

impl Fixture {
    fn touch_down(&mut self) -> PointerId {
        let at = self.press;
        let id = self.tree.new_contact();
        self.tree.touch_down(id, at);
        id
    }

    fn touch_to(&mut self, pointer: PointerId, dy: f32) {
        let at = Point::new(self.press.x, self.press.y + dy);
        self.tree.touch_move(pointer, at);
    }

    fn touch_up(&mut self, pointer: PointerId, dy: f32) {
        let at = Point::new(self.press.x, self.press.y + dy);
        self.tree.touch_up(pointer, at);
    }

    /// The dual-role node's own membership: its role and where it stands.
    fn view_member(&self, pointer: PointerId) -> Vec<(MemberRole, MemberState)> {
        self.tree
            .sequence_members(pointer)
            .into_iter()
            .filter(|(id, _, _)| *id == self.view)
            .map(|(_, role, state)| (role, state))
            .collect()
    }
}

/// `view` — `pan_claim(both)` + `on_drag` + `on_scroll` + `on_tap` — over an
/// inert leaf, so the press is taken by the view's own arena.
///
/// This is `SceneView`'s shape with selection or magnetism on: one `HandlerSet`
/// carrying both the claim and the drag handlers.
fn dual_role_captor(activation: Option<DragActivation>) -> Fixture {
    dual_role_captor_with(activation, false)
}

/// [`dual_role_captor`], optionally with an `on_long_press` on the same node.
///
/// Separate because the two interact through a defect that predates this
/// mechanism and belongs to neither fixture — see
/// [`a_long_press_recognizer_on_the_same_node_disarms_the_grab`].
fn dual_role_captor_with(activation: Option<DragActivation>, long_press: bool) -> Fixture {
    let log = Log::default();
    let mut tree = WidgetTree::new();
    tree.set_density(TargetDensity::Compact);
    let leaf = tree.add(Leaf::new());

    let drag_started = log.drag_started.clone();
    let scrolls = log.scrolls.clone();
    let taps = log.taps.clone();
    let long_presses = log.long_presses.clone();
    let cancels = log.cancels.clone();

    let view = Stack::new()
        .add_child(leaf)
        .pan_claim(PanClaim::both())
        .on_drag(move |phase, _c| {
            if matches!(phase, DragPhase::Started { .. }) {
                drag_started.set(drag_started.get() + 1);
            }
        })
        .on_scroll(move |_e, _c| {
            scrolls.set(scrolls.get() + 1);
            EventResponse::Handled
        })
        .on_tap(move |_e, _c| taps.set(taps.get() + 1))
        .on_pointer_cancel(move |_p, reason, _c| cancels.borrow_mut().push(reason));
    let view = if long_press {
        view.on_long_press(move |_e, _c| long_presses.set(long_presses.get() + 1))
    } else {
        view
    };
    let view = match activation {
        Some(a) => tree.add(view.drag_activation(a)),
        None => tree.add(view),
    };

    tree.layout(SizeProposal::exact(400.0, 400.0));
    Fixture {
        tree,
        view,
        log,
        press: Point::new(200.0, 200.0),
    }
}

/// The same `view`, but with an interactive `item` between it and the press —
/// so the *child* takes the capture and the dual-role node is a strict
/// ancestor.
///
/// This is `SceneView` with a heavyweight (`add_widget_item`) child under the
/// finger: the captor is a real arena widget and the view reaches the
/// arbitration through the ancestor walk.
fn dual_role_ancestor() -> Fixture {
    let log = Log::default();
    let mut tree = WidgetTree::new();
    tree.set_density(TargetDensity::Compact);
    let leaf = tree.add(Leaf::new());
    let item = tree.add(Stack::new().add_child(leaf).on_tap(|_e, _c| {}));

    let drag_started = log.drag_started.clone();
    let scrolls = log.scrolls.clone();
    let cancels = log.cancels.clone();

    let view = tree.add(
        Stack::new()
            .add_child(item)
            .pan_claim(PanClaim::both())
            .on_drag(move |phase, _c| {
                if matches!(phase, DragPhase::Started { .. }) {
                    drag_started.set(drag_started.get() + 1);
                }
            })
            .on_scroll(move |_e, _c| {
                scrolls.set(scrolls.get() + 1);
                EventResponse::Handled
            })
            .on_pointer_cancel(move |_p, reason, _c| cancels.borrow_mut().push(reason)),
    );

    tree.layout(SizeProposal::exact(400.0, 400.0));
    Fixture {
        tree,
        view,
        log,
        press: Point::new(200.0, 200.0),
    }
}

// ---------------------------------------------------------------------------
// The defect: a dual-role node could not pan under a finger
// ---------------------------------------------------------------------------

/// The whole point. A finger on a node that claims a pan **and** owns a drag
/// pans it, instead of the drag latching at `drag_slop` and taking the press
/// before the claim's `pan_slop` is reached.
#[test]
fn a_finger_pans_a_node_that_also_owns_a_drag() {
    let mut fx = dual_role_captor(None);
    let p = fx.touch_down();

    // 20 dp: past drag_slop (18), short of pan_slop (36). Before the fix the
    // self-drag latched here and decided the sequence.
    fx.touch_to(p, 20.0);
    assert_eq!(
        fx.log.drag_started.get(),
        0,
        "the deferred self-drag must not latch at drag_slop while a pan is eligible"
    );
    assert_eq!(fx.tree.sequence_winner(p), None, "nobody has won at 20 dp");

    // 40 dp: past pan_slop. The claim takes the press and delivers movement.
    fx.touch_to(p, 40.0);
    assert_eq!(
        fx.tree.sequence_winner(p),
        Some(fx.view),
        "the pan claim wins at pan_slop"
    );
    assert_eq!(
        fx.view_member(p),
        vec![(MemberRole::Pan(PanClaim::both()), MemberState::Won)],
        "and it won as the pan it was competing as"
    );
    assert!(
        fx.log.scrolls.get() > 0,
        "a won pan delivers scroll, which the old behaviour never reached"
    );
    assert_eq!(fx.log.drag_started.get(), 0, "and the drag never started");

    fx.touch_up(p, 40.0);
    fx.tree.assert_no_leaked_pointer_state();
}

/// The other half of the deferral: hold past `long_press` and the node's own
/// drag is armed, so the same 20 dp that was a tap before the hold is a drag
/// after it — and no scroll is delivered.
#[test]
fn a_hold_arms_the_dual_role_nodes_own_drag() {
    let mut fx = dual_role_captor(None);
    let p = fx.touch_down();
    fx.tree.advance_time(LONG_PRESS);

    fx.touch_to(p, 20.0);
    assert_eq!(
        fx.log.drag_started.get(),
        1,
        "past the hold the self-drag is armed and latches at drag_slop"
    );
    assert_eq!(fx.tree.sequence_winner(p), Some(fx.view));
    assert_eq!(
        fx.view_member(p),
        vec![(MemberRole::Gesture, MemberState::Won)],
        "the member report names the half that actually won"
    );
    assert_eq!(fx.log.scrolls.get(), 0, "and the pan delivered nothing");

    fx.touch_up(p, 20.0);
    fx.tree.assert_no_leaked_pointer_state();
}

/// A node that declares `DragActivation::Immediate` opts out of the deferral
/// entirely: its drag latches at `drag_slop` as it always did.
#[test]
fn an_immediate_activation_keeps_the_old_latch() {
    let mut fx = dual_role_captor(Some(DragActivation::Immediate));
    let p = fx.touch_down();

    fx.touch_to(p, 20.0);
    assert_eq!(
        fx.log.drag_started.get(),
        1,
        "Immediate is the opt-out: no hold required"
    );
    assert_eq!(fx.tree.sequence_winner(p), Some(fx.view));
    assert_eq!(fx.log.scrolls.get(), 0);

    fx.touch_up(p, 20.0);
    fx.tree.assert_no_leaked_pointer_state();
}

/// The dead band the deferral introduces: travel between `drag_slop` and
/// `pan_slop` used to produce a drag and now completes as a tap, because
/// `TapBoundary::Bounds` governs a coarse pointer on a node it never leaves.
#[test]
fn the_dead_band_between_the_two_slops_is_a_tap() {
    let mut fx = dual_role_captor(None);
    let p = fx.touch_down();
    fx.touch_to(p, 20.0);
    fx.touch_up(p, 20.0);

    assert_eq!(fx.log.drag_started.get(), 0, "no drag");
    assert_eq!(fx.log.scrolls.get(), 0, "no scroll");
    assert_eq!(fx.log.taps.get(), 1, "one tap");
    fx.tree.assert_no_leaked_pointer_state();
}

// ---------------------------------------------------------------------------
// The heavyweight door: the dual-role node as a strict ancestor
// ---------------------------------------------------------------------------

/// The ancestor walk reaches the same refusal through `enrol_drag`, so a press
/// that lands on an interactive child never gave the dual-role ancestor's drag
/// a say at all. Past the hold it must.
#[test]
fn an_ancestor_dual_role_nodes_own_drag_runs_after_a_hold() {
    let mut fx = dual_role_ancestor();
    let p = fx.touch_down();
    fx.tree.advance_time(LONG_PRESS);

    fx.touch_to(p, 20.0);
    assert_eq!(
        fx.log.drag_started.get(),
        1,
        "the ancestor's own drag competes once the hold arms it — before the \
         fix `enrol_drag` was refused and its recognizer was never fed"
    );
    assert_eq!(fx.tree.sequence_winner(p), Some(fx.view));
    assert_eq!(
        fx.view_member(p),
        vec![(MemberRole::Gesture, MemberState::Won)]
    );
    assert_eq!(fx.log.scrolls.get(), 0);

    fx.touch_up(p, 20.0);
    fx.tree.assert_no_leaked_pointer_state();
}

/// …and without the hold, the same press pans: the ancestor's drag stays out
/// of the way exactly as the captor's does.
#[test]
fn an_ancestor_dual_role_node_still_pans_without_a_hold() {
    let mut fx = dual_role_ancestor();
    let p = fx.touch_down();

    fx.touch_to(p, 20.0);
    assert_eq!(fx.log.drag_started.get(), 0);
    assert_eq!(fx.tree.sequence_winner(p), None);

    fx.touch_to(p, 40.0);
    assert_eq!(fx.tree.sequence_winner(p), Some(fx.view));
    assert!(fx.log.scrolls.get() > 0);
    assert_eq!(fx.log.drag_started.get(), 0);

    fx.touch_up(p, 40.0);
    fx.tree.assert_no_leaked_pointer_state();
}

// ---------------------------------------------------------------------------
// One hold means one thing
// ---------------------------------------------------------------------------

/// `has_deferred_grab` is what keeps a hold from meaning two things. The
/// self-drag's deferral is a deferred grab too, so the long press on the same
/// node must not fire — otherwise the hold that arms the drag *also* opens a
/// context menu.
#[test]
fn the_hold_that_arms_the_self_drag_is_not_also_a_long_press() {
    let mut fx = dual_role_captor_with(None, true);
    let p = fx.touch_down();
    fx.tree.advance_time(LONG_PRESS + Duration::from_millis(50));

    assert_eq!(
        fx.log.long_presses.get(),
        0,
        "the hold is spent arming the grab; it cannot also be a long press"
    );

    fx.touch_up(p, 0.0);
    fx.tree.assert_no_leaked_pointer_state();
}

/// **A defect that predates this mechanism, pinned so it is not rediscovered.**
///
/// `long_press_is_a_grab` suppresses the *dispatch* of a long press whose hold
/// is already spoken for as a grab, but the tick that produced it has already
/// run: `GestureArena::arbitrate` resets every peer recognizer when one wins,
/// and `DragRecognizer` is such a peer. So on a node that declares
/// `on_long_press` **and** a drag deferred to the long-press deadline, the hold
/// arms a grab and then destroys it — the hold ends up meaning *neither*.
///
/// It is not this mechanism's doing and not this mechanism's to fix. The same
/// press on the **shipped** deferred-ancestor shape (a `Gesture` member enrolled
/// through `enrol_drag`, no dual-role node anywhere) behaves identically:
///
/// ```text
/// container: on_drag + on_long_press,  inside a scroller with a vertical claim
/// press, advance 500 ms, move 20 dp → drags = 0, long presses = 0, winner None
/// ```
///
/// The fix belongs in the arena — the long press must *fail* rather than
/// recognize-then-be-dropped, so its peers are never reset — which is
/// `gesture/arena.rs` / `gesture/long_press.rs`, not the arbitration.
///
/// **When it is fixed, this test's assertions flip**: `drag_started` becomes 1
/// and the node wins.
#[test]
fn a_long_press_recognizer_on_the_same_node_disarms_the_grab() {
    let mut fx = dual_role_captor_with(None, true);
    let p = fx.touch_down();
    fx.tree.advance_time(LONG_PRESS);
    fx.touch_to(p, 20.0);

    assert_eq!(
        fx.log.long_presses.get(),
        0,
        "the long press is suppressed, as `long_press_is_a_grab` intends"
    );
    assert_eq!(
        fx.log.drag_started.get(),
        0,
        "…but the grab it was arming is gone too, because the arena reset the \
         DragRecognizer when the long press won it. Pre-existing; see this \
         test's doc comment"
    );

    fx.touch_up(p, 20.0);
    fx.tree.assert_no_leaked_pointer_state();
}

/// The control for the rule above: a claimant that carries **no** drag of its
/// own has no deferred grab, so its long press is untouched.
#[test]
fn a_claimant_without_a_drag_still_long_presses() {
    let mut fx = tapping_claimant();
    let p = fx.touch_down();
    fx.tree.advance_time(LONG_PRESS + Duration::from_millis(50));

    assert_eq!(
        fx.log.long_presses.get(),
        1,
        "nothing on this sequence is deferred, so the hold is just a hold"
    );

    fx.touch_up(p, 0.0);
    fx.tree.assert_no_leaked_pointer_state();
}

// ---------------------------------------------------------------------------
// The companion defect: a won pan still fired the claimant's own tap
// ---------------------------------------------------------------------------

/// A pan claimant with a tap handler and **no** drag — a plain scroll surface.
fn tapping_claimant() -> Fixture {
    let log = Log::default();
    let mut tree = WidgetTree::new();
    tree.set_density(TargetDensity::Compact);
    let leaf = tree.add(Leaf::new());

    let scrolls = log.scrolls.clone();
    let taps = log.taps.clone();
    let long_presses = log.long_presses.clone();
    let cancels = log.cancels.clone();

    let view = tree.add(
        Stack::new()
            .add_child(leaf)
            .pan_claim(PanClaim::vertical())
            .on_scroll(move |_e, _c| {
                scrolls.set(scrolls.get() + 1);
                EventResponse::Handled
            })
            .on_tap(move |_e, _c| taps.set(taps.get() + 1))
            .on_long_press(move |_e, _c| long_presses.set(long_presses.get() + 1))
            .on_pointer_cancel(move |_p, reason, _c| cancels.borrow_mut().push(reason)),
    );

    tree.layout(SizeProposal::exact(400.0, 400.0));
    Fixture {
        tree,
        view,
        log,
        press: Point::new(200.0, 200.0),
    }
}

/// A claimant that fills the viewport is never revoked by the slide-off sweep —
/// a coarse pointer's tap boundary is the node's own rect, and a finger panning
/// a full-window surface never leaves it. So a pan that won still fired the
/// claimant's own `on_tap` on the release. It must not.
#[test]
fn a_won_pan_revokes_the_claimants_own_tap_family() {
    let mut fx = tapping_claimant();
    let p = fx.touch_down();
    fx.touch_to(p, 40.0);
    assert_eq!(
        fx.tree.sequence_winner(p),
        Some(fx.view),
        "the pan won at pan_slop"
    );
    fx.touch_up(p, 40.0);

    assert_eq!(
        fx.log.taps.get(),
        0,
        "a pan that won IS the press: its claimant's tap must not fire too"
    );
    assert!(fx.log.scrolls.get() > 0);
    fx.tree.assert_no_leaked_pointer_state();
}

/// The revocation is the `cancel_taps` grain, not a whole-arena cancel: the
/// winner is not sent a `PointerCancel` (it did not lose anything), and the
/// next press on the same node taps normally.
#[test]
fn the_tap_revocation_is_tap_family_only_and_leaves_the_node_usable() {
    let mut fx = tapping_claimant();
    let p = fx.touch_down();
    fx.touch_to(p, 40.0);
    fx.touch_up(p, 40.0);
    assert!(
        fx.log.cancels.borrow().is_empty(),
        "the winner is not cancelled: {:?}",
        fx.log.cancels.borrow()
    );

    // A second, stationary press still taps.
    let p2 = fx.touch_down();
    fx.touch_up(p2, 0.0);
    assert_eq!(
        fx.log.taps.get(),
        1,
        "the node still taps on the next press"
    );
    fx.tree.assert_no_leaked_pointer_state();
}

/// The same revocation reaches a *dual-role* claimant, where the tap used to be
/// masked by the drag that wrongly won the press.
#[test]
fn a_won_pan_revokes_a_dual_role_nodes_tap_too() {
    let mut fx = dual_role_captor(None);
    let p = fx.touch_down();
    fx.touch_to(p, 40.0);
    fx.touch_up(p, 40.0);

    assert_eq!(fx.log.taps.get(), 0, "the pan owns the press");
    assert_eq!(fx.log.drag_started.get(), 0);
    assert!(fx.log.scrolls.get() > 0);
    fx.tree.assert_no_leaked_pointer_state();
}

// ---------------------------------------------------------------------------
// A won pan owns the rest of the press
// ---------------------------------------------------------------------------

/// The pan half winning has to take the self-drag out **for good**, not merely
/// outrank it: a finger that scrolls, rests half a second mid-scroll and then
/// carries on would otherwise cross its own deferral's deadline and start the
/// node's drag under a moving pan — a marquee drawn while the camera is
/// panning. The withdrawal is what stops it, and the `Won` arm of the arena
/// gate is what lets the withdrawal bite.
#[test]
fn a_deferral_that_ripens_mid_pan_does_not_start_a_grab() {
    let mut fx = dual_role_captor(None);
    let p = fx.touch_down();
    fx.touch_to(p, 40.0);
    assert_eq!(
        fx.tree.sequence_winner(p),
        Some(fx.view),
        "the pan took the press before the hold could ripen"
    );

    // Rest past the deferral's deadline, then carry on scrolling.
    fx.tree.advance_time(LONG_PRESS + Duration::from_millis(50));
    fx.touch_to(p, 80.0);

    assert_eq!(
        fx.log.drag_started.get(),
        0,
        "the self-drag was withdrawn when the pan won; a ripened deadline must \
         not bring it back"
    );
    assert!(fx.log.scrolls.get() > 0, "and the pan goes on panning");

    fx.touch_up(p, 80.0);
    fx.tree.assert_no_leaked_pointer_state();
}

/// The same for the ancestor door, where the drag would be fed by the
/// arbitration walk rather than by the capture dispatch.
#[test]
fn a_deferral_that_ripens_mid_pan_does_not_start_an_ancestors_grab() {
    let mut fx = dual_role_ancestor();
    let p = fx.touch_down();
    fx.touch_to(p, 40.0);
    assert_eq!(fx.tree.sequence_winner(p), Some(fx.view));

    fx.tree.advance_time(LONG_PRESS + Duration::from_millis(50));
    fx.touch_to(p, 80.0);

    assert_eq!(fx.log.drag_started.get(), 0);
    assert!(fx.log.scrolls.get() > 0);

    fx.touch_up(p, 80.0);
    fx.tree.assert_no_leaked_pointer_state();
}

/// The second way a deferred self-drag goes out: the press leaves the tap
/// boundary, which for a coarse pointer is the node's own rect. That travel is
/// a pan, not a considered grab — the same reading `rejects_on_tap_slop` gives
/// it for a member deferred whole. The member itself is a pan claimant and goes
/// on competing; only its drag half withdraws.
///
/// Needs a press near an edge, because the travel has to leave the node's
/// bounds while staying **under** `pan_slop` — on a viewport-filling surface it
/// never can, which is why that case is governed by the pan win instead. Same
/// shape as the matrix's `scene marquee in a scroller, press at the edge` row.
#[test]
fn a_deferred_self_drag_withdraws_when_the_press_leaves_the_node() {
    let mut fx = dual_role_captor(None);
    // 10 dp above the bottom edge of the 400 × 400 fixture.
    fx.press = Point::new(200.0, 390.0);
    let p = fx.touch_down();

    // 20 dp puts the finger outside the node and is still under pan_slop (36).
    fx.touch_to(p, 20.0);
    assert_eq!(fx.tree.sequence_winner(p), None, "no pan yet at 20 dp");

    // Past the deadline the deferral would have ripened at, then 30 dp total —
    // over drag_slop (18), still under pan_slop.
    fx.tree.advance_time(LONG_PRESS + Duration::from_millis(50));
    fx.touch_to(p, 30.0);

    assert_eq!(
        fx.log.drag_started.get(),
        0,
        "the self-drag withdrew when the press left the node; its deadline \
         ripening afterwards must not revive it"
    );
    assert_eq!(fx.tree.sequence_winner(p), None, "and nothing has won");

    fx.touch_up(p, 30.0);
    fx.tree.assert_no_leaked_pointer_state();
}

// ---------------------------------------------------------------------------
// The mouse is unreachable by construction — for the *implicit* door
// ---------------------------------------------------------------------------
//
// Everything in this file is the deferral: a live `Pan` member whose grab was
// put off to the long-press deadline. A mouse enrols no pan member, so it never
// enters that arm, and the two tests below are the proof rather than the claim.
//
// `long_press_is_a_grab` has a **second** door, the explicit
// `LongPressRole::DragHandle` ancestor walk, and that one is not closed to a
// mouse by construction — it is closed by a gate, because a role is a
// declaration and a declaration reaches whatever is asked of it. Until the gate
// existed, declaring `DragHandle` deleted every `on_long_press` in the subtree
// under the mouse, which is what `.reorderable(true)` did to data-view rows.
// Pinned next to that mechanism, in `widget_tree::touch_route`'s tests
// (`a_mouse_hold_under_a_drag_handle_role_still_fires_the_nodes_own_long_press`
// and its finger twin), not here.

/// A mouse enrols no pan member — `GestureProfile::MOUSE` has no `pan_slop` and
/// `PanClaim::devices` defaults to direct pointers — so the dual-role arm is
/// never entered and the drag latches at 5 dp exactly as it always did.
#[test]
fn a_mouse_on_a_dual_role_node_is_untouched() {
    let mut fx = dual_role_captor(None);
    let press = fx.press;
    fx.tree.pointer_down_button(press, PointerButton::Primary);

    assert_eq!(
        fx.tree.sequence_members(PointerId::MOUSE),
        vec![(fx.view, MemberRole::Gesture, MemberState::Possible)],
        "a mouse enrols the node's DRAG, not a pan: the new arm is unreachable"
    );

    fx.tree.pointer_move(Point::new(press.x, press.y + 4.0));
    assert_eq!(
        fx.log.drag_started.get(),
        0,
        "4 dp is under the mouse latch"
    );
    fx.tree.pointer_move(Point::new(press.x, press.y + 6.0));
    assert_eq!(fx.log.drag_started.get(), 1, "and 6 dp is over it");
    assert_eq!(fx.tree.sequence_winner(PointerId::MOUSE), Some(fx.view));
    assert_eq!(fx.log.scrolls.get(), 0);

    fx.tree
        .pointer_up_button(Point::new(press.x, press.y + 6.0), PointerButton::Primary);
    fx.tree.assert_no_leaked_pointer_state();
}

/// The same for the ancestor door: a mouse press on the interactive child
/// enrols the ancestor as an ordinary `Gesture` member, which is what it has
/// always been.
#[test]
fn a_mouse_on_a_dual_role_ancestor_is_untouched() {
    let mut fx = dual_role_ancestor();
    let press = fx.press;
    fx.tree.pointer_down_button(press, PointerButton::Primary);

    assert_eq!(
        fx.view_member(PointerId::MOUSE),
        vec![(MemberRole::Gesture, MemberState::Possible)]
    );

    fx.tree.pointer_move(Point::new(press.x, press.y + 4.0));
    assert_eq!(fx.log.drag_started.get(), 0);
    fx.tree.pointer_move(Point::new(press.x, press.y + 6.0));
    assert_eq!(fx.log.drag_started.get(), 1);
    assert_eq!(fx.log.scrolls.get(), 0);

    fx.tree
        .pointer_up_button(Point::new(press.x, press.y + 6.0), PointerButton::Primary);
    fx.tree.assert_no_leaked_pointer_state();
}

// ---------------------------------------------------------------------------
// Per-press activation
// ---------------------------------------------------------------------------

/// `.drag_activation(..)` is a build-time node property, and one `on_drag` can
/// mean several things — a marquee, an item grab, a magnet port — whose answer
/// depends on what the press landed on. `EventContext::set_drag_activation`
/// answers per press.
fn per_press_activation(answer: Rc<Cell<Option<DragActivation>>>) -> Fixture {
    let log = Log::default();
    let mut tree = WidgetTree::new();
    tree.set_density(TargetDensity::Compact);
    let leaf = tree.add(Leaf::new());

    let drag_started = log.drag_started.clone();
    let scrolls = log.scrolls.clone();

    let view = tree.add(
        Stack::new()
            .add_child(leaf)
            .pan_claim(PanClaim::both())
            .on_pointer_event(move |event, ctx| {
                if matches!(event, WidgetEvent::PointerDown { .. })
                    && let Some(a) = answer.get()
                {
                    ctx.set_drag_activation(a);
                }
                EventResponse::Ignored
            })
            .on_drag(move |phase, _c| {
                if matches!(phase, DragPhase::Started { .. }) {
                    drag_started.set(drag_started.get() + 1);
                }
            })
            .on_scroll(move |_e, _c| {
                scrolls.set(scrolls.get() + 1);
                EventResponse::Handled
            }),
    );

    tree.layout(SizeProposal::exact(400.0, 400.0));
    Fixture {
        tree,
        view,
        log,
        press: Point::new(200.0, 200.0),
    }
}

/// The press handler says "this one is a grab", and the drag latches without a
/// hold even though the node's declared activation is `Auto`.
#[test]
fn a_press_handler_can_arm_the_self_drag_immediately() {
    let answer = Rc::new(Cell::new(Some(DragActivation::Immediate)));
    let mut fx = per_press_activation(answer);
    let p = fx.touch_down();
    fx.touch_to(p, 20.0);

    assert_eq!(fx.log.drag_started.get(), 1);
    assert_eq!(fx.tree.sequence_winner(p), Some(fx.view));
    assert_eq!(fx.log.scrolls.get(), 0);

    fx.touch_up(p, 20.0);
    fx.tree.assert_no_leaked_pointer_state();
}

/// The override is scoped to **one press**. It is stashed on the
/// `PointerSequence`, not written back onto the node, so a press that answers
/// `Immediate` cannot leave the node armed for the next one — which matters
/// because `on_pointer_event` previews on every press inside the subtree,
/// including ones this node does not own.
#[test]
fn a_per_press_activation_does_not_outlive_its_press() {
    let answer = Rc::new(Cell::new(Some(DragActivation::Immediate)));
    let mut fx = per_press_activation(answer.clone());

    let p = fx.touch_down();
    fx.touch_to(p, 20.0);
    fx.touch_up(p, 20.0);
    assert_eq!(fx.log.drag_started.get(), 1, "the first press was a grab");

    // The second press answers nothing: the node's own `Auto` governs again.
    answer.set(None);
    let p2 = fx.touch_down();
    fx.touch_to(p2, 20.0);
    assert_eq!(
        fx.log.drag_started.get(),
        1,
        "the previous press's override must not survive it"
    );
    fx.touch_to(p2, 40.0);
    assert!(fx.log.scrolls.get() > 0, "so this press pans");

    fx.touch_up(p2, 40.0);
    fx.tree.assert_no_leaked_pointer_state();
}

/// An explicit `Auto` from the press handler resolves like any other `Auto`:
/// with an eligible pan competitor it defers.
#[test]
fn a_per_press_auto_still_defers_to_the_pan() {
    let answer = Rc::new(Cell::new(Some(DragActivation::Auto)));
    let mut fx = per_press_activation(answer);
    let p = fx.touch_down();
    fx.touch_to(p, 20.0);
    assert_eq!(fx.log.drag_started.get(), 0);
    fx.touch_to(p, 40.0);
    assert!(fx.log.scrolls.get() > 0);

    fx.touch_up(p, 40.0);
    fx.tree.assert_no_leaked_pointer_state();
}

// ---------------------------------------------------------------------------
// The release-before-dispatch ordering this design leans on
// ---------------------------------------------------------------------------

/// Blocking a deferred self-drag silences the node's **whole** gesture arena,
/// taps included. That a quick tap still fires depends on `end_sequence`
/// nulling the sequence *before* the release is dispatched to the captor
/// (`pointer_state.rs`'s `end_sequence`, then `pointer_router.rs`'s `PointerUp`
/// arm). Swap those two lines and this test reddens.
#[test]
fn a_tap_survives_the_deferral_because_the_sequence_ends_first() {
    let mut fx = dual_role_captor(None);
    let p = fx.touch_down();
    // Still inside the deferral: the arena is blocked for this whole press.
    fx.tree.advance_time(Duration::from_millis(100));
    fx.touch_up(p, 0.0);

    assert_eq!(
        fx.log.taps.get(),
        1,
        "the release is dispatched after the sequence is gone, so the arena is \
         no longer blocked and the tap completes"
    );
    fx.tree.assert_no_leaked_pointer_state();
}

// ---------------------------------------------------------------------------
// …and the hold it spends is spent on ONE node
// ---------------------------------------------------------------------------

/// How many times a node's context-menu factory was asked, and the fixture it
/// lives in.
struct MenuFixture {
    fx: Fixture,
    /// The node the press is aimed at — the child for the descendant probes,
    /// the dual-role node itself for the own-node one.
    target: WidgetId,
    menus: Rc<Cell<usize>>,
    child_long_presses: Rc<Cell<usize>>,
}

/// A dual-role `view` — `pan_claim(both)` + `on_drag` — over an interactive
/// `child`. `child_menu` puts a context-menu factory on the child, `view_menu`
/// on the view, and `child_long_press` an `on_long_press` on the child.
///
/// This is `SceneView` with selection or magnetism on, holding a heavyweight
/// (`add_widget_item`) widget.
fn dual_role_with_menus(child_menu: bool, view_menu: bool, child_long_press: bool) -> MenuFixture {
    let log = Log::default();
    let menus = Rc::new(Cell::new(0usize));
    let child_long_presses = Rc::new(Cell::new(0usize));
    let mut tree = WidgetTree::new();
    tree.set_density(TargetDensity::Compact);
    let leaf = tree.add(Leaf::new());

    let mut child = Stack::new().add_child(leaf).on_tap(|_e, _c| {});
    if child_menu {
        let counter = menus.clone();
        child = child.context_menu(move |_pos, _ctx| {
            counter.set(counter.get() + 1);
            Some(Box::new(Leaf::new()) as Box<dyn Widget>)
        });
    }
    if child_long_press {
        let counter = child_long_presses.clone();
        child = child.on_long_press(move |_e, _c| counter.set(counter.get() + 1));
    }
    let child = tree.add(child);

    let drag_started = log.drag_started.clone();
    let scrolls = log.scrolls.clone();
    let cancels = log.cancels.clone();
    let mut view = Stack::new()
        .add_child(child)
        .pan_claim(PanClaim::both())
        .on_drag(move |phase, _c| {
            if matches!(phase, DragPhase::Started { .. }) {
                drag_started.set(drag_started.get() + 1);
            }
        })
        .on_scroll(move |_e, _c| {
            scrolls.set(scrolls.get() + 1);
            EventResponse::Handled
        })
        .on_pointer_cancel(move |_p, reason, _c| cancels.borrow_mut().push(reason));
    if view_menu {
        let counter = menus.clone();
        view = view.context_menu(move |_pos, _ctx| {
            counter.set(counter.get() + 1);
            Some(Box::new(Leaf::new()) as Box<dyn Widget>)
        });
    }
    let view = tree.add(view);

    tree.layout(SizeProposal::exact(400.0, 400.0));
    let target = if child_menu || child_long_press {
        child
    } else {
        view
    };
    MenuFixture {
        fx: Fixture {
            tree,
            view,
            log,
            press: Point::new(200.0, 200.0),
        },
        target,
        menus,
        child_long_presses,
    }
}

/// A heavyweight widget inside a dual-role container keeps its **own** touch
/// long press.
///
/// The container's marquee is armed by a hold, but that is a statement about
/// the container, not about everything inside it. Reading the deferral
/// sequence-wide took the hold away from every descendant — so with selection
/// or magnetism on, no `add_widget_item` widget in a `SceneView` could long
/// press at all.
#[test]
fn a_child_of_a_dual_role_ancestor_keeps_its_long_press() {
    let mut m = dual_role_with_menus(false, false, true);
    let p = m.fx.touch_down();
    m.fx.tree
        .advance_time(LONG_PRESS + Duration::from_millis(50));

    assert_eq!(
        m.child_long_presses.get(),
        1,
        "the ancestor's deferred marquee must not consume the child's hold"
    );
    assert_eq!(
        m.fx.log.drag_started.get(),
        0,
        "and holding still starts nothing: the marquee needs travel too"
    );

    m.fx.touch_up(p, 0.0);
    m.fx.tree.assert_no_leaked_pointer_state();
}

/// The same for the tree-owned route, which is the one that matters most: a
/// finger has no secondary button, so the hold **is** the context-menu route.
/// Losing it is an accessibility loss, not a trade-off.
#[test]
fn a_child_of_a_dual_role_ancestor_keeps_its_touch_context_menu() {
    let mut m = dual_role_with_menus(true, false, false);
    let at = m.fx.tree.bounds(m.target).center();
    m.fx.press = at;
    let p = m.fx.touch_down();
    assert_eq!(m.menus.get(), 0, "not on the press");

    m.fx.tree
        .advance_time(LONG_PRESS + Duration::from_millis(50));
    assert_eq!(
        m.menus.get(),
        1,
        "the hold opens the child's context menu; the ancestor's deferred \
         marquee is not a claim on it"
    );

    m.fx.touch_up(p, 0.0);
    m.fx.tree.assert_no_leaked_pointer_state();
}

/// The control, and the half of the rule that must **not** relax: the node
/// whose own grab the hold arms does not also get the hold. A press on the
/// dual-role node's own empty space arms its marquee, so its own context menu
/// stays shut.
///
/// This is `SceneView`'s empty-space press: lightweight items are not arena
/// nodes, so the view itself is the target.
///
/// The way out is per press, not per subtree: a handler that knows this press
/// is not a grab answers `EventContext::set_drag_activation(Immediate)`, which
/// spends no hold — see [`a_view_menu_reopens_when_the_press_is_not_a_grab`].
#[test]
fn a_dual_role_nodes_own_hold_is_still_spent_on_its_own_grab() {
    let log = Log::default();
    let menus = Rc::new(Cell::new(0usize));
    let mut tree = WidgetTree::new();
    tree.set_density(TargetDensity::Compact);
    // An inert leaf, so the dual-role node itself takes the press.
    let leaf = tree.add(Leaf::new());

    let counter = menus.clone();
    let drag_started = log.drag_started.clone();
    let view = tree.add(
        Stack::new()
            .add_child(leaf)
            .pan_claim(PanClaim::both())
            .on_drag(move |phase, _c| {
                if matches!(phase, DragPhase::Started { .. }) {
                    drag_started.set(drag_started.get() + 1);
                }
            })
            .context_menu(move |_pos, _ctx| {
                counter.set(counter.get() + 1);
                Some(Box::new(Leaf::new()) as Box<dyn Widget>)
            }),
    );
    tree.layout(SizeProposal::exact(400.0, 400.0));
    let mut fx = Fixture {
        tree,
        view,
        log,
        press: Point::new(200.0, 200.0),
    };

    let p = fx.touch_down();
    fx.tree.advance_time(LONG_PRESS + Duration::from_millis(50));
    assert_eq!(
        menus.get(),
        0,
        "one hold, one meaning: this one is arming the marquee"
    );

    fx.touch_up(p, 0.0);
    fx.tree.assert_no_leaked_pointer_state();
}

/// The span, from the other end. The press lands on an interactive child that
/// owns **no** menu of its own, so the hold would open the dual-role
/// ancestor's — and that ancestor's grab is armed by this very hold. Suppressed
/// again, for the same reason as the press-on-the-node case: a hold is spent by
/// any node between the press and the affordance it would open.
///
/// The contrast with
/// [`a_child_of_a_dual_role_ancestor_keeps_its_touch_context_menu`] is the whole
/// rule in two tests: same tree, same press, same hold — the menu opens when it
/// is the *child's*, and does not when it is the ancestor's.
#[test]
fn an_inherited_menu_is_suppressed_by_its_own_owners_deferred_grab() {
    let mut m = dual_role_with_menus(false, true, false);
    let at = m.fx.tree.bounds(m.target).center();
    m.fx.press = at;
    let p = m.fx.touch_down();
    m.fx.tree
        .advance_time(LONG_PRESS + Duration::from_millis(50));

    assert_eq!(
        m.menus.get(),
        0,
        "the menu the hold would open belongs to the node the hold is arming"
    );

    m.fx.touch_up(p, 0.0);
    m.fx.tree.assert_no_leaked_pointer_state();
}

/// …and the per-press escape really is one. `Immediate` spends no hold, so the
/// node's own context menu is reachable again on the presses a handler says are
/// not grabs.
#[test]
fn a_view_menu_reopens_when_the_press_is_not_a_grab() {
    let log = Log::default();
    let menus = Rc::new(Cell::new(0usize));
    let mut tree = WidgetTree::new();
    tree.set_density(TargetDensity::Compact);
    let leaf = tree.add(Leaf::new());

    let counter = menus.clone();
    let drag_started = log.drag_started.clone();
    let view = tree.add(
        Stack::new()
            .add_child(leaf)
            .pan_claim(PanClaim::both())
            .on_pointer_event(|event, ctx| {
                if matches!(event, WidgetEvent::PointerDown { .. }) {
                    ctx.set_drag_activation(DragActivation::Immediate);
                }
                EventResponse::Ignored
            })
            .on_drag(move |phase, _c| {
                if matches!(phase, DragPhase::Started { .. }) {
                    drag_started.set(drag_started.get() + 1);
                }
            })
            .context_menu(move |_pos, _ctx| {
                counter.set(counter.get() + 1);
                Some(Box::new(Leaf::new()) as Box<dyn Widget>)
            }),
    );
    tree.layout(SizeProposal::exact(400.0, 400.0));
    let mut fx = Fixture {
        tree,
        view,
        log,
        press: Point::new(200.0, 200.0),
    };

    let p = fx.touch_down();
    fx.tree.advance_time(LONG_PRESS + Duration::from_millis(50));
    assert_eq!(
        menus.get(),
        1,
        "an Immediate self-drag defers nothing, so the hold is free for the menu"
    );

    fx.touch_up(p, 0.0);
    fx.tree.assert_no_leaked_pointer_state();
}

// ---------------------------------------------------------------------------
// The deferral is a hold, not a timer
// ---------------------------------------------------------------------------

/// A **slow** pan is still a pan.
///
/// Arming the self-drag on the clock alone made the deferral a timer: a finger
/// that crossed `long_press` mid-travel armed the grab whatever it had
/// travelled, so a deliberate, slow scene pan — the ordinary case when
/// positioning precisely — became a marquee, and once the marquee had the press
/// the pan was dead for the rest of it. `long_press_slop` (18 dp for a finger)
/// is the travel a hold tolerates, and 25 dp is not a hold.
#[test]
fn a_slow_finger_still_pans_a_dual_role_node() {
    let mut fx = dual_role_captor(None);
    let p = fx.touch_down();

    fx.tree.advance_time(Duration::from_millis(200));
    fx.touch_to(p, 10.0);
    fx.tree.advance_time(Duration::from_millis(200));
    fx.touch_to(p, 25.0);
    fx.tree.advance_time(Duration::from_millis(200));
    fx.touch_to(p, 30.0);

    assert_eq!(
        fx.log.drag_started.get(),
        0,
        "the press had already wandered 25 dp when the deadline passed; that is \
         not a hold and must not arm the grab"
    );

    fx.touch_to(p, 60.0);
    assert_eq!(
        fx.tree.sequence_winner(p),
        Some(fx.view),
        "so the pan is still there to be won at pan_slop"
    );
    assert!(fx.log.scrolls.get() > 0, "and it delivers");
    assert_eq!(fx.log.drag_started.get(), 0);

    fx.touch_up(p, 60.0);
    fx.tree.assert_no_leaked_pointer_state();
}

/// The second shape: travel first, then rest out the deadline. A press that
/// has already left the hold's slop does not become a hold by sitting still
/// afterwards — the same latch `LongPressRecognizer` applies to itself.
#[test]
fn a_press_that_wandered_then_rested_is_not_a_hold() {
    let mut fx = dual_role_captor(None);
    let p = fx.touch_down();

    fx.touch_to(p, 25.0);
    fx.tree
        .advance_time(LONG_PRESS + Duration::from_millis(100));
    fx.touch_to(p, 26.0);

    assert_eq!(
        fx.log.drag_started.get(),
        0,
        "25 dp of travel disqualified the hold before its deadline; resting \
         past the deadline does not re-qualify it"
    );
    assert_eq!(fx.tree.sequence_winner(p), None, "and nothing has won");

    fx.touch_up(p, 26.0);
    fx.tree.assert_no_leaked_pointer_state();
}

/// The case no test covered: travel that crosses the deadline while staying
/// **under `pan_slop`**, so neither half can win on travel alone. The grab must
/// not arm — and the pan must still be available when the finger commits.
#[test]
fn sub_pan_slop_travel_across_the_deadline_arms_no_grab() {
    let mut fx = dual_role_captor(None);
    let p = fx.touch_down();

    // 20 dp: over long_press_slop (18), under pan_slop (36).
    fx.tree.advance_time(Duration::from_millis(300));
    fx.touch_to(p, 20.0);
    // Cross the deadline without going any further.
    fx.tree.advance_time(Duration::from_millis(300));
    fx.touch_to(p, 22.0);

    assert_eq!(fx.log.drag_started.get(), 0, "no grab");
    assert_eq!(fx.log.scrolls.get(), 0, "and no pan yet either");
    assert_eq!(fx.tree.sequence_winner(p), None);

    // The pan is still winnable.
    fx.touch_to(p, 40.0);
    assert_eq!(fx.tree.sequence_winner(p), Some(fx.view));
    assert!(fx.log.scrolls.get() > 0);

    fx.touch_up(p, 40.0);
    fx.tree.assert_no_leaked_pointer_state();
}

/// The boundary from the other side: travel **inside** `long_press_slop` does
/// not disqualify the hold. A finger is not a tripod, and a hold that failed on
/// the first reported pixel of wander would arm nothing on real hardware.
#[test]
fn travel_inside_the_hold_slop_still_arms_the_grab() {
    let mut fx = dual_role_captor(None);
    let p = fx.touch_down();

    // 15 dp of wander, under the finger's 18 dp long_press_slop.
    fx.tree.advance_time(Duration::from_millis(200));
    fx.touch_to(p, 15.0);
    fx.tree.advance_time(LONG_PRESS);

    // Now drag: from 15 dp the recognizer needs drag_slop (18) more.
    fx.touch_to(p, 40.0);
    assert_eq!(
        fx.log.drag_started.get(),
        1,
        "the hold was served, so the grab is armed and latches on travel"
    );
    assert_eq!(
        fx.view_member(p),
        vec![(MemberRole::Gesture, MemberState::Won)],
        "and the member report names the half that won"
    );
    assert_eq!(fx.log.scrolls.get(), 0);

    fx.touch_up(p, 40.0);
    fx.tree.assert_no_leaked_pointer_state();
}

/// The ancestor door takes the same rule, so a heavyweight scene item's press
/// does not arm the container's marquee by outlasting the clock either.
#[test]
fn a_slow_finger_still_pans_through_a_dual_role_ancestor() {
    let mut fx = dual_role_ancestor();
    let p = fx.touch_down();

    fx.tree.advance_time(Duration::from_millis(200));
    fx.touch_to(p, 25.0);
    fx.tree.advance_time(Duration::from_millis(400));
    fx.touch_to(p, 30.0);
    assert_eq!(fx.log.drag_started.get(), 0);

    fx.touch_to(p, 60.0);
    assert_eq!(fx.tree.sequence_winner(p), Some(fx.view));
    assert!(fx.log.scrolls.get() > 0);

    fx.touch_up(p, 60.0);
    fx.tree.assert_no_leaked_pointer_state();
}

/// The same scoping fixes a defect that **predates** the dual-role mechanism,
/// and it is worth pinning because it was the wider half of the problem.
///
/// The shipped deferred-ancestor shape — a `container` with `on_drag` inside a
/// `scroller` that claims a pan — is enrolled by `enrol_drag`, which stamps
/// `eligible_at` and so reads as a deferred grab. Sequence-wide, that took the
/// touch long press and the touch context menu away from **every control
/// beneath it**: a reorderable list, a draggable card, a dock header. Per node,
/// the container's hold is the container's business and the child keeps its own.
#[test]
fn a_deferred_drag_ancestor_does_not_eat_the_hold_of_a_child() {
    let long_presses = Rc::new(Cell::new(0usize));
    let menus = Rc::new(Cell::new(0usize));
    let scrolls = Rc::new(Cell::new(0usize));
    let mut tree = WidgetTree::new();
    tree.set_density(TargetDensity::Compact);
    let leaf = tree.add(Leaf::new());

    let lp = long_presses.clone();
    let mn = menus.clone();
    let child = tree.add(
        Stack::new()
            .add_child(leaf)
            .on_tap(|_e, _c| {})
            .on_long_press(move |_e, _c| lp.set(lp.get() + 1))
            .context_menu(move |_pos, _ctx| {
                mn.set(mn.get() + 1);
                Some(Box::new(Leaf::new()) as Box<dyn Widget>)
            }),
    );
    // A plain drag-capable container — no claim of its own.
    let container = tree.add(Stack::new().add_child(child).on_drag(|_p, _c| {}));
    let sc = scrolls.clone();
    let _scroller = tree.add(
        Stack::new()
            .add_child(container)
            .pan_claim(PanClaim::vertical())
            .on_scroll(move |_e, _c| {
                sc.set(sc.get() + 1);
                EventResponse::Handled
            }),
    );
    tree.layout(SizeProposal::exact(400.0, 400.0));

    let p = tree.new_contact();
    tree.touch_down(p, Point::new(200.0, 200.0));
    tree.advance_time(LONG_PRESS + Duration::from_millis(50));

    assert_eq!(
        long_presses.get(),
        1,
        "the container's deferred reorder is not a claim on the child's hold"
    );
    // Rule 1 keeps the tree route off a path that carries its own
    // `on_long_press`, so the menu is the widget's to open — which is exactly
    // what the child just did.
    assert_eq!(menus.get(), 0, "and the widget's own handler owns the hold");

    tree.touch_up(p, Point::new(200.0, 200.0));
    tree.assert_no_leaked_pointer_state();
}
