// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Pan delivery along a claimant-only chain, the tree's fling pump, the single
//! pinch ingress, and the palm fallback.
//!
//! # A pan is a `Scroll`
//!
//! A finger dragging a scrollable produces a synthesised
//! [`WidgetEvent::Scroll`] — the very event a mouse wheel produces — pushed
//! through [`dispatch_scroll`](WidgetTree::dispatch_scroll), the same ingress
//! door a backend uses, with
//! [`ScrollSource::TouchPan`] on it. No
//! widget gains an `on_pan`, no widget gains a second delta path, and the
//! fourteen surfaces that already implement `on_scroll` scroll under a finger
//! without being touched.
//!
//! What the source changes is only the **route**: [`ScrollDelivery::for_source`]
//! answers [`ClaimantChain`](ScrollDelivery::ClaimantChain) for a pan and
//! [`Bubble`](ScrollDelivery::Bubble) for everything else, so wheel and
//! trackpad keep the route they have always had.
//!
//! # The boundary rule, which is the point of this module
//!
//! **The claim stays with the inner container for the whole gesture.** Per
//! event, delivery is all-or-nothing exactly as the wheel is today:
//! `scroll_response` answers `Handled` if the axis absorbed anything and
//! `Ignored` at a hard boundary. On `Ignored` the router re-delivers **the same
//! whole event** to the next container outward.
//!
//! There is **no fractional residual and no back-channel**. Both are tempting
//! and both are wrong here. [`EventResponse`](crate::event::EventResponse) is
//! binary — it carries no "I took 30 of your 50 pixels" — so a residual would
//! mean a new return type on every scroll handler in the workspace, i.e. a
//! delta-path change in fourteen scrollables, to buy a difference the user sees
//! for one frame of one gesture. The chain hands over events, never remainders.
//!
//! Three consequences follow, and each is a test:
//!
//! * an [`OverscrollBehavior::Contain`] claimant **stops** the chain, even
//!   having absorbed nothing;
//! * a claimant the chain moves past receives **no `PointerCancel` and no
//!   pan-ended** — it did not lose the gesture, it declined one event, and the
//!   next event is offered to it first again;
//! * a fling crossing a boundary chains identically, because it is dispatched
//!   through the same door and walks the same list.
//!
//! # The chain visits only the claimants
//!
//! [`ScrollDelivery::ClaimantChain`] walks the frozen `pan_candidates` list and
//! **never the generic bubble**. This is not an optimisation. The bubble path
//! from a nested list to the window root passes through nodes that handle
//! `on_scroll` without being scroll containers at all — a `SpinBox` increments
//! its value on wheel, a `TabBar` remaps wheel to horizontal tab scrolling. A
//! boundary pan that reached either would change a number or switch a tab,
//! silently, because the user ran out of list.
//! [`PanClaim`] is the declaration that
//! distinguishes them, and the chain reads nothing else.
//!
//! # One pinch ingress
//!
//! [`dispatch_os_gesture`](WidgetTree::dispatch_os_gesture) is the only way a
//! [`GestureEvent::PinchStarted`] / `PinchChanged` / `PinchEnded` reaches a
//! widget. The OS trackpad stream and the two-contact
//! [`TouchPinchRecognizer`] both go through it, so `on_pinch` cannot be
//! reachable on one input and dead on the other.
//!
//! Reference: `docs/kinetic-scrolling.md`.

use std::collections::HashMap;

use teksilo_canvas::{Point, Vec2};

use crate::WidgetId;
use crate::event::{Modifiers, ScrollDelta, WidgetEvent};
use crate::gesture::{GestureEvent, PalmWatch, PanRecognizer, TouchPinchRecognizer};
use crate::kinetic::FlingDriver;
use crate::overscroll::OverscrollBehavior;
use crate::pointer::touch_action::PanClaim;
use crate::pointer::{CancelReason, PointerId, ScrollPhase, ScrollSample, ScrollSource};

use super::WidgetTree;

/// How a scroll event finds its receivers.
///
/// Decided from the sample's [`ScrollSource`], so nothing has to remember which
/// producer gets which route: the wheel and the trackpad keep
/// [`Bubble`](Self::Bubble) — what every scroll did before the touch programme
/// — and a pan synthesised from a direct pointer takes
/// [`ClaimantChain`](Self::ClaimantChain).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ScrollDelivery {
    /// Hit-test, then walk the ancestor chain, offering the event to every
    /// handler on the way up. What a wheel notch does.
    Bubble,
    /// Walk **only** the frozen list of
    /// [`PanClaim`] holders, from the
    /// claimant outward, offering the whole event to each in turn until one
    /// answers `Handled` or a [`Contain`](OverscrollBehavior::Contain)
    /// claimant stops the walk. What a finger does.
    ClaimantChain,
}

impl ScrollDelivery {
    /// The route a sample from `source` takes.
    ///
    /// Only [`ScrollSource::TouchPan`] chains along claimants — a
    /// programmatic scroll, a wheel notch and a trackpad stream all bubble,
    /// because none of them belongs to a pan claimant and all three have
    /// always reached ordinary `on_scroll` handlers.
    pub fn for_source(source: ScrollSource) -> Self {
        match source {
            ScrollSource::TouchPan => Self::ClaimantChain,
            _ => Self::Bubble,
        }
    }
}

/// One direct pointer's live pan.
#[derive(Debug)]
struct PanSession {
    /// The competitor: the innermost eligible claim, its slop, its tracker.
    recognizer: PanRecognizer,
    /// Every claimant from the hit target outward, innermost first, frozen at
    /// the press — a mid-gesture rebuild must not silently re-route a pan that
    /// is already under way.
    candidates: Vec<(WidgetId, PanClaim)>,
    /// The claimant, once the arbitration has decided for one. `None` while the
    /// press could still turn out to be a tap.
    owner: Option<WidgetId>,
    /// Whether a [`ScrollPhase::Began`] has gone out, so the samples after it
    /// are [`Changed`](ScrollPhase::Changed).
    began: bool,
    /// Modifiers at the press, carried onto every synthesised sample so a
    /// Ctrl-held pan reads the same as a Ctrl-held wheel.
    modifiers: Modifiers,
}

impl PanSession {
    /// The chain to walk: the claimant and everything outward of it.
    ///
    /// The whole candidate list before a claimant is decided — which never
    /// reaches a delivery, since nothing is delivered until one is.
    fn chain(&self) -> &[(WidgetId, PanClaim)] {
        match self
            .owner
            .and_then(|owner| self.candidates.iter().position(|(id, _)| *id == owner))
        {
            Some(index) => &self.candidates[index..],
            None => &self.candidates,
        }
    }
}

/// Everything the touch-motion layer owns for one tree: live pans, live
/// coasts, the pinch, and the palm watches.
///
/// One struct rather than seven fields on [`WidgetTree`] — the tree is already
/// wide, and these seven have one lifetime and one owner between them.
#[derive(Debug)]
pub(crate) struct TouchMotion {
    /// One entry per direct pointer whose press found an eligible pan claim.
    pans: HashMap<PointerId, PanSession>,
    /// The tree's fling pump.
    driver: FlingDriver,
    /// The chain a coasting target chains along, frozen when the fling started
    /// — a coast must chain exactly as the pan that launched it did.
    fling_chains: HashMap<WidgetId, Vec<(WidgetId, PanClaim)>>,
    /// Whether the last claimant-chain walk found anyone able to move.
    ///
    /// A one-slot result, written by the walk and read by the fling pump
    /// immediately after its dispatch has drained (the pump runs at dispatch
    /// depth zero, so "immediately after" is exact). It exists because
    /// [`dispatch_scroll`](WidgetTree::dispatch_scroll) returns nothing — the
    /// alternative was a second, synchronous delivery path for flings, and one
    /// delivery path is the whole point.
    last_chain_absorbed: bool,
    /// The one pinch recognizer for the window. Not per node: a pinch is
    /// arbitrated by contact count, not by which widget each finger landed on.
    pinch: TouchPinchRecognizer,
    /// The widget the live pinch is addressed to, resolved at `PinchStarted`
    /// and held, so `Changed` / `Ended` cannot drift to another node as the
    /// fingers move.
    pinch_target: Option<WidgetId>,
    /// One watch per live direct pointer, for the palm fallback.
    palms: HashMap<PointerId, PalmWatch>,
    /// Whether the backend classifies palms itself. When it does the fallback
    /// heuristic is off: the digitiser's answer is better than a guess, and
    /// `PointerTable::would_admit` has already refused what it flagged.
    backend_reports_palm: bool,
}

impl TouchMotion {
    /// A layer wired to `scheduler`, so a coast keeps waking the event loop
    /// through the tree's existing per-frame path rather than a second one.
    pub(crate) fn new(scheduler: crate::frame_tick_scheduler::FrameTickScheduler) -> Self {
        Self {
            pans: HashMap::new(),
            driver: FlingDriver::with_scheduler(scheduler),
            fling_chains: HashMap::new(),
            last_chain_absorbed: false,
            pinch: TouchPinchRecognizer::new(),
            pinch_target: None,
            palms: HashMap::new(),
            backend_reports_palm: false,
        }
    }
}

impl WidgetTree {
    // -----------------------------------------------------------------
    // Pan sessions
    // -----------------------------------------------------------------

    /// Open a pan session for the press being dispatched, if any claimant along
    /// the hit path is eligible.
    ///
    /// Called from the `PointerDown` arm straight after
    /// [`begin_sequence`](Self::begin_sequence), so the frozen `TouchAction`
    /// and the enrolled pan members are already in hand and the chain recorded
    /// here is exactly the one the arbitration picks its winner from.
    ///
    /// Also the point at which a press **stops a coast**: catching a flying
    /// list is a reflex, and a list that ignored the catch would read as
    /// broken.
    pub(super) fn begin_pan(&mut self, target: WidgetId, position: Point, modifiers: Modifiers) {
        let pointer = self.current_input.pointer;
        let action = self.effective_touch_action(target);
        let candidates = self.pan_candidates(target, action);

        // Catching the list: every claimant under this press stops coasting.
        // Before the pointer-kind gate on purpose — a *mouse* click on a
        // coasting list must stop it too, and a mouse never opens a pan
        // session.
        for (id, _) in &candidates {
            self.stop_fling(*id);
        }

        if !pointer.kind.is_direct() {
            // A mouse has no `pan_slop` at all, so it could never arm a pan.
            // Leaving here keeps that a fact about the router rather than an
            // accident of the profile.
            return;
        }
        let profile = self.current_profile();
        if profile.pan_slop.is_none() {
            return;
        }
        let Some((_, claim)) = candidates
            .iter()
            .copied()
            .find(|(_, claim)| claim.devices.contains(pointer.kind))
        else {
            return;
        };

        let mut recognizer = PanRecognizer::new(claim);
        recognizer.press(position, self.sequence_now());
        self.touch_motion.pans.insert(
            pointer.id,
            PanSession {
                recognizer,
                candidates,
                owner: None,
                began: false,
                modifiers,
            },
        );
    }

    /// Whether `pointer`'s press opened a pan session — i.e. whether a claimant
    /// along its hit path accepts this pointer kind.
    ///
    /// Read by the press-feedback delay: a press that nothing can scroll out
    /// from under has no ambiguity to wait out, so only a press *inside a
    /// claimant* withholds its visual. See
    /// `WidgetTree::begin_press`.
    pub(crate) fn pan_session_open(&self, pointer: PointerId) -> bool {
        self.touch_motion.pans.contains_key(&pointer)
    }

    /// The arbitration decided for a pan claimant. Record it: from here on
    /// every sample for this pointer is delivered as a synthesised scroll.
    pub(super) fn note_pan_claimed(&mut self, pointer: PointerId, owner: WidgetId) {
        if let Some(session) = self.touch_motion.pans.get_mut(&pointer) {
            session.owner = Some(owner);
        }
    }

    /// Feed one move into this pointer's pan session and, once the claim has
    /// been decided, deliver the movement as a scroll.
    ///
    /// A no-op for a pointer with no session, which is every mouse.
    pub(super) fn advance_pan(&mut self, position: Point, ops: &mut dyn crate::window::WindowOps) {
        let pointer = self.current_pointer_id();
        let now = self.sequence_now();
        let coalesced = self.current_input.coalesced.clone();

        let Some(session) = self.touch_motion.pans.get_mut(&pointer) else {
            return;
        };
        let delta = session.recognizer.feed_coalesced(&coalesced, position, now);
        if session.owner.is_none() {
            // Undecided: the tracker is still fed, so a claim taken on the very
            // next sample already has a velocity behind it.
            return;
        }
        if delta.x == 0.0 && delta.y == 0.0 {
            // Nothing moved on a claimed axis. Spending a whole chain walk to
            // move nothing would be waste, and it would burn the `Began` phase
            // on a sample the receiver cannot act on.
            return;
        }
        let phase = if session.began {
            ScrollPhase::Changed
        } else {
            session.began = true;
            ScrollPhase::Began
        };
        self.deliver_pan(pointer, delta, phase, position, ops);
    }

    /// The press ended. Hand off to a fling if the claim is kinetic and the
    /// release was fast enough, then close the session.
    pub(super) fn end_pan(&mut self, position: Point, ops: &mut dyn crate::window::WindowOps) {
        let pointer = self.current_pointer_id();
        let now = self.sequence_now();
        let profile = self.current_profile();
        let Some(mut session) = self.touch_motion.pans.remove(&pointer) else {
            return;
        };
        let (Some(owner), true) = (session.owner, session.began) else {
            // The press never became a pan — a tap, or a drag someone else won.
            return;
        };
        session.recognizer.feed(position, now);
        let velocity = session.recognizer.velocity(&profile);
        let chain: Vec<(WidgetId, PanClaim)> = session.chain().to_vec();

        // One `Ended` closes the gesture for the receiver whether or not a
        // fling follows, so a scrollable can release its rubber band. The
        // session is already out of the map, so the chain travels with the
        // event rather than being looked up.
        self.dispatch_chained_scroll(
            chain.clone(),
            Vec2::ZERO,
            ScrollPhase::Ended,
            Some(position),
            session.modifiers,
            ops,
        );

        if !session.recognizer.should_fling(velocity, &profile) {
            return;
        }
        // A fling is the same scroll the pan was, so it carries the same sign
        // convention: the content follows the finger, so the offset moves
        // against it — which `deliver_pan` does by negating, and which the
        // driver's own deltas therefore have to arrive pre-negated for.
        self.start_fling(owner, Vec2::new(-velocity.x, -velocity.y), chain);
    }

    /// The claimants `pointer`'s live pan would chain along, or an empty list
    /// when it has no session. Read by the cancel funnel, which stops every
    /// coast the revoked gesture could have started.
    pub(super) fn pan_chain_ids(&self, pointer: PointerId) -> Vec<WidgetId> {
        self.touch_motion
            .pans
            .get(&pointer)
            .map(|session| session.chain().iter().map(|(id, _)| *id).collect())
            .unwrap_or_default()
    }

    /// Forget this pointer's pan session without delivering anything.
    ///
    /// The cancel funnel's path: the interaction was taken away, so there is no
    /// release to hand a velocity to.
    pub(super) fn abandon_pan(&mut self, pointer: PointerId) {
        self.touch_motion.pans.remove(&pointer);
    }

    // -----------------------------------------------------------------
    // Delivery
    // -----------------------------------------------------------------

    /// Deliver one synthesised scroll for `pointer`'s live pan.
    ///
    /// `delta` is the movement of the **contact**; the scroll offset moves
    /// against it, so this negates — a finger dragging down moves the content
    /// down, which is the offset going up, which is exactly what
    /// `event_translation` already does for the wheel.
    pub(super) fn deliver_pan(
        &mut self,
        pointer: PointerId,
        delta: Vec2,
        phase: ScrollPhase,
        position: Point,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        let Some(session) = self.touch_motion.pans.get(&pointer) else {
            return;
        };
        let chain: Vec<(WidgetId, PanClaim)> = session.chain().to_vec();
        let modifiers = session.modifiers;
        self.dispatch_chained_scroll(
            chain,
            Vec2::new(-delta.x, -delta.y),
            phase,
            Some(position),
            modifiers,
            ops,
        );
    }

    /// Push one [`ScrollSource::TouchPan`] sample through the ordinary scroll
    /// door with `chain` armed as its route.
    ///
    /// The chain is parked for the duration of the dispatch rather than looked
    /// up inside it, because the two producers know different things: a pan
    /// has a live session to read, a fling has only the frozen chain it was
    /// launched with.
    fn dispatch_chained_scroll(
        &mut self,
        chain: Vec<(WidgetId, PanClaim)>,
        offset_delta: Vec2,
        phase: ScrollPhase,
        position: Option<Point>,
        modifiers: Modifiers,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        let pointer = self
            .pointers
            .get(self.current_pointer_id())
            .map(|e| e.info)
            .unwrap_or(self.current_input.pointer);
        let sample = ScrollSample {
            delta: ScrollDelta::Pixels {
                x: offset_delta.x,
                y: offset_delta.y,
            },
            position,
            phase,
            source: ScrollSource::TouchPan,
            pointer,
            modifiers,
        };
        self.armed_chain = Some(chain);
        self.dispatch_scroll_with_ops(sample, ops);
    }

    /// Route a scroll along [`ScrollDelivery::ClaimantChain`], and record
    /// whether anyone took it.
    ///
    /// The chain is whatever the producer armed; failing that — an
    /// externally-produced `TouchPan` sample, which is what a platform backend
    /// that recognises pans itself would send — it is derived from the sample's
    /// own position, which is the same list the press would have frozen.
    pub(super) fn route_scroll_along_chain(
        &mut self,
        event: &WidgetEvent,
        position: Option<Point>,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        let chain = match self.armed_chain.take() {
            Some(chain) => chain,
            None => {
                let pointer = self.current_input.pointer;
                let Some(target) = position.and_then(|p| self.hit_test_for(p, &pointer)) else {
                    return;
                };
                let action = self.effective_touch_action(target);
                self.pan_candidates(target, action)
            }
        };
        let absorbed = self.walk_claimant_chain(&chain, event, ops);
        self.touch_motion.last_chain_absorbed = absorbed;
    }

    /// Offer `event` to each claimant in turn, innermost first, and report
    /// whether one took it.
    ///
    /// The one place the boundary rule is written down:
    ///
    /// * `Handled` — the claimant absorbed something; stop.
    /// * `Ignored` + [`Contain`](OverscrollBehavior::Contain) — the claimant
    ///   absorbed nothing but refuses to let the event out; stop, and report
    ///   the event as consumed, because a contained scroll is not the next
    ///   container's business.
    /// * `Ignored` + [`Chain`](OverscrollBehavior::Chain) — re-deliver the
    ///   **same whole event** to the next claimant outward.
    ///
    /// A claimant the chain moves past is told nothing at all. It has not lost
    /// the gesture — the next sample is offered to it first again — so a cancel
    /// or a pan-ended here would be a lie, and would tear down the scrollable's
    /// own state in the middle of a drag it is still winning.
    fn walk_claimant_chain(
        &mut self,
        chain: &[(WidgetId, PanClaim)],
        event: &WidgetEvent,
        ops: &mut dyn crate::window::WindowOps,
    ) -> bool {
        for &(id, _) in chain {
            if !self.arena.is_active(id) {
                // A claimant destroyed mid-gesture is skipped rather than
                // ending the chain: the containers outward of it are still
                // there and still entitled to the event.
                continue;
            }
            if self.dispatch_to_widget_direct_returning_handled(id, event, &mut *ops) {
                crate::trace_input!(Samples, "pan absorbed by {id:?}");
                return true;
            }
            if self.overscroll_behavior_of(id) == OverscrollBehavior::Contain {
                crate::trace_input!(Samples, "pan contained at {id:?}: the chain stops here");
                return true;
            }
        }
        false
    }

    /// What `id` declared about letting a boundary scroll out.
    fn overscroll_behavior_of(&self, id: WidgetId) -> OverscrollBehavior {
        self.arena
            .get(id)
            .map(|n| n.overscroll_behavior)
            .unwrap_or(OverscrollBehavior::Chain)
    }

    // -----------------------------------------------------------------
    // The fling pump
    // -----------------------------------------------------------------

    /// Begin coasting `target` at `velocity` (in scroll-offset space),
    /// chaining along `chain` when it runs out.
    ///
    /// Public so a surface that drives its own release — a `SceneView`, a
    /// custom canvas — can hand the tree a coast instead of integrating one.
    /// `prefers_reduced_motion` collapses it to nothing: a re-dispatched fling
    /// has no settle to fall back to, because the target already has the
    /// content where the finger left it.
    pub fn start_fling(
        &mut self,
        target: WidgetId,
        velocity: Vec2,
        chain: Vec<(WidgetId, PanClaim)>,
    ) {
        let physics = self.effective_theme.input.scroll_physics.physics;
        self.touch_motion
            .driver
            .set_tokens(&self.effective_theme.input.scroll_physics);
        self.touch_motion
            .driver
            .set_reduced_motion(self.prefers_reduced_motion);
        self.touch_motion
            .driver
            .start(target, velocity, physics, self.input_now());
        if self.touch_motion.driver.is_flinging(target) {
            self.touch_motion.fling_chains.insert(target, chain);
        } else {
            self.touch_motion.fling_chains.remove(&target);
        }
    }

    /// Stop `target`'s coast, if it has one. Idempotent.
    pub fn stop_fling(&mut self, target: WidgetId) {
        self.touch_motion.driver.stop(target);
        self.touch_motion.fling_chains.remove(&target);
    }

    /// Whether `target` is coasting.
    pub fn is_flinging(&self, target: WidgetId) -> bool {
        self.touch_motion.driver.is_flinging(target)
    }

    /// Advance every coast to `now` and dispatch what it produced.
    ///
    /// Each delta goes through the same door and along the same frozen chain a
    /// pan does, so a flick that runs out of inner list scrolls the outer one.
    /// A coast the whole chain declines is **stopped**: it has nothing left to
    /// move, and spinning a simulation against a wall is a frame budget spent
    /// on nothing.
    pub fn tick_flings(&mut self, now: std::time::Instant) {
        let mut noop = crate::window::NoopWindowOps;
        self.tick_flings_with_ops(now, &mut noop);
    }

    /// [`tick_flings`](Self::tick_flings) with the caller's
    /// [`WindowOps`](crate::window::WindowOps) sink.
    pub fn tick_flings_with_ops(
        &mut self,
        now: std::time::Instant,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        if self.touch_motion.driver.is_empty() {
            return;
        }
        let now = self.event_time_for(now);
        let steps = self.touch_motion.driver.tick(now);
        for (target, delta) in steps {
            let Some(chain) = self.touch_motion.fling_chains.get(&target).cloned() else {
                continue;
            };
            // A coast has no contact behind it, so it is addressed by the chain
            // alone; the position is the target's own centre, which is what a
            // receiver that reads one (a zoom-at-cursor handler) should see.
            let position = self
                .arena
                .is_active(target)
                .then(|| self.bounds(target).center());
            self.dispatch_chained_scroll(
                chain,
                delta,
                ScrollPhase::Fling,
                position,
                Modifiers::NONE,
                &mut *ops,
            );
            if !self.touch_motion.last_chain_absorbed {
                // Nobody on the chain could move: the coast has reached the
                // outermost boundary and is over.
                self.stop_fling(target);
            }
        }
    }

    // -----------------------------------------------------------------
    // Pinch
    // -----------------------------------------------------------------

    /// Route a pre-recognized gesture that carries no position of its own.
    ///
    /// **The single ingress for pinch.** The OS trackpad stream
    /// (`PinchGesture` / `RotationGesture`, which winit reports without a
    /// position) and the two-contact [`TouchPinchRecognizer`] both arrive here,
    /// so `on_pinch` cannot be reachable on one input and dead on the other.
    ///
    /// `at` is the gesture's own position when it has one — the touch
    /// recognizer supplies the contact midpoint. With `None` the route is the
    /// hover owner's last position, then the hovered widget, then the focused
    /// one, then the root: the OS says only *that* a pinch happened, and the
    /// pointer that could have said where is a trackpad, whose cursor is the
    /// hover owner.
    pub fn dispatch_os_gesture(
        &mut self,
        gesture: GestureEvent,
        at: Option<Point>,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        let Some(target) = self.os_gesture_target(at) else {
            return;
        };
        self.dispatch_to_widget(target, &WidgetEvent::Gesture { gesture }, ops);
    }

    /// Who a positionless OS gesture is addressed to.
    fn os_gesture_target(&self, at: Option<Point>) -> Option<WidgetId> {
        let hover_position = self.pointers.hover_owner().map(|e| e.position);
        let pointer = self.current_input.pointer;
        at.or(hover_position)
            .and_then(|p| self.hit_test_for(p, &pointer))
            .or_else(|| self.hovered_id())
            .or(self.focused)
            .or_else(|| self.roots().first().copied())
    }

    /// Feed one contact event into the window's pinch recognizer and dispatch
    /// whatever it produced.
    ///
    /// Split from the pan path deliberately: a pinch is arbitrated by contact
    /// count rather than by the press arbitration, so it neither enrols in a
    /// [`PointerSequence`](crate::gesture::PointerSequence) nor consults one.
    pub(super) fn feed_pinch(
        &mut self,
        phase: PinchFeed,
        position: Point,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        let pointer = self.current_input.pointer;
        if !pointer.kind.is_direct() {
            return;
        }
        let recognized = match phase {
            PinchFeed::Down => {
                // The subtree must permit pinch-zoom, read at the contact's own
                // position and folded from the root down like every other
                // `TouchAction` question. Asked only on the press: once a pinch
                // is running, a finger straying over a `touch-action: none`
                // sibling must not tear it down.
                let permitted = self
                    .hit_test_for(position, &pointer)
                    .is_some_and(|target| self.effective_touch_action(target).allows_pinch());
                permitted
                    .then(|| self.touch_motion.pinch.contact_down(pointer.id, position))
                    .flatten()
            }
            PinchFeed::Move => self.touch_motion.pinch.contact_moved(pointer.id, position),
            PinchFeed::Up => self.touch_motion.pinch.contact_up(pointer.id),
        };
        if let Some(gesture) = recognized {
            self.emit_pinch(gesture, ops);
        }
    }

    /// The window's pinch is revoked along with `pointer`.
    pub(super) fn cancel_pinch(
        &mut self,
        pointer: PointerId,
        reason: CancelReason,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        if !self.touch_motion.pinch.contact_ids().contains(&pointer) {
            return;
        }
        if let Some(gesture) = self.touch_motion.pinch.cancel(reason) {
            self.emit_pinch(gesture, ops);
        }
    }

    /// Send one pinch phase to the node the gesture is addressed to.
    ///
    /// The target is resolved once, at `PinchStarted`, and held: resolving it
    /// per phase would let the addressee drift to another widget as the fingers
    /// spread across a boundary, and `Ended` would then arrive somewhere that
    /// never saw a `Started`.
    fn emit_pinch(&mut self, gesture: GestureEvent, ops: &mut dyn crate::window::WindowOps) {
        if let GestureEvent::PinchStarted { center } = gesture {
            self.touch_motion.pinch_target = self.os_gesture_target(Some(center));
        }
        let Some(target) = self.touch_motion.pinch_target else {
            return;
        };
        self.dispatch_to_widget(target, &WidgetEvent::Gesture { gesture }, ops);
        if matches!(
            gesture,
            GestureEvent::PinchEnded | GestureEvent::PinchCancelled { .. }
        ) {
            self.touch_motion.pinch_target = None;
        }
    }

    /// Whether a two-contact pinch is in progress.
    pub fn touch_pinch_active(&self) -> bool {
        self.touch_motion.pinch.is_active()
    }

    // -----------------------------------------------------------------
    // Palm
    // -----------------------------------------------------------------

    /// Declare whether the backend classifies palms itself.
    ///
    /// `false` — the default, and what every `BackendCaps` row Teksilo ships
    /// today reports — turns on the conservative fallback in
    /// [`PalmWatch`]. Set it `true` from a backend that advertises
    /// `reports_palm`, where the digitiser's own answer is better than any
    /// heuristic and has already been applied at `PointerTable::would_admit`.
    pub fn set_backend_reports_palm(&mut self, reports: bool) {
        self.touch_motion.backend_reports_palm = reports;
        if reports {
            self.touch_motion.palms.clear();
        }
    }

    /// Whether the palm fallback is running.
    pub fn palm_fallback_active(&self) -> bool {
        !self.touch_motion.backend_reports_palm
    }

    /// Start watching the contact being pressed.
    pub(super) fn begin_palm_watch(&mut self, position: Point) {
        let pointer = self.current_input.pointer;
        if self.touch_motion.backend_reports_palm || !pointer.kind.is_direct() {
            return;
        }
        self.touch_motion
            .palms
            .insert(pointer.id, PalmWatch::press(position, &pointer.axes));
    }

    /// Fold one sample into the watch.
    pub(super) fn note_palm_sample(&mut self, position: Point) {
        let pointer = self.current_input.pointer;
        let profile = self.current_profile();
        if let Some(watch) = self.touch_motion.palms.get_mut(&pointer.id) {
            watch.sample(position, &pointer.axes, &profile);
        }
    }

    /// Whether the contact that is releasing should be revoked as a palm.
    ///
    /// Consumes the watch either way: the contact is over.
    pub(super) fn take_palm_verdict(&mut self, pointer: PointerId) -> bool {
        self.touch_motion
            .palms
            .remove(&pointer)
            .is_some_and(|watch| watch.is_palm())
    }

    /// Drop a watch without a verdict — the contact was cancelled, so there is
    /// no release to judge.
    pub(super) fn forget_palm_watch(&mut self, pointer: PointerId) {
        self.touch_motion.palms.remove(&pointer);
    }

    // -----------------------------------------------------------------
    // Deadlines
    // -----------------------------------------------------------------

    /// The earliest wall-clock instant at which the **input** layer wants the
    /// event loop back: a pending gesture deadline (a long press), a press
    /// whose feedback delay has not elapsed, a standing hold about to reach
    /// `max_hold`, or a live fling simulation.
    ///
    /// Folded into [`next_timer_deadline`](Self::next_timer_deadline) beside
    /// the tooltip, overlay and animation terms, so there is one
    /// `ControlFlow::WaitUntil` over the one clock rather than a second timer
    /// path for input.
    pub fn next_input_deadline(&self) -> Option<std::time::Instant> {
        let fling = self
            .touch_motion
            .driver
            .next_deadline()
            .map(|t| self.instant_for(t));
        let hold = self
            .next_sequence_hold_deadline()
            .map(|t| self.instant_for(t));
        [
            self.next_gesture_deadline(),
            fling,
            hold,
            self.next_press_deadline(),
        ]
        .into_iter()
        .flatten()
        .min()
    }
}

/// Which contact phase [`WidgetTree::feed_pinch`] is being told about.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(super) enum PinchFeed {
    /// A contact went down.
    Down,
    /// A contact moved.
    Move,
    /// A contact lifted.
    Up,
}

#[cfg(test)]
mod tests;
