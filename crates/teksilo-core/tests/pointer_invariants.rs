// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The pointer-arbitration invariants: what must hold for **every** press, not
//! for the named shapes `arbitration_matrix.rs` enumerates.
//!
//! The matrix pins outcomes. This pins the rules those outcomes may never
//! break, over randomly assembled competitor stacks driven by randomly
//! generated gesture scripts:
//!
//! 1. a sequence has at most one winner, and the winner never changes;
//! 2. no node is told `PeerClaimed` twice for one press;
//! 3. every node told `PeerClaimed` was a competitor, and is `Rejected` after;
//! 4. no sequence outlives its pointer;
//! 5. [`WidgetTree::assert_no_leaked_pointer_state`] holds after every script.
//!
//! **The asymmetry in rule 2 and 3 is deliberate and is the point.** "Exactly
//! one cancel per loser" is *false* as stated: only members knocked out by
//! `PointerSequence::decide` are told. A competitor rejected **at enrolment**,
//! because the sequence was already decided (an explicit capture by an indirect
//! pointer), and a competitor that **withdrew itself** — a deferred
//! `DragActivation::AfterLongPress` member whose press left the tap boundary,
//! or one whose node went away and was swept by `revalidate_sequence` — never
//! becomes a live loser and is never cancelled. So the invariant is *at most*
//! one, plus "a cancel implies membership and rejection", which together are
//! what the framework actually guarantees. Writing it the other way round would
//! be a property that fails on correct code.
//!
//! # Placement
//!
//! `tests/`, because every item the suite touches is `pub`: `WidgetTree`, the
//! A21 touch/pen helpers, `sequence_winner`/`sequence_members`, `MemberRole`,
//! `MemberState`, `CancelReason`, `PanClaim`, `TouchAction`. The two widget
//! shapes come from `tests/common/mod.rs` for the reason its module doc gives.
//! See `docs/property-testing.md` for the decision procedure.
//!
//! # Not re-litigated here
//!
//! The four cross-hit-path regressions (`ancestor_drag_starts_through_descendant_tap_capture`,
//! `ancestor_drag_starts_through_deeply_nested_tap_capture`,
//! `click_on_tap_child_then_hover_does_not_start_ancestor_drag`,
//! `gesture_dead_zone_blocks_ancestor_drag_arming`) were re-expressed against
//! `sequence_members` when the sequence landed and live in
//! `widget_tree/gesture_dispatch_impl.rs` and `widget_tree/drag_drop_impl.rs`.
//! `a_peer_claim_revokes_each_loser_exactly_once`
//! (`widget_tree/pointer_cancel.rs`) is the hand-built instance of rule 2.
//!
//! # Deeper run
//!
//! ```text
//! PROPTEST_CASES=4096 cargo test -p teksilo-core --test pointer_invariants
//! ```

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{Leaf, Stack};
use proptest::prelude::*;
use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::event::{EventResponse, PointerButton, WidgetEvent};
use teksilo_core::gesture::MemberState;
use teksilo_core::pointer::touch_action::{PanClaim, TouchAction};
use teksilo_core::pointer::{CancelReason, PointerId};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_tokens::{DragActivation, PenKind, PointerKind};

// ---------------------------------------------------------------------------
// The model
// ---------------------------------------------------------------------------

/// What one node in a generated competitor stack declares.
///
/// Every field is something the arbitration reads: the handler kit decides
/// which recognizers the node installs and therefore which `MemberRole` it can
/// hold, and the three declarations decide whether it is deferred, filtered out
/// or a boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NodeSpec {
    tap: bool,
    drag: bool,
    /// Captures the pointer on the press and answers `Handled` — the splitter /
    /// dock-handle / column-grip shape.
    captures: bool,
    /// Answers `Handled` from the preview pass without capturing.
    previews: bool,
    claims_pan: bool,
    action: TouchAction,
    activation: DragActivation,
    dead_zone: bool,
}

/// One generated gesture: who presses, where it goes, and how it ends.
#[derive(Clone, Debug)]
struct Script {
    kind: PointerKind,
    /// Offsets from the press point, in order.
    steps: Vec<(f32, f32)>,
    /// End with a platform cancel rather than a lift.
    cancelled: bool,
}

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

/// Cost: the widest generated case is 5 nodes × 14 samples = 70 dispatches
/// through the real router, each of which walks a path of at most 5 nodes and
/// allocates nothing beyond the per-sample `Vec`s the router already builds.
/// Two orders of magnitude cheaper per case than the tree-model suites in
/// `teksilo-data`, so the default 256 cases is left alone. Nothing here can
/// allocate proportionally to a *generated number*: the only unbounded-looking
/// quantity is the offset, which is a coordinate and is clamped to ±120 dp.
fn arb_node() -> impl Strategy<Value = NodeSpec> {
    (
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        arb_action(),
        arb_activation(),
        // Dead zones are rare in real trees and, as a boundary, they truncate
        // the interesting part of the fixture — so they are generated at about
        // one node in eight rather than one in two.
        prop::sample::select(vec![false, false, false, false, false, false, false, true]),
    )
        .prop_map(
            |(tap, drag, captures, previews, claims_pan, action, activation, dead_zone)| NodeSpec {
                tap,
                drag,
                captures,
                previews,
                claims_pan,
                action,
                activation,
                dead_zone,
            },
        )
}

fn arb_action() -> impl Strategy<Value = TouchAction> {
    prop::sample::select(vec![
        // AUTO weighted heavily: it is what a node that declares nothing has,
        // and a stack of exotic declarations explores the filters rather than
        // the arbitration.
        TouchAction::AUTO,
        TouchAction::AUTO,
        TouchAction::AUTO,
        TouchAction::NONE,
        TouchAction::PAN,
        TouchAction::PAN_X,
        TouchAction::PAN_Y,
        TouchAction::MANIPULATION,
    ])
}

fn arb_activation() -> impl Strategy<Value = DragActivation> {
    prop::sample::select(vec![
        DragActivation::Auto,
        DragActivation::Auto,
        DragActivation::Immediate,
        DragActivation::AfterLongPress,
    ])
}

fn arb_kind() -> impl Strategy<Value = PointerKind> {
    prop::sample::select(vec![
        PointerKind::Mouse,
        PointerKind::Touch,
        PointerKind::Pen(PenKind::Pen),
    ])
}

/// A stack of 1–5 nodes, root first. The last is the leaf the press lands on.
fn arb_stack() -> impl Strategy<Value = Vec<NodeSpec>> {
    prop::collection::vec(arb_node(), 1..=5)
}

/// A movement script: up to 14 offsets in ±120 dp, and whether it ends in a
/// platform cancel.
///
/// The range spans every threshold in the token table (`slop_precise` 2,
/// `drag_slop` 5/18, `tap_slop` 5/18, `pan_slop` 8/36) with room either side,
/// and the fixture is 400 dp square, so an offset can also leave a member's
/// bounds — which is what makes the coarse `TapBoundary::Bounds` rule
/// reachable.
fn arb_script() -> impl Strategy<Value = Script> {
    (
        arb_kind(),
        prop::collection::vec((-120.0f32..120.0, -120.0f32..120.0), 0..=14),
        any::<bool>(),
    )
        .prop_map(|(kind, steps, cancelled)| Script {
            kind,
            steps,
            cancelled,
        })
}

fn arb_stack_and_script() -> impl Strategy<Value = (Vec<NodeSpec>, Script)> {
    (arb_stack(), arb_script())
}

// ---------------------------------------------------------------------------
// Building and driving
// ---------------------------------------------------------------------------

/// What one run observed.
struct Run {
    tree: WidgetTree,
    pointer: PointerId,
    ids: Vec<WidgetId>,
    /// Every `(node, reason)` any node was told, in delivery order.
    cancels: Vec<(WidgetId, CancelReason)>,
    /// The winner after each sample, press first.
    winners: Vec<Option<WidgetId>>,
    /// Every member state seen for every node, so a state that went backwards
    /// is visible after the fact.
    states: Vec<Vec<(WidgetId, MemberState)>>,
}

/// Every `(node, reason)` the run's nodes were told, in delivery order.
type CancelLog = Rc<RefCell<Vec<(WidgetId, CancelReason)>>>;

fn build(stack: &[NodeSpec]) -> (WidgetTree, Vec<WidgetId>, CancelLog) {
    let log: CancelLog = Rc::new(RefCell::new(Vec::new()));
    let mut tree = WidgetTree::new();
    // Built leaf-first, because a `Stack` takes its children by id.
    let mut ids: Vec<WidgetId> = Vec::new();
    for (depth, spec) in stack.iter().enumerate().rev() {
        // The one `Cell` each node writes its own id into, so its cancel
        // handler can name itself. Installed before the id exists, filled the
        // moment it does.
        let slot = Rc::new(std::cell::Cell::new(None::<WidgetId>));
        let (log_for_node, slot_for_node) = (log.clone(), slot.clone());
        let base = Stack::new();
        let base = match ids.last() {
            Some(&child) => base.child(child),
            None => base,
        };
        // A leaf that declares nothing at all would still be a legal node, but
        // the `Stack` shape is uniform and its `place_children` gives every
        // node the whole bounds — which is what puts the press on all of them.
        let _ = depth;
        let mut w = base
            .on_pointer_cancel(move |_p, reason, _c| {
                if let Some(id) = slot_for_node.get() {
                    log_for_node.borrow_mut().push((id, reason));
                }
            })
            .touch_action(spec.action)
            .drag_activation(spec.activation);
        if spec.tap {
            w = w.on_tap(|_e, _c| {});
        }
        if spec.drag {
            w = w.on_drag(|_p, _c| {});
        }
        if spec.captures {
            w = w.on_pointer_event(|event, ctx| {
                if matches!(event, WidgetEvent::PointerDown { .. }) {
                    ctx.capture_pointer();
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            });
        } else if spec.previews {
            w = w.on_pointer_event(|event, _ctx| {
                if matches!(event, WidgetEvent::PointerDown { .. }) {
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            });
        }
        if spec.claims_pan {
            w = w.pan_claim(PanClaim::both());
        }
        if spec.dead_zone {
            w = w.gesture_dead_zone(true);
        }
        let id = tree.add(w);
        slot.set(Some(id));
        ids.push(id);
    }
    // A real leaf under the innermost stack, so the press always lands on
    // something that competes for nothing.
    let leaf = tree.add(Leaf::new());
    let _ = leaf;
    ids.reverse();
    tree.layout(SizeProposal::exact(400.0, 400.0));
    (tree, ids, log)
}

fn run_script(stack: &[NodeSpec], script: &Script) -> Run {
    let (mut tree, ids, log) = build(stack);
    let at = Point::new(200.0, 200.0);

    let pointer = match script.kind {
        PointerKind::Touch => {
            let id = tree.new_contact();
            tree.touch_down(id, at);
            id
        }
        PointerKind::Pen(_) => {
            tree.pen_down(at, 0.5, (0.0, 0.0));
            tree.live_pointers()
                .find(|p| matches!(p.kind, PointerKind::Pen(_)))
                .map(|p| p.id)
                .expect("the pen was admitted")
        }
        _ => {
            tree.pointer_down_button(at, PointerButton::Primary);
            PointerId::MOUSE
        }
    };

    let snapshot = |tree: &WidgetTree| -> Vec<(WidgetId, MemberState)> {
        tree.sequence_members(pointer)
            .into_iter()
            .map(|(id, _, state)| (id, state))
            .collect()
    };

    let mut winners = vec![tree.sequence_winner(pointer)];
    let mut states = vec![snapshot(&tree)];
    let mut last = at;
    for &(dx, dy) in &script.steps {
        last = Point::new(at.x + dx, at.y + dy);
        match script.kind {
            PointerKind::Touch => tree.touch_move(pointer, last),
            PointerKind::Pen(_) => tree.pen_move(last, 0.5, (0.0, 0.0)),
            _ => tree.pointer_move(last),
        }
        winners.push(tree.sequence_winner(pointer));
        states.push(snapshot(&tree));
    }

    match (script.kind, script.cancelled) {
        (PointerKind::Touch, true) => tree.touch_cancel(pointer, last),
        (PointerKind::Touch, false) => tree.touch_up(pointer, last),
        // A hovering-capable pointer has no `PointerPhase::Cancel` from a
        // backend that is not also a proximity-leave, so its revocation enters
        // through the funnel itself — the same door `WindowDeactivated` and
        // `ModalOpened` use. A contact goes through `dispatch_pointer`, because
        // that is what a `wl_touch.cancel` does.
        (_, true) => {
            let mut noop = teksilo_core::window::NoopWindowOps;
            tree.cancel_pointer(pointer, CancelReason::Platform, &mut noop);
            // The funnel revokes the press; the pointer itself is still in the
            // table, exactly as a mouse whose drag was taken away still is.
            if !script.kind.hovers() {
                tree.pointer_up_button(last, PointerButton::Primary);
            }
        }
        (PointerKind::Pen(_), false) => tree.pen_up(last, 0.0, (0.0, 0.0)),
        (_, false) => tree.pointer_up_button(last, PointerButton::Primary),
    }

    let cancels = log.borrow().clone();
    Run {
        tree,
        pointer,
        ids,
        cancels,
        winners,
        states,
    }
}

// ── 1. A sequence has at most one winner, and once it has one that winner
//      never changes for the rest of the press ──
proptest! {
    #[test]
    fn a_sequence_never_changes_its_winner((stack, script) in arb_stack_and_script()) {
        let run = run_script(&stack, &script);
        let mut settled: Option<WidgetId> = None;
        for (step, winner) in run.winners.iter().enumerate() {
            match (settled, winner) {
                (None, Some(w)) => settled = Some(*w),
                (Some(had), Some(now)) => prop_assert_eq!(
                    had, *now,
                    "step {}: the sequence changed owner from {:?} to {:?} \
                     (stack={:?}, script={:?}, winners={:?})",
                    step, had, now, stack, script, run.winners
                ),
                (Some(had), None) => prop_assert!(
                    false,
                    "step {}: the sequence un-decided after {:?} had won \
                     (stack={:?}, script={:?}, winners={:?})",
                    step, had, stack, script, run.winners
                ),
                (None, None) => {}
            }
        }
    }
}

// ── 2. At most one `PeerClaimed` per node per press. A loser is knocked out
//      once, by `PointerSequence::decide`, and a decided sequence returns
//      early — so a second one would mean the same competitor was in the
//      running twice ──
proptest! {
    #[test]
    fn no_node_is_peer_claimed_twice((stack, script) in arb_stack_and_script()) {
        let run = run_script(&stack, &script);
        for &id in &run.ids {
            let claims = run
                .cancels
                .iter()
                .filter(|(who, reason)| *who == id && *reason == CancelReason::PeerClaimed)
                .count();
            prop_assert!(
                claims <= 1,
                "{:?} was told PeerClaimed {} times (stack={:?}, script={:?}, cancels={:?})",
                id, claims, stack, script, run.cancels
            );
        }
    }
}

// ── 3. A `PeerClaimed` implies membership: every node told it was a
//      competitor for this press, and is `Rejected` by the end of it ──
proptest! {
    #[test]
    fn a_peer_claim_only_ever_reaches_a_rejected_member(
        (stack, script) in arb_stack_and_script()
    ) {
        let run = run_script(&stack, &script);
        // The last member snapshot taken while the sequence still existed.
        let last = run.states.last().cloned().unwrap_or_default();
        for (who, reason) in &run.cancels {
            if *reason != CancelReason::PeerClaimed {
                continue;
            }
            let state = last.iter().find(|(id, _)| id == who).map(|(_, s)| *s);
            prop_assert_eq!(
                state,
                Some(MemberState::Rejected),
                "{:?} was told PeerClaimed but its final member state was {:?} \
                 (stack={:?}, script={:?}, members={:?})",
                who, state, stack, script, last
            );
        }
    }
}

// ── 4. No sequence outlives its pointer: the terminating Up or Cancel clears
//      the members and the winner alike ──
proptest! {
    #[test]
    fn no_sequence_outlives_its_pointer((stack, script) in arb_stack_and_script()) {
        let run = run_script(&stack, &script);
        prop_assert!(
            run.tree.sequence_members(run.pointer).is_empty(),
            "the sequence outlived the pointer: {:?} (stack={:?}, script={:?})",
            run.tree.sequence_members(run.pointer), stack, script
        );
        prop_assert_eq!(
            run.tree.sequence_winner(run.pointer),
            None,
            "a winner outlived the pointer (stack={:?}, script={:?})",
            stack, script
        );
    }
}

// ── 5. Every script leaves the tree clean: no contact still on the surface,
//      no capture, no sequence, no live arena, no press ──
proptest! {
    #[test]
    fn every_script_leaves_no_pointer_state_behind(
        (stack, script) in arb_stack_and_script()
    ) {
        let run = run_script(&stack, &script);
        run.tree.assert_no_leaked_pointer_state();
    }
}

// ── 6. A member's state only ever moves forward. `Possible` may become
//      `Held`, `Won` or `Rejected`; `Won` and `Rejected` are terminal. A
//      resurrection would let a competitor knocked out by one sample win on
//      the next, which is the failure mode `decide`'s "report only the live
//      ones" rule exists to prevent ──
proptest! {
    #[test]
    fn a_member_state_never_goes_backwards((stack, script) in arb_stack_and_script()) {
        fn rank(state: MemberState) -> u8 {
            match state {
                MemberState::Possible => 0,
                MemberState::Held => 1,
                MemberState::Won | MemberState::Rejected => 2,
                _ => 0,
            }
        }
        let run = run_script(&stack, &script);
        for &id in &run.ids {
            let mut seen: Option<MemberState> = None;
            for (step, snapshot) in run.states.iter().enumerate() {
                let Some((_, state)) = snapshot.iter().find(|(other, _)| *other == id) else {
                    continue;
                };
                if let Some(before) = seen {
                    prop_assert!(
                        rank(*state) >= rank(before),
                        "{:?} went from {:?} back to {:?} at step {} \
                         (stack={:?}, script={:?})",
                        id, before, state, step, stack, script
                    );
                    if rank(before) == 2 {
                        prop_assert_eq!(
                            *state, before,
                            "{:?} left the terminal state {:?} for {:?} at step {} \
                             (stack={:?}, script={:?})",
                            id, before, state, step, stack, script
                        );
                    }
                }
                seen = Some(*state);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The TapStreak contract
// ---------------------------------------------------------------------------

/// The tap streak escalates on the node, and the interval that ends it is on
/// the one virtual clock.
///
/// The escalation half is the integration-level statement of
/// `a_mouse_triple_click_still_escalates_through_the_tree`
/// (`widget_tree/gesture_dispatch_impl.rs`), which this does not otherwise
/// re-litigate. The half that is **not** covered anywhere else is the reset:
/// `TapStreak` ends a streak once `multi_tap_interval` has passed, and until
/// the clock work in this package that interval was measured against the wall
/// clock, so no test could state it without either sleeping or being flaky.
#[test]
fn a_tap_streak_escalates_and_then_expires_on_the_virtual_clock() {
    use std::cell::Cell;

    let singles = Rc::new(Cell::new(0));
    let doubles = Rc::new(Cell::new(0));
    let triples = Rc::new(Cell::new(0));
    let (s, d, t) = (singles.clone(), doubles.clone(), triples.clone());

    let mut tree = WidgetTree::new();
    tree.add(
        Leaf::new()
            .on_tap(move |_e, _c| s.set(s.get() + 1))
            .on_double_tap(move |_e, _c| d.set(d.get() + 1))
            .on_triple_tap(move |_e, _c| t.set(t.get() + 1)),
    );
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let at = Point::new(50.0, 50.0);

    for _ in 0..3 {
        tree.tap_with(PointerKind::Mouse, at);
    }
    assert_eq!(doubles.get(), 1, "click 2 fires DoubleTap");
    assert_eq!(triples.get(), 1, "click 3 fires TripleTap");

    // Past the interval the streak is over, so the next pair must escalate from
    // one again rather than continuing to a fourth.
    let interval = teksilo_tokens::GestureProfile::MOUSE.multi_tap_interval;
    tree.advance_input_time(interval + std::time::Duration::from_millis(1));

    let doubles_before = doubles.get();
    let triples_before = triples.get();
    tree.tap_with(PointerKind::Mouse, at);
    assert_eq!(
        doubles.get(),
        doubles_before,
        "the first click after the interval starts a new streak"
    );
    tree.tap_with(PointerKind::Mouse, at);
    assert_eq!(doubles.get(), doubles_before + 1, "and the second doubles");
    assert_eq!(
        triples.get(),
        triples_before,
        "…without escalating straight to a triple"
    );

    tree.assert_no_leaked_pointer_state();
}
