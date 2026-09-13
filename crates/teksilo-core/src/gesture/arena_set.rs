// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! One node's gesture recognizers, instantiated per contact.
//!
//! A [`GestureArena`] follows one press from down to up. That was enough while
//! the only pointer was a mouse, which is singular by construction. A
//! touchscreen is not: two fingers on one node are two independent presses that
//! overlap in time, and each needs its own recognizer state.
//!
//! So a node carries a [`GestureArenaSet`]: the recognizer *list* decided once
//! at install time ([`GestureProto`]), an arena instantiated lazily per live
//! [`PointerId`], and — outliving every one of them — the node's
//! [`TapStreak`]. The streak has to sit here rather than inside
//! `DoubleTapRecognizer` because each touch tap is a **new** `PointerId`: state
//! held in the recognizer would be destroyed with tap one's arena, before tap
//! two arrived.

use crate::pointer::{EventTime, PointerId};

use super::config::MultiContact;
use super::{
    GestureArena, GestureEvent, GestureRecognizer, RawPointerEvent, RecognizerContext, TapStreak,
};

/// Whether a recognized gesture leaves the node's tap streak intact.
///
/// Only the counted taps themselves do. Anything else completing on the node —
/// a drag, a swipe, a long press — is continuation rule 5 firing: the user did
/// something other than tap, so the next tap starts a fresh streak.
pub(crate) fn preserves_streak(gesture: &GestureEvent) -> bool {
    matches!(
        gesture,
        GestureEvent::Tap(_) | GestureEvent::DoubleTap(_) | GestureEvent::TripleTap(_)
    )
}

/// How to build one recognizer for a newly arrived contact.
///
/// A boxed factory rather than a closed description enum: `GestureArena::add`
/// accepts any `impl GestureRecognizer`, and a set that could only rebuild the
/// six stock recognizers would quietly be a narrower API than the arena it
/// replaces. The cost is one allocation per recognizer per node, paid once at
/// install.
pub struct GestureProto {
    build: Box<dyn Fn() -> Box<dyn GestureRecognizer>>,
}

impl GestureProto {
    /// A prototype that calls `build` for each new contact.
    pub fn new<R: GestureRecognizer + 'static>(build: impl Fn() -> R + 'static) -> Self {
        Self {
            build: Box::new(move || Box::new(build())),
        }
    }

    /// A prototype whose factory already boxes.
    pub fn boxed(build: impl Fn() -> Box<dyn GestureRecognizer> + 'static) -> Self {
        Self {
            build: Box::new(build),
        }
    }

    fn instantiate(&self) -> Box<dyn GestureRecognizer> {
        (self.build)()
    }
}

impl std::fmt::Debug for GestureProto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GestureProto")
    }
}

/// Every gesture recognizer one node carries, plus the state that spans its
/// contacts.
pub struct GestureArenaSet {
    /// The recognizer list, decided once when the node's handlers are read.
    protos: Vec<GestureProto>,
    /// One arena per live contact, in arrival order. Short — one entry for a
    /// mouse, a handful for a multi-touch surface — so a `Vec` scan beats a map.
    live: Vec<(PointerId, GestureArena)>,
    /// The node's tap streak. Outlives every entry in `live`; see the module
    /// docs.
    streak: TapStreak,
    /// How many contacts this node serves at once.
    multi_contact: MultiContact,
    /// Contacts refused under [`MultiContact::First`], so their later moves and
    /// releases are dropped too rather than being mistaken for a new press.
    refused: Vec<PointerId>,
}

impl GestureArenaSet {
    /// A set with no recognizers.
    pub fn new() -> Self {
        Self {
            protos: Vec::new(),
            live: Vec::new(),
            streak: TapStreak::EMPTY,
            multi_contact: MultiContact::First,
            refused: Vec::new(),
        }
    }

    /// Declare a recognizer. Every contact this node accepts gets its own
    /// instance, built by `build`.
    pub fn add<R: GestureRecognizer + 'static>(&mut self, build: impl Fn() -> R + 'static) {
        self.protos.push(GestureProto::new(build));
    }

    /// Declare a recognizer from a ready-made prototype.
    pub fn add_proto(&mut self, proto: GestureProto) {
        self.protos.push(proto);
    }

    /// How many contacts this node serves at once. Refreshed from the node
    /// every time its handlers are read, so a rebuild can change the policy.
    pub fn set_multi_contact(&mut self, policy: MultiContact) {
        self.multi_contact = policy;
    }

    /// The node's multi-contact policy.
    pub fn multi_contact(&self) -> MultiContact {
        self.multi_contact
    }

    /// Whether any recognizer is declared.
    pub fn is_empty(&self) -> bool {
        self.protos.is_empty()
    }

    /// How many recognizers are declared.
    pub fn len(&self) -> usize {
        self.protos.len()
    }

    /// Whether any contact is currently being followed.
    pub fn is_live(&self) -> bool {
        !self.live.is_empty()
    }

    /// The node's tap streak, for tests and for the inspector.
    pub fn streak(&self) -> &TapStreak {
        &self.streak
    }

    /// Feed one contact's event.
    ///
    /// The streak is advanced *before* the recognizers run, so a recognizer
    /// reading `cx.streak.count()` sees the ordinal of the tap in front of it.
    pub fn process(
        &mut self,
        event: &RawPointerEvent,
        cx: &RecognizerContext,
    ) -> Option<GestureEvent> {
        let id = event.pointer().id;
        let terminal = matches!(
            event,
            RawPointerEvent::Up { .. } | RawPointerEvent::Cancel { .. }
        );

        if let Some(pos) = self.refused.iter().position(|p| *p == id) {
            // Refused at its press; keep swallowing until it lifts.
            if terminal {
                self.refused.swap_remove(pos);
            }
            return None;
        }

        let index = match self.live.iter().position(|(p, _)| *p == id) {
            Some(index) => index,
            None => {
                let RawPointerEvent::Down { .. } = event else {
                    // A move or a release for a contact this node never
                    // accepted — nothing to feed.
                    return None;
                };
                if self.multi_contact == MultiContact::First && !self.live.is_empty() {
                    // Terminated here: not delivered to this node, and not
                    // bubbled to an ancestor either — the caller treats an
                    // arena-bearing node as having consumed the press.
                    //
                    // The refusal is **arena-scoped**, and only that. It stops
                    // the extra contact taking this node's press and completing
                    // a tap of its own, and it stops the contact reaching an
                    // ancestor *arena*. It does not stop the contact opening
                    // its own `PointerSequence`, and so it does not stop it
                    // enrolling an enclosing pan claimant and panning: capture,
                    // arbitration and press ownership are per pointer. That is
                    // what the platforms do — a second finger inside a scroll
                    // view scrolls it — and it is asserted by
                    // `a_second_finger_on_a_button_in_a_scroller_pans_the_scroller`
                    // in `crates/teksilo-core/tests/arbitration_matrix.rs`,
                    // beside `two_fingers_on_one_button_fire_one_tap` for the
                    // half this *does* govern.
                    self.refused.push(id);
                    return None;
                }
                self.live.push((id, self.instantiate()));
                self.live.len() - 1
            }
        };

        if let Some((press, button)) = self.live[index].1.observe_tap(event, &cx.profile) {
            self.streak.advance(cx.now, &cx.profile, press, button);
        }

        // Splice the node streak into the context. `mem::take` is what lets the
        // set hand out a shared borrow of its own streak while holding a
        // mutable borrow of its arenas.
        let streak = std::mem::take(&mut self.streak);
        let scoped = cx.with_streak(&streak);
        let recognized = self.live[index].1.process_with(event, &scoped);
        self.streak = streak;

        if let Some(gesture) = &recognized
            && !preserves_streak(gesture)
        {
            self.streak.reset();
        }
        if matches!(event, RawPointerEvent::Cancel { .. }) {
            self.streak.reset();
        }
        if terminal {
            self.live.remove(index);
        }
        recognized
    }

    /// Advance every live contact's time-driven recognizers.
    ///
    /// Returns one entry per contact that just recognized, so a long press on
    /// two fingers reports twice rather than one of them being lost.
    pub fn tick(&mut self, cx: &RecognizerContext) -> Vec<(PointerId, GestureEvent)> {
        if self.live.is_empty() {
            return Vec::new();
        }
        let streak = std::mem::take(&mut self.streak);
        let scoped = cx.with_streak(&streak);
        let mut out = Vec::new();
        for (id, arena) in &mut self.live {
            if let Some(gesture) = arena.tick_with(&scoped) {
                out.push((*id, gesture));
            }
        }
        self.streak = streak;
        if out.iter().any(|(_, g)| !preserves_streak(g)) {
            self.streak.reset();
        }
        out
    }

    /// Earliest instant at which any live contact wants [`tick`](Self::tick).
    pub fn next_deadline(&self) -> Option<EventTime> {
        self.live
            .iter()
            .filter_map(|(_, arena)| arena.next_deadline())
            .min()
    }

    /// Revoke one contact entirely: every recognizer it was feeding is
    /// cancelled and the contact stops being followed.
    ///
    /// Terminal — the contact's own later events are ignored, exactly as if it
    /// had lifted.
    pub fn cancel(&mut self, id: PointerId) {
        if let Some(index) = self.live.iter().position(|(p, _)| *p == id) {
            self.live[index].1.cancel();
            self.live.remove(index);
        }
        self.refused.retain(|p| *p != id);
        // A press that was taken away never counted as a tap.
        self.streak.reset();
    }

    /// Revoke only the tap family on one contact, leaving a drag it started
    /// running.
    ///
    /// The WCAG 2.2 SC 2.5.2 "slide off to abort" path: the activation is
    /// abandoned, the drag is not.
    pub fn cancel_taps(&mut self, id: PointerId) {
        if let Some((_, arena)) = self.live.iter_mut().find(|(p, _)| *p == id) {
            arena.cancel_taps();
        }
        self.streak.reset();
    }

    /// Stop following one contact without cancelling anything — what the tree
    /// calls when a press ends outside the normal event path.
    pub fn end(&mut self, id: PointerId) {
        self.live.retain(|(p, _)| *p != id);
        self.refused.retain(|p| *p != id);
    }

    /// Reset every live contact's recognizers and the node's streak.
    pub fn reset(&mut self) {
        for (_, arena) in &mut self.live {
            arena.reset();
        }
        self.live.clear();
        self.refused.clear();
        self.streak.reset();
    }

    fn instantiate(&self) -> GestureArena {
        let mut arena = GestureArena::new();
        for proto in &self.protos {
            arena.add_boxed(proto.instantiate());
        }
        arena
    }

    /// How many contacts are currently being followed. Test-facing.
    #[cfg(test)]
    pub(crate) fn live_count(&self) -> usize {
        self.live.len()
    }

    /// Whether `id` is currently being followed. Test-facing.
    #[cfg(test)]
    pub(crate) fn is_following(&self, id: PointerId) -> bool {
        self.live.iter().any(|(p, _)| *p == id)
    }
}

impl Default for GestureArenaSet {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for GestureArenaSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureArenaSet")
            .field("num_recognizers", &self.protos.len())
            .field("live_contacts", &self.live.len())
            .field("streak", &self.streak.count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use teksilo_canvas::{Point, Rect};
    use teksilo_tokens::GestureProfile;

    use super::*;
    use crate::event::{Modifiers, PointerButton};
    use crate::gesture::test_helpers::*;
    use crate::gesture::{
        DoubleTapRecognizer, DragRecognizer, LongPressRecognizer, TapRecognizer,
        TripleTapRecognizer,
    };
    use crate::pointer::PointerInfo;
    use teksilo_tokens::PointerKind;

    fn cx_for(pointer: PointerInfo, now: EventTime) -> RecognizerContext<'static> {
        let profile = match pointer.kind {
            PointerKind::Touch => GestureProfile::TOUCH,
            _ => GestureProfile::MOUSE,
        };
        RecognizerContext::new(now, profile, Rect::new(0.0, 0.0, 100.0, 40.0), pointer)
    }

    /// A fresh touch contact: a new id, as the platform mints one per press.
    fn new_touch(now: EventTime) -> PointerInfo {
        let id = crate::pointer::PointerIdAllocator::global()
            .begin(crate::pointer::BackendDeviceKey::DEFAULT, rand_os_id());
        PointerInfo::touch(id, now)
    }

    /// Distinct per call so the allocator mints a distinct `PointerId`.
    fn rand_os_id() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }

    fn tap(
        set: &mut GestureArenaSet,
        pointer: PointerInfo,
        at: Point,
        down_ms: u64,
        up_ms: u64,
    ) -> Vec<GestureEvent> {
        let mut out = Vec::new();
        let d = EventTime::from_millis(down_ms);
        let u = EventTime::from_millis(up_ms);
        if let Some(g) = set.process(&retimed(down(at), pointer, d), &cx_for(pointer, d)) {
            out.push(g);
        }
        if let Some(g) = set.process(&retimed(up(at), pointer, u), &cx_for(pointer, u)) {
            out.push(g);
        }
        out
    }

    fn multi_tap_set() -> GestureArenaSet {
        let mut set = GestureArenaSet::new();
        set.add(DoubleTapRecognizer::new);
        set.add(TripleTapRecognizer::new);
        set
    }

    #[test]
    fn a_touch_double_tap_is_recognized_across_two_pointer_ids() {
        // The reason the streak had to leave the recognizers: on a touchscreen
        // each tap is a fresh contact, so tap one's arena is gone before tap
        // two arrives.
        let mut set = multi_tap_set();
        let at = Point::new(20.0, 20.0);

        let first = new_touch(EventTime::from_millis(0));
        assert!(tap(&mut set, first, at, 0, 40).is_empty());

        let second = new_touch(EventTime::from_millis(150));
        assert_ne!(first.id, second.id, "each touch press mints a new id");
        let gestures = tap(&mut set, second, at, 150, 190);
        assert!(
            gestures
                .iter()
                .any(|g| matches!(g, GestureEvent::DoubleTap(_))),
            "expected a DoubleTap across two contacts, got {gestures:?}"
        );
    }

    #[test]
    fn a_touch_triple_tap_is_recognized_across_three_pointer_ids() {
        let mut set = multi_tap_set();
        let at = Point::new(20.0, 20.0);
        tap(&mut set, new_touch(EventTime::from_millis(0)), at, 0, 40);
        tap(
            &mut set,
            new_touch(EventTime::from_millis(150)),
            at,
            150,
            190,
        );
        let gestures = tap(
            &mut set,
            new_touch(EventTime::from_millis(300)),
            at,
            300,
            340,
        );
        assert!(
            gestures
                .iter()
                .any(|g| matches!(g, GestureEvent::TripleTap(_))),
            "expected a TripleTap across three contacts, got {gestures:?}"
        );
    }

    #[test]
    fn a_touch_double_tap_needs_the_taps_close_together_in_time() {
        let mut set = multi_tap_set();
        let at = Point::new(20.0, 20.0);
        tap(&mut set, new_touch(EventTime::from_millis(0)), at, 0, 40);
        // 301 ms after the first release: past the 300 ms window.
        let gestures = tap(
            &mut set,
            new_touch(EventTime::from_millis(320)),
            at,
            320,
            341,
        );
        assert!(gestures.is_empty(), "expected no gesture, got {gestures:?}");
    }

    #[test]
    fn a_touch_double_tap_needs_the_taps_close_together_in_space() {
        let mut set = multi_tap_set();
        tap(
            &mut set,
            new_touch(EventTime::from_millis(0)),
            Point::new(0.0, 0.0),
            0,
            40,
        );
        // 41 dp apart: past touch's 40 dp multi-tap slop.
        let gestures = tap(
            &mut set,
            new_touch(EventTime::from_millis(150)),
            Point::new(41.0, 0.0),
            150,
            190,
        );
        assert!(gestures.is_empty(), "expected no gesture, got {gestures:?}");
    }

    #[test]
    fn the_streak_does_not_continue_across_a_different_button() {
        let mut set = GestureArenaSet::new();
        set.add(|| DoubleTapRecognizer::new().accept_any_button());
        let pointer = mouse_pointer();
        let at = Point::new(5.0, 5.0);
        let click = |set: &mut GestureArenaSet, button, down_ms: u64, up_ms: u64| {
            let d = EventTime::from_millis(down_ms);
            let u = EventTime::from_millis(up_ms);
            set.process(
                &retimed(down_btn(at, button), pointer, d),
                &cx_for(pointer, d),
            );
            set.process(
                &retimed(up_btn(at, button), pointer, u),
                &cx_for(pointer, u),
            )
        };
        click(&mut set, PointerButton::Primary, 0, 20);
        let second = click(&mut set, PointerButton::Secondary, 100, 120);
        assert!(
            second.is_none(),
            "a mixed-button pair is never a double tap"
        );
    }

    #[test]
    fn the_streak_does_not_continue_after_another_gesture_completes() {
        // Continuation rule 5. Tap, then a drag that completes on the same
        // node, then a tap: the pair straddling the drag is not a double tap.
        let mut set = GestureArenaSet::new();
        set.add(DoubleTapRecognizer::new);
        set.add(DragRecognizer::new);
        let pointer = mouse_pointer();
        let at = Point::new(5.0, 5.0);

        tap(&mut set, pointer, at, 0, 20);

        // A drag: press, travel past the slop, release.
        let d = EventTime::from_millis(40);
        set.process(&retimed(down(at), pointer, d), &cx_for(pointer, d));
        let m = EventTime::from_millis(50);
        set.process(
            &retimed(move_to(Point::new(60.0, 5.0)), pointer, m),
            &cx_for(pointer, m),
        );
        let u = EventTime::from_millis(60);
        let ended = set.process(
            &retimed(up(Point::new(60.0, 5.0)), pointer, u),
            &cx_for(pointer, u),
        );
        assert!(matches!(ended, Some(GestureEvent::DragEnded { .. })));

        let after = tap(&mut set, pointer, at, 80, 100);
        assert!(
            after.is_empty(),
            "a completed drag breaks the streak, got {after:?}"
        );
    }

    #[test]
    fn two_contacts_on_one_node_get_independent_arenas() {
        let mut set = GestureArenaSet::new();
        set.add(DragRecognizer::new);
        set.set_multi_contact(MultiContact::All);

        let a = new_touch(EventTime::from_millis(0));
        let b = new_touch(EventTime::from_millis(0));
        let cx = cx_for(a, EventTime::from_millis(0));

        set.process(
            &retimed(down(Point::new(0.0, 0.0)), a, EventTime::ZERO),
            &cx,
        );
        set.process(
            &retimed(down(Point::new(50.0, 0.0)), b, EventTime::ZERO),
            &cx_for(b, EventTime::ZERO),
        );
        assert_eq!(set.live_count(), 2);
        assert!(set.is_following(a.id) && set.is_following(b.id));

        // Only contact `a` travels: only `a`'s arena arms a drag.
        let started = set.process(
            &retimed(
                move_to(Point::new(40.0, 0.0)),
                a,
                EventTime::from_millis(10),
            ),
            &cx_for(a, EventTime::from_millis(10)),
        );
        assert!(matches!(started, Some(GestureEvent::DragStarted { .. })));

        // `b` has not moved from its own press point, so it is still pending.
        let b_still = set.process(
            &retimed(
                move_to(Point::new(52.0, 0.0)),
                b,
                EventTime::from_millis(11),
            ),
            &cx_for(b, EventTime::from_millis(11)),
        );
        assert!(
            b_still.is_none(),
            "the second contact must judge travel from its own press, got {b_still:?}"
        );
    }

    #[test]
    fn under_multi_contact_first_a_second_contact_is_refused() {
        let mut set = GestureArenaSet::new();
        set.add(TapRecognizer::new);
        assert_eq!(set.multi_contact(), MultiContact::First);

        let a = new_touch(EventTime::ZERO);
        let b = new_touch(EventTime::ZERO);
        set.process(
            &retimed(down(Point::new(0.0, 0.0)), a, EventTime::ZERO),
            &cx_for(a, EventTime::ZERO),
        );
        set.process(
            &retimed(down(Point::new(9.0, 0.0)), b, EventTime::ZERO),
            &cx_for(b, EventTime::ZERO),
        );

        assert_eq!(set.live_count(), 1, "only the first contact is followed");
        assert!(set.is_following(a.id));
        assert!(!set.is_following(b.id));

        // The refused contact's release produces nothing — it is terminated at
        // this node, not delivered and not left half-followed.
        let refused_up = set.process(
            &retimed(up(Point::new(9.0, 0.0)), b, EventTime::from_millis(20)),
            &cx_for(b, EventTime::from_millis(20)),
        );
        assert!(refused_up.is_none());

        // The first contact still completes normally.
        let tapped = set.process(
            &retimed(up(Point::new(0.0, 0.0)), a, EventTime::from_millis(30)),
            &cx_for(a, EventTime::from_millis(30)),
        );
        assert!(matches!(tapped, Some(GestureEvent::Tap(_))));
    }

    #[test]
    fn cancel_taps_kills_the_tap_family_and_leaves_a_live_drag_alone() {
        let mut set = GestureArenaSet::new();
        set.add(TapRecognizer::new);
        set.add(|| LongPressRecognizer::new().min_duration(Duration::from_millis(50)));
        set.add(|| DragRecognizer::new().threshold(5.0));

        let pointer = mouse_pointer();
        let cx = cx_for(pointer, EventTime::ZERO);
        set.process(
            &retimed(down(Point::new(0.0, 0.0)), pointer, EventTime::ZERO),
            &cx,
        );
        let started = set.process(
            &retimed(
                move_to(Point::new(20.0, 0.0)),
                pointer,
                EventTime::from_millis(5),
            ),
            &cx_for(pointer, EventTime::from_millis(5)),
        );
        assert!(matches!(started, Some(GestureEvent::DragStarted { .. })));

        set.cancel_taps(pointer.id);

        // The drag keeps reporting.
        let moved = set.process(
            &retimed(
                move_to(Point::new(30.0, 0.0)),
                pointer,
                EventTime::from_millis(10),
            ),
            &cx_for(pointer, EventTime::from_millis(10)),
        );
        assert!(
            matches!(moved, Some(GestureEvent::DragMoved { .. })),
            "cancel_taps must leave a live drag running, got {moved:?}"
        );

        // The long press never fires, however long it is held.
        let ticked = set.tick(&cx_for(pointer, EventTime::from_millis(900)));
        assert!(
            ticked.is_empty(),
            "cancel_taps must revoke the long press, got {ticked:?}"
        );

        let ended = set.process(
            &retimed(
                up(Point::new(30.0, 0.0)),
                pointer,
                EventTime::from_millis(20),
            ),
            &cx_for(pointer, EventTime::from_millis(20)),
        );
        assert!(matches!(ended, Some(GestureEvent::DragEnded { .. })));
    }

    #[test]
    fn cancel_is_terminal_for_the_contact() {
        let mut set = GestureArenaSet::new();
        set.add(|| DragRecognizer::new().threshold(5.0));
        let pointer = mouse_pointer();
        let cx = cx_for(pointer, EventTime::ZERO);
        set.process(
            &retimed(down(Point::new(0.0, 0.0)), pointer, EventTime::ZERO),
            &cx,
        );
        set.cancel(pointer.id);
        assert!(!set.is_following(pointer.id));

        let after = set.process(
            &retimed(
                move_to(Point::new(40.0, 0.0)),
                pointer,
                EventTime::from_millis(10),
            ),
            &cx_for(pointer, EventTime::from_millis(10)),
        );
        assert!(after.is_none(), "a cancelled contact is not followed");
    }

    #[test]
    fn a_cancel_event_unwinds_a_running_drag() {
        let mut set = GestureArenaSet::new();
        set.add(|| DragRecognizer::new().threshold(5.0));
        let pointer = mouse_pointer();
        set.process(
            &retimed(down(Point::new(0.0, 0.0)), pointer, EventTime::ZERO),
            &cx_for(pointer, EventTime::ZERO),
        );
        set.process(
            &retimed(
                move_to(Point::new(40.0, 0.0)),
                pointer,
                EventTime::from_millis(5),
            ),
            &cx_for(pointer, EventTime::from_millis(5)),
        );
        let cancelled = set.process(
            &retimed(
                cancel_at(Point::new(40.0, 0.0)),
                pointer,
                EventTime::from_millis(9),
            ),
            &cx_for(pointer, EventTime::from_millis(9)),
        );
        assert!(
            matches!(cancelled, Some(GestureEvent::DragCancelled { .. })),
            "a running drag owes its handler an unwind, got {cancelled:?}"
        );
        assert!(!set.is_following(pointer.id));
    }

    #[test]
    fn a_contact_stops_being_followed_when_it_lifts() {
        let mut set = GestureArenaSet::new();
        set.add(TapRecognizer::new);
        let pointer = new_touch(EventTime::ZERO);
        set.process(
            &retimed(down(Point::new(0.0, 0.0)), pointer, EventTime::ZERO),
            &cx_for(pointer, EventTime::ZERO),
        );
        assert!(set.is_following(pointer.id));
        set.process(
            &retimed(
                up(Point::new(0.0, 0.0)),
                pointer,
                EventTime::from_millis(20),
            ),
            &cx_for(pointer, EventTime::from_millis(20)),
        );
        assert!(
            !set.is_following(pointer.id),
            "a lifted contact must not leak an arena — touch ids are per press"
        );
    }

    #[test]
    fn a_long_press_reports_per_contact() {
        let mut set = GestureArenaSet::new();
        set.add(|| LongPressRecognizer::new().min_duration(Duration::from_millis(100)));
        set.set_multi_contact(MultiContact::All);
        let a = new_touch(EventTime::ZERO);
        let b = new_touch(EventTime::ZERO);
        set.process(
            &retimed(down(Point::new(0.0, 0.0)), a, EventTime::ZERO),
            &cx_for(a, EventTime::ZERO),
        );
        set.process(
            &retimed(down(Point::new(60.0, 0.0)), b, EventTime::ZERO),
            &cx_for(b, EventTime::ZERO),
        );

        let fired = set.tick(&cx_for(a, EventTime::from_millis(200)));
        assert_eq!(fired.len(), 2, "each contact holds its own timer");
        assert!(
            fired
                .iter()
                .all(|(_, g)| matches!(g, GestureEvent::LongPress(_)))
        );
    }

    #[test]
    fn the_deadline_is_the_earliest_of_every_live_contact() {
        let mut set = GestureArenaSet::new();
        set.add(|| LongPressRecognizer::new().min_duration(Duration::from_millis(100)));
        set.set_multi_contact(MultiContact::All);
        assert!(set.next_deadline().is_none());

        let a = new_touch(EventTime::ZERO);
        set.process(
            &retimed(down(Point::new(0.0, 0.0)), a, EventTime::ZERO),
            &cx_for(a, EventTime::ZERO),
        );
        let b = new_touch(EventTime::from_millis(30));
        set.process(
            &retimed(down(Point::new(60.0, 0.0)), b, EventTime::from_millis(30)),
            &cx_for(b, EventTime::from_millis(30)),
        );
        assert_eq!(set.next_deadline(), Some(EventTime::from_millis(100)));
    }

    #[test]
    fn a_recognizer_reads_its_thresholds_from_the_profile() {
        // 10 dp of travel: a mouse drag (5 dp slop) arms, a finger drag
        // (18 dp slop) does not — same recognizer, same events.
        let travel = |profile: GestureProfile, pointer: PointerInfo| {
            let mut set = GestureArenaSet::new();
            set.add(DragRecognizer::new);
            let cx = RecognizerContext::new(EventTime::ZERO, profile, Rect::ZERO, pointer);
            set.process(
                &retimed(down(Point::new(0.0, 0.0)), pointer, EventTime::ZERO),
                &cx,
            );
            set.process(
                &retimed(
                    move_to(Point::new(10.0, 0.0)),
                    pointer,
                    EventTime::from_millis(5),
                ),
                &RecognizerContext::new(EventTime::from_millis(5), profile, Rect::ZERO, pointer),
            )
        };
        assert!(matches!(
            travel(GestureProfile::MOUSE, mouse_pointer()),
            Some(GestureEvent::DragStarted { .. })
        ));
        assert!(travel(GestureProfile::TOUCH, new_touch(EventTime::ZERO)).is_none());
    }

    #[test]
    fn a_tap_event_carries_the_pointer_that_produced_it() {
        let mut set = GestureArenaSet::new();
        set.add(TapRecognizer::new);
        let pointer = new_touch(EventTime::ZERO);
        set.process(
            &retimed(down(Point::new(0.0, 0.0)), pointer, EventTime::ZERO),
            &cx_for(pointer, EventTime::ZERO),
        );
        let tapped = set.process(
            &retimed(
                up(Point::new(0.0, 0.0)),
                pointer,
                EventTime::from_millis(20),
            ),
            &cx_for(pointer, EventTime::from_millis(20)),
        );
        match tapped {
            Some(GestureEvent::Tap(event)) => {
                assert_eq!(event.pointer.id, pointer.id);
                assert_eq!(event.pointer.kind, PointerKind::Touch);
                assert_eq!(event.modifiers, Modifiers::NONE);
            }
            other => panic!("expected a Tap carrying its pointer, got {other:?}"),
        }
    }
}
