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
    }

    /// The current time on this tree's input timeline.
    pub fn input_now(&self) -> crate::pointer::EventTime {
        self.input_clock.now()
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
            self.dispatch_to_widget(old, &WidgetEvent::PointerLeave, &mut *ops);
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
    ///   framework never trusts a holder to answer;
    /// * a member armed by [`DragActivation::AfterLongPress`] withdraws once
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
    /// * a member deferred by [`DragActivation::AfterLongPress`] cannot win
    ///   before its timer and self-rejects once the press leaves the tap
    ///   boundary.
    ///
    /// The member whose id is the captor is skipped **and stops the walk**: it
    /// is already being driven by the capture dispatch, and letting an ancestor
    /// past it is exactly the "innermost drag owns the gesture" rule.
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
            if Some(id) == owner && !matches!(role, MemberRole::RawDrag) {
                // Driven by the capture dispatch; nothing above it may win.
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
            if won || self.active_drag.is_some() {
                let winner = if won { id } else { owner.unwrap_or(id) };
                self.decide_sequence(winner);
                // A pan claimant that won owns the rest of the press as a
                // *scroll*: from here every sample for this contact is
                // synthesised onto the claimant chain rather than delivered as
                // a pointer move. Recorded only for a genuine pan win — an
                // `active_drag` takeover names a different winner entirely.
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
    pub(super) fn sequence_blocks_arena(&self, id: WidgetId) -> bool {
        use crate::gesture::MemberState;

        let Some(sequence) = self.current_sequence() else {
            return false;
        };
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
                !member.is_eligible_at(self.sequence_now())
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
            } => crate::gesture::RawPointerEvent::Down {
                position: *position,
                button: *button,
                modifiers: *modifiers,
                pointer: cx.pointer,
                time: cx.now,
            },
            WidgetEvent::PointerMove { position } => crate::gesture::RawPointerEvent::Move {
                position: *position,
                pointer: cx.pointer,
                time: cx.now,
            },
            WidgetEvent::PointerUp {
                position,
                button,
                modifiers,
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
    /// Normally the wall clock. But once [`tick_animations`](Self::tick_animations)
    /// has driven this tree, the scheduler is *only* ever ticked at
    /// [`Self::sim_clock`] — so an animation stamped `Instant::now()` is measured
    /// against a clock that may never reach its start. A headless test
    /// interleaving `layout()` (which promotes) with `tick_animations()` (which
    /// ticks) advances the two clocks independently: simulated time by whatever
    /// the test asks for, real time by however long the test actually takes. The
    /// moment real time overtakes simulated time, every animation armed from then
    /// on has a start in the scheduler's future and its progress **freezes** —
    /// not slowly, completely, and no number of further ticks recovers it.
    ///
    /// That made animated layout tests fail as a function of machine load rather
    /// than of behaviour: green run alone or on a couple of threads, red once the
    /// runner filled the cores and each test's wall-clock time stretched past the
    /// simulated time it was asking for. The overlay manager already keeps its
    /// real and simulated timestamps apart for this reason; animations now agree
    /// on one clock the same way.
    pub(super) fn animation_clock(&self) -> std::time::Instant {
        if self.sim_driven {
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
            for (_pointer, gesture) in gestures {
                let mut ctx = self.make_event_context(&mut *ops);
                if let Some(node) = self.arena.get_mut(id) {
                    Self::dispatch_recognized_gesture(node, gesture, &mut ctx);
                }
                self.collect_from_ctx(ctx, id);
            }
            self.arena.mark_needs_paint(id);
        }
        self.active_ids_scratch = ids;
    }

    /// Read an `Instant` handed in by the event loop on this tree's input
    /// timeline.
    ///
    /// The two clocks share an epoch by construction (see
    /// [`input_clock`](Self::input_clock)), so this is a subtraction. A clock
    /// with no wall-clock anchor — a
    /// [`ManualClock`](crate::pointer::clock::ManualClock) in a test — ignores
    /// the argument and answers with its own reading, which is the whole point
    /// of installing one.
    pub(super) fn event_time_for(&self, now: std::time::Instant) -> crate::pointer::EventTime {
        match self.input_clock.epoch() {
            Some(epoch) => {
                crate::pointer::EventTime::from_duration(now.saturating_duration_since(epoch))
            }
            None => self.input_now(),
        }
    }

    /// Turn an input-timeline deadline back into an `Instant` for the event
    /// loop, which schedules in wall-clock terms.
    pub(super) fn instant_for(&self, time: crate::pointer::EventTime) -> std::time::Instant {
        match self.input_clock.epoch() {
            Some(epoch) => epoch + time.as_duration(),
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
        let parent = tree.add(StackWidget::new().add_child(child));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // A press inside the child captures the pointer to it.
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(50.0, 25.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
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

        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(25.0, 50.0),
        });
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

        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(25.0, 50.0),
        });
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
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: at,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: at,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

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
                    WidgetEvent::PointerMove { position } => seen.borrow_mut().push(*position),
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

        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(30.0, 40.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(tree.pointer_captured_by(), Some(knob));
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(30.0, 40.0),
        });

        // The container slides its child 20 dp to the trailing side.
        offset.set(20.0);
        tree.arena.mark_all_dirty();
        tree.layout(SizeProposal::exact(100.0, 100.0));

        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(30.0, 40.0),
        });

        assert_eq!(
            *seen.borrow(),
            vec![Point::new(30.0, 40.0), Point::new(10.0, 40.0)],
            "the same window position must localise against the captor's new origin"
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
