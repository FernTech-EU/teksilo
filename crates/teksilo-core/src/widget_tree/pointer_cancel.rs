// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The cancel funnel: one queued path by which a pointer interaction is taken
//! away, and the enumerated set of things allowed to take it.
//!
//! # Why a funnel
//!
//! Before this module the framework revoked interactions by *forgetting* them.
//! A window that lost focus dropped every capture in the table, and a parked
//! subtree simply stopped being hit-testable: the widget mid-drag was
//! never told, so it kept its own half of the interaction — a latched
//! selection, a grabbed divider, a highlighted drop target — with no event
//! coming that could ever clear it. `PointerUp` cannot stand in, because "the
//! user finished" and "the system took it away" call for opposite responses:
//! an `Up` on a drag *drops*, and dropping a file because the window lost
//! focus is a data-loss bug.
//!
//! So every revocation now goes through [`WidgetTree::cancel_pointer`], which
//! tears the interaction down in one order and delivers exactly one
//! [`WidgetEvent::PointerCancel`] carrying the [`CancelReason`] that names who
//! took it.
//!
//! # Two granularities, and they are not the same act
//!
//! * [`cancel_pointer`](WidgetTree::cancel_pointer) revokes the **whole
//!   pointer**. The sequence dies, the capture is given back, the table entry
//!   goes (for a contact), and nothing more will be delivered for that
//!   pointer.
//! * [`revoke_sequence_member`](WidgetTree::revoke_sequence_member) revokes
//!   **one competitor** of a sequence that is still alive. Its recognizers are
//!   cancelled and it is told so, and the pointer carries on — its winner
//!   keeps receiving moves and will still get its `Up`. This is what a peer
//!   claim does: exactly one member wins, and every other member is told it
//!   lost. Revoking a member is emphatically not cancelling the pointer, and
//!   confusing the two would make an ancestor's losing drag kill the tap that
//!   beat it.
//!
//! # Queued, always
//!
//! A cancel raised from inside a handler must not unwind the sample that
//! handler is standing on. It therefore rides the same
//! [`pending_dispatch`](WidgetTree::pending_dispatch) queue P07 built for
//! nested dispatch, as a [`QueuedDispatch::Cancel`] entry rather than a second
//! queue of its own: one queue means one order, and the cancel a handler
//! raised lands after the event that provoked it rather than in the middle of
//! it. At depth zero the queue is drained immediately, so a caller outside a
//! dispatch — `set_window_active`, an overlay teardown — sees the cancel take
//! effect before its own call returns.
//!
//! Reference: `docs/touch-and-pen.md` §3.3.

use super::*;
use crate::pointer::{CancelReason, PointerId};

impl WidgetTree {
    // -----------------------------------------------------------------
    // The funnel
    // -----------------------------------------------------------------

    /// Revoke `pointer`'s interaction, for `reason`.
    ///
    /// **Always queued** behind the sample currently being dispatched, and a
    /// **no-op if that interaction has already finished** by the time the
    /// queue drains — a cancel raised from a handler must not fire against a
    /// press the very same sample completed. "Finished" means the pointer is
    /// no longer live, or holds no capture and has no sequence, or its
    /// sequence is already inside its own terminal dispatch; all three say
    /// there is nothing left to revoke.
    ///
    /// The teardown runs in one order, and the order is load-bearing: every
    /// competitor's recognizer state goes first (so nothing can recognize on
    /// the way out), then the arbitration, then the capture, then the drag
    /// session, then the table entry, and the event is delivered last — to a
    /// tree that has already forgotten the interaction, so a handler that
    /// reacts by capturing or dragging starts from a clean slate rather than
    /// racing the teardown.
    ///
    /// Two teardown steps named in the design are absent because their subject
    /// does not exist yet: the framework press signal (P11) and the fling
    /// driver (P13/P21) each clear at the marked point below.
    pub fn cancel_pointer(
        &mut self,
        pointer: PointerId,
        reason: CancelReason,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        self.cancel_pointer_to(pointer, reason, None, ops);
    }

    /// [`cancel_pointer`](Self::cancel_pointer), addressed to a widget the
    /// caller names rather than to whoever the table says holds the pointer.
    ///
    /// For a producer whose own teardown has already given the capture back
    /// before it raises the cancel — the OS-drag escalation hands the pointer
    /// to the platform first — so the funnel would otherwise have nobody left
    /// to tell. The named widget is used only while it is still there; a
    /// recipient destroyed in the meantime falls back to the ordinary chain.
    pub fn cancel_pointer_to(
        &mut self,
        pointer: PointerId,
        reason: CancelReason,
        recipient: Option<WidgetId>,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        self.enqueue_cancel(pointer, reason, recipient);
        if self.dispatch_depth == 0 {
            self.drain_pending_dispatch(ops);
        }
    }

    /// Revoke every live pointer, for `reason`. The window went away under
    /// them, a modal opened over them, the platform took the seat.
    pub fn cancel_all_pointers(
        &mut self,
        reason: CancelReason,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        let live: Vec<PointerId> = self.pointers.iter().map(|e| e.info.id).collect();
        for id in live {
            self.enqueue_cancel(id, reason, None);
        }
        if self.dispatch_depth == 0 {
            self.drain_pending_dispatch(ops);
        }
    }

    /// Revoke every pointer whose interaction is **anchored inside** `root`.
    ///
    /// "Anchored inside" means the pointer's captor is `root` or a descendant
    /// of it: that widget is the one about to stop existing, and the pointer
    /// it holds would otherwise be stranded on it. A pointer merely passing
    /// over the subtree is not anchored in it and is left alone.
    ///
    /// A pointer whose press is no longer revocable is skipped, which is what
    /// makes the named exemption work: tapping a menu item whose own handler
    /// closes its menu must complete the tap, not have it cancelled out from
    /// under itself by the teardown it asked for.
    pub fn cancel_pointers_in_subtree(
        &mut self,
        root: WidgetId,
        reason: CancelReason,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        let anchored: Vec<PointerId> = self
            .pointers
            .iter()
            .filter(|entry| {
                entry
                    .captured_by
                    .is_some_and(|captor| captor == root || self.is_descendant_of(captor, root))
            })
            .map(|entry| entry.info.id)
            .collect();
        let mut queued = false;
        for id in anchored {
            if !self.press_is_revocable(id) {
                crate::trace_input!(
                    Gestures,
                    "{id:?} is inside the parked subtree but its press has already ended: not cancelled"
                );
                continue;
            }
            self.enqueue_cancel(id, reason, None);
            queued = true;
        }
        if queued && self.dispatch_depth == 0 {
            self.drain_pending_dispatch(ops);
        }
    }

    /// Park `root`'s subtree and cancel every pointer it was holding.
    ///
    /// The tree-level door onto [`WidgetArena::set_dormant`](crate::arena::WidgetArena::set_dormant):
    /// parking is invisible to hit-testing and to dispatch, so a widget parked
    /// mid-interaction would keep whatever the press latched and never receive
    /// another event. Every caller that parks a subtree which could plausibly
    /// contain a live pointer goes through here; the audit of the ones that do
    /// not is in `docs/touch-and-pen.md` §3.3.
    pub(crate) fn park_subtree(&mut self, root: WidgetId) {
        let mut noop = crate::window::NoopWindowOps;
        self.park_subtree_with_ops(root, &mut noop);
    }

    /// [`park_subtree`](Self::park_subtree) with the caller's
    /// [`WindowOps`](crate::window::WindowOps).
    ///
    /// The cancel is raised **before** the subtree is parked, so the widget is
    /// still active when it is told to let go — a `PointerCancel` delivered to
    /// a node the dispatcher has just made dormant would be dropped, which is
    /// precisely the silent teardown this replaces.
    pub(crate) fn park_subtree_with_ops(
        &mut self,
        root: WidgetId,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        self.cancel_pointers_in_subtree(root, CancelReason::SubtreeParked, ops);
        let _parked = self.arena.set_dormant(root);
    }

    /// Revoke one **member** of a live sequence, leaving the sequence, the
    /// capture and the pointer itself alone.
    ///
    /// The member's recognizers are cancelled for this contact and the member
    /// widget is told with a [`WidgetEvent::PointerCancel`]. Unlike
    /// [`cancel_pointer`](Self::cancel_pointer) this is delivered **now**
    /// rather than queued: it is raised from the arbitration itself, which
    /// already runs at a point where the sequence is consistent, and a member
    /// that lost must stop recognizing before the same sample reaches it
    /// through the ordinary bubble.
    ///
    /// A member whose node has already been destroyed gets the recognizer
    /// teardown and no event — there is nothing left to deliver to. A merely
    /// *dormant* one still exists and is told directly, without a bubble,
    /// exactly as a dormant node is told about a lost focus.
    pub(super) fn revoke_sequence_member(
        &mut self,
        pointer: PointerId,
        member: WidgetId,
        reason: CancelReason,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        crate::trace_input!(
            Gestures,
            "member {member:?} of {pointer:?} revoked: {reason:?}"
        );
        self.cancel_member_arena(member, pointer);
        if self.arena.get(member).is_none() {
            return;
        }
        let event = self.pointer_cancel_event(pointer, reason);
        self.dispatch_to_widget_direct(member, &event, ops);
    }

    // -----------------------------------------------------------------
    // Queue plumbing
    // -----------------------------------------------------------------

    /// Put a cancel on the shared dispatch queue, unless one for the same
    /// pointer is already waiting there.
    ///
    /// The de-duplication is what keeps "exactly one cancel" true when two
    /// producers fire on the same sample — an overlay dismissal that also
    /// parks the subtree it lived in, say. The first reason wins, because it
    /// is the one that describes what actually happened.
    fn enqueue_cancel(
        &mut self,
        pointer: PointerId,
        reason: CancelReason,
        recipient: Option<WidgetId>,
    ) {
        if self.pending_dispatch.iter().any(|queued| {
            matches!(queued, pointer_router::QueuedDispatch::Cancel { pointer: p, .. } if *p == pointer)
        }) {
            return;
        }
        crate::trace_input!(Samples, "cancel queued for {pointer:?}: {reason:?}");
        self.pending_dispatch
            .push_back(pointer_router::QueuedDispatch::Cancel {
                pointer,
                reason,
                recipient,
            });
    }

    /// Run one queued cancel, at dispatch depth zero.
    pub(super) fn run_one_cancel(
        &mut self,
        pointer: PointerId,
        reason: CancelReason,
        recipient: Option<WidgetId>,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        if !self.press_is_revocable(pointer) {
            crate::trace_input!(
                Samples,
                "cancel for {pointer:?} ({reason:?}) dropped: nothing left to revoke"
            );
            return;
        }
        crate::trace_input!(Samples, "cancelling {pointer:?}: {reason:?}");

        // Serve the teardown as *this* pointer's sample: every helper below
        // that reads "the pointer being dispatched" — the recognizer context,
        // the drag's capture release, `EventContext::pointer()` inside the
        // handler — must answer with the pointer being cancelled and not with
        // whatever the outer dispatch was serving. Restored on the way out, so
        // a cancel drained after an outer sample leaves that sample's snapshot
        // as it found it.
        let snapshot = crate::pointer::InputSnapshot {
            pointer: self
                .pointers
                .get(pointer)
                .map(|e| e.info)
                .unwrap_or_else(|| crate::pointer::PointerInfo::mouse(self.input_now())),
            position: self.pointers.get(pointer).map(|e| e.position),
            ..Default::default()
        };
        let previous_input = std::mem::replace(&mut self.current_input, snapshot);
        // Anything the cancel's own handler dispatches is queued behind it,
        // exactly as it would be from inside an ordinary sample.
        self.dispatch_depth += 1;
        self.tear_down_cancelled_pointer(pointer, reason, recipient, ops);
        self.dispatch_depth -= 1;
        self.current_input = previous_input;
    }

    /// The ordered teardown itself, with `current_input` already serving
    /// `pointer`. See [`cancel_pointer`](Self::cancel_pointer) for why the
    /// order is what it is.
    fn tear_down_cancelled_pointer(
        &mut self,
        pointer: PointerId,
        reason: CancelReason,
        recipient: Option<WidgetId>,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        // 1. Every competitor's recognizer state, before anything else can
        //    recognize on the way out.
        let members: Vec<WidgetId> = self
            .pointers
            .get(pointer)
            .and_then(|e| e.sequence.as_ref())
            .map(|s| s.members().iter().map(|m| m.id).collect())
            .unwrap_or_default();
        for member in members {
            self.cancel_member_arena(member, pointer);
        }
        // The captor's own arena is not necessarily a member (a plain tap owner
        // never enrols), and it is the node most likely to be holding
        // recognizer state for this contact.
        let captor = self.pointers.get(pointer).and_then(|e| e.captured_by);
        if let Some(captor) = captor {
            self.cancel_member_arena(captor, pointer);
        }
        // …and anything else that saw the press but is neither: an ancestor
        // that took the capture off the node whose arena is still following the
        // contact. See `release_arenas_following`.
        self.release_arenas_following(pointer);

        // 2. The arbitration. Whoever it decided for is about to be told the
        //    press it won has been taken away.
        let recipient = recipient
            .filter(|id| self.arena.get(*id).is_some())
            .or_else(|| self.cancel_recipient(pointer));
        if let Some(entry) = self.pointers.get_mut(pointer) {
            entry.sequence = None;
        }

        // 3. The capture.
        self.set_pointer_capture(pointer, None);

        // 4. The drag session this pointer was driving, if it was driving one.
        //    `cancel_active_drag` is the same teardown Escape runs: the current
        //    drop target is told to clear its feedback and the source is told
        //    the drag ended as `Cancelled`, so nothing is left highlighted and
        //    no payload is silently dropped where the pointer happened to be.
        if captor.is_some() && self.pointer_owns_active_drag_via(captor) {
            self.cancel_active_drag(ops);
        }

        // 5. P11 clears the framework press signal here, and P13/P21 stop this
        //    pointer's fling — neither subject exists in the tree yet.

        // 6. The table entry, for a pointer that ceases to exist when it is
        //    taken away. A hovering-capable pointer does not: a mouse whose
        //    press was cancelled is still there, still hovering, and its entry
        //    is what every singular accessor reads — the same rule
        //    `dispatch_pointer_with_ops` applies to an `Up`.
        let hovers = self
            .pointers
            .get(pointer)
            .is_some_and(|e| e.info.kind.hovers());
        if !hovers {
            self.pointers.end(pointer);
        }

        // A cancel is terminal: an `Up` that arrives for this pointer
        // afterwards — a platform that sends both, a test that sends one by
        // hand — must not complete the interaction that was taken away.
        if !self.cancelled_pointers.contains(&pointer) {
            self.cancelled_pointers.push(pointer);
        }

        // 7. The event, last, to a tree that has already let go.
        if let Some(recipient) = recipient {
            let event = self.pointer_cancel_event(pointer, reason);
            self.dispatch_to_widget_direct(recipient, &event, ops);
        }
    }

    // -----------------------------------------------------------------
    // Predicates the producers and the funnel share
    // -----------------------------------------------------------------

    /// Whether `pointer` still has an interaction that can be taken away.
    ///
    /// Three ways the answer is no, and all three mean the same thing — there
    /// is nothing left for a `PointerCancel` to revoke:
    ///
    /// * the pointer is not live at all (a contact that lifted);
    /// * it holds no capture and has no sequence, so it is merely hovering;
    /// * its sequence is inside its own terminal dispatch
    ///   ([`PointerSequence::is_terminating`](crate::gesture::PointerSequence::is_terminating)),
    ///   so the press has completed and only its epilogue is still running.
    ///
    /// The second case is what carries the design's named exemption. By the
    /// time a menu item's `on_tap` runs, the release sweep has already closed
    /// that pointer's sequence (`end_sequence` clears it before the `Up` is
    /// delivered), so a cancel the handler's own overlay teardown queues finds
    /// no press to revoke and the tap completes. The third case covers the
    /// same window for any future producer that fires while a sequence is
    /// installed but already terminating.
    pub(super) fn press_is_revocable(&self, pointer: PointerId) -> bool {
        let Some(entry) = self.pointers.get(pointer) else {
            return false;
        };
        match entry.sequence.as_ref() {
            Some(sequence) => !sequence.is_terminating(),
            None => entry.captured_by.is_some(),
        }
    }

    /// Who receives the `PointerCancel`: the widget holding the capture, and
    /// failing that the last widget that accepted an event from this pointer.
    ///
    /// The first of the two that still *exists* wins. A captor destroyed by
    /// the very rebuild that provoked the cancel cannot be told anything, and
    /// falling through to the last acceptor is how the widget that was
    /// actually interacting still hears about it.
    fn cancel_recipient(&self, pointer: PointerId) -> Option<WidgetId> {
        let entry = self.pointers.get(pointer)?;
        [entry.captured_by, entry.last_accepted]
            .into_iter()
            .flatten()
            .find(|id| self.arena.get(*id).is_some())
    }

    /// The `PointerCancel` for this pointer, positioned where it last was.
    fn pointer_cancel_event(&self, pointer: PointerId, reason: CancelReason) -> WidgetEvent {
        let entry = self.pointers.get(pointer);
        WidgetEvent::PointerCancel {
            position: entry.map(|e| e.position),
            reason,
            pointer: entry
                .map(|e| e.info)
                .unwrap_or_else(|| crate::pointer::PointerInfo::mouse(self.input_now())),
        }
    }

    /// Whether the in-flight drag belongs to the pointer whose capture is
    /// `captor`.
    ///
    /// An internal drag captures the pointer it started from onto its own
    /// source widget (`collect_from_ctx`'s drag-start arm), so holding that
    /// capture *is* what it means to be driving the drag. An external (OS)
    /// drag takes no capture and has no in-app source: it belongs to the
    /// platform backend that began it, and no in-app pointer cancel may end
    /// it.
    fn pointer_owns_active_drag_via(&self, captor: Option<WidgetId>) -> bool {
        self.active_drag
            .as_ref()
            .and_then(|drag| drag.source_widget)
            .is_some_and(|source| captor == Some(source))
    }
}

// -------------------------------------------------------------------------
// P09: the cancel taxonomy — one funnel, an enumerated producer set
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{EventResponse, Modifiers, PointerButton, WidgetEvent};
    use crate::pointer::{
        BackendDeviceKey, EventTime, PointerIdAllocator, PointerInfo, PointerPhase, PointerSample,
    };
    use crate::test_widgets::{FillWidget, StackWidget};
    use crate::widget_builder::WidgetBuilder;
    use std::cell::RefCell;
    use std::rc::Rc;
    use teksilo_canvas::{Point, SizeProposal};

    /// Every cancel a test observed, in delivery order.
    type Log = Rc<RefCell<Vec<(WidgetId, CancelReason)>>>;

    fn log() -> Log {
        Rc::new(RefCell::new(Vec::new()))
    }

    /// A widget that records the cancels it is told about.
    fn recorder(log: &Log, id_slot: Rc<std::cell::Cell<Option<WidgetId>>>) -> impl Widget {
        let log = log.clone();
        FillWidget::new().on_pointer_cancel(move |_pointer, reason, _ctx| {
            let id = id_slot.get().expect("the recorder's id was never recorded");
            log.borrow_mut().push((id, reason));
        })
    }

    fn press(tree: &mut WidgetTree, at: Point) {
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: at,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
    }

    fn moved(tree: &mut WidgetTree, at: Point) {
        tree.dispatch_event(WidgetEvent::PointerMove { position: at });
    }

    fn release(tree: &mut WidgetTree, at: Point) {
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: at,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
    }

    /// A fresh contact: the platform mints a new id per press.
    fn new_contact() -> PointerId {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(31_000);
        PointerIdAllocator::global().begin(
            BackendDeviceKey::DEFAULT,
            NEXT.fetch_add(1, Ordering::Relaxed),
        )
    }

    fn touch(id: PointerId, phase: PointerPhase, at: Point) -> PointerSample {
        PointerSample {
            pointer: PointerInfo::touch(id, EventTime::ZERO),
            phase,
            position: at,
            button: None,
            modifiers: Modifiers::NONE,
            coalesced: Vec::new(),
        }
    }

    /// A leaf that takes the pointer on press and drives itself from moves —
    /// the splitter-handle / column-grip shape, and the simplest thing that
    /// has an interaction a cancel can take away.
    fn grip(log: &Log, id_slot: Rc<std::cell::Cell<Option<WidgetId>>>) -> impl Widget {
        let log = log.clone();
        let slot = id_slot.clone();
        FillWidget::new()
            .on_pointer_event(|event, ctx| {
                if matches!(event, WidgetEvent::PointerDown { .. }) {
                    ctx.capture_pointer();
                }
                EventResponse::Ignored
            })
            .on_pointer_cancel(move |_pointer, reason, _ctx| {
                let id = slot.get().expect("the grip's id was never recorded");
                log.borrow_mut().push((id, reason));
            })
    }

    /// Build a one-leaf tree whose leaf grips the pointer, and return it.
    fn tree_with_grip(log: &Log) -> (WidgetTree, WidgetId) {
        let slot = Rc::new(std::cell::Cell::new(None));
        let mut tree = WidgetTree::new();
        let id = tree.add(grip(log, slot.clone()));
        slot.set(Some(id));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        (tree, id)
    }

    // ---------------------------------------------------------------
    // Producer: window deactivation
    // ---------------------------------------------------------------

    /// The producer that replaces the framework's oldest silent teardown.
    /// Deactivating a window used to drop every capture without a word; now
    /// the widget holding one is told, so it can let go of what the press
    /// latched.
    #[test]
    fn deactivating_the_window_cancels_the_pointer_it_stranded() {
        let log = log();
        let (mut tree, id) = tree_with_grip(&log);

        press(&mut tree, Point::new(20.0, 50.0));
        assert_eq!(tree.captured_by(PointerId::MOUSE), Some(id));

        tree.set_window_active(false);

        assert_eq!(
            *log.borrow(),
            vec![(id, CancelReason::WindowDeactivated)],
            "the widget that captured the pointer is told the window took it away"
        );
        assert_eq!(
            tree.captured_by(PointerId::MOUSE),
            None,
            "and the capture is released, as it always was"
        );
        tree.assert_no_leaked_pointer_state();
    }

    /// A window deactivated with nothing going on cancels nothing: there is no
    /// interaction to revoke, and firing a `PointerCancel` at a widget the
    /// mouse merely rests over would be noise.
    #[test]
    fn deactivating_the_window_with_no_live_press_cancels_nothing() {
        let log = log();
        let (mut tree, _id) = tree_with_grip(&log);

        moved(&mut tree, Point::new(20.0, 50.0));
        tree.set_window_active(false);

        assert!(log.borrow().is_empty());
        tree.assert_no_leaked_pointer_state();
    }

    /// The platform itself revoked the contact — a `wl_touch.cancel`, a
    /// `WM_POINTERCAPTURECHANGED`, a compositor grab. It reaches the funnel
    /// directly from the sample door rather than being lowered onto an event,
    /// because lowering would first *admit* the pointer the sample revokes.
    #[test]
    fn a_platform_cancel_sample_tears_the_contact_down() {
        let log = log();
        let slot = Rc::new(std::cell::Cell::new(None));
        let mut tree = WidgetTree::new();
        let leaf = tree.add(grip(&log, slot.clone()));
        slot.set(Some(leaf));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let contact = new_contact();
        tree.dispatch_pointer(touch(contact, PointerPhase::Down, Point::new(20.0, 50.0)));
        assert_eq!(tree.captured_by(contact), Some(leaf));

        tree.dispatch_pointer(touch(contact, PointerPhase::Cancel, Point::new(20.0, 50.0)));

        assert_eq!(*log.borrow(), vec![(leaf, CancelReason::Platform)]);
        assert!(
            tree.pointers.get(contact).is_none(),
            "a revoked contact leaves the table, as a lifted one does"
        );
        tree.assert_no_leaked_pointer_state();
    }

    /// A cancel sample for a pointer the tree never saw must not *create* one.
    /// Admitting it would leave an entry behind that nothing can ever remove.
    #[test]
    fn a_cancel_for_an_unknown_pointer_admits_nothing() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let ghost = new_contact();
        tree.dispatch_pointer(touch(ghost, PointerPhase::Cancel, Point::new(20.0, 50.0)));

        assert!(tree.pointers.get(ghost).is_none());
        tree.assert_no_leaked_pointer_state();
    }

    /// The recipient of last resort. A press that took no capture still has a
    /// widget that was interacting with it — the last one that answered
    /// `Handled` — and that is who must be told the press was taken away.
    #[test]
    fn a_cancel_without_a_capture_reaches_the_last_widget_that_accepted() {
        let log = log();
        let slot = Rc::new(std::cell::Cell::new(None));

        let mut tree = WidgetTree::new();
        let leaf = {
            let l = log.clone();
            let s = slot.clone();
            tree.add(
                FillWidget::new()
                    // Handles the press without taking the pointer — the shape
                    // of a widget that paints a press state and nothing else.
                    .on_pointer_event(|event, _ctx| {
                        if matches!(event, WidgetEvent::PointerDown { .. }) {
                            return EventResponse::Handled;
                        }
                        EventResponse::Ignored
                    })
                    .on_pointer_cancel(move |_p, reason, _ctx| {
                        let id = s.get().expect("the leaf's id was never recorded");
                        l.borrow_mut().push((id, reason));
                    }),
            )
        };
        slot.set(Some(leaf));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        press(&mut tree, Point::new(20.0, 50.0));
        assert_eq!(
            tree.captured_by(PointerId::MOUSE),
            None,
            "the widget took the press without taking the pointer"
        );

        tree.set_window_active(false);

        assert_eq!(*log.borrow(), vec![(leaf, CancelReason::WindowDeactivated)]);
        tree.assert_no_leaked_pointer_state();
    }

    // ---------------------------------------------------------------
    // Producer: a modal opening
    // ---------------------------------------------------------------

    /// A modal opens over the press: the surface being worked on is now behind
    /// a scrim, and the `Up` that would have completed the press will land on
    /// the modal instead.
    #[test]
    fn opening_a_modal_cancels_every_live_pointer() {
        let log = log();
        let (mut tree, id) = tree_with_grip(&log);

        press(&mut tree, Point::new(20.0, 50.0));

        let modal_content = tree.add(FillWidget::new());
        tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: modal_content,
            anchor: id,
            placement: crate::overlay::OverlayPlacement::Centered,
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        assert_eq!(*log.borrow(), vec![(id, CancelReason::ModalOpened)]);
        assert_eq!(tree.captured_by(PointerId::MOUSE), None);
        tree.assert_no_leaked_pointer_state();
    }

    /// Only a modal. A menu, a popover, a tooltip or a drag preview opens over
    /// an interaction that legitimately continues.
    #[test]
    fn opening_a_non_modal_overlay_cancels_nothing() {
        let log = log();
        let (mut tree, id) = tree_with_grip(&log);

        press(&mut tree, Point::new(20.0, 50.0));

        let popover = tree.add(FillWidget::new());
        tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: popover,
            anchor: id,
            placement: crate::overlay::OverlayPlacement::Below,
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        assert!(log.borrow().is_empty(), "a popover is not a modal");
        assert_eq!(tree.captured_by(PointerId::MOUSE), Some(id));
        release(&mut tree, Point::new(20.0, 50.0));
        tree.assert_no_leaked_pointer_state();
    }

    // ---------------------------------------------------------------
    // Producer: a subtree going dormant
    // ---------------------------------------------------------------

    /// Parking a subtree is invisible to hit-testing and to dispatch, so a
    /// widget parked mid-press would keep what the press latched with no event
    /// left that could clear it.
    #[test]
    fn parking_a_subtree_cancels_the_pointer_inside_it() {
        let log = log();
        let slot = Rc::new(std::cell::Cell::new(None));
        let mut tree = WidgetTree::new();
        let leaf = tree.add(grip(&log, slot.clone()));
        slot.set(Some(leaf));
        let branch = tree.add(StackWidget::new().add_child(leaf));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        press(&mut tree, Point::new(20.0, 50.0));
        assert_eq!(tree.captured_by(PointerId::MOUSE), Some(leaf));

        tree.set_dormant(branch);

        assert_eq!(*log.borrow(), vec![(leaf, CancelReason::SubtreeParked)]);
        assert_eq!(tree.captured_by(PointerId::MOUSE), None);
        tree.assert_no_leaked_pointer_state();
    }

    /// The parked-ids plumbing is what makes the producer above possible: a
    /// caller that cannot see which nodes went to sleep cannot cancel the
    /// pointers holding them. `set_dormant` reports the whole subtree, its
    /// root first, and reports it once per node however deep the nesting.
    #[test]
    fn set_dormant_reports_the_whole_parked_subtree() {
        let mut tree = WidgetTree::new();
        let leaf_a = tree.add(FillWidget::new());
        let leaf_b = tree.add(FillWidget::new());
        let inner = tree.add(StackWidget::new().add_child(leaf_a).add_child(leaf_b));
        let outer = tree.add(StackWidget::new().add_child(inner));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let parked = tree.arena.set_dormant(outer);

        assert_eq!(parked.first(), Some(&outer), "the root is reported first");
        let mut sorted = parked.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            parked.len(),
            "each node is reported exactly once"
        );
        for id in [outer, inner, leaf_a, leaf_b] {
            assert!(
                parked.contains(&id),
                "{id:?} is missing from the parked set"
            );
        }
    }

    /// The other way a subtree parks: a `visible_when` gate flipping false,
    /// resolved by the layout pass rather than by an explicit call. It is the
    /// busiest `set_dormant` caller in the framework, and it goes through the
    /// same door.
    #[test]
    fn a_visible_when_gate_closing_cancels_the_pointer_inside_it() {
        let log = log();
        let slot = Rc::new(std::cell::Cell::new(None));
        let shown = crate::signal::Signal::new(true);

        let mut tree = WidgetTree::new();
        let leaf = tree.add(grip(&log, slot.clone()));
        slot.set(Some(leaf));
        let branch = tree.add(
            StackWidget::new()
                .add_child(leaf)
                .visible_when(shown.clone()),
        );
        let _root = tree.add(StackWidget::new().add_child(branch));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let at = tree.bounds(leaf).center();
        press(&mut tree, at);
        assert_eq!(tree.captured_by(PointerId::MOUSE), Some(leaf));

        shown.set(false);
        tree.layout(SizeProposal::exact(200.0, 100.0));

        assert_eq!(*log.borrow(), vec![(leaf, CancelReason::SubtreeParked)]);
        assert_eq!(tree.captured_by(PointerId::MOUSE), None);
        tree.assert_no_leaked_pointer_state();
    }

    // ---------------------------------------------------------------
    // Producer: an overlay being dismissed, and its named exemption
    // ---------------------------------------------------------------

    /// A pointer anchored inside an overlay that is torn down under it is
    /// stranded on a widget that no longer takes events.
    #[test]
    fn dismissing_an_overlay_cancels_a_pointer_anchored_inside_it() {
        let log = log();
        let slot = Rc::new(std::cell::Cell::new(None));
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let content = tree.add(grip(&log, slot.clone()));
        slot.set(Some(content));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let overlay = tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: content,
            anchor,
            placement: crate::overlay::OverlayPlacement::Below,
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // The press lands on the overlay's own content, which grips the
        // pointer — a scrollbar thumb inside a dropdown, say.
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: tree.bounds(content).center(),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(tree.captured_by(PointerId::MOUSE), Some(content));

        tree.dismiss_overlay(overlay);

        assert_eq!(
            *log.borrow(),
            vec![(content, CancelReason::OverlayDismissed)]
        );
        tree.assert_no_leaked_pointer_state();
    }

    /// **The named exemption.** Tapping a menu item whose own handler closes
    /// its menu must complete the tap. The overlay teardown the handler asks
    /// for happens while that pointer's press is already over — the release
    /// sweep ran before the `Up` was delivered — so there is nothing left for a
    /// cancel to revoke, and the item is never told its own activation was
    /// taken away.
    #[test]
    fn tapping_a_menu_item_that_closes_its_own_menu_completes_the_tap() {
        for kind in ["mouse", "touch"] {
            let log = log();
            let tapped = Rc::new(std::cell::Cell::new(0_u32));
            let overlay_slot = Rc::new(std::cell::Cell::new(None));

            let mut tree = WidgetTree::new();
            let anchor = tree.add(FillWidget::new());
            let item_slot = Rc::new(std::cell::Cell::new(None));
            let item = {
                let t = tapped.clone();
                let o = overlay_slot.clone();
                let l = log.clone();
                let s = item_slot.clone();
                tree.add(
                    FillWidget::new()
                        .on_tap(move |_e, ctx| {
                            t.set(t.get() + 1);
                            // The menu item closes its own menu, exactly as a
                            // real one does.
                            if let Some(id) = o.get() {
                                ctx.dismiss_overlay(id);
                            }
                        })
                        .on_pointer_cancel(move |_p, reason, _ctx| {
                            let id = s.get().expect("the item's id was never recorded");
                            l.borrow_mut().push((id, reason));
                        }),
                )
            };
            item_slot.set(Some(item));
            tree.layout(SizeProposal::exact(200.0, 100.0));

            let overlay = tree.show_overlay(crate::overlay::OverlayRequest {
                content_id: item,
                anchor,
                placement: crate::overlay::OverlayPlacement::Below,
                dismiss: crate::overlay::DismissBehavior::Manual,
                layer: crate::overlay::OverlayLayer::InTree,
                parent_overlay: None,
                on_dismiss: None,
                fade_duration: None,
            });
            overlay_slot.set(Some(overlay));
            tree.layout(SizeProposal::exact(200.0, 100.0));
            let at = tree.bounds(item).center();

            if kind == "mouse" {
                press(&mut tree, at);
                release(&mut tree, at);
            } else {
                let contact = new_contact();
                tree.dispatch_pointer(touch(contact, PointerPhase::Down, at));
                tree.dispatch_pointer(touch(contact, PointerPhase::Up, at));
            }

            assert_eq!(tapped.get(), 1, "the {kind} tap completed");
            assert!(
                log.borrow().is_empty(),
                "the {kind} tap must not be cancelled by the teardown it asked \
                 for: {:?}",
                log.borrow()
            );
            tree.assert_no_leaked_pointer_state();
        }
    }

    // ---------------------------------------------------------------
    // Producer: a peer claiming the sequence
    // ---------------------------------------------------------------

    /// The member granularity, and the one that is emphatically *not* a
    /// pointer cancel: two nested drag-capable ancestors compete for a press a
    /// tapping descendant is holding, the inner one wins at the mouse latch,
    /// and the outer one is told it lost — **once**, on that sample, and never
    /// again however many more moves arrive.
    #[test]
    fn a_peer_claim_revokes_each_loser_exactly_once() {
        let log = log();
        let outer_slot = Rc::new(std::cell::Cell::new(None));

        let mut tree = WidgetTree::new();
        // The tap owner: it takes the capture, which is what enrols the
        // drag-capable ancestors above it as competitors.
        let child = tree.add(FillWidget::new().on_tap(|_e, _c| {}));
        let inner = tree.add(StackWidget::new().add_child(child).on_drag(|_phase, _c| {}));
        let outer = tree.add(
            StackWidget::new()
                .add_child(inner)
                .on_drag(|_phase, _c| {})
                .on_pointer_cancel({
                    let log = log.clone();
                    let slot = outer_slot.clone();
                    move |_p, reason, _ctx| {
                        let id = slot.get().expect("the outer id was never recorded");
                        log.borrow_mut().push((id, reason));
                    }
                }),
        );
        outer_slot.set(Some(outer));
        tree.layout(SizeProposal::exact(400.0, 100.0));

        press(&mut tree, Point::new(20.0, 50.0));
        // Ten samples well past the mouse drag latch: the inner drag wins on
        // the first one that clears 5 dp, and the outer must be revoked then
        // and only then.
        for step in 1..=10 {
            moved(&mut tree, Point::new(20.0 + step as f32 * 10.0, 50.0));
        }

        assert_eq!(
            tree.sequence_winner(PointerId::MOUSE),
            Some(inner),
            "the innermost drag won the press"
        );
        assert_eq!(
            *log.borrow(),
            vec![(outer, CancelReason::PeerClaimed)],
            "the loser is revoked once, on the sample the peer won, and never again"
        );
        assert!(
            tree.pointers.get(PointerId::MOUSE).is_some(),
            "revoking a member is not cancelling the pointer: it is still live"
        );

        release(&mut tree, Point::new(120.0, 50.0));
        assert_eq!(
            log.borrow().len(),
            1,
            "and the release adds nothing: the loser was already told"
        );
        tree.assert_no_leaked_pointer_state();
    }

    // ---------------------------------------------------------------
    // Producer: rebuild / member death
    // ---------------------------------------------------------------

    /// A competitor destroyed mid-press is revoked **individually**: the
    /// pointer and its winner are untouched.
    #[test]
    fn a_destroyed_member_is_revoked_alone() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().on_tap(|_e, _c| {}));
        let ancestor = tree.add(StackWidget::new().add_child(child).on_drag(|_phase, _c| {}));
        tree.layout(SizeProposal::exact(400.0, 100.0));

        press(&mut tree, Point::new(20.0, 50.0));
        assert!(
            tree.sequence_members(PointerId::MOUSE)
                .iter()
                .any(|(id, _, _)| *id == ancestor),
            "the ancestor drag is enrolled through the child's capture"
        );

        tree.destroy_subtree_for_testing(ancestor);
        moved(&mut tree, Point::new(21.0, 50.0));

        assert!(
            tree.sequence_members(PointerId::MOUSE)
                .iter()
                .all(|(id, _, _)| *id != ancestor),
            "the dead member is dropped from the arbitration"
        );
    }

    /// The captor dying is a different act: the press belonged to it, so the
    /// whole pointer is cancelled and the capture it left behind is given back.
    #[test]
    fn a_destroyed_captor_cancels_the_whole_pointer() {
        let log = log();
        let slot = Rc::new(std::cell::Cell::new(None));
        let mut tree = WidgetTree::new();
        let leaf = tree.add(grip(&log, slot.clone()));
        slot.set(Some(leaf));
        let host = tree.add(StackWidget::new().add_child(leaf));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        press(&mut tree, Point::new(20.0, 50.0));
        assert_eq!(tree.captured_by(PointerId::MOUSE), Some(leaf));

        // Destroy the captor without a layout in between, so the sequence's own
        // revalidation is what notices — the mandated hook.
        tree.destroy_subtree_for_testing(host);
        moved(&mut tree, Point::new(21.0, 50.0));

        assert_eq!(
            tree.captured_by(PointerId::MOUSE),
            None,
            "the orphaned capture is given back"
        );
        assert!(
            tree.sequence_members(PointerId::MOUSE).is_empty(),
            "and the arbitration is over"
        );
        tree.assert_no_leaked_pointer_state();
    }

    // ---------------------------------------------------------------
    // Producer: an OS drag taking the pointer
    // ---------------------------------------------------------------

    /// Escalating an in-app drag to the OS hands the pointer to the platform:
    /// this window sees no further move and no `Up`, so whatever the press
    /// still had going has to be revoked here.
    #[test]
    fn escalating_to_an_os_drag_cancels_the_source_pointer() {
        /// Stands in for the platform backend: accepts the hand-off and
        /// records that it did.
        struct AcceptingOps {
            began: std::cell::Cell<bool>,
        }
        impl crate::window::WindowOps for AcceptingOps {
            fn open_window(
                &mut self,
                _config: crate::window::WindowConfig,
            ) -> crate::window::TeksiloWindowId {
                panic!("not used in this test")
            }
            fn find_window(&self, _id: &str) -> Option<crate::window::TeksiloWindowId> {
                None
            }
            fn window_state(
                &self,
                _id: crate::window::TeksiloWindowId,
            ) -> Option<crate::window::WindowState> {
                None
            }
            fn windows(&self) -> Vec<crate::window::WindowState> {
                Vec::new()
            }
            fn focus_window(&mut self, _id: crate::window::TeksiloWindowId) {}
            fn close_window_by_id(&mut self, _id: crate::window::TeksiloWindowId) {}
            fn begin_os_drag(
                &mut self,
                _data: crate::drag_payload::OutboundDragData,
                _image: Option<crate::drag_payload::DragImageData>,
            ) -> bool {
                self.began.set(true);
                true
            }
        }

        let log = log();
        let slot = Rc::new(std::cell::Cell::new(None));
        let mut tree = WidgetTree::new();
        let source = tree.add(recorder(&log, slot.clone()));
        slot.set(Some(source));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let mut ops = AcceptingOps {
            began: std::cell::Cell::new(false),
        };
        press(&mut tree, Point::new(20.0, 50.0));

        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(
            source,
            crate::drag_payload::DragPayload::typed(7_u32).with_mime("text/plain", b"7".to_vec()),
        );
        tree.collect_from_ctx(ctx, source);
        assert!(tree.active_drag.is_some());

        // Out of the window: the drag escalates.
        tree.dispatch_event_with_ops(
            WidgetEvent::PointerMove {
                position: Point::new(-40.0, 50.0),
            },
            &mut ops,
        );

        assert!(ops.began.get(), "the platform took the drag");
        assert_eq!(
            *log.borrow(),
            vec![(source, CancelReason::OsDragStarted)],
            "and the source is told the in-app half of the interaction is over"
        );
        tree.assert_no_leaked_pointer_state();
    }

    // ---------------------------------------------------------------
    // The queue, and terminality
    // ---------------------------------------------------------------

    /// A cancel raised from inside a handler is **queued**: the handler that
    /// raised it finishes on the state it started with, and the teardown runs
    /// afterwards — before the top-level dispatch returns.
    #[test]
    fn a_cancel_raised_from_a_handler_is_queued_not_reentrant() {
        let observed_capture = Rc::new(std::cell::Cell::new(None));
        let cancelled = Rc::new(std::cell::Cell::new(false));

        let mut tree = WidgetTree::new();
        let leaf = {
            let seen = observed_capture.clone();
            let done = cancelled.clone();
            tree.add(
                FillWidget::new()
                    .on_pointer_event(move |event, ctx| {
                        if matches!(event, WidgetEvent::PointerDown { .. }) {
                            ctx.capture_pointer();
                        }
                        if matches!(event, WidgetEvent::PointerMove { .. }) {
                            ctx.cancel_pointer_sequence(CancelReason::Deactivated);
                            // Still owned, right here: the cancel has not run.
                            seen.set(Some(ctx.owns_pointer()));
                        }
                        EventResponse::Ignored
                    })
                    .on_pointer_cancel(move |_p, _reason, _ctx| done.set(true)),
            )
        };
        tree.layout(SizeProposal::exact(200.0, 100.0));

        press(&mut tree, Point::new(20.0, 50.0));
        moved(&mut tree, Point::new(30.0, 50.0));

        assert_eq!(
            observed_capture.get(),
            Some(true),
            "the handler that raised the cancel still owned the pointer when it returned"
        );
        assert!(
            cancelled.get(),
            "and the teardown ran once the sample finished"
        );
        assert!(
            !tree.has_pending_dispatch(),
            "the queue is empty again before the dispatch returns"
        );
        assert_eq!(tree.captured_by(PointerId::MOUSE), None);
        assert_eq!(tree.arena.get(leaf).map(|_| ()), Some(()));
        tree.assert_no_leaked_pointer_state();
    }

    /// A cancel that a handler raises against a pointer which lifts in the very
    /// same sample must not fire: by the time the queue drains there is no
    /// interaction left to revoke.
    #[test]
    fn a_cancel_does_not_fire_if_the_pointer_lifted_in_the_same_sample() {
        let log = log();
        let slot = Rc::new(std::cell::Cell::new(None));

        let mut tree = WidgetTree::new();
        let leaf = {
            let l = log.clone();
            let s = slot.clone();
            tree.add(
                FillWidget::new()
                    .on_pointer_event(|event, ctx| {
                        if matches!(event, WidgetEvent::PointerDown { .. }) {
                            ctx.capture_pointer();
                        }
                        if matches!(event, WidgetEvent::PointerUp { .. }) {
                            // The release itself asks for a cancel — the shape
                            // a menu item's "close my menu" handler has.
                            ctx.cancel_pointer_sequence(CancelReason::OverlayDismissed);
                        }
                        EventResponse::Ignored
                    })
                    .on_pointer_cancel(move |_p, reason, _ctx| {
                        let id = s.get().expect("the leaf's id was never recorded");
                        l.borrow_mut().push((id, reason));
                    }),
            )
        };
        slot.set(Some(leaf));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let contact = new_contact();
        tree.dispatch_pointer(touch(contact, PointerPhase::Down, Point::new(20.0, 50.0)));
        tree.dispatch_pointer(touch(contact, PointerPhase::Up, Point::new(20.0, 50.0)));

        assert!(
            log.borrow().is_empty(),
            "the press was already over when the queued cancel drained: {:?}",
            log.borrow()
        );
        tree.assert_no_leaked_pointer_state();
    }

    /// `PointerCancel` is terminal. An `Up` that arrives for a press the system
    /// already took away completes nothing — the widget has been told to let
    /// go, and handing it back the release would resurrect an interaction that
    /// no longer exists.
    #[test]
    fn no_pointer_up_follows_a_cancel() {
        let ups = Rc::new(std::cell::Cell::new(0_u32));
        let cancels = Rc::new(std::cell::Cell::new(0_u32));

        let mut tree = WidgetTree::new();
        {
            let u = ups.clone();
            let c = cancels.clone();
            tree.add(
                FillWidget::new()
                    .on_tap(move |_e, _ctx| u.set(u.get() + 1))
                    .on_pointer_cancel(move |_p, _reason, _ctx| c.set(c.get() + 1)),
            );
        }
        tree.layout(SizeProposal::exact(200.0, 100.0));

        press(&mut tree, Point::new(20.0, 50.0));
        tree.set_window_active(false);
        assert_eq!(cancels.get(), 1);

        // The platform (or a confused caller) sends the release anyway.
        release(&mut tree, Point::new(20.0, 50.0));
        assert_eq!(ups.get(), 0, "the tap must not complete after its cancel");

        // …and the next press is a fresh interaction, unaffected.
        tree.set_window_active(true);
        press(&mut tree, Point::new(20.0, 50.0));
        release(&mut tree, Point::new(20.0, 50.0));
        assert_eq!(ups.get(), 1, "the next press works normally");
        tree.assert_no_leaked_pointer_state();
    }

    /// The whole-pointer and member-level granularities are different acts,
    /// and the difference has to be visible from outside: a member revocation
    /// leaves the pointer, its capture and its winner exactly where they were.
    #[test]
    fn revoking_a_member_leaves_the_pointer_alive() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().on_tap(|_e, _c| {}));
        let ancestor = tree.add(StackWidget::new().add_child(child).on_drag(|_phase, _c| {}));
        tree.layout(SizeProposal::exact(400.0, 100.0));

        press(&mut tree, Point::new(20.0, 50.0));
        let mut noop = crate::window::NoopWindowOps;
        tree.revoke_sequence_member(
            PointerId::MOUSE,
            ancestor,
            CancelReason::PeerClaimed,
            &mut noop,
        );

        assert_eq!(
            tree.captured_by(PointerId::MOUSE),
            Some(child),
            "the capture survives a member revocation"
        );
        assert!(
            tree.pointers.get(PointerId::MOUSE).is_some(),
            "and so does the pointer"
        );

        release(&mut tree, Point::new(20.0, 50.0));
        tree.assert_no_leaked_pointer_state();
    }

    /// A drag the cancelled pointer was driving ends as `Cancelled`, not as a
    /// drop: releasing the payload wherever the pointer happened to be is how
    /// a lost window focus turns into a data-loss bug.
    #[test]
    fn cancelling_the_pointer_driving_a_drag_cancels_the_drag() {
        let outcome = Rc::new(RefCell::new(None));

        let mut tree = WidgetTree::new();
        let source = {
            let o = outcome.clone();
            tree.add(FillWidget::new().on_drag_ended(move |result, _ctx| {
                *o.borrow_mut() = Some(result);
            }))
        };
        tree.layout(SizeProposal::exact(200.0, 100.0));

        press(&mut tree, Point::new(20.0, 50.0));
        let mut ctx = crate::widget::EventContext::new();
        ctx.start_drag(source, crate::drag_payload::DragPayload::typed(1_u8));
        tree.collect_from_ctx(ctx, source);
        assert!(tree.active_drag.is_some());

        tree.set_window_active(false);

        assert!(tree.active_drag.is_none(), "the drag session is torn down");
        assert_eq!(
            *outcome.borrow(),
            Some(crate::drag_payload::DropOutcome::Cancelled),
            "and its source is told it was cancelled, not dropped"
        );
        tree.assert_no_leaked_pointer_state();
    }

    /// A drag mid-flight is exactly the case the funnel exists for. Its phases
    /// must not report an `Ended` — that is the `Up` story — and the widget
    /// must hear about the revocation instead.
    #[test]
    fn a_cancelled_drag_reports_no_ended_phase() {
        let phases = Rc::new(RefCell::new(Vec::new()));
        let cancels = Rc::new(std::cell::Cell::new(0_u32));

        let mut tree = WidgetTree::new();
        {
            let p = phases.clone();
            let c = cancels.clone();
            tree.add(
                FillWidget::new()
                    .on_drag(move |phase, _ctx| {
                        p.borrow_mut().push(std::mem::discriminant(&phase));
                    })
                    .on_pointer_cancel(move |_p, _reason, _ctx| c.set(c.get() + 1)),
            );
        }
        tree.layout(SizeProposal::exact(400.0, 100.0));

        press(&mut tree, Point::new(20.0, 50.0));
        for step in 1..=3 {
            moved(&mut tree, Point::new(20.0 + step as f32 * 10.0, 50.0));
        }
        let before = phases.borrow().len();
        assert!(before >= 2, "the drag started and moved");

        tree.set_window_active(false);

        assert_eq!(cancels.get(), 1, "the dragging widget is told");
        assert_eq!(
            phases.borrow().len(),
            before,
            "and no further drag phase — least of all an Ended — is reported"
        );
        tree.assert_no_leaked_pointer_state();
    }
}
