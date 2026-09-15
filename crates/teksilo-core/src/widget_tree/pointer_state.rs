// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Pointer / hover / capture / gesture-owner state: the per-node probes
//! the router consults, the ancestor drag-observer bookkeeping, and the
//! gesture-recognizer tick that drives them.

use super::*;
use crate::pointer::touch_action::{Axis, PanClaim, TouchAction};

impl WidgetTree {
    /// This tree's input clock — the one source of
    /// [`EventTime`](crate::pointer::EventTime)s for everything the pointer
    /// path does.
    ///
    /// A [`MonotonicClock`](crate::pointer::clock::MonotonicClock) anchored at
    /// the tree epoch by default. The epoch is the same `Instant`
    /// [`simulated_now`](Self::simulated_now) starts at, so the input timeline
    /// and the simulated animation timeline are one axis rather than two.
    pub fn input_clock(&self) -> std::rc::Rc<dyn crate::pointer::clock::InputClock> {
        self.input_clock.clone()
    }

    /// Replace the input clock.
    ///
    /// A headless test installs a
    /// [`ManualClock`](crate::pointer::clock::ManualClock) here so gesture
    /// deadlines fire exactly when it says, with no sleeping and no dependence
    /// on how long the test itself took.
    pub fn set_input_clock(&mut self, clock: std::rc::Rc<dyn crate::pointer::clock::InputClock>) {
        self.input_clock = clock;
        // The axis just changed underneath. Any offset carried forward from a
        // hand-back belongs to the *old* clock's readings and means nothing
        // against the new one, so it goes with it; then re-anchor the simulated
        // origin against the new clock (or drop it, if the new clock is itself
        // the virtual axis).
        self.sim_input_offset = std::time::Duration::ZERO;
        self.sim_input_origin = None;
        self.rearm_sim_input_origin();
    }

    /// Put this tree on the simulated clock, if it is not already.
    ///
    /// Called from the one door that moves simulated time, and it moves **both**
    /// axes together:
    ///
    /// * the animation scheduler's stored instants are rebased from the wall
    ///   clock onto [`simulated_now`](Self::simulated_now), so an animation
    ///   in flight when the freeze happens keeps the phase it had. Without the
    ///   rebase every such animation would be measured from a start lying in
    ///   the simulated clock's future — the simulated clock reads the tree's
    ///   epoch plus whatever has been advanced, which on a live window is far
    ///   behind the wall clock — and its elapsed time would clamp to zero for
    ///   good;
    /// * the input axis is anchored where it stood — see the
    ///   `sim_input_origin` field — so no stamp taken before the switch lands
    ///   in the virtual future.
    ///
    /// Re-entrant on purpose: [`resume_real_time`](Self::resume_real_time) may
    /// have handed time back since the last call, and the next advance has to
    /// take it again.
    pub(super) fn enter_simulated_mode(&mut self) {
        if !self.sim_time_frozen {
            self.sim_time_frozen = true;
            self.animation_scheduler
                .rebase(std::time::Instant::now(), self.sim_clock);
        }
        if self.sim_input_origin.is_none() {
            self.rearm_sim_input_origin();
        }
    }

    /// Hand time back to the wall clock, carrying forward everything that was
    /// advanced while it was simulated.
    ///
    /// **Who calls this.** A host that shares a live tree with a real event
    /// loop — the debug automation bridge — after each operation that may have
    /// advanced the clock. A headless test does not: a test wants the freeze,
    /// and wants it to survive between calls, so that two samples it dispatches
    /// without advancing are stamped the *same* instant rather than however
    /// many microseconds apart the machine happened to run them.
    ///
    /// **The animation axis** is handed back by rebasing the scheduler's stored
    /// instants the other way, the exact inverse of what
    /// `enter_simulated_mode` did. An animation half-way through when the
    /// operation ends is half-way through on the wall clock too, and the next
    /// real layout pass advances it from there — neither snapped to its end
    /// (which is what ticking at the wall clock against a start stamped on the
    /// simulated one gives) nor stuck (which is what ticking a live tree at a
    /// simulated clock nothing is advancing any more gives).
    ///
    /// **The input axis cannot simply drop its origin.** A frozen axis that has
    /// been advanced reads *ahead* of the raw clock; dropping the origin would
    /// send [`input_now`](Self::input_now) backwards, and a monotone
    /// [`EventTime`](crate::pointer::EventTime) is a platform conformance
    /// invariant every velocity tracker, tap streak and hold relies on. So the
    /// gap is measured — afresh, against this hand-back's own readings, never
    /// added to what a previous one measured, and floored at zero for the case
    /// where the raw clock is already the later of the two — and kept in
    /// `sim_input_offset`: the reading at this instant is the later of the
    /// frozen reading and the raw one, and it moves with the wall clock from
    /// here.
    ///
    /// A no-op on a tree that is not simulating time, so calling it after every
    /// operation costs nothing.
    pub fn resume_real_time(&mut self) {
        if !self.sim_time_frozen {
            return;
        }
        self.sim_time_frozen = false;
        self.animation_scheduler
            .rebase(self.sim_clock, std::time::Instant::now());
        let Some((base, base_at)) = self.sim_input_origin.take() else {
            // An unanchored clock — a `ManualClock` — *is* the virtual axis and
            // was never frozen against the wall clock, so there is nothing to
            // carry forward.
            return;
        };
        let frozen = base + self.sim_clock.saturating_duration_since(base_at);
        // `saturating_since` rather than `-`: on a tree advanced by less than
        // it spent on the wall clock the raw reading is already ahead, and the
        // right offset is then none at all.
        self.sim_input_offset = frozen.saturating_since(self.input_clock.now());
    }

    /// Anchor (or drop) the simulated input origin against the current clock.
    fn rearm_sim_input_origin(&mut self) {
        self.sim_input_origin = if self.sim_time_frozen && self.input_clock.epoch().is_some() {
            // `input_now`, not the raw clock: after a hand-back the axis runs
            // an offset ahead of the clock, and re-freezing at the raw reading
            // would step it backwards by exactly that offset.
            Some((self.input_now(), self.sim_clock))
        } else {
            // Either the tree still runs on real time, or its clock has no
            // wall-clock anchor and is moved directly by `advance_time`.
            None
        };
    }

    /// The current time on this tree's input timeline.
    ///
    /// While the axis is frozen this is a reading of
    /// [`sim_clock`](Self::simulated_now), not of the wall clock, so a deadline
    /// can only be reached by advancing the clock. Once
    /// [`resume_real_time`](Self::resume_real_time) has handed it back it is
    /// the clock again, plus everything that was advanced.
    pub fn input_now(&self) -> crate::pointer::EventTime {
        match self.sim_input_origin {
            Some((base, base_at)) => base + self.sim_clock.saturating_duration_since(base_at),
            None => self.input_clock.now() + self.sim_input_offset,
        }
    }

    /// Everything a recognizer on `id` is allowed to know beyond the event in
    /// front of it: now, the profile for the pointer being dispatched, the
    /// node's own bounds, and the pointer itself.
    ///
    /// Rebuilt per dispatch rather than cached, so a theme change, a density
    /// change or a different pointer kind reaches the recognizers without any
    /// of them holding a copy of a threshold.
    pub(crate) fn recognizer_context(
        &self,
        id: WidgetId,
    ) -> crate::gesture::RecognizerContext<'static> {
        let pointer = self.current_input.pointer;
        let profile = *self.effective_theme.input.profile(pointer.kind);
        let size = self.bounds(id).size();
        // A dispatch that carries no timestamp of its own (a hand-built
        // `WidgetEvent` from a test) reads the tree clock instead.
        let now = if pointer.time == crate::pointer::EventTime::ZERO {
            self.input_now()
        } else {
            pointer.time
        };
        crate::gesture::RecognizerContext::new(
            now,
            profile,
            Rect::new(0.0, 0.0, size.width, size.height),
            pointer,
        )
    }

    // -----------------------------------------------------------------
    // The pointer table
    // -----------------------------------------------------------------

    /// The pointer this dispatch is serving.
    ///
    /// Outside a pointer dispatch the input snapshot holds its default — the
    /// mouse — which is exactly what every legacy `WidgetEvent` has always
    /// meant, so a caller that names no pointer keeps naming the mouse.
    pub(crate) fn current_pointer_id(&self) -> crate::pointer::PointerId {
        self.current_input.pointer.id
    }

    /// The widget holding the capture of the pointer this dispatch is serving.
    ///
    /// Capture is **per pointer**: two contacts hold independent captures and
    /// each is released only by its own Up or Cancel. For the mouse — the only
    /// pointer that existed before the touch programme — this is the old
    /// singular `pointer_captured_by`, unchanged.
    pub(crate) fn current_pointer_capture(&self) -> Option<WidgetId> {
        self.pointers
            .get(self.current_pointer_id())
            .and_then(|e| e.captured_by)
    }

    /// Set (or clear) the capture of the pointer this dispatch is serving.
    pub(crate) fn set_current_pointer_capture(&mut self, captor: Option<WidgetId>) {
        let id = self.current_pointer_id();
        if let Some(entry) = self.pointers.get_mut(id) {
            entry.captured_by = captor;
        }
    }

    /// Set (or clear) the capture of a *named* pointer — the door
    /// [`EventContext::capture_pointer_id`](crate::widget::EventContext::capture_pointer_id)
    /// opens for a handler driving a pointer other than the one it is serving.
    ///
    /// A capture asked for on a pointer that is not live is dropped, not
    /// invented: no sample will ever be delivered to it, so an entry conjured
    /// to hold it would be a capture nothing can release. Reaching this means
    /// the handler ran outside a pointer dispatch entirely (an assistive
    /// technology action, a timer), where there is no pointer to capture.
    pub(crate) fn set_pointer_capture(
        &mut self,
        pointer: crate::pointer::PointerId,
        captor: Option<WidgetId>,
    ) {
        if let Some(entry) = self.pointers.get_mut(pointer) {
            entry.captured_by = captor;
        }
    }

    /// Release the capture a drag session was holding.
    ///
    /// A drag owns one pointer, but which one is not recorded on the session
    /// yet (arbitration lands with the gesture package), so this releases both
    /// the pointer being dispatched and anything the drag's source widget
    /// still holds. For the mouse those are the same capture, which is why
    /// this is a faithful stand-in for the old blanket clear.
    pub(super) fn release_drag_capture(&mut self, source: Option<WidgetId>) {
        self.set_current_pointer_capture(None);
        if let Some(src) = source {
            self.pointers.release_captures_of(src);
        }
    }

    /// The widget the **hover owner** is over.
    ///
    /// Hover belongs to the hover owner and to nobody else: a contact never
    /// produces hover, so a finger arriving beside a hovering mouse leaves
    /// this — and every `on_hover` handler, tooltip dwell and cursor shape —
    /// exactly where it was.
    pub(crate) fn hovered_id(&self) -> Option<WidgetId> {
        self.pointers.hover_owner().and_then(|e| e.hovered)
    }

    /// Where the hover owner is, if one is live.
    ///
    /// The position hover recovery must re-hit-test at. Distinct from
    /// [`last_pointer_position`](Self::last_pointer_position), which reports
    /// the *primary* pointer and so answers for a touch-only device too —
    /// where re-deriving hover from it would invent a hover no finger ever
    /// produced.
    pub(crate) fn hover_owner_position(&self) -> Option<teksilo_canvas::Point> {
        self.pointers.hover_owner().map(|e| e.position)
    }

    /// Admit the sample being dispatched into the pointer table, refreshing
    /// its position and its [`PointerInfo`](crate::pointer::PointerInfo), and
    /// publish the modality.
    ///
    /// Returns `false` when the table refused the pointer (a palm, or the
    /// contact cap), in which case the sample must not be dispatched at all.
    pub(super) fn admit_current_pointer(
        &mut self,
        position: teksilo_canvas::Point,
        is_down: bool,
        is_move: bool,
    ) -> bool {
        let info = self.current_input.pointer;
        // Where this pointer was *before* this sample. Only a move of the
        // hover owner updates `previous_pointer_position`, because the one
        // reader — the overlay safe triangle — wants the last sample that was
        // still over the anchor the pointer is leaving, and a press or a
        // second contact is not that.
        let was_hover_owner = self.pointers.hover_owner_id() == Some(info.id);
        let before = self.pointers.get(info.id).map(|e| e.position);
        if self.pointers.admit(info, position, is_down).is_none() {
            return false;
        }
        if is_move && was_hover_owner {
            self.previous_pointer_position = before;
        }
        if self.last_pointer_kind_signal.get() != info.kind {
            self.last_pointer_kind_signal.set(info.kind);
        }
        true
    }

    /// Hand the hover-owner role to the pointer being dispatched, and take
    /// hover away from whoever held it.
    ///
    /// The later sample wins: on a machine with both a mouse and a pen, the
    /// device the user just moved owns hover, and the one that lost it is sent
    /// a [`PointerLeave`](crate::event::WidgetEvent::PointerLeave) for the
    /// widget it was over — otherwise that widget stays lit for a pointer that
    /// is no longer pointing at it. A contact is refused outright.
    pub(super) fn claim_hover_owner_for_current(&mut self, ops: &mut dyn crate::window::WindowOps) {
        let id = self.current_pointer_id();
        let Some(displaced) = self.pointers.claim_hover_owner(id) else {
            return;
        };
        let stale = self
            .pointers
            .get_mut(displaced)
            .and_then(|entry| entry.hovered.take());
        if let Some(old) = stale {
            // Credited to the pointer that just *lost* the role — it is the one
            // no longer pointing at `old` — not to the claimant.
            let leave = WidgetEvent::PointerLeave {
                pointer: self
                    .pointers
                    .get(displaced)
                    .map(|entry| entry.info)
                    .unwrap_or_else(|| crate::pointer::PointerInfo::mouse(self.input_now())),
            };
            self.dispatch_to_widget(old, &leave, &mut *ops);
            self.tooltip_pointer_leave(old, &mut *ops);
        }
        // The new owner starts with no hover of its own; the move that gave it
        // the role establishes one immediately afterwards.
        self.update_hover_within_signals(stale, None);
        self.set_hovered(None);
    }

    /// Whether `id` carries a drag or swipe handler (hence gets a drag/swipe
    /// recognizer once its arena is built).
    fn widget_has_drag(&self, id: WidgetId) -> bool {
        self.arena
            .get(id)
            .map(|n| n.any_handler(|h| h.on_drag.is_some() || h.on_swipe.is_some()))
            .unwrap_or(false)
    }

    /// Whether `id` is a gesture dead-zone boundary — a press inside its
    /// subtree must not arm a drag/swipe on any ancestor above it. See
    /// [`WidgetNode::gesture_dead_zone`](crate::arena::WidgetNode::gesture_dead_zone).
    fn is_gesture_dead_zone(&self, id: WidgetId) -> bool {
        self.arena
            .get(id)
            .map(|n| n.gesture_dead_zone)
            .unwrap_or(false)
    }

    /// Whether `id` is a keyboard-capture surface — while focused it
    /// receives every `KeyDown` raw, bypassing shortcut resolution. See
    /// [`WidgetNode::keyboard_capture`](crate::arena::WidgetNode::keyboard_capture).
    pub(super) fn is_keyboard_capture(&self, id: WidgetId) -> bool {
        self.arena
            .get(id)
            .map(|n| n.keyboard_capture)
            .unwrap_or(false)
    }

    // -----------------------------------------------------------------
    // The arbitration spine: one `PointerSequence` per live pointer
    // -----------------------------------------------------------------

    /// The frozen hit path for `target`: target → root.
    fn hit_path(&self, target: WidgetId) -> Vec<WidgetId> {
        let mut path = Vec::new();
        let mut current = Some(target);
        while let Some(id) = current {
            path.push(id);
            current = self.arena.parent(id);
        }
        path
    }

    /// The **innermost** gesture dead zone on `path`.
    ///
    /// Nothing at or above it may be enrolled — for a mouse exactly as for a
    /// finger. A dead zone is deliberately *not* sugar for
    /// [`TouchAction::NONE`]: a mouse ignores touch actions entirely, so the
    /// substitution would delete the mouse behaviour the flag exists for, and
    /// on a direct pointer it would drop the latch to `slop_precise` and turn
    /// the `DeadZone` widget's own regression into a 2 px hair trigger.
    fn dead_zone_on(&self, path: &[WidgetId]) -> Option<WidgetId> {
        path.iter()
            .copied()
            .find(|id| self.is_gesture_dead_zone(*id))
    }

    /// The gesture profile for the pointer this dispatch is serving.
    pub(super) fn current_profile(&self) -> teksilo_tokens::GestureProfile {
        *self
            .effective_theme
            .input
            .profile(self.current_input.pointer.kind)
    }

    /// The sequence for the pointer this dispatch is serving.
    pub(crate) fn current_sequence(&self) -> Option<&crate::gesture::PointerSequence> {
        self.pointers
            .get(self.current_pointer_id())
            .and_then(|e| e.sequence.as_ref())
    }

    /// Run `f` against the current pointer's sequence, taking it out of the
    /// table for the duration so `self` stays fully borrowable.
    ///
    /// The sequence is put back only if the pointer is still live afterwards —
    /// a handler that ended the pointer must not have its sequence resurrected.
    fn with_sequence<R>(
        &mut self,
        f: impl FnOnce(&mut Self, &mut crate::gesture::PointerSequence) -> R,
    ) -> Option<R> {
        let pointer = self.current_pointer_id();
        let mut sequence = self.pointers.get_mut(pointer)?.sequence.take()?;
        let result = f(self, &mut sequence);
        if let Some(entry) = self.pointers.get_mut(pointer) {
            entry.sequence = Some(sequence);
        }
        Some(result)
    }

    /// Open the arbitration for the press being dispatched, **before** any
    /// handler runs.
    ///
    /// It has to be before: `ctx.touch_action()` reports the frozen value from
    /// inside the press handler, and an explicit `capture_pointer()` made there
    /// needs a sequence to enrol into. Pan claimants are enrolled here too, so
    /// that the `DragActivation::Auto` question — "is anything else already
    /// claiming this axis?" — has an answer during the press.
    ///
    /// A direct pointer forms **no sequence at all** when
    /// [`InputTokens::touch_enabled`](teksilo_tokens::InputTokens::touch_enabled)
    /// is off: the kill switch means the framework arbitrates nothing for a
    /// contact, and the sample takes the legacy route unchanged.
    pub(super) fn begin_sequence(&mut self, target: WidgetId, position: teksilo_canvas::Point) {
        use crate::gesture::{MemberRole, PointerSequence};

        let pointer = self.current_input.pointer;
        if pointer.kind.is_direct() && !self.effective_theme.input.touch_enabled {
            crate::trace_input!(
                Gestures,
                "no sequence for {:?}: touch_enabled=false",
                pointer.id
            );
            return;
        }
        let path = self.hit_path(target);
        let touch_action = self.effective_touch_action(target);
        let boundary = self.dead_zone_on(&path);
        let now = self.recognizer_context(target).now;
        let mut sequence =
            PointerSequence::new(pointer, path, touch_action, boundary, position, now);

        // Pan claimants: direct pointers only. `PanClaim::devices` defaults to
        // DIRECT and the mouse profile has no `pan_slop` at all, so this loop
        // adds nothing for a mouse — which is what keeps every mouse sequence
        // arbitrating exactly as `drag_observers` did.
        let profile = self.current_profile();
        if pointer.kind.is_direct() {
            for (id, claim) in self.pan_candidates(target, touch_action) {
                if sequence.pan_is_eligible(&claim, &profile) {
                    sequence.enrol(id, MemberRole::Pan(claim));
                }
            }
        }

        crate::trace_input!(
            Gestures,
            "sequence opened for {:?}: action={:?} dead_zone={:?} pan_members={}",
            pointer.id,
            touch_action,
            boundary,
            sequence.members().len()
        );
        if let Some(entry) = self.pointers.get_mut(pointer.id) {
            entry.sequence = Some(sequence);
        }
    }

    /// Enrol the competitors that only become knowable once the press has been
    /// dispatched, and feed each of them the `Down`.
    ///
    /// The gesture members are the pre-existing drag observers, expressed on
    /// the sequence and with the same three rules:
    ///
    /// * the captured widget's own drag owns the gesture — it is enrolled as
    ///   the innermost member and no ancestor is;
    /// * a dead-zone boundary stops the walk;
    /// * only nodes carrying `on_drag` / `on_swipe` compete.
    ///
    /// The captured widget's arena has already seen this `Down` through the
    /// normal bubble, so only the ancestors are fed here.
    pub(super) fn enrol_sequence_members(
        &mut self,
        down_event: &WidgetEvent,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        use crate::gesture::MemberRole;
        use teksilo_tokens::DragActivation;

        let captured = self.current_pointer_capture();
        let profile = self.current_profile();

        // Which ancestors compete, decided against the frozen path.
        let Some(to_enrol) = self.with_sequence(|tree, sequence| {
            sequence.set_capture(captured);
            sequence.set_pressed_owner(captured);
            // A decided sequence still enrols its competitors — as **rejected**
            // ones. Enrolling them is what lets the router stop their
            // recognizers from being fed at all: an ancestor that never becomes
            // a member is invisible to the arbitration, and an explicit captor
            // would lose the press to it on the very next move.
            let decided = sequence.is_decided();
            let Some(captured) = captured else {
                // Nothing took the press, so there is nothing for an ancestor
                // to observe *through* — the pre-existing gate, kept verbatim.
                return Vec::new();
            };
            if tree.widget_has_drag(captured) {
                // The innermost drag owns the gesture: it is the member, and no
                // ancestor is. Its own arena drives it through the capture
                // route, so it is never fed here.
                if sequence.enrol(captured, MemberRole::Gesture) && decided {
                    sequence.reject(captured);
                }
                return Vec::new();
            }
            if !sequence.may_enrol(captured) {
                // The press landed inside a dead zone (the captured control
                // *is* the dead zone) — arm no ancestor at all.
                return Vec::new();
            }
            let mut out = Vec::new();
            let mut current = tree.arena.parent(captured);
            while let Some(id) = current {
                if !sequence.may_enrol(id) {
                    break;
                }
                if tree.widget_has_drag(id) {
                    let activation = tree
                        .arena
                        .get(id)
                        .map(|n| n.drag_activation)
                        .unwrap_or(DragActivation::Auto);
                    if sequence.enrol_drag(id, MemberRole::Gesture, activation, &profile) {
                        if decided {
                            sequence.reject(id);
                        } else {
                            out.push((id, true));
                        }
                    }
                }
                current = tree.arena.parent(id);
            }
            out
        }) else {
            return;
        };

        for (id, feed) in to_enrol {
            if !feed {
                continue;
            }
            // Build the arena (the bubble never reached this ancestor) and feed
            // it the press so its DragRecognizer records the origin.
            {
                let WidgetTree {
                    arena,
                    gesture_owners,
                    ..
                } = self;
                if let Some(node) = arena.get_mut(id) {
                    Self::ensure_gesture_arena(node, id, gesture_owners);
                }
            }
            self.feed_member_arena(id, down_event, &mut *ops);
        }
    }

    /// Record where the pointer is, so every positional threshold reads one
    /// number rather than each member tracking its own.
    pub(super) fn note_sequence_position(&mut self, position: teksilo_canvas::Point) {
        let pointer = self.current_pointer_id();
        if let Some(entry) = self.pointers.get_mut(pointer)
            && let Some(sequence) = entry.sequence.as_mut()
        {
            sequence.set_last_position(position);
        }
    }

    /// Now, on the input timeline, for the sample being dispatched.
    ///
    /// A hand-built `WidgetEvent` carries no timestamp, so it reads the tree
    /// clock — which is what lets a test drive a deadline with a
    /// [`ManualClock`](crate::pointer::clock::ManualClock).
    pub(super) fn sequence_now(&self) -> crate::pointer::EventTime {
        let stamped = self.current_input.pointer.time;
        if stamped == crate::pointer::EventTime::ZERO {
            self.input_now()
        } else {
            stamped
        }
    }

    /// **Timers before positional thresholds** — step 4 of the decision
    /// procedure, run before the move is dispatched anywhere.
    ///
    /// It has to be before: a member reached through the ordinary capture
    /// bubble would otherwise recognize on this very sample, and a deferred
    /// drag that should have withdrawn — or a peer that should have been frozen
    /// by a hold — would already have won by the time the arbitration was
    /// consulted.
    ///
    /// Three rules, all no-ops for a press that stayed where it landed:
    ///
    /// * a hold older than `profile.max_hold` is released, because the
    ///   framework never trusts a holder to answer — the one rule here that is
    ///   *also* driven by the clock, through
    ///   [`expire_sequence_holds`](Self::expire_sequence_holds), so a contact
    ///   that never moves is released on time too;
    /// * a member armed by [`DragActivation::AfterLongPress`](teksilo_tokens::DragActivation::AfterLongPress) withdraws once
    ///   the press leaves the tap boundary — that travel is a pan, not a
    ///   considered grab;
    /// * the pressed node's **tap family** is revoked, once, when the press
    ///   leaves the tap boundary — WCAG 2.2 SC 2.5.2's "slide off to abort":
    ///   the activation is abandoned, a drag the same press started is not.
    ///
    /// All three read the one [`TapBoundary`](crate::gesture::TapBoundary)
    /// predicate, which is also what `TapRecognizer` fails on, so the router
    /// and the recognizer cannot disagree about whether a press has slid off.
    pub(super) fn tick_sequence_timers(&mut self) {
        use crate::gesture::MemberState;

        let profile = self.current_profile();
        let now = self.sequence_now();
        let Some(revoke) = self.with_sequence(|tree, sequence| {
            sequence.expire_holds(now, &profile);
            if sequence.is_decided() {
                return None;
            }
            let origin = sequence.press_origin();
            let position = sequence.last_position();
            let boundary = crate::gesture::TapBoundary::for_pointer(&sequence.pointer(), &profile);
            let left = |id: WidgetId| {
                let bounds = tree.arena.is_active(id).then(|| tree.arena.bounds(id));
                boundary.left(origin, position, bounds, &profile)
            };
            let rejects: Vec<WidgetId> = sequence
                .members()
                .iter()
                .filter(|m| m.state == MemberState::Possible && m.rejects_on_tap_slop)
                .filter(|m| left(m.id))
                .map(|m| m.id)
                .collect();
            for id in rejects {
                sequence.reject(id);
            }
            let owner = sequence.pressed_owner()?;
            if sequence.taps_cancelled() || !left(owner) {
                return None;
            }
            sequence.set_taps_cancelled();
            Some(owner)
        }) else {
            return;
        };
        let Some(owner) = revoke else {
            return;
        };
        let pointer = self.current_pointer_id();
        if let Some(node) = self.arena.get_mut(owner)
            && let Some(set) = node.handlers.gesture_arena.as_mut()
        {
            set.cancel_taps(pointer);
        }
    }

    /// Advance an undecided sequence with a move: **timers before positional
    /// thresholds**, then members innermost-first.
    ///
    /// * a `RawDrag` member wins past the sequence's latch slop;
    /// * a `Gesture` member wins when its own recognizer recognizes — which for
    ///   a mouse is at `drag_slop`, the 5.0 it has always been;
    /// * a `Pan` member wins only on an axis the frozen `TouchAction` permits
    ///   and only past `pan_slop`, which a mouse profile does not have;
    /// * a member deferred by [`DragActivation::AfterLongPress`](teksilo_tokens::DragActivation::AfterLongPress) cannot win
    ///   before its timer and self-rejects once the press leaves the tap
    ///   boundary.
    ///
    /// A `Gesture` member whose id is the press owner is skipped **and stops
    /// the walk**: its recognizer is already being driven by the capture
    /// dispatch, and letting an ancestor past it would break the "innermost
    /// drag owns the gesture" rule. A `RawDrag` and a `Pan` are not, because
    /// this walk is the *only* place either is ever evaluated — the press owner
    /// is very often a pan claimant, since an implicit arena capture makes any
    /// node with a tap handler the owner, and an editing surface has both.
    /// `RawPreview` rides along in the same match and is dead there: a preview
    /// claim decides the sequence as it is enrolled, and a decided sequence
    /// yields no candidates at all.
    pub(super) fn advance_sequence(
        &mut self,
        move_event: &WidgetEvent,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        use crate::gesture::MemberRole;

        let profile = self.current_profile();
        let now = self.sequence_now();

        let Some(candidates) = self.with_sequence(|_, sequence| {
            if sequence.is_decided() || sequence.is_held() {
                // No peer may win while a member is deferring its own answer.
                return Vec::new();
            }
            sequence
                .members()
                .iter()
                .filter(|m| m.is_eligible_at(now))
                .map(|m| (m.id, m.role))
                .collect()
        }) else {
            return;
        };

        // The stop rule reads the node whose arena took the **press**, not the
        // live captor: a member that wins mid-dispatch takes the capture, and
        // keying on the live value would make the winner look like the thing
        // that stops the walk.
        let owner = self.current_sequence().and_then(|s| s.pressed_owner());
        for (id, role) in candidates {
            // The stop rule is about the roles the **capture dispatch** drives,
            // and only those. A `Gesture` member's recognizer is fed through the
            // ordinary capture bubble, so evaluating it here would double-drive
            // it, and letting an ancestor past it would break "the innermost
            // drag owns the gesture" — it stops the walk.
            //
            // A `RawDrag` and a `Pan` are both decided *here* and nowhere else:
            // a raw drag on the sequence's own travel, a pan on
            // `pan_axis_past_slop`, whose product is a synthesised `Scroll`
            // rather than a `GestureEvent` fed to an arena. Breaking at either
            // would mean a member that also owns the press arena could never
            // win — and for a pan claimant that is every editing surface, which
            // takes the press for its caret (so the implicit arena capture makes
            // it the `pressed_owner`) and scrolls itself under a finger.
            if Some(id) == owner && matches!(role, MemberRole::Gesture | MemberRole::RawPreview) {
                break;
            }
            let won = match role {
                MemberRole::Gesture => self.feed_member_arena(id, move_event, &mut *ops),
                MemberRole::RawDrag => self
                    .current_sequence()
                    .is_some_and(|s| s.travel() >= s.latch_slop(&profile)),
                MemberRole::Pan(claim) => self
                    .current_sequence()
                    .and_then(|s| s.pan_axis_past_slop(&claim, &profile))
                    .is_some(),
                MemberRole::RawPreview => false,
            };
            // The `active_drag` half is a **guard**, not a second way in. The
            // router enters this walk only while no drag is in flight (both
            // call sites in `pointer_router.rs` test `active_drag.is_none()`),
            // and the only thing here that can raise one is
            // `feed_member_arena` on this very candidate — which reports it as
            // `won` in the same breath. So the `else` winner below is not
            // reachable on today's call paths, and the winner recorded is
            // always the member that won. What DOES happen when a surface
            // raises its own drag from the capture dispatch is that this walk
            // is not entered at all, leaving the sequence undecided:
            // `a_drag_raised_by_the_press_owner_takes_the_press_out_of_the_arbitration`
            // (`pan_arbiter/tests.rs`) pins that.
            if won || self.active_drag.is_some() {
                let winner = if won { id } else { owner.unwrap_or(id) };
                self.decide_sequence(winner);
                // A pan claimant that won owns the rest of the press as a
                // *scroll*: from here every sample for this contact is
                // synthesised onto the claimant chain rather than delivered as
                // a pointer move. Recorded only for a genuine pan win.
                if won && matches!(role, MemberRole::Pan(_)) {
                    let pointer = self.current_pointer_id();
                    self.note_pan_claimed(pointer, winner);
                }
                return;
            }
        }
    }

    /// Declare `winner` the owner of the current sequence and cancel every
    /// competitor it knocked out — each exactly once.
    ///
    /// "Exactly once" is structural rather than bookkept:
    /// [`PointerSequence::decide`](crate::gesture::PointerSequence::decide)
    /// reports only the members that were still live and flips them to
    /// `Rejected` as it goes, and a decided sequence returns early above — so a
    /// loser knocked out in an earlier sample cannot be knocked out again.
    pub(super) fn decide_sequence(&mut self, winner: WidgetId) {
        let mut noop = crate::window::NoopWindowOps;
        self.decide_sequence_with_ops(winner, &mut noop);
    }

    /// [`decide_sequence`](Self::decide_sequence) with the caller's
    /// [`WindowOps`](crate::window::WindowOps), so a loser's
    /// `on_pointer_cancel` can reach the multi-window API like any other
    /// handler.
    pub(super) fn decide_sequence_with_ops(
        &mut self,
        winner: WidgetId,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        let Some(losers) = self.with_sequence(|_, sequence| {
            if sequence.is_decided() {
                return Vec::new();
            }
            crate::trace_input!(
                Gestures,
                "sequence for {:?} decided: {:?}",
                sequence.pointer().id,
                winner
            );
            sequence.decide(winner)
        }) else {
            return;
        };
        let pointer = self.current_pointer_id();
        for id in losers {
            // Member-level, not pointer-level: the pointer is very much alive
            // and its winner is about to go on using it.
            self.revoke_sequence_member(
                pointer,
                id,
                crate::pointer::CancelReason::PeerClaimed,
                &mut *ops,
            );
        }
    }

    /// Take a member out of the running and revoke whatever its recognizers had
    /// accumulated for this contact.
    pub(super) fn cancel_member_arena(&mut self, id: WidgetId, pointer: crate::pointer::PointerId) {
        if let Some(node) = self.arena.get_mut(id)
            && let Some(set) = node.handlers.gesture_arena.as_mut()
        {
            set.cancel(pointer);
        }
    }

    /// Re-check every member against the arena, once per sample.
    ///
    /// A member whose node was destroyed is cancelled **individually** and
    /// dropped; the sequence itself dies only when its winner or its captor
    /// goes away, because those are the two nodes the press actually belongs
    /// to.
    pub(super) fn revalidate_sequence(&mut self, ops: &mut dyn crate::window::WindowOps) {
        let pointer = self.current_pointer_id();
        let capture = self.current_pointer_capture();
        let Some((dead, lost_owner)) = self.with_sequence(|tree, sequence| {
            sequence.set_capture(capture);
            let dead = sequence.revalidate(&tree.arena);
            // Which owner died decides how the cancel reads: a winner that went
            // away had won the press outright, a captor that went away leaves
            // the capture with nobody holding it.
            let lost_owner = sequence
                .lost_owner(&tree.arena)
                .then(|| match sequence.winner() {
                    Some(winner) if !tree.arena.is_active(winner) => {
                        crate::pointer::CancelReason::WidgetDestroyed
                    }
                    _ => crate::pointer::CancelReason::CaptureOrphaned,
                });
            if lost_owner.is_some() {
                // The cancel that finishes the teardown is queued behind this
                // sample, so the sequence outlives this line by one dispatch.
                // Nothing may win a press whose owner is already gone in the
                // meantime — withdraw every competitor now, which also silences
                // their recognizers for the rest of the sample
                // (`sequence_blocks_arena`).
                let live: Vec<WidgetId> = sequence.members().iter().map(|m| m.id).collect();
                for id in live {
                    sequence.reject(id);
                }
            }
            (dead, lost_owner)
        }) else {
            return;
        };
        for id in dead {
            self.revoke_sequence_member(
                pointer,
                id,
                crate::pointer::CancelReason::WidgetDestroyed,
                &mut *ops,
            );
        }
        if let Some(reason) = lost_owner {
            crate::trace_input!(
                Gestures,
                "sequence for {pointer:?} cancelled: its owner is gone ({reason:?})"
            );
            self.cancel_pointer(pointer, reason, &mut *ops);
        }
    }

    /// The release sweep. The pointer sequence ended without a positional
    /// competitor latching, so feed the terminating `Up` to every member that
    /// is still following the press.
    ///
    /// This is what stops an ancestor `DragRecognizer` — armed on the press
    /// while an interactive descendant held the capture — from staying armed
    /// indefinitely and starting a phantom drag on the next *hover* move.
    pub(super) fn end_sequence(
        &mut self,
        up_event: &WidgetEvent,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        use crate::gesture::MemberRole;

        let pointer = self.current_pointer_id();
        let capture = self.current_pointer_capture();
        let Some(members) = self.with_sequence(|_, sequence| {
            sequence.set_terminating(true);
            sequence.live_ids_with(|role| matches!(role, MemberRole::Gesture))
        }) else {
            return;
        };
        for id in members {
            if Some(id) == capture {
                // Its own arena is about to see this `Up` through the capture
                // dispatch; feeding it twice would count the release twice.
                continue;
            }
            // An `Up` while the recognizer is not mid-drag resolves it to
            // `Failed` and clears `down_position` — no gesture is produced, so
            // this only tidies recognizer state.
            self.feed_member_arena(id, up_event, &mut *ops);
        }
        if let Some(entry) = self.pointers.get_mut(pointer) {
            entry.sequence = None;
        }
    }

    /// Stop every gesture arena that is still following `pointer`.
    ///
    /// A press ends at exactly one node — the captor — and the release is
    /// delivered only there, so any *other* arena that saw the `Down` is left
    /// following a contact that no longer exists. It happens on the ordinary
    /// path: an ancestor drag that wins a press a tapping descendant was
    /// holding takes the capture with it, and the descendant's arena never
    /// sees the `Up`.
    ///
    /// A stale entry is not inert. The next press reuses it instead of
    /// instantiating fresh recognizers, so a contact starts mid-gesture on
    /// state left over from the previous one. Ended rather than cancelled: the
    /// press was completed, not revoked, and the node's
    /// [`TapStreak`](crate::gesture::TapStreak) — which lives outside the live
    /// set precisely so it can outlive a contact — must survive, or touch
    /// double-tap would be impossible.
    pub(super) fn release_arenas_following(&mut self, pointer: crate::pointer::PointerId) {
        let owners: Vec<WidgetId> = self.gesture_owners.iter().copied().collect();
        for id in owners {
            if let Some(node) = self.arena.get_mut(id)
                && let Some(set) = node.handlers.gesture_arena.as_mut()
            {
                set.end(pointer);
            }
        }
    }

    /// Apply the arbitration acts a handler queued on its context.
    pub(super) fn apply_gesture_acts(
        &mut self,
        acts: &[crate::widget::GestureAct],
        source: WidgetId,
    ) {
        use crate::widget::GestureAct;

        let now = self.sequence_now();
        let mut claim = false;
        self.with_sequence(|_, sequence| {
            for act in acts {
                match act {
                    GestureAct::Claim => claim = true,
                    GestureAct::Reject => {
                        claim = false;
                        sequence.reject(source);
                    }
                    GestureAct::Hold => {
                        claim = false;
                        if !sequence.has_member(source) {
                            sequence.enrol(source, crate::gesture::MemberRole::Gesture);
                        }
                        sequence.hold(source, now);
                    }
                    GestureAct::Release => {
                        sequence.release_hold(source);
                    }
                }
            }
        });
        if claim {
            self.with_sequence(|_, sequence| {
                if !sequence.has_member(source) {
                    sequence.enrol(source, crate::gesture::MemberRole::Gesture);
                }
            });
            self.decide_sequence(source);
        }
    }

    /// A handler took the pointer with an explicit
    /// [`capture_pointer`](crate::widget::EventContext::capture_pointer).
    ///
    /// That is an arbitration act, not plumbing: the caller is enrolled as a
    /// [`MemberRole::RawDrag`](crate::gesture::MemberRole::RawDrag) competitor,
    /// and for a precise pointer with no eligible pan competitor the sequence
    /// is decided there and then — which is what makes the splitter handle, the
    /// dock resize handle and the table column grip win their own presses
    /// instead of losing them to an ancestor that happens to carry `on_drag`.
    pub(super) fn note_explicit_capture(&mut self, source: WidgetId) {
        use crate::gesture::MemberRole;

        let Some(decide) = self.with_sequence(|_, sequence| {
            if sequence.is_decided() {
                return false;
            }
            if !sequence.enrol(source, MemberRole::RawDrag) && !sequence.has_member(source) {
                return false;
            }
            // A precise pointer has no pan competitor by construction
            // (`GestureProfile::pan_slop` is `None` for a mouse), so this is a
            // decision at press. A contact defers to `drag_slop` instead, which
            // is what lets a scroller still beat it at `pan_slop`.
            !sequence.pointer().kind.is_direct() && !sequence.has_eligible_pan()
        }) else {
            return;
        };
        if decide {
            self.decide_sequence(source);
        }
    }

    /// A recognizer on `source` produced a gesture that owns the rest of the
    /// press (a drag or a swipe). That is the observable act of winning, so it
    /// decides the sequence — whether the recognizer was reached through the
    /// ordinary capture bubble or fed by the arbitration itself.
    pub(super) fn note_gesture_recognized(&mut self, source: WidgetId) {
        use crate::gesture::MemberRole;

        let claimed = self
            .with_sequence(|_, sequence| {
                if sequence.is_decided() {
                    return false;
                }
                if !sequence.has_member(source) {
                    sequence.enrol(source, MemberRole::Gesture);
                }
                sequence.has_member(source)
            })
            .unwrap_or(false);
        if claimed {
            self.decide_sequence(source);
        }
    }

    /// Whether `id`'s gesture recognizers must be kept out of this pointer
    /// event.
    ///
    /// A member that lost — because a peer won, because it withdrew, or because
    /// another member is holding — must not go on recognizing through the
    /// ordinary bubble. Only its *recognizers* are silenced: its
    /// `on_pointer_event`, `on_hover` and everything else still run, because
    /// losing an arbitration is not the same as being removed from the tree.
    ///
    /// Inert for every sequence nothing has decided, held or rejected — which
    /// is every plain mouse tap.
    ///
    /// Asked of the sequence belonging to the pointer being dispatched. The
    /// timer-driven path has no pointer being dispatched and asks
    /// [`sequence_blocks_arena_for`](Self::sequence_blocks_arena_for) instead,
    /// naming the contact whose gesture is in hand.
    pub(super) fn sequence_blocks_arena(&self, id: WidgetId) -> bool {
        match self.current_sequence() {
            Some(sequence) => Self::sequence_blocks_member(sequence, id, self.sequence_now()),
            None => false,
        }
    }

    /// [`sequence_blocks_arena`](Self::sequence_blocks_arena) for a named
    /// contact and a named instant, rather than for whatever sample is being
    /// dispatched.
    ///
    /// The timer path needs both: nothing is being dispatched during a tick, so
    /// `current_sequence` would answer about the wrong contact (or about none),
    /// and `sequence_now` would answer with the timestamp of the last sample
    /// dispatched — which for a contact resting on a control is its own press.
    pub(super) fn sequence_blocks_arena_for(
        &self,
        pointer: crate::pointer::PointerId,
        id: WidgetId,
        now: crate::pointer::EventTime,
    ) -> bool {
        match self.pointers.get(pointer).and_then(|e| e.sequence.as_ref()) {
            Some(sequence) => Self::sequence_blocks_member(sequence, id, now),
            None => false,
        }
    }

    /// The rule itself, shared by both doors above.
    fn sequence_blocks_member(
        sequence: &crate::gesture::PointerSequence,
        id: WidgetId,
        now: crate::pointer::EventTime,
    ) -> bool {
        use crate::gesture::MemberState;

        let Some(member) = sequence.members().iter().find(|m| m.id == id) else {
            return false;
        };
        match member.state {
            MemberState::Rejected => true,
            MemberState::Won => false,
            _ => {
                if let Some(winner) = sequence.winner() {
                    return winner != id;
                }
                // A hold freezes every peer: no one may win while a member is
                // still deciding.
                if sequence.is_held() && member.state != MemberState::Held {
                    return true;
                }
                // A member deferred by `DragActivation::AfterLongPress` cannot
                // win before its timer, and that has to hold on the ordinary
                // bubble too — otherwise the deferral would only bind the
                // arbitration's own walk.
                !member.is_eligible_at(now)
            }
        }
    }

    /// A preview handler answered `Handled` on a press. The **root-first**
    /// preview pass is the first step of the decision procedure, so this claims
    /// the sequence outright.
    pub(super) fn note_preview_claim(&mut self, source: WidgetId) {
        use crate::gesture::MemberRole;

        let claimed = self
            .with_sequence(|_, sequence| {
                if sequence.is_decided() {
                    return false;
                }
                sequence.enrol(source, MemberRole::RawPreview)
            })
            .unwrap_or(false);
        if claimed {
            self.decide_sequence(source);
        }
    }

    /// The winner of `pointer`'s sequence, if one has been decided.
    pub fn sequence_winner(&self, pointer: crate::pointer::PointerId) -> Option<WidgetId> {
        self.pointers
            .get(pointer)
            .and_then(|e| e.sequence.as_ref())
            .and_then(|s| s.winner())
    }

    /// Every competitor for `pointer`'s press, innermost first.
    ///
    /// The observable form of the cross-widget arbitration, and the successor
    /// to the old `armed_drag_observers()`: an app can assert that a press on a
    /// control inside a draggable container enrols no ancestor at all.
    pub fn sequence_members(
        &self,
        pointer: crate::pointer::PointerId,
    ) -> Vec<(
        WidgetId,
        crate::gesture::MemberRole,
        crate::gesture::MemberState,
    )> {
        self.pointers
            .get(pointer)
            .and_then(|e| e.sequence.as_ref())
            .map(|s| s.member_report())
            .unwrap_or_default()
    }

    /// The [`TouchAction`] frozen for the pointer this dispatch is serving —
    /// what [`EventContext::touch_action`](crate::widget::EventContext::touch_action)
    /// reports.
    pub(crate) fn current_frozen_touch_action(&self) -> TouchAction {
        self.current_sequence()
            .map(|s| s.touch_action())
            .unwrap_or(TouchAction::AUTO)
    }

    /// The [`TouchAction`] frozen for `pointer`'s press.
    pub fn sequence_touch_action(&self, pointer: crate::pointer::PointerId) -> TouchAction {
        self.pointers
            .get(pointer)
            .and_then(|e| e.sequence.as_ref())
            .map(|s| s.touch_action())
            .unwrap_or(TouchAction::AUTO)
    }

    /// Feed one raw pointer event to `id`'s gesture arena set WITHOUT firing
    /// its `on_pointer_event` or taking the implicit capture (another node
    /// already holds it). Returns `true` if a gesture was recognized, in which
    /// case it is dispatched so the `on_drag` handler's `start_drag` runs and
    /// `active_drag` takes over.
    fn feed_member_arena(
        &mut self,
        id: WidgetId,
        event: &WidgetEvent,
        ops: &mut dyn crate::window::WindowOps,
    ) -> bool {
        let localized = self.localize_event(id, event);
        let event = localized.as_ref().unwrap_or(event);
        let cx = self.recognizer_context(id);
        let raw = match event {
            WidgetEvent::PointerDown {
                position,
                button,
                modifiers,
                ..
            } => crate::gesture::RawPointerEvent::Down {
                position: *position,
                button: *button,
                modifiers: *modifiers,
                pointer: cx.pointer,
                time: cx.now,
            },
            WidgetEvent::PointerMove { position, .. } => crate::gesture::RawPointerEvent::Move {
                position: *position,
                pointer: cx.pointer,
                time: cx.now,
            },
            WidgetEvent::PointerUp {
                position,
                button,
                modifiers,
                ..
            } => crate::gesture::RawPointerEvent::Up {
                position: *position,
                button: *button,
                modifiers: *modifiers,
                pointer: cx.pointer,
                time: cx.now,
            },
            _ => return false,
        };
        let mut ctx = self.make_event_context(&mut *ops);
        let WidgetTree { arena, .. } = self;
        let recognized = if let Some(node) = arena.get_mut(id) {
            if let Some(arena_ref) = node.handlers.gesture_arena.as_mut() {
                if let Some(gesture) = arena_ref.process(&raw, &cx) {
                    Self::dispatch_recognized_gesture(node, gesture, &mut ctx);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };
        self.collect_from_ctx(ctx, id);
        recognized
    }

    /// The clock a newly promoted animation must be stamped with: the same one
    /// the scheduler will later be ticked against.
    ///
    /// The wall clock, unless [`advance_time`](Self::advance_time) has taken
    /// this tree onto its simulated one and not yet handed it back — see
    /// [`resume_real_time`](Self::resume_real_time), which is what returns the
    /// answer to the wall clock, rebasing the scheduler as it goes.
    ///
    /// Everything that reads a time on the animation axis has to read it here,
    /// promotion and tick alike, or the two drift apart and the drift *is* the
    /// elapsed time the animation is measured by. Promoting at
    /// `Instant::now()` while ticking at [`Self::sim_clock`] gave every
    /// animation a start in the scheduler's future and froze its progress
    /// completely, with no number of further ticks recovering it — a failure
    /// that reproduced as a function of machine load rather than of behaviour,
    /// green when the suite ran alone and red once the runner filled the cores
    /// and each test's wall-clock time stretched past the simulated time it was
    /// asking for. The overlay manager keeps its real and simulated timestamps
    /// apart for the same reason.
    pub(super) fn animation_clock(&self) -> std::time::Instant {
        if self.sim_time_frozen {
            self.sim_clock
        } else {
            std::time::Instant::now()
        }
    }

    /// Advance time-driven gesture recognizers (currently only
    /// [`crate::gesture::LongPressRecognizer`]) across every widget that
    /// has a gesture arena. Must be called by the event loop on each
    /// wake-up; otherwise long-press will never fire during an idle hold.
    ///
    /// When a recognizer transitions to `Recognized`, the corresponding
    /// handler on the owning widget is invoked with a fresh
    /// [`EventContext`], and any commands / overlay requests it emits are
    /// collected through the normal post-event path.
    /// Release every hold that has stood for `profile.max_hold`, across every
    /// contact — the time-driven half of
    /// [`tick_sequence_timers`](Self::tick_sequence_timers), lifted out so the
    /// gesture tick can run it too.
    ///
    /// Two things separate it from its move-driven sibling and are why it is a
    /// distinct function rather than a call to that one.
    ///
    /// * **It reads the caller's `now`, not the sample's.** `sequence_now`
    ///   answers with the timestamp of the last event *dispatched*, which
    ///   during a tick is the press — so calling `tick_sequence_timers` from
    ///   here would expire holds against the instant they were taken and never
    ///   expire anything at all.
    /// * **It is not scoped to the current pointer.** The rest of the sequence
    ///   machinery serves the contact being dispatched; a tick serves the whole
    ///   tree, and two fingers each holding on their own node must both be
    ///   released.
    ///
    /// The other two rules in `tick_sequence_timers` — the deferred member's
    /// withdrawal and the tap family's revocation — stay behind, because both
    /// are decided by the [`TapBoundary`](crate::gesture::TapBoundary)
    /// against where the pointer now is. A contact that has not moved cannot
    /// have left the boundary, so running them here could only ever repeat the
    /// answer the last move already gave.
    pub(super) fn expire_sequence_holds(&mut self, now: crate::pointer::EventTime) {
        let Self {
            pointers,
            effective_theme,
            ..
        } = self;
        for entry in pointers.iter_mut() {
            let Some(sequence) = entry.sequence.as_mut() else {
                continue;
            };
            let profile = effective_theme.input.profile(sequence.pointer().kind);
            sequence.expire_holds(now, profile);
        }
    }

    pub fn tick_gestures(&mut self, now: std::time::Instant) {
        let mut noop = crate::window::NoopWindowOps;
        self.tick_gestures_with_ops(now, &mut noop);
    }

    /// App-facing variant of [`tick_gestures`](Self::tick_gestures)
    /// that accepts a real [`WindowOps`](crate::window::WindowOps)
    /// sink so gesture-recognized handlers can call the multi-window
    /// API synchronously.
    pub fn tick_gestures_with_ops(
        &mut self,
        now: std::time::Instant,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        // Snapshot the gesture-owners set into the reusable scratch.
        // Previously this iterated every active widget; in practice
        // only a tiny fraction carry a gesture arena, so visiting the
        // rest was pure overhead.
        // `mem::take` lets the loop borrow `&mut self` for
        // `make_event_context` etc. without conflicting with the
        // scratch buffer; we put the storage back at the end.
        // The fling pump rides the same pass. It is an input deadline like a
        // long press, it is folded into the same `WaitUntil`
        // (`next_input_deadline`), and giving it its own call site would mean
        // every host had to learn a second one.
        self.tick_flings_with_ops(now, &mut *ops);
        // …and so does the press-feedback delay, for the same reason: it is an
        // input deadline folded into the same `WaitUntil`, and a finger resting
        // on a control produces no further samples to resolve it from.
        self.resolve_press_delays(self.event_time_for(now));
        // …and so does a standing hold's expiry, for the third time for the same
        // reason: a contact resting on a control produces no further samples,
        // and the framework's promise is that it stops trusting a holder after
        // `max_hold` — not that it stops trusting one after `max_hold` *and* a
        // move. See `expire_sequence_holds`.
        self.expire_sequence_holds(self.event_time_for(now));
        // …and so does the tree-owned long press, for the fourth time for the
        // same reason. It is not a recognizer: the affordances it reaches (a
        // context-menu factory the router walks up to, a tooltip on a node that
        // may be **disabled** and so has no arena at all) are the tree's, not a
        // widget's. See `super::touch_route`.
        self.resolve_touch_routes(self.event_time_for(now), &mut *ops);

        let mut ids = std::mem::take(&mut self.active_ids_scratch);
        ids.clear();
        ids.extend(
            self.gesture_owners
                .iter()
                .copied()
                .filter(|id| self.arena.is_active(*id)),
        );
        let now = self.event_time_for(now);
        for &id in &ids {
            let cx = self.recognizer_context(id);
            let cx = crate::gesture::RecognizerContext { now, ..cx };
            let gestures = match self.arena.get_mut(id) {
                Some(node) => node
                    .handlers
                    .gesture_arena
                    .as_mut()
                    .map(|arena| arena.tick(&cx))
                    .unwrap_or_default(),
                None => Vec::new(),
            };
            if gestures.is_empty() {
                continue;
            }

            // One entry per contact: two fingers holding on the same node both
            // long-press, and neither may be dropped.
            for (pointer, gesture) in gestures {
                // The arbitration binds the timer path exactly as it binds the
                // sample path: a member that has been rejected, or that a peer's
                // hold has frozen, does not get to deliver a gesture just
                // because its own timer came due. Asked per contact, since two
                // fingers on one node are two independent sequences.
                //
                // It comes out one step later than on the sample path, which
                // withholds the *feed* — a tick is not addressed to a member,
                // so the whole node's recognizers advance and the gesture is
                // then dropped rather than deferred. That is what `Rejected`
                // wants anyway; for the transient `Held` case it means a peer
                // silenced at the instant its timer ripened loses that gesture
                // rather than firing it late, and the hold that silenced it is
                // released in this same pass (`expire_sequence_holds`, above)
                // once it reaches `max_hold`.
                if self.sequence_blocks_arena_for(pointer, id, now) {
                    continue;
                }
                // One hold cannot mean two things. Where the hold is what arms
                // a grab — a reorderable row under a finger, whose drag member
                // was deferred to this very deadline — the row's own long press
                // does not also fire. A mouse is untouched: it enrols no pan
                // competitor, so nothing on its sequence is ever deferred.
                if matches!(gesture, crate::gesture::GestureEvent::LongPress(_))
                    && self.long_press_is_a_grab(pointer, id)
                {
                    continue;
                }
                // Install the contact this gesture belongs to for the length
                // of the dispatch. A hold is recognised here, by a deadline,
                // not by a sample — and `current_input` is saved-and-restored
                // around every dispatch (`run_one_dispatch`), so without this
                // it holds `InputSnapshot::default()` and every handler reached
                // from a hold is told it is serving **the mouse**, whatever the
                // device was.
                //
                // The snapshot is more than the device: it is the key
                // `make_event_context` builds the rest of the context from, so
                // every answer looked up by `current_pointer_id()` comes out
                // for the wrong pointer without it. What the context carries
                // in, and all of it: the device and the id the snapshot holds
                // outright (`EventContext::pointer_kind`, `pointer`), the
                // captor (`current_pointer_capture`), the frozen `TouchAction`
                // (`current_frozen_touch_action`, keyed through the contact's
                // sequence) and the press snapshot (`current_press_snapshot`).
                // What the context carries back out is a separate list, below.
                // The fling pump resolves its pointer from the table the same
                // way (`dispatch_chained_scroll`).
                let installed = self
                    .pointers
                    .get(pointer)
                    .map(|entry| entry.info)
                    .unwrap_or(self.current_input.pointer);
                let previous_input = std::mem::replace(
                    &mut self.current_input,
                    crate::pointer::InputSnapshot::for_recognized_gesture(installed),
                );
                let mut ctx = self.make_event_context(&mut *ops);
                if let Some(node) = self.arena.get_mut(id) {
                    Self::dispatch_recognized_gesture(node, gesture, &mut ctx);
                }
                // `collect_from_ctx` **after** the restore would be wrong:
                // two of the requests a handler can queue name no pointer and
                // are applied to whichever one `current_pointer_id()` answers
                // with at collection time — an unnamed
                // `capture_pointer()`/`release_pointer()`, and
                // `cancel_pointer_sequence()`. Collected after the restore,
                // a hold that captured would have captured the mouse and a
                // hold that cancelled would have cancelled it. The sample path
                // has the same order — `run_one_dispatch` restores only once
                // `dispatch_event_impl`, collection included, has returned.
                self.collect_from_ctx(ctx, id);
                self.current_input = previous_input;
            }
            self.arena.mark_needs_paint(id);
        }
        self.active_ids_scratch = ids;
    }

    /// Read an `Instant` handed in by the event loop on this tree's input
    /// timeline.
    ///
    /// Three answers, and not one of them is unconditionally the plain
    /// subtraction from the shared epoch (see
    /// [`input_clock`](Self::input_clock)) that a single axis would suggest:
    /// two ignore the caller's instant outright, and the third subtracts and
    /// then adds back whatever was advanced before the axis was handed back.
    ///
    /// * **Time is simulated.** The argument is discarded: the tree has one
    ///   now, and it is not the caller's — an `Instant` handed in by a real
    ///   event loop is on an axis this tree has stopped following.
    /// * **Time is real, under an anchored clock.** The caller's instant is
    ///   honoured, as the distance from the shared epoch, *plus* whatever was
    ///   advanced before the axis was handed back — a subtraction and then an
    ///   addition, because the axis runs `sim_input_offset` ahead of the clock
    ///   the caller read.
    /// * **A clock with no wall-clock anchor** — a
    ///   [`ManualClock`](crate::pointer::clock::ManualClock) in a test — also
    ///   ignores the argument and answers with its own reading, which is the
    ///   whole point of installing one.
    pub(super) fn event_time_for(&self, now: std::time::Instant) -> crate::pointer::EventTime {
        match self.input_clock.epoch() {
            // While the axis is frozen the tree has one now, and it is not the
            // caller's: an `Instant` handed in by a real event loop is on an
            // axis this tree has stopped following for the length of the
            // advance.
            Some(_) if self.sim_input_origin.is_some() => self.input_now(),
            // Otherwise the caller's instant is honoured — two real events
            // milliseconds apart must not be stamped the same moment — shifted
            // by whatever was advanced before the axis was handed back.
            Some(epoch) => {
                crate::pointer::EventTime::from_duration(now.saturating_duration_since(epoch))
                    + self.sim_input_offset
            }
            None => self.input_now(),
        }
    }

    /// Turn an input-timeline deadline back into an `Instant` for the event
    /// loop, which schedules in wall-clock terms.
    pub(super) fn instant_for(&self, time: crate::pointer::EventTime) -> std::time::Instant {
        match self.input_clock.epoch() {
            // Inverse of the frozen branch of `event_time_for`: a deadline on
            // the virtual axis is reported against the virtual clock, so what
            // comes back is comparable with `simulated_now()` and not with a
            // wall clock this tree is not following for the length of the
            // advance.
            Some(_) if self.sim_input_origin.is_some() => {
                self.sim_clock + time.saturating_since(self.input_now())
            }
            // Inverse of the offset branch. Subtracting what was advanced is
            // what keeps this in the future: the deadline was stamped on an
            // axis running `sim_input_offset` ahead of the clock the event loop
            // schedules against, and reporting it unshifted would hand back an
            // instant already past — a `WaitUntil` that can never ripen and a
            // loop that spins on it.
            Some(epoch) => epoch + time.as_duration().saturating_sub(self.sim_input_offset),
            // An unanchored clock has no wall-clock answer; the best available
            // one is "as far from now as it is from the clock's reading".
            None => std::time::Instant::now() + time.saturating_since(self.input_now()),
        }
    }

    /// Earliest wall-clock deadline at which any active gesture arena
    /// needs [`WidgetTree::tick_gestures`] called — typically a pending
    /// long-press timeout. Returns `None` when no recognizer is waiting.
    pub fn next_gesture_deadline(&self) -> Option<std::time::Instant> {
        // Iterate just the widgets that actually carry a gesture arena.
        // `filter` for `is_active` skips dormant entries that may still
        // be in the set after a hide-without-detach.
        self.gesture_owners
            .iter()
            .copied()
            .filter(|id| self.arena.is_active(*id))
            .filter_map(|id| self.arena.get(id))
            .filter_map(|node| node.handlers.gesture_arena.as_ref())
            .filter_map(|arena| arena.next_deadline())
            .min()
            .map(|deadline| self.instant_for(deadline))
    }

    /// The earliest instant at which a standing hold reaches its
    /// `max_hold` and [`expire_sequence_holds`](Self::expire_sequence_holds)
    /// has work.
    ///
    /// Folded into [`next_input_deadline`](Self::next_input_deadline) beside
    /// the gesture, fling and press-feedback terms. A deadline the tick can
    /// serve but nothing reports is a wake the event loop never takes, which
    /// leaves the hold standing exactly as long as it did before the tick knew
    /// how to release it.
    pub(super) fn next_sequence_hold_deadline(&self) -> Option<crate::pointer::EventTime> {
        self.pointers
            .iter()
            .filter_map(|entry| entry.sequence.as_ref())
            .filter_map(|sequence| {
                let profile = self.effective_theme.input.profile(sequence.pointer().kind);
                sequence.next_hold_deadline(profile)
            })
            .min()
    }

    /// The [`TouchAction`] permitted for `target`: every node's own
    /// declaration from the root down to `target` (inclusive), intersected.
    /// An ancestor's `NONE` wins no matter what a descendant declares —
    /// intersection is absorbing at `NONE` (see
    /// `crate::pointer::touch_action`), so this needs no early exit to get
    /// that right; it just folds the whole chain.
    ///
    /// One of the two path folds the arbitration reads: `begin_sequence` asks
    /// it what a press may do, and `feed_pinch` asks it whether a subtree
    /// admits a two-contact pinch.
    pub(crate) fn effective_touch_action(&self, target: WidgetId) -> TouchAction {
        let mut chain = Vec::new();
        let mut current = Some(target);
        while let Some(id) = current {
            chain.push(id);
            current = self.arena.parent(id);
        }
        // `chain` is target..=root (innermost first); fold root-to-target so
        // the read matches the CSS `touch-action` model this mirrors — an
        // ancestor's declaration is applied before a descendant's narrows it
        // further. `intersect` is commutative and associative, so the fold
        // order can never change the *answer*, only which step "loses" reads
        // as the natural one.
        chain
            .iter()
            .rev()
            .map(|&id| {
                self.arena
                    .get(id)
                    .map(|n| n.touch_action)
                    .unwrap_or(TouchAction::AUTO)
            })
            .fold(TouchAction::AUTO, TouchAction::intersect)
    }

    /// Every [`PanClaim`] from `target` up to the root, **innermost first**
    /// — the order a boundary pan chains along (a nested scrollable hits its
    /// edge and hands off to its container), so it is normative. A claimant
    /// is excluded entirely — never narrowed — when `allowed` forbids any
    /// axis it declares.
    ///
    /// The second of the two path folds the arbitration reads, and the chain
    /// a synthesised pan is delivered along — see `widget_tree::pan_arbiter`.
    pub(crate) fn pan_candidates(
        &self,
        target: WidgetId,
        allowed: TouchAction,
    ) -> Vec<(WidgetId, PanClaim)> {
        let mut result = Vec::new();
        let mut current = Some(target);
        while let Some(id) = current {
            if let Some(claim) = self.arena.get(id).and_then(|n| n.pan_claim) {
                let x_ok = !claim.axes.contains(Axis::X) || allowed.allows_pan_x();
                let y_ok = !claim.axes.contains(Axis::Y) || allowed.allows_pan_y();
                if x_ok && y_ok {
                    result.push((id, claim));
                }
            }
            current = self.arena.parent(id);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::FillWidget;
    use crate::widget_builder::WidgetBuilder;

    #[test]
    fn destroy_subtree_clears_dangling_pointer_capture() {
        use crate::event::{EventResponse, Modifiers, PointerButton};
        use crate::test_widgets::StackWidget;

        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().on_pointer_event(|event, ctx| {
            if matches!(event, WidgetEvent::PointerDown { .. }) {
                ctx.capture_pointer();
            }
            EventResponse::Ignored
        }));
        let parent = tree.add(StackWidget::new().child(child));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // A press inside the child captures the pointer to it.
        tree.dispatch_event(WidgetEvent::pointer_down(
            Point::new(50.0, 25.0),
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        assert_eq!(
            tree.pointer_captured_by(),
            Some(child),
            "PointerDown handler should have captured the pointer"
        );

        // Tearing down the capturing subtree (e.g. mid-gesture rebuild) must
        // release the capture eagerly rather than leaving a dangling id that
        // swallows every later Move/Up until the next layout pass heals it.
        tree.destroy_subtree(parent);
        assert_eq!(
            tree.pointer_captured_by(),
            None,
            "destroy_subtree must clear a capture anchored at a destroyed widget"
        );
    }
}

/// The two path folds `effective_touch_action` / `pan_candidates` declare
/// for the arbitration package (P08) — pure plumbing, exercised here in
/// isolation since nothing dispatches through them yet.
#[cfg(test)]
mod touch_action_tests {
    use super::*;
    use crate::pointer::touch_action::PanAxes;
    use crate::test_widgets::FillWidget;
    use crate::widget_builder::WidgetBuilder;

    /// A 20-deep chain, root to target, where one mid-level node declares
    /// `PAN_Y` and another declares `PINCH_ZOOM`. Neither permission is
    /// shared by the other, so the intersection collapses to `NONE` — the
    /// clearest possible demonstration that the fold really intersects the
    /// *whole* chain rather than reading only the nearest declaration.
    #[test]
    fn effective_touch_action_intersects_the_whole_root_to_target_chain() {
        let mut tree = WidgetTree::new();
        let mut chain = vec![tree.add(FillWidget::new())]; // depth 0: the root
        for depth in 1..20usize {
            let parent = *chain.last().expect("root was pushed");
            let id = if depth == 5 {
                tree.add_child(parent, FillWidget::new().touch_action(TouchAction::PAN_Y))
            } else if depth == 12 {
                tree.add_child(
                    parent,
                    FillWidget::new().touch_action(TouchAction::PINCH_ZOOM),
                )
            } else {
                tree.add_child(parent, FillWidget::new())
            };
            chain.push(id);
        }
        assert_eq!(chain.len(), 20, "the path must be 20 nodes deep");
        let target = *chain.last().expect("chain is non-empty");
        tree.layout(SizeProposal::exact(50.0, 50.0));

        assert_eq!(
            tree.effective_touch_action(target),
            TouchAction::NONE,
            "PAN_Y at depth 5 and PINCH_ZOOM at depth 12 share no permission"
        );

        // Every node above depth 5 (inclusive) is untouched: the plain
        // `AUTO` prefix intersects down to exactly `PAN_Y`.
        assert_eq!(tree.effective_touch_action(chain[5]), TouchAction::PAN_Y);
    }

    /// `pan_candidates` walks target-to-root (innermost first) and drops a
    /// claimant whose declared axes the allowed action forbids, rather than
    /// narrowing it.
    #[test]
    fn pan_candidates_orders_innermost_first_and_filters_by_allowed_axes() {
        let mut tree = WidgetTree::new();
        // root claims X; an unclaimed node in between; target (innermost)
        // claims Y.
        let root = tree.add(FillWidget::new().scroll_container(PanAxes::X));
        let mid = tree.add_child(root, FillWidget::new());
        let target = tree.add_child(mid, FillWidget::new().scroll_container(PanAxes::Y));
        tree.layout(SizeProposal::exact(50.0, 50.0));

        let x_claim = PanClaim {
            axes: PanAxes::X,
            devices: teksilo_tokens::PointerKindMask::DIRECT,
            kinetic: true,
        };
        let y_claim = PanClaim {
            axes: PanAxes::Y,
            devices: teksilo_tokens::PointerKindMask::DIRECT,
            kinetic: true,
        };

        // Both axes allowed: both claims survive, innermost (target) first.
        assert_eq!(
            tree.pan_candidates(target, TouchAction::PAN),
            vec![(target, y_claim), (root, x_claim)]
        );

        // Only PAN_X allowed: target's Y-axis claim is forbidden and
        // excluded outright; root's X-axis claim still survives.
        assert_eq!(
            tree.pan_candidates(target, TouchAction::PAN_X),
            vec![(root, x_claim)]
        );

        // Only PAN_Y allowed: the reverse — root's claim is excluded,
        // target's survives.
        assert_eq!(
            tree.pan_candidates(target, TouchAction::PAN_Y),
            vec![(target, y_claim)]
        );
    }
}

/// The one-clock rule: the input timeline and the tree's simulated clock are
/// one axis, seeded from one epoch.
#[cfg(test)]
mod clock_tests {
    use super::*;
    use crate::pointer::EventTime;
    use crate::pointer::clock::ManualClock;

    /// The whole point of taking the epoch as a parameter rather than
    /// capturing it: `EventTime::ZERO` and the tree's simulated clock name the
    /// *same* instant, so one `advance_time` moves gesture deadlines and
    /// animations against the same origin.
    ///
    /// If this ever fails, the two timelines have drifted apart and a test that
    /// advances one has silently stopped advancing the other.
    #[test]
    fn the_input_clock_shares_the_trees_epoch() {
        let tree = WidgetTree::new();
        assert_eq!(
            tree.input_clock().epoch(),
            Some(tree.simulated_now()),
            "the input clock must be anchored at the instant sim_clock starts from"
        );
    }

    /// …and stays one axis as simulated time moves: the offset between the
    /// simulated clock and the epoch is exactly what was advanced.
    #[test]
    fn advancing_simulated_time_moves_along_the_input_axis() {
        let mut tree = WidgetTree::new();
        let epoch = tree
            .input_clock()
            .epoch()
            .expect("the default input clock is monotonic");

        tree.advance_time(std::time::Duration::from_millis(400));
        assert_eq!(
            tree.simulated_now().duration_since(epoch),
            std::time::Duration::from_millis(400)
        );

        tree.advance_time(std::time::Duration::from_millis(100));
        assert_eq!(
            tree.simulated_now().duration_since(epoch),
            std::time::Duration::from_millis(500)
        );
    }

    /// The simulated input axis continues from where the wall clock left it,
    /// rather than restarting at the simulated clock's own offset.
    ///
    /// A stamp taken before the switch would otherwise land in the virtual
    /// *future*: `sim_clock` only moves when it is advanced, so on a tree that
    /// has been alive for 50 ms it still reads the epoch while every sample
    /// dispatched so far is stamped 50 ms. Every interval measured from one of
    /// those is then clamped to zero — a hold that can never elapse, a coast
    /// that never starts.
    #[test]
    fn the_simulated_input_axis_continues_from_the_wall_clock() {
        let mut tree = WidgetTree::new();
        // Time the tree spent on the wall clock before anything simulated it —
        // in a real suite this is however long the test took to get here.
        std::thread::sleep(std::time::Duration::from_millis(50));
        let stamped_before_the_switch = tree.input_now();
        assert!(
            stamped_before_the_switch >= EventTime::from_millis(50),
            "the default clock is the wall clock until told otherwise: {stamped_before_the_switch:?}"
        );

        tree.advance_time(std::time::Duration::from_millis(20));
        let after_one = tree.input_now();
        assert!(
            after_one >= stamped_before_the_switch + std::time::Duration::from_millis(20),
            "the axis carries on from the reading it had, not from the epoch: \
             {stamped_before_the_switch:?} -> {after_one:?}"
        );

        // …and from then on it moves by exactly what is advanced, and by
        // nothing else — however long this test itself takes.
        std::thread::sleep(std::time::Duration::from_millis(20));
        tree.advance_time(std::time::Duration::from_millis(30));
        assert_eq!(
            tree.input_now(),
            after_one + std::time::Duration::from_millis(30),
            "a simulated tree's input timeline answers to the clock alone"
        );
    }

    /// A test can take the input timeline over entirely.
    #[test]
    fn a_manual_clock_replaces_the_default() {
        let mut tree = WidgetTree::new();
        let manual = std::rc::Rc::new(ManualClock::new(EventTime::from_millis(30)));
        tree.set_input_clock(manual.clone());

        assert_eq!(tree.input_now(), EventTime::from_millis(30));
        manual.advance(std::time::Duration::from_millis(70));
        assert_eq!(tree.input_now(), EventTime::from_millis(100));
        // Reading does not move it — a manual clock is only moved by its owner.
        assert_eq!(tree.input_now(), EventTime::from_millis(100));
    }

    /// The default clock actually reads the wall clock, so a real window's
    /// gestures advance without anyone ticking anything.
    #[test]
    fn the_default_clock_is_monotonic() {
        let tree = WidgetTree::new();
        let a = tree.input_now();
        let b = tree.input_now();
        assert!(b >= a);
    }

    /// Handing the axis back never steps it backwards.
    ///
    /// The whole reason the hand-back is not just "drop the origin": a frozen
    /// axis that has been advanced reads *ahead* of the raw clock, and every
    /// stamp already issued sits at that reading. Going back to the raw clock
    /// would re-issue times that have already been handed out — a velocity
    /// tracker fitting a negative interval, a tap streak whose second tap is
    /// older than its first, a hold that un-elapses.
    #[test]
    fn handing_the_axis_back_never_steps_it_backwards() {
        let mut tree = WidgetTree::new();
        tree.advance_time(std::time::Duration::from_millis(500));
        let frozen = tree.input_now();

        tree.resume_real_time();
        let resumed = tree.input_now();

        assert!(
            resumed >= frozen,
            "the axis must carry the advance forward, not discard it: \
             {frozen:?} -> {resumed:?}"
        );
        // …and it keeps every bit of what was advanced, rather than trading it
        // for however little wall time the test itself took.
        assert!(
            resumed >= EventTime::from_millis(500),
            "500 ms was advanced and must still be on the axis: {resumed:?}"
        );
    }

    /// After the hand-back, real events are stamped from the real clock again:
    /// two of them separated by real time are two distinct moments.
    ///
    /// This is the whole point of the hand-back. While the axis is frozen every
    /// dispatch reads one instant, which is exactly right for a test driving
    /// the clock itself and exactly wrong for a live window: a bridge that
    /// advanced the clock once would leave every subsequent human keystroke,
    /// tap and drag stamped the same moment, and no gesture decided by time
    /// could ever be recognized again.
    #[test]
    fn after_the_hand_back_real_events_get_distinct_and_later_times() {
        use crate::test_widgets::FillWidget;
        use crate::widget_builder::WidgetBuilder;
        use std::cell::RefCell;
        use std::rc::Rc;

        let seen: Rc<RefCell<Vec<EventTime>>> = Rc::new(RefCell::new(Vec::new()));
        let log = seen.clone();
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().on_pointer_event(move |_event, ctx| {
            log.borrow_mut().push(ctx.pointer().time);
            crate::event::EventResponse::Ignored
        }));
        tree.layout(SizeProposal::exact(100.0, 100.0));

        tree.pointer_move(Point::new(10.0, 10.0));
        tree.advance_time(std::time::Duration::from_millis(200));
        tree.resume_real_time();

        tree.pointer_move(Point::new(20.0, 20.0));
        std::thread::sleep(std::time::Duration::from_millis(5));
        tree.pointer_move(Point::new(30.0, 30.0));

        let seen = seen.borrow();
        assert_eq!(seen.len(), 3, "three moves reached the widget: {seen:?}");
        assert!(
            seen[1] >= seen[0] + std::time::Duration::from_millis(200),
            "an event after the advance is at least the advance later: {seen:?}"
        );
        assert!(
            seen[2] > seen[1],
            "two real events 5 ms apart are two moments, not one: {seen:?}"
        );
    }

    /// …and the next advance freezes it again.
    ///
    /// The hand-back is not a latch in the other direction: a bridge running a
    /// second operation must get the same determinism the first one did, and a
    /// test's helpers (which enter simulated mode on every sample) must keep
    /// stamping two un-advanced samples the same instant.
    #[test]
    fn the_freeze_comes_back_after_a_hand_back() {
        let mut tree = WidgetTree::new();
        tree.advance_time(std::time::Duration::from_millis(100));
        tree.resume_real_time();

        tree.advance_time(std::time::Duration::from_millis(100));
        let a = tree.input_now();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let b = tree.input_now();
        assert_eq!(a, b, "re-frozen: the wall clock stopped moving the axis");
    }

    /// Every hand-back re-measures the offset against its own readings; none
    /// of them adds to what the last one measured.
    ///
    /// The live bridge hands the axis back after **every** operation, so this
    /// is its ordinary path rather than an edge case — and accumulating
    /// instead of assigning compounds: each cycle would carry the previous
    /// offset into the frozen reading *and* add the previous offset again, so
    /// the axis would run `2ⁿ − 1` advances ahead after `n` of them. Four
    /// 50 ms cycles is 200 ms of advance and would be reported as 750 ms, and
    /// every duration measured from a stamp taken before the run — a hold, a
    /// tap streak, a fling's velocity window — would be wrong by the
    /// difference.
    ///
    /// **This is the only guard on the offset's magnitude.** Its companion
    /// `a_deadline_armed_after_the_hand_back_is_in_the_future` is insensitive
    /// to it by construction — the offset cancels between `event_time_for` and
    /// `instant_for`, so that test holds for a wrong offset as readily as for a
    /// right one — and nothing else asserts a number. So the
    /// ceiling below is expressed **per cycle**: the allowance scales with the
    /// work done, so the error it admits per hand-back stays at
    /// `SLACK_PER_CYCLE` whatever `CYCLES` is. A single absolute ceiling
    /// instead divides by the cycle count — slack at a handful of cycles, and
    /// firing on the loop's own wall-clock noise at a hundred.
    #[test]
    fn repeated_hand_backs_re_measure_the_offset_rather_than_accumulating_it() {
        const CYCLES: u32 = 4;
        const PER_CYCLE: std::time::Duration = std::time::Duration::from_millis(50);
        // What one hand-back may cost beyond what it advanced: the wall clock
        // moves while the loop runs, and the loop's own work is not free.
        const SLACK_PER_CYCLE: std::time::Duration = std::time::Duration::from_millis(10);
        let advanced = PER_CYCLE * CYCLES;

        let mut tree = WidgetTree::new();
        let before = tree.input_now();
        for _ in 0..CYCLES {
            tree.advance_time(PER_CYCLE);
            tree.resume_real_time();
        }
        let after = tree.input_now();
        let gained = after.saturating_since(before);

        assert!(
            gained >= advanced,
            "every advance must still be on the axis: {gained:?} < {advanced:?}"
        );
        // The wall clock also moved while the loop ran, so the axis is allowed
        // to have gained a little more than was advanced — but only a little,
        // and the allowance is per hand-back rather than for the run.
        // Accumulating would put it at 750 ms, three and a half times over.
        assert!(
            gained <= advanced + SLACK_PER_CYCLE * CYCLES,
            "the axis gained {gained:?} for {advanced:?} of advancing over \
             {CYCLES} hand-backs — the offset is compounding across them"
        );
    }

    /// An animation in flight when the tree is put on the simulated clock
    /// keeps the phase it had, and goes on progressing.
    ///
    /// The scheduler stores absolute instants, and the simulated clock reads
    /// the tree's epoch plus whatever has been advanced — on a live window,
    /// far behind the wall clock the animation was stamped against. Measuring
    /// it there without rebasing clamps its elapsed time to roughly zero and
    /// it never moves again, however many frames the operation advances.
    #[test]
    fn an_animation_in_flight_keeps_its_phase_when_time_is_taken_over() {
        use crate::signal::Signal;
        use crate::test_widgets::FillWidget;
        use std::time::Duration;

        let mut tree = WidgetTree::new();
        let owner = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 100.0));

        // Long enough that a loaded CI runner overshooting the sleep below by
        // a few hundred milliseconds — a macOS runner has been seen to take
        // 250 ms over a 100 ms sleep — cannot run it to completion before the
        // take-over, which would leave nothing for the advance to move.
        const DURATION: Duration = Duration::from_millis(4000);

        let value = Signal::<f32>::new_animated(0.0);
        tree.register_animated_signal(&value, owner);
        value.animate_to(1.0, DURATION, teksilo_tokens::Easing::Linear);

        // Promote and age it on the wall clock, exactly as a live window does.
        tree.layout(SizeProposal::exact(100.0, 100.0));
        std::thread::sleep(Duration::from_millis(100));
        tree.layout(SizeProposal::exact(100.0, 100.0));
        let on_the_wall_clock = value.get();
        // 100 ms of 4000 is 0.025; the sleep never undershoots, and the bound
        // above allows the runner nearly two seconds of overshoot.
        assert!(
            (0.02..0.5).contains(&on_the_wall_clock),
            "in flight after 100 ms of {DURATION:?}: {on_the_wall_clock}"
        );

        // Now an automation operation takes time over and advances a further
        // 1000 ms. The animation must have moved on by exactly that share of
        // its duration — not stuck where the wall clock left it, and not
        // restarted from the simulated clock's own reading.
        tree.advance_time(Duration::from_millis(1000));
        let simulated = value.get();
        let moved = simulated - on_the_wall_clock;
        // 1000 ms of 4000 is 0.25. Whatever wall-clock time passed between the
        // reading above and the take-over is in there too, so the bound is
        // loose above — by 400 ms — and tight below.
        assert!(
            (0.245..0.35).contains(&moved),
            "the advance keeps the phase and adds its own 1000 ms of \
             {DURATION:?}: {on_the_wall_clock} -> {simulated}"
        );
    }

    /// …and once time is handed back, the wall clock goes on driving it from
    /// where the advance left it — neither frozen nor snapped to its end.
    ///
    /// The two failures this rules out are the two halves of getting the axis
    /// wrong on a *live* attached window. Ticking at the simulated clock a
    /// live tree no longer advances freezes every animation outright. Ticking
    /// at the wall clock against a start stamped on the simulated one hands
    /// the animation an elapsed time of the tree's whole age and completes it
    /// on the first real frame.
    #[test]
    fn an_animation_goes_on_progressing_after_the_hand_back() {
        use crate::signal::Signal;
        use crate::test_widgets::FillWidget;
        use std::time::Duration;

        let mut tree = WidgetTree::new();
        let owner = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 100.0));

        // The tree spends real time alive before anything simulates it, and
        // more of it than will be advanced. That is the live condition, and it
        // is what makes the *second* assertion below discriminating: with the
        // wall clock ahead of the simulated one, an un-rebased hand-back hands
        // the animation an elapsed time longer than its whole duration.
        std::thread::sleep(Duration::from_millis(300));

        let value = Signal::<f32>::new_animated(0.0);
        tree.register_animated_signal(&value, owner);
        // 2 s rather than something short: the 100 ms slept after the
        // hand-back is a floor, not a figure, and a loaded runner has
        // overshot it by 150 ms — the "not snapped to the end" bound below
        // must hold through that.
        value.animate_to(
            1.0,
            Duration::from_millis(2000),
            teksilo_tokens::Easing::Linear,
        );

        // The operation promotes it and advances it a twentieth of the way.
        tree.advance_time(Duration::from_millis(100));
        let at_hand_back = value.get();
        assert!(
            (0.04..0.06).contains(&at_hand_back),
            "a twentieth through after 100 ms of 2000: {at_hand_back}"
        );

        // The operation ends and the window goes back to painting frames.
        tree.resume_real_time();
        std::thread::sleep(Duration::from_millis(100));
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let after = value.get();
        // At least 100 ms of 2000 (0.05) moved it; the sleep only overshoots.
        assert!(
            after > at_hand_back + 0.045,
            "frozen: 100 ms of real time moved it from {at_hand_back} to {after}"
        );
        assert!(
            after < 0.95,
            "snapped to the end: 100 ms of 2000 took it from {at_hand_back} to {after}"
        );
    }

    /// A `max_duration` cap measures the animation's own age across the
    /// hand-back, not the tree's.
    ///
    /// `started_at` is the only stored instant the cap reads, and until this
    /// test nothing asserted that the rebase shifts it: no production caller
    /// sets `max_duration` at all, so the branch is entered only from the
    /// public
    /// [`Signal::try_animate_with_options`](crate::signal::Signal::try_animate_with_options)
    /// and from the scheduler's own unit tests, which never change axis.
    /// Left behind on the abandoned axis, `started_at` makes the cap measure
    /// the tree's whole wall-clock age instead of the animation's own elapsed
    /// time, and the first real frame after the hand-back retires the
    /// animation outright — the same snap the rebase exists to prevent,
    /// arriving through a different door.
    ///
    /// The cap is deliberately smaller than the tree's age at the final layout
    /// and larger than the animation's own elapsed time there, so the two ways
    /// of measuring it disagree about whether it has been reached.
    #[test]
    fn a_capped_animation_survives_the_hand_back() {
        use crate::animation::AnimationRequest;
        use crate::signal::Signal;
        use crate::test_widgets::FillWidget;
        use std::time::Duration;

        // The animation lives 200 ms before the final reading — 100 simulated,
        // 100 real — and the tree at least 600 ms. The cap sits between the
        // two with room on both sides: a loaded runner overshoots a sleep by
        // 100 ms or more, and every overshoot ages the tree further but the
        // animation only through the real half.
        const CAP: Duration = Duration::from_millis(400);
        const DURATION: Duration = Duration::from_millis(2000);

        let mut tree = WidgetTree::new();
        let owner = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 100.0));

        // Age the tree past the cap before the animation is armed at all.
        std::thread::sleep(Duration::from_millis(500));

        let value = Signal::<f32>::new_animated(0.0);
        tree.register_animated_signal(&value, owner);
        value
            .try_animate_with_options(AnimationRequest {
                target: 1.0,
                duration: DURATION,
                easing: teksilo_tokens::Easing::Linear,
                max_duration: Some(CAP),
                ..AnimationRequest::default()
            })
            .expect("an animated signal accepts a request");

        // 100 ms simulated, then 100 ms real: 200 ms of the animation's own
        // life, against a tree already older than the cap.
        tree.advance_time(Duration::from_millis(100));
        let at_hand_back = value.get();
        tree.resume_real_time();
        std::thread::sleep(Duration::from_millis(100));
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let after = value.get();
        assert!(
            tree.has_active_animations(),
            "a {CAP:?} cap retired a {DURATION:?} animation 200 ms in: \
             {at_hand_back} -> {after}"
        );
        assert!(
            after > at_hand_back,
            "the animation must go on progressing: {at_hand_back} -> {after}"
        );
    }

    /// An animation paused across a hand-back resumes from where it was
    /// paused, rather than being driven backwards by the gap between the axes.
    ///
    /// The pause mark is the scheduler's one instant that is not per-animation,
    /// and until this test nothing asserted that the rebase shifts it: on
    /// reactivate the scheduler moves each `start_time` forward by
    /// `now - paused_at`, so a mark left behind on the abandoned axis measures
    /// the whole gap between the axes and puts the start ahead of the reading
    /// that follows it — the animation's elapsed time collapses, and it
    /// replays from near zero once the wall clock reaches the new start.
    ///
    /// The wall clock is deliberately left further ahead of the simulated one
    /// than the real time that elapses after the hand-back, which is exactly
    /// the condition under which the collapse leaves the animation *behind*
    /// where it was paused rather than merely slowed.
    #[test]
    fn an_animation_paused_across_the_hand_back_resumes_forwards() {
        use crate::signal::Signal;
        use crate::test_widgets::FillWidget;
        use std::time::Duration;

        let mut tree = WidgetTree::new();
        let owner = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 100.0));
        std::thread::sleep(Duration::from_millis(300));

        let value = Signal::<f32>::new_animated(0.0);
        tree.register_animated_signal(&value, owner);
        value.animate_to(
            1.0,
            Duration::from_millis(1000),
            teksilo_tokens::Easing::Linear,
        );

        tree.advance_time(Duration::from_millis(100));
        let at_pause = value.get();
        assert!(
            (0.05..0.2).contains(&at_pause),
            "a tenth through after 100 ms of 1000: {at_pause}"
        );

        // The window loses focus while the operation still owns the clock, and
        // regains it after the hand-back — the ordering that makes the pause
        // mark and the reading it is subtracted from land on different axes.
        tree.set_window_active(false);
        tree.resume_real_time();
        tree.set_window_active(true);

        std::thread::sleep(Duration::from_millis(150));
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let after = value.get();
        assert!(
            after > at_pause,
            "frozen or driven backwards across a paused hand-back: \
             {at_pause} -> {after}"
        );
    }

    /// …and the same, with the gap between the two axes the other way round:
    /// the pause mark is stamped on the axis the scheduler is measured
    /// against, not on the wall clock.
    ///
    /// The twin of the test above, and it cannot be merged with it. Which of
    /// the two mistakes is observable depends on the *sign* of the gap at the
    /// moment of the pause: a mark the rebase left behind only yields a
    /// spurious offset while the wall clock leads, and a mark taken from the
    /// wall clock instead of the animation clock only survives the
    /// subtraction — rather than flooring at zero — while the simulated clock
    /// leads. Each test rules out the sign the other needs, so each covers one
    /// mistake.
    ///
    /// Here the simulated clock is advanced past a tree milliseconds old, so
    /// it leads — and a wall-clock mark, shifted by the hand-back's rebase
    /// like the animation-axis instant it is not, comes out a whole advance
    /// early and is subtracted from the reading on reactivate as if the window
    /// had been dark for that long.
    #[test]
    fn a_pause_mark_is_stamped_on_the_animation_axis() {
        use crate::signal::Signal;
        use crate::test_widgets::FillWidget;
        use std::time::Duration;

        let mut tree = WidgetTree::new();
        let owner = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let value = Signal::<f32>::new_animated(0.0);
        tree.register_animated_signal(&value, owner);
        value.animate_to(
            1.0,
            Duration::from_millis(5000),
            teksilo_tokens::Easing::Linear,
        );

        // A second of simulated time on a tree milliseconds old: the advance,
        // not a sleep, is what separates the axes, and it separates them the
        // other way.
        tree.advance_time(Duration::from_millis(1000));
        let at_pause = value.get();
        assert!(
            (0.15..0.25).contains(&at_pause),
            "a fifth through after 1000 ms of 5000: {at_pause}"
        );

        tree.set_window_active(false);
        tree.resume_real_time();
        tree.set_window_active(true);

        std::thread::sleep(Duration::from_millis(150));
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let after = value.get();
        assert!(
            after > at_pause,
            "the deactivation cost the animation its phase: {at_pause} -> {after}"
        );
    }

    /// A gesture the **timer** recognised is dispatched under its own contact.
    ///
    /// The whole class of defect this pins: a hold is not a sample, so nothing
    /// on the sample path installs a snapshot for it, and `current_input` is
    /// saved-and-restored around every dispatch — so a handler reached from a
    /// hold used to be told, unconditionally, that it was serving the mouse.
    /// It was measured that way (a probe on a real `long_press_at(Touch, ..)`
    /// printed `Mouse`), and it cost the first host of the touch-text contract
    /// a duplicate guard: `TouchSelection::on_long_press` refused every finger.
    ///
    /// This asserts the whole of what the context carries **in**: the device,
    /// the id, the captor, the frozen `TouchAction` and the press snapshot.
    /// What a handler asks the tree *for* from inside a hold — a capture, a
    /// cancel — travels the other way and is asserted by
    /// `a_hold_captures_and_cancels_the_contact_that_held`.
    ///
    /// The mouse half is not decoration: it is what proves the fix installs the
    /// *holding contact* rather than hard-coding a finger.
    #[test]
    fn a_hold_is_dispatched_under_the_contact_that_held() {
        use crate::TouchAction;
        use crate::test_widgets::FillWidget;
        use crate::widget_builder::WidgetBuilder;

        /// Every answer the context is built from the installed snapshot.
        #[derive(Debug, Clone, Copy)]
        struct Answers {
            kind: teksilo_tokens::PointerKind,
            id: crate::pointer::PointerId,
            captor: Option<WidgetId>,
            touch_action: TouchAction,
            press_inside: bool,
        }

        for kind in [
            teksilo_tokens::PointerKind::Touch,
            teksilo_tokens::PointerKind::Mouse,
        ] {
            let seen: std::rc::Rc<std::cell::Cell<Option<Answers>>> = Default::default();
            let mut tree = WidgetTree::new();
            let held_by = {
                let seen = seen.clone();
                tree.add(
                    // A declared, non-`AUTO` action so the frozen value is
                    // distinguishable from the neutral one a pointer with no
                    // sequence answers with.
                    FillWidget::new()
                        .touch_action(TouchAction::PAN_Y)
                        .on_long_press(move |_e, ctx| {
                            seen.set(Some(Answers {
                                kind: ctx.pointer_kind(),
                                id: ctx.pointer().id,
                                // Read off the field rather than through
                                // `owns_pointer()`, which also needs a
                                // `dispatch_node` — and a timer dispatch,
                                // addressed to a node rather than walking to
                                // one, sets none.
                                captor: ctx.pointer_captor,
                                touch_action: ctx.touch_action(),
                                press_inside: ctx.press_is_inside(),
                            }));
                        }),
                )
            };
            tree.layout(SizeProposal::exact(100.0, 100.0));

            let held = tree.long_press_at(kind, Point::new(50.0, 50.0));
            let seen = seen.get().expect("the hold was dispatched");

            assert_eq!(
                seen.kind, kind,
                "a {kind:?} hold was dispatched as {:?}",
                seen.kind
            );
            assert_eq!(
                seen.id, held,
                "a hold must carry the identity of the contact that held"
            );
            assert_eq!(
                seen.captor,
                Some(held_by),
                "the captor is looked up by the dispatched pointer, and the \
                 holding contact's is the node whose arena took its press"
            );
            assert_eq!(
                seen.touch_action,
                TouchAction::PAN_Y,
                "the frozen action is read off the dispatched pointer's \
                 sequence, so a hold under the wrong pointer reads the \
                 neutral {:?} of a pointer that has none",
                TouchAction::AUTO
            );
            assert!(
                seen.press_inside,
                "the press snapshot is keyed by the dispatched pointer, and \
                 the contact that held is holding a press"
            );
        }
    }

    /// What a hold handler asks the tree **for** is applied to the contact that
    /// held.
    ///
    /// The other half of
    /// `a_hold_is_dispatched_under_the_contact_that_held`: that one asserts
    /// what the context is built with, this one what the context is collected
    /// into. Two requests carry no pointer of their own
    /// and are resolved against `current_pointer_id()` at collection —
    /// `capture_pointer()` and `cancel_pointer_sequence()` — so collecting
    /// after `current_input` is restored, rather than before, silently
    /// addresses both to the mouse.
    ///
    /// The mouse is made live in both arms and is *not* the contact under test,
    /// so a misrouted request lands somewhere the assertions can see rather
    /// than on a pointer the table does not hold.
    #[test]
    fn a_hold_captures_and_cancels_the_contact_that_held() {
        use crate::pointer::{CancelReason, PointerId};
        use crate::test_widgets::FillWidget;
        use crate::widget_builder::WidgetBuilder;

        // The capture the handler takes rides on the contact, and the mouse —
        // live, hovering, holding nothing — is left alone. The contact's own
        // entry is already captured by this node (the arena takes it implicitly
        // at the press), so the mouse half is what a misroute shows up in.
        {
            let mut tree = WidgetTree::new();
            let node = tree.add(FillWidget::new().on_long_press(|_e, ctx| {
                ctx.capture_pointer();
            }));
            tree.layout(SizeProposal::exact(100.0, 100.0));
            tree.pointer_move(Point::new(10.0, 10.0));

            let contact = hold_a_finger(&mut tree, Point::new(50.0, 50.0));

            assert_eq!(
                tree.captured_by(contact),
                Some(node),
                "a capture taken from a hold belongs to the contact that held"
            );
            assert_eq!(
                tree.captured_by(PointerId::MOUSE),
                None,
                "the mouse was not the thing holding, and must not have been \
                 captured on its behalf"
            );
        }

        // The cancel the handler raises revokes the contact, and only it. The
        // finger's entry is gone (a contact that is taken away ceases to exist);
        // the mouse's press-less entry is untouched.
        {
            let mut tree = WidgetTree::new();
            tree.add(FillWidget::new().on_long_press(|_e, ctx| {
                ctx.cancel_pointer_sequence(CancelReason::WidgetDestroyed);
            }));
            tree.layout(SizeProposal::exact(100.0, 100.0));
            tree.pointer_move(Point::new(10.0, 10.0));

            let contact = hold_a_finger(&mut tree, Point::new(50.0, 50.0));

            assert!(
                !tree.live_pointers().any(|p| p.id == contact),
                "a cancel raised from a hold revokes the contact that held"
            );
            assert!(
                tree.live_pointers().any(|p| p.id == PointerId::MOUSE),
                "and revokes nothing else"
            );
        }
    }

    /// Press one finger at `at` and let its hold ripen, without releasing it.
    ///
    /// [`long_press_at`](WidgetTree::long_press_at) lifts the contact, and a
    /// lift takes the capture back and ends the entry — so what the hold's own
    /// handler did to the pointer table is only observable before it.
    fn hold_a_finger(tree: &mut WidgetTree, at: Point) -> crate::pointer::PointerId {
        let hold = tree
            .effective_theme
            .input
            .profile(teksilo_tokens::PointerKind::Touch)
            .long_press;
        let contact = tree.new_contact();
        tree.touch_down(contact, at);
        tree.advance_input_time(hold);
        contact
    }

    /// A deadline armed after the hand-back is reported to the event loop as a
    /// *future* instant.
    ///
    /// `instant_for` is what the winit loop turns into
    /// `ControlFlow::WaitUntil`. A deadline reported in the past is not a
    /// harmless rounding error: the loop wakes immediately, finds nothing
    /// ripe, re-derives the same past instant and spins at full CPU on a
    /// deadline that can never arrive.
    ///
    /// The advance is deliberately a large fraction of the hold, so that
    /// shifting by it once too often or once too few — the two ways
    /// `instant_for` can be wrong — moves the answer by far more than the
    /// tolerance below. Reported *early* is the spinning loop above; reported
    /// *late* is a long press the user waits an extra advance for.
    ///
    /// What this test does **not** cover is the offset's magnitude: it is
    /// subtracted here by exactly the amount `event_time_for` added, so the two
    /// cancel and the assertions below hold for a wrong offset as readily as
    /// for a right one. That number is guarded only by
    /// `repeated_hand_backs_re_measure_the_offset_rather_than_accumulating_it`.
    #[test]
    fn a_deadline_armed_after_the_hand_back_is_in_the_future() {
        use crate::test_widgets::FillWidget;
        use crate::widget_builder::WidgetBuilder;

        // The tree spends real time alive before anything simulates it — which
        // on a live app is every second since launch, and is what makes a
        // deadline reported against the simulated clock land in the past.
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().on_long_press(|_e, _c| {}));
        tree.layout(SizeProposal::exact(100.0, 100.0));
        std::thread::sleep(std::time::Duration::from_millis(60));

        let advanced = std::time::Duration::from_millis(200);
        tree.advance_time(advanced);
        tree.resume_real_time();

        tree.pointer_down_button(Point::new(50.0, 50.0), PointerButton::Primary);
        let hold = tree
            .theme()
            .input
            .profile(teksilo_tokens::PointerKind::Mouse)
            .long_press;
        let slack = std::time::Duration::from_millis(50);
        // The two assertions below only mean something while the advance
        // dominates the tolerance. Stated here so a later change to either
        // constant fails loudly instead of quietly re-opening the gap a 10 ms
        // advance and a 30 ms tolerance left.
        assert!(
            slack * 4 <= advanced && advanced * 4 >= hold,
            "the tolerance must be a fraction of the advance, and the advance a \
             large fraction of the hold: slack {slack:?}, advanced {advanced:?}, \
             hold {hold:?}"
        );
        let deadline = tree
            .next_timer_deadline()
            .expect("a held press has a long-press deadline");
        let wait = deadline.saturating_duration_since(std::time::Instant::now());

        assert!(
            wait > std::time::Duration::ZERO,
            "the loop must be given something it can wait for"
        );
        // And it is the hold away — not the hold minus what was advanced
        // (subtracted a second time, the spinning loop), and not the hold plus
        // it (never subtracted at all). The tolerance is a quarter of the
        // advance, so neither can hide inside it.
        assert!(
            wait <= hold,
            "wake in ~{hold:?} after a {advanced:?} advance, got {wait:?} — reported late"
        );
        assert!(
            wait + slack >= hold,
            "wake in ~{hold:?} after a {advanced:?} advance, got {wait:?} — reported early"
        );
        tree.pointer_up_button(Point::new(50.0, 50.0), PointerButton::Primary);
    }
}

/// The per-pointer table replacing the tree's singular pointer state: two
/// contacts hold two captures, hover belongs to the hover owner alone, the
/// contact cap is enforced at the door, and a nested dispatch waits its turn.
#[cfg(test)]
mod pointer_table_tests {
    use super::*;
    use crate::event::{EventResponse, Modifiers, PointerButton};
    use crate::pointer::{
        BackendDeviceKey, EventTime, PointerId, PointerIdAllocator, PointerInfo, PointerPhase,
        PointerSample,
    };
    use crate::test_widgets::FillWidget;
    use crate::widget_builder::WidgetBuilder;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A fresh contact identity. Minted through the real allocator so it is
    /// monotonic and distinct from [`PointerId::MOUSE`].
    fn contact_id(raw: u64) -> PointerId {
        let alloc = PointerIdAllocator::global();
        let device = BackendDeviceKey::new(0xC0FFEE);
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

    /// A widget that captures the pointer on press and holds it. The shape of
    /// every real drag handle (a slider knob, a splitter divider).
    fn capturing() -> impl crate::widget::Widget + 'static {
        FillWidget::new().on_pointer_event(|event, ctx| {
            if matches!(event, WidgetEvent::PointerDown { .. }) {
                ctx.capture_pointer();
            }
            EventResponse::Ignored
        })
    }

    /// Two contacts on two widgets hold **independent** captures, and one
    /// lifting leaves the other's alone. With a single `pointer_captured_by`
    /// the second press overwrote the first, and the first finger's stream
    /// silently moved to the second widget.
    #[test]
    fn two_contacts_hold_independent_captures() {
        let mut tree = WidgetTree::new();
        let a = tree.add(capturing());
        let b = tree.add(capturing());
        let _root = tree.add(SideBySide { a, b });
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let first = contact_id(1);
        let second = contact_id(2);
        tree.dispatch_pointer(contact(first, PointerPhase::Down, Point::new(25.0, 50.0)));
        tree.dispatch_pointer(contact(second, PointerPhase::Down, Point::new(75.0, 50.0)));

        assert_eq!(tree.captured_by(first), Some(a));
        assert_eq!(tree.captured_by(second), Some(b));

        tree.dispatch_pointer(contact(first, PointerPhase::Up, Point::new(25.0, 50.0)));
        assert_eq!(tree.captured_by(first), None, "the lifted contact is gone");
        assert_eq!(
            tree.captured_by(second),
            Some(b),
            "one contact lifting must not release the other's capture"
        );
    }

    /// A finger arriving while the mouse hovers must not touch hover at all —
    /// not the id, not the signal, not the `on_hover` handlers behind it.
    #[test]
    fn a_second_contact_never_churns_the_hover_signal() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new());
        let b = tree.add(FillWidget::new());
        let _root = tree.add(SideBySide { a, b });
        tree.layout(SizeProposal::exact(100.0, 100.0));

        tree.dispatch_event(WidgetEvent::pointer_move(Point::new(25.0, 50.0)));
        assert_eq!(tree.hovered(), Some(a));

        let churn = Rc::new(std::cell::Cell::new(0usize));
        let observer = {
            let churn = churn.clone();
            tree.hovered_signal()
                .observe(move |_| churn.set(churn.get() + 1))
        };

        let finger = contact_id(3);
        tree.dispatch_pointer(contact(finger, PointerPhase::Down, Point::new(75.0, 50.0)));
        tree.dispatch_pointer(contact(finger, PointerPhase::Move, Point::new(80.0, 50.0)));
        tree.dispatch_pointer(contact(finger, PointerPhase::Up, Point::new(80.0, 50.0)));

        assert_eq!(
            tree.hovered(),
            Some(a),
            "the mouse is still hovering where it was"
        );
        assert_eq!(churn.get(), 0, "a contact must not write the hover signal");
        drop(observer);
    }

    /// …and the contact is never the hover owner, so it has no hover of its
    /// own to report either.
    #[test]
    fn a_contact_is_never_the_hover_owner() {
        let mut tree = WidgetTree::new();
        let target = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let finger = contact_id(4);
        tree.dispatch_pointer(contact(finger, PointerPhase::Down, Point::new(50.0, 50.0)));
        tree.dispatch_pointer(contact(finger, PointerPhase::Move, Point::new(52.0, 50.0)));

        assert_eq!(tree.hover_owner(), None, "a finger cannot own hover");
        assert_eq!(tree.hovered(), None);
        assert_eq!(tree.hovered_for(finger), None);
        assert_eq!(
            tree.primary_pointer().map(|p| p.id),
            Some(finger),
            "it is still the primary pointer — primary and hover owner are not the same role"
        );
        assert_eq!(
            tree.pointer_position(finger),
            Some(Point::new(52.0, 50.0)),
            "and its position is tracked all the same"
        );
        let _ = target;
    }

    /// A synthetic pen, since nothing produces a real one until the pen
    /// package lands: a hovering-capable pointer *does* take the role, and the
    /// mouse it displaces is told its widget is no longer hovered.
    #[test]
    fn a_pen_takes_the_hover_owner_role_from_the_mouse() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new());
        let b = tree.add(FillWidget::new());
        let _root = tree.add(SideBySide { a, b });
        tree.layout(SizeProposal::exact(100.0, 100.0));

        tree.dispatch_event(WidgetEvent::pointer_move(Point::new(25.0, 50.0)));
        assert_eq!(tree.hovered(), Some(a));

        let stylus = contact_id(5);
        let mut pen = PointerInfo::touch(stylus, EventTime::ZERO);
        pen.kind = teksilo_tokens::PointerKind::Pen(teksilo_tokens::PenKind::Pen);
        let mut sample = contact(stylus, PointerPhase::Move, Point::new(75.0, 50.0));
        sample.pointer = pen;
        tree.dispatch_pointer(sample);

        assert_eq!(
            tree.hover_owner().map(|p| p.id),
            Some(stylus),
            "the later hovering sample wins the role"
        );
        assert_eq!(tree.hovered(), Some(b));
        assert_eq!(
            tree.hovered_for(PointerId::MOUSE),
            None,
            "the displaced owner was told to let go"
        );
    }

    /// The tenth simultaneous contact is admitted; the eleventh is refused at
    /// the door and produces no event at all.
    #[test]
    fn the_eleventh_contact_is_dropped_at_the_door() {
        use crate::pointer::table::PointerTable;

        let presses = Rc::new(std::cell::Cell::new(0usize));
        let mut tree = WidgetTree::new();
        let counted = {
            let presses = presses.clone();
            tree.add(FillWidget::new().on_pointer_event(move |event, _ctx| {
                if matches!(event, WidgetEvent::PointerDown { .. }) {
                    presses.set(presses.get() + 1);
                }
                EventResponse::Ignored
            }))
        };
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let ids: Vec<_> = (0..PointerTable::DEFAULT_CAP)
            .map(|n| contact_id(100 + n as u64))
            .collect();
        for &id in &ids {
            tree.dispatch_pointer(contact(id, PointerPhase::Down, Point::new(50.0, 50.0)));
        }
        assert_eq!(presses.get(), PointerTable::DEFAULT_CAP);
        assert_eq!(tree.live_pointers().count(), PointerTable::DEFAULT_CAP);

        let overflow = contact_id(200);
        tree.dispatch_pointer(contact(
            overflow,
            PointerPhase::Down,
            Point::new(50.0, 50.0),
        ));
        assert_eq!(
            presses.get(),
            PointerTable::DEFAULT_CAP,
            "the eleventh contact must not reach a widget"
        );
        assert_eq!(tree.captured_by(overflow), None);
        assert_eq!(tree.live_pointers().count(), PointerTable::DEFAULT_CAP);
        let _ = counted;
    }

    /// A palm the digitizer flagged never reaches a widget either.
    #[test]
    fn a_palm_never_reaches_a_widget() {
        let presses = Rc::new(std::cell::Cell::new(0usize));
        let mut tree = WidgetTree::new();
        {
            let presses = presses.clone();
            tree.add(FillWidget::new().on_pointer_event(move |event, _ctx| {
                if matches!(event, WidgetEvent::PointerDown { .. }) {
                    presses.set(presses.get() + 1);
                }
                EventResponse::Ignored
            }));
        }
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let id = contact_id(300);
        let mut sample = contact(id, PointerPhase::Down, Point::new(50.0, 50.0));
        sample.pointer.palm = true;
        tree.dispatch_pointer(sample);
        assert_eq!(presses.get(), 0);
        assert_eq!(tree.live_pointers().count(), 0);
    }

    /// A dispatch reached from inside a dispatch is queued, not run inline:
    /// the rest of the outer bubble runs on the state it started with, and the
    /// nested dispatch replays afterwards — still before the top-level call
    /// returns.
    #[test]
    fn a_nested_dispatch_is_queued_and_drained_after() {
        let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

        let mut tree = WidgetTree::new();
        let other = {
            let log = log.clone();
            tree.add(FillWidget::new().on_tap(move |_e, _ctx| {
                log.borrow_mut().push("nested");
            }))
        };
        // Two handlers on the child, in the order the bubble runs them:
        // `on_pointer_event` (the pre-gesture intercept) queues the nested
        // dispatch, `on_tap` is the outer work still to come after it. That
        // pair is what makes the ordering below evidence rather than
        // coincidence.
        let child = {
            let log_pointer = log.clone();
            let log_tap = log.clone();
            tree.add(
                FillWidget::new()
                    .on_pointer_event(move |event, ctx| {
                        if matches!(event, WidgetEvent::PointerUp { .. }) {
                            log_pointer.borrow_mut().push("child-press");
                            // Re-enters the dispatch door from inside a handler.
                            ctx.synthetic_click(other);
                        }
                        EventResponse::Ignored
                    })
                    .on_tap(move |_e, _ctx| {
                        log_tap.borrow_mut().push("child-tap");
                    }),
            )
        };
        // Keep the two halves disjoint, so the synthetic click lands on
        // `other` and not back on `child` (which would re-enter for ever).
        let _root = tree.add(SideBySide { a: child, b: other });
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let at = tree.bounds(child).center();
        tree.dispatch_event(WidgetEvent::pointer_down(
            at,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        tree.dispatch_event(WidgetEvent::pointer_up(
            at,
            PointerButton::Primary,
            Modifiers::NONE,
        ));

        assert_eq!(
            *log.borrow(),
            vec!["child-press", "child-tap", "nested"],
            "the outer dispatch must finish on the state it started with, and the \
             nested dispatch replay only once it has — run inline it would read \
             child-press, nested, child-tap"
        );
        assert!(
            !tree.has_pending_dispatch(),
            "the queue must be empty again before the top-level call returns"
        );
    }

    /// Localisation reads the captor's **current** bounds on every event, so a
    /// captured control inside a container that moves keeps reporting sensible
    /// widget-local coordinates rather than coordinates relative to where it
    /// used to be.
    #[test]
    fn a_captured_widget_localises_against_its_moving_bounds() {
        let seen: Rc<RefCell<Vec<Point>>> = Rc::new(RefCell::new(Vec::new()));
        let mut tree = WidgetTree::new();
        let knob = {
            let seen = seen.clone();
            tree.add(FillWidget::new().on_pointer_event(move |event, ctx| {
                match event {
                    WidgetEvent::PointerDown { .. } => ctx.capture_pointer(),
                    WidgetEvent::PointerMove { position, .. } => seen.borrow_mut().push(*position),
                    _ => {}
                }
                EventResponse::Ignored
            }))
        };
        let offset = crate::signal::Signal::new(0.0f32);
        let _root = tree.add(ShiftedSlot {
            child: knob,
            offset: offset.clone(),
        });
        tree.layout(SizeProposal::exact(100.0, 100.0));

        tree.dispatch_event(WidgetEvent::pointer_down(
            Point::new(30.0, 40.0),
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        assert_eq!(tree.pointer_captured_by(), Some(knob));
        tree.dispatch_event(WidgetEvent::pointer_move(Point::new(30.0, 40.0)));

        // The container slides its child 20 dp to the trailing side.
        offset.set(20.0);
        tree.arena.mark_all_dirty();
        tree.layout(SizeProposal::exact(100.0, 100.0));

        tree.dispatch_event(WidgetEvent::pointer_move(Point::new(30.0, 40.0)));

        assert_eq!(
            *seen.borrow(),
            vec![Point::new(30.0, 40.0), Point::new(10.0, 40.0)],
            "the same window position must localise against the captor's new origin"
        );
    }

    /// The localisation contract is **deliberately asymmetric**, and this pins
    /// the asymmetry so nobody "fixes" it into a defect.
    ///
    /// `localize_event` rewrites `PointerDown` / `PointerUp` / `PointerMove` /
    /// `Gesture` into the receiver's own space, and has no arm for `Scroll` or
    /// `PointerCancel`. That is why those two name their position
    /// `window_position`: the router routes by the first (hit-testing is
    /// necessarily window-space), and `common/scrollable.rs` feeds it to
    /// `KineticScroller::pan`, whose tracker follows the *pointer* — and since
    /// localisation resolves against the captor's **current** bounds on every
    /// event (the test above), a localised value would fold the measured
    /// widget's own motion into the velocity.
    ///
    /// Two independent things keep it that way, and the two assertions below
    /// answer for one each — which is why both are here rather than one
    /// standing in for the other:
    ///
    /// * **`Scroll`** reaches its handler through the localising route
    ///   (`dispatch_to_widget` → `localize_event`), so the missing arm is the
    ///   whole of its protection. Adding one reads like a tidy-up; the scroll
    ///   assertion is what goes red.
    /// * **`PointerCancel`** is delivered by the cancel funnel through
    ///   `dispatch_to_widget_direct`, which does not localise at all, so an arm
    ///   added to `localize_event` would be inert on that path. What guards it
    ///   is that `pointer_cancel_event` records the pointer table's own
    ///   (window-space) position verbatim; localising it at the funnel, which
    ///   knows the recipient and could, is the single change the cancel
    ///   assertion catches.
    #[test]
    fn scroll_and_cancel_stay_in_window_space_while_a_press_is_localised() {
        #[derive(Default)]
        struct Seen {
            press: Vec<Point>,
            scroll: Vec<Option<Point>>,
            cancel: Vec<Option<Point>>,
        }
        let seen: Rc<RefCell<Seen>> = Rc::new(RefCell::new(Seen::default()));
        let mut tree = WidgetTree::new();
        let target = {
            let a = seen.clone();
            let b = seen.clone();
            tree.add(
                FillWidget::new()
                    .on_pointer_event(move |event, ctx| {
                        match event {
                            WidgetEvent::PointerDown { position, .. } => {
                                a.borrow_mut().press.push(*position);
                                // Hold the pointer so the cancel funnel has
                                // someone to address.
                                ctx.capture_pointer();
                            }
                            WidgetEvent::PointerCancel {
                                window_position, ..
                            } => a.borrow_mut().cancel.push(*window_position),
                            _ => {}
                        }
                        EventResponse::Ignored
                    })
                    .on_scroll(move |event, _ctx| {
                        if let WidgetEvent::Scroll {
                            window_position, ..
                        } = event
                        {
                            b.borrow_mut().scroll.push(*window_position);
                        }
                        EventResponse::Ignored
                    }),
            )
        };
        // The slot puts its child 20 dp along, so window x and local x differ by
        // exactly 20 and a localised value is distinguishable from a raw one.
        let _root = tree.add(ShiftedSlot {
            child: target,
            offset: crate::signal::Signal::new(20.0f32),
        });
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let at = Point::new(30.0, 40.0);
        tree.dispatch_event(WidgetEvent::pointer_down(
            at,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: crate::event::ScrollDelta::Lines { x: 0.0, y: 1.0 },
            modifiers: Modifiers::NONE,
            window_position: Some(at),
            phase: crate::pointer::ScrollPhase::Discrete,
            pointer: crate::pointer::PointerInfo::mouse(crate::pointer::EventTime::ZERO),
        });
        let captor = tree.pointer_captured_by();
        assert_eq!(
            captor,
            Some(target),
            "the press must have taken the capture"
        );
        let pointer = tree.pointers.primary_id().expect("a live pointer");
        tree.cancel_pointer(
            pointer,
            crate::pointer::CancelReason::Platform,
            &mut crate::window::NoopWindowOps,
        );

        let seen = seen.borrow();
        assert_eq!(
            seen.press,
            vec![Point::new(10.0, 40.0)],
            "a press is localised: window x 30 minus the slot's 20 dp offset"
        );
        assert_eq!(
            seen.scroll,
            vec![Some(at)],
            "`Scroll::window_position` must arrive as produced: localising it would feed \
             the kinetic tracker a frame that moves with the widget it measures"
        );
        assert_eq!(
            seen.cancel,
            vec![Some(at)],
            "`PointerCancel::window_position` must arrive as the revoking path recorded \
             it, for the same reason"
        );
    }

    // --- fixtures --------------------------------------------------------

    /// Splits its bounds down the middle: `a` on the leading half, `b` on the
    /// trailing one, so two pointers can land on two different widgets.
    #[derive(Debug)]
    struct SideBySide {
        a: WidgetId,
        b: WidgetId,
    }

    impl crate::widget::Widget for SideBySide {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &crate::widget::LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
        fn place_children(
            &self,
            bounds: Rect,
            _proposal: SizeProposal,
            children: &mut [crate::widget::WidgetPlacement],
            _ctx: &crate::widget::LayoutContext,
        ) {
            let half = bounds.width / 2.0;
            for (index, c) in children.iter_mut().enumerate() {
                c.origin = Point::new(bounds.x + half * index as f32, bounds.y);
                c.size = teksilo_canvas::Size::new(half, bounds.height);
            }
        }
        fn children(&self) -> Vec<WidgetId> {
            vec![self.a, self.b]
        }
    }

    /// Places its single child at a signal-driven horizontal offset, so a test
    /// can move a captured widget between two pointer samples.
    #[derive(Debug)]
    struct ShiftedSlot {
        child: WidgetId,
        offset: crate::signal::Signal<f32>,
    }

    impl crate::widget::Widget for ShiftedSlot {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &crate::widget::LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
        fn place_children(
            &self,
            bounds: Rect,
            _proposal: SizeProposal,
            children: &mut [crate::widget::WidgetPlacement],
            _ctx: &crate::widget::LayoutContext,
        ) {
            for c in children.iter_mut() {
                c.origin = Point::new(bounds.x + self.offset.get(), bounds.y);
                c.size = bounds.size();
            }
        }
        fn children(&self) -> Vec<WidgetId> {
            vec![self.child]
        }
    }
}

/// The arbitration binds the **timer** path, not only the sample path.
#[cfg(test)]
mod tick_arbitration_tests {
    use super::*;
    use crate::event::EventResponse;
    use crate::test_widgets::{FillWidget, StackWidget};
    use crate::widget_builder::WidgetBuilder;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Duration;

    /// A tree whose innermost child holds the sequence on its press, under an
    /// ancestor that competes for the same press (`on_drag` is what enrols it)
    /// and also carries a long-press recognizer.
    ///
    /// `max_hold` is raised past `long_press` so the hold is still standing
    /// when the ancestor's timer comes due; with the shipped 250 ms hold and
    /// 500 ms long press the hold always expires first and the two never
    /// overlap, so there would be nothing to observe.
    fn tree_with_a_holder_under_a_long_pressing_peer(
        hold_on_press: bool,
    ) -> (WidgetTree, Rc<Cell<bool>>) {
        let long_pressed = Rc::new(Cell::new(false));
        let flag = long_pressed.clone();

        let mut tree = WidgetTree::new();
        let mut theme = tree.theme().clone();
        theme.input.gestures.mouse.max_hold = Duration::from_millis(2000);
        tree.set_theme(theme);

        let child = tree.add(FillWidget::new().on_pointer_event(move |event, ctx| {
            if hold_on_press && matches!(event, WidgetEvent::PointerDown { .. }) {
                ctx.hold_gesture();
            }
            EventResponse::Ignored
        }));
        tree.add(
            StackWidget::new()
                .child(child)
                .on_drag(|_phase, _c| {})
                .on_long_press(move |_e, _c| flag.set(true)),
        );
        tree.layout(SizeProposal::exact(300.0, 50.0));
        (tree, long_pressed)
    }

    /// The control: with nothing holding, the peer's long press does fire on
    /// the tick. Without this the test below would pass on a fixture that
    /// could never long-press at all.
    #[test]
    fn a_peers_long_press_fires_on_the_tick_when_nothing_holds() {
        let (mut tree, long_pressed) = tree_with_a_holder_under_a_long_pressing_peer(false);
        tree.pointer_down_button(Point::new(20.0, 25.0), PointerButton::Primary);
        tree.advance_time(Duration::from_millis(600));
        assert!(
            long_pressed.get(),
            "the ancestor is a member with a long-press recognizer and its \
             timer came due"
        );
    }

    /// …and it does not while a peer is holding.
    ///
    /// `hold_gesture` freezes the arbitration: no other member may win while a
    /// member is still deciding. The sample path has always honoured that
    /// (`sequence_blocks_arena`); the timer path dispatched whatever a
    /// recognizer produced, so a long press whose deadline happened to fall
    /// inside a hold fired anyway — which is the same recognizer winning, one
    /// door over.
    #[test]
    fn a_peers_long_press_does_not_fire_on_the_tick_while_a_member_holds() {
        let (mut tree, long_pressed) = tree_with_a_holder_under_a_long_pressing_peer(true);
        tree.pointer_down_button(Point::new(20.0, 25.0), PointerButton::Primary);
        assert!(
            tree.sequence_members(crate::pointer::PointerId::MOUSE)
                .iter()
                .any(|(_, _, state)| *state == crate::gesture::MemberState::Held),
            "the fixture must actually be holding"
        );

        tree.advance_time(Duration::from_millis(600));
        assert!(
            !long_pressed.get(),
            "no peer may win while a member is holding — the timer path is not \
             a way around the arbitration"
        );
    }
}
