// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`TouchPinchRecognizer`] — two contacts, one pinch stream, one ingress.
//!
//! # Why this exists
//!
//! `SceneView::on_pinch` was reachable on macOS and nowhere else. The OS
//! trackpad path (`PinchGesture` / `RotationGesture`) produced
//! [`GestureEvent::PinchStarted`] / [`PinchChanged`](GestureEvent::PinchChanged)
//! / [`PinchEnded`](GestureEvent::PinchEnded); a touchscreen produced nothing at
//! all, because no recognizer in the framework ever looked at a second contact.
//! A second, touch-only event family would have doubled the handler surface and
//! guaranteed drift — which is how the trackpad translator became dead code in
//! the first place.
//!
//! So this recognizer emits **the same three variants the OS path emits**, and
//! the router hands both streams to the same
//! `WidgetTree::dispatch_os_gesture`. There is exactly one ingress, and a
//! widget that implements `on_pinch` gets a touchscreen for free.
//!
//! # Three or more contacts
//!
//! A pinch is defined on two points. Rotation for three or more is not
//! merely harder — it is *undefined*, because there is no unique rigid
//! transform through three moving points. Rather than average something and
//! hope, the recognizer takes the two **earliest** contacts and ignores the
//! rest, which is Android's `ScaleGestureDetector` behaviour and iOS's.
//! "Earliest" rather than "nearest" so the gesture never re-anchors under the
//! user's fingers mid-pinch.
//!
//! A contact leaving mid-pinch **ends** the gesture rather than promoting a
//! spare into its slot: promoting would teleport the centre and the span, which
//! reads as the content jumping.
//!
//! # No clock
//!
//! A pinch has no timing at all — it is pure geometry over the live contact
//! positions — so nothing here reads a clock, not even a
//! [`RecognizerContext::now`](super::RecognizerContext::now).

use teksilo_canvas::Point;

use crate::pointer::{CancelReason, PointerId};

use super::{GestureEvent, GestureRecognizer, GestureResult, RawPointerEvent, RecognizerContext};

/// Below this span, in logical pixels, a scale ratio is meaningless: two
/// contacts a hair apart divide by near-zero and produce a scale of thousands.
/// Two fingers cannot physically be closer than about this anyway.
const MIN_SPAN: f32 = 1.0;

/// One of the two contacts the pinch is defined on.
#[derive(Copy, Clone, Debug, PartialEq)]
struct Contact {
    id: PointerId,
    position: Point,
}

/// Turns two live contacts into the pinch stream `on_pinch` already speaks.
///
/// [`wants_all_pointers`](GestureRecognizer::wants_all_pointers) is `true`:
/// unlike every other recognizer in this module it must see *every* contact,
/// not just the one whose arena it sits in.
#[derive(Clone, Debug, Default)]
pub struct TouchPinchRecognizer {
    /// The two earliest live contacts, in arrival order. A third contact finds
    /// both slots full and is ignored.
    contacts: [Option<Contact>; 2],
    /// The distance between the contacts when the gesture began — the
    /// denominator of every later scale.
    start_span: f32,
    /// The angle of the contact-to-contact vector when the gesture began.
    start_angle: f32,
    /// The most recent scale, kept so a consumer can ask without re-deriving.
    cumulative_scale: f32,
    /// Total rotation in radians since the start, **unwrapped**: accumulated
    /// as shortest-arc steps so a pinch turned past ±π keeps growing rather
    /// than jumping by 2π.
    cumulative_rotation: f32,
    /// The angle at the previous sample, for the unwrapping above.
    last_angle: f32,
    /// Whether a `PinchStarted` has been emitted and not yet balanced.
    active: bool,
}

impl TouchPinchRecognizer {
    /// A recognizer with no contacts.
    pub fn new() -> Self {
        Self {
            contacts: [None; 2],
            start_span: 0.0,
            start_angle: 0.0,
            cumulative_scale: 1.0,
            cumulative_rotation: 0.0,
            last_angle: 0.0,
            active: false,
        }
    }

    /// Whether a pinch is in progress.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// The ids of the contacts the pinch is following, in arrival order.
    pub fn contact_ids(&self) -> Vec<PointerId> {
        self.contacts.iter().flatten().map(|c| c.id).collect()
    }

    /// The midpoint between the two contacts, or `None` with fewer than two.
    pub fn center(&self) -> Option<Point> {
        let (a, b) = self.pair()?;
        Some(Point::new(
            (a.position.x + b.position.x) / 2.0,
            (a.position.y + b.position.y) / 2.0,
        ))
    }

    /// The current scale relative to the span at the start — `1.0` before a
    /// pinch begins.
    pub fn scale(&self) -> f32 {
        self.cumulative_scale
    }

    /// Total rotation in radians since the start, unwrapped.
    pub fn rotation(&self) -> f32 {
        self.cumulative_rotation
    }

    /// A contact went down.
    ///
    /// Returns [`GestureEvent::PinchStarted`] when this is the second contact,
    /// `None` for the first and for every one after the second.
    pub fn contact_down(&mut self, id: PointerId, position: Point) -> Option<GestureEvent> {
        if self.contacts.iter().flatten().any(|c| c.id == id) {
            // A repeated Down for a contact already tracked (a backend that
            // re-announces, a test replaying) — refresh rather than duplicate.
            return self.contact_moved(id, position);
        }
        let slot = self.contacts.iter().position(Option::is_none)?;
        self.contacts[slot] = Some(Contact { id, position });

        let (a, b) = self.pair()?;
        let span = distance(a.position, b.position);
        if span < MIN_SPAN {
            // Two contacts on top of one another: hold the slots but do not
            // start, or the first sample would report an absurd scale.
            return None;
        }
        self.start_span = span;
        self.start_angle = angle(a.position, b.position);
        self.last_angle = self.start_angle;
        self.cumulative_scale = 1.0;
        self.cumulative_rotation = 0.0;
        self.active = true;
        Some(GestureEvent::PinchStarted {
            center: self.center()?,
        })
    }

    /// A contact moved.
    ///
    /// Returns [`GestureEvent::PinchChanged`] while a pinch is active and the
    /// moved contact is one of the two it follows; `None` otherwise — a third
    /// finger sliding around never disturbs the pinch.
    pub fn contact_moved(&mut self, id: PointerId, position: Point) -> Option<GestureEvent> {
        let slot = self
            .contacts
            .iter()
            .position(|c| c.is_some_and(|c| c.id == id))?;
        self.contacts[slot] = Some(Contact { id, position });
        if !self.active {
            return None;
        }
        let (a, b) = self.pair()?;
        let span = distance(a.position, b.position);
        if self.start_span >= MIN_SPAN {
            self.cumulative_scale = span / self.start_span;
        }
        let now = angle(a.position, b.position);
        self.cumulative_rotation += shortest_arc(now - self.last_angle);
        self.last_angle = now;
        Some(GestureEvent::PinchChanged {
            center: self.center()?,
            scale: self.cumulative_scale,
            rotation: self.cumulative_rotation,
        })
    }

    /// A contact lifted.
    ///
    /// Returns [`GestureEvent::PinchEnded`] when the lifted contact was one of
    /// the pinch's two and a pinch was running. The other contact is kept, so
    /// putting a second finger back down starts a fresh pinch from the new
    /// span rather than resuming the old one.
    pub fn contact_up(&mut self, id: PointerId) -> Option<GestureEvent> {
        let slot = self
            .contacts
            .iter()
            .position(|c| c.is_some_and(|c| c.id == id))?;
        self.contacts[slot] = None;
        self.active.then(|| {
            self.active = false;
            GestureEvent::PinchEnded
        })
    }

    /// The interaction was taken away rather than finished.
    ///
    /// Returns [`GestureEvent::PinchCancelled`] when a pinch was running.
    /// Both slots are cleared: a cancel revokes the whole gesture, not one
    /// finger of it.
    pub fn cancel(&mut self, reason: CancelReason) -> Option<GestureEvent> {
        let was_active = self.active;
        self.contacts = [None; 2];
        self.active = false;
        was_active.then_some(GestureEvent::PinchCancelled { reason })
    }

    /// Both contacts, or `None` while only one is down.
    fn pair(&self) -> Option<(Contact, Contact)> {
        Some((self.contacts[0]?, self.contacts[1]?))
    }
}

impl GestureRecognizer for TouchPinchRecognizer {
    fn process(&mut self, event: &RawPointerEvent, _cx: &RecognizerContext) -> GestureResult {
        let id = event.pointer().id;
        let recognized = match event {
            RawPointerEvent::Down { position, .. } => self.contact_down(id, *position),
            RawPointerEvent::Move { position, .. } => self.contact_moved(id, *position),
            RawPointerEvent::Up { .. } => self.contact_up(id),
            RawPointerEvent::Cancel { reason, .. } => self.cancel(*reason),
        };
        match recognized {
            Some(gesture) => GestureResult::Recognized(gesture),
            None => GestureResult::Pending,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }

    /// Above the swipe, which is the most decisive single-contact recognizer:
    /// once two fingers are down, no one-finger reading of the same samples
    /// should win.
    fn priority(&self) -> u32 {
        40
    }

    /// The declaration that makes this recognizer different from every other
    /// one in the module.
    fn wants_all_pointers(&self) -> bool {
        true
    }
}

/// Euclidean distance, in logical pixels.
fn distance(a: Point, b: Point) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    (dx * dx + dy * dy).sqrt()
}

/// The angle of the vector from `a` to `b`, in radians.
fn angle(a: Point, b: Point) -> f32 {
    (b.y - a.y).atan2(b.x - a.x)
}

/// Fold an angular difference into `(-π, π]`, so the accumulated rotation
/// counts turns instead of wrapping.
fn shortest_arc(mut d: f32) -> f32 {
    while d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    }
    while d <= -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pointer::{BackendDeviceKey, PointerIdAllocator};

    /// `n` distinct contact ids, minted the way the platform layer mints them
    /// — `begin` always returns a fresh id, so the key it is filed under is
    /// irrelevant here.
    fn ids(n: u64) -> Vec<PointerId> {
        (0..n)
            .map(|i| PointerIdAllocator::global().begin(BackendDeviceKey::DEFAULT, i))
            .collect()
    }

    #[test]
    fn one_contact_starts_nothing() {
        let p = ids(1);
        let mut pinch = TouchPinchRecognizer::new();
        assert!(pinch.contact_down(p[0], Point::new(0.0, 0.0)).is_none());
        assert!(!pinch.is_active());
        assert!(pinch.center().is_none());
    }

    #[test]
    fn two_contacts_start_and_the_scale_is_the_span_ratio() {
        let p = ids(2);
        let mut pinch = TouchPinchRecognizer::new();
        assert!(pinch.contact_down(p[0], Point::new(0.0, 0.0)).is_none());
        let started = pinch
            .contact_down(p[1], Point::new(100.0, 0.0))
            .expect("the second contact starts the pinch");
        match started {
            GestureEvent::PinchStarted { center } => {
                assert_eq!(center, Point::new(50.0, 0.0));
            }
            other => panic!("expected PinchStarted, got {other:?}"),
        }

        // Spread to twice the span.
        let changed = pinch
            .contact_moved(p[1], Point::new(200.0, 0.0))
            .expect("a move on a tracked contact reports a change");
        match changed {
            GestureEvent::PinchChanged { scale, center, .. } => {
                assert!((scale - 2.0).abs() < 1e-5, "span doubled, scale is {scale}");
                assert_eq!(center, Point::new(100.0, 0.0));
            }
            other => panic!("expected PinchChanged, got {other:?}"),
        }
    }

    #[test]
    fn rotation_accumulates_and_unwraps_past_pi() {
        let p = ids(2);
        let mut pinch = TouchPinchRecognizer::new();
        pinch.contact_down(p[0], Point::new(0.0, 0.0));
        pinch.contact_down(p[1], Point::new(100.0, 0.0));

        // Rotate the second contact around the first in eight 45° steps: a
        // full turn, which a wrapping implementation would report as ~0.
        for step in 1..=8 {
            let theta = std::f32::consts::FRAC_PI_4 * step as f32;
            pinch.contact_moved(p[1], Point::new(100.0 * theta.cos(), 100.0 * theta.sin()));
        }
        let turns = pinch.rotation() / std::f32::consts::TAU;
        assert!(
            (turns - 1.0).abs() < 1e-3,
            "a full turn should read as one turn, got {turns}"
        );
    }

    #[test]
    fn a_third_contact_is_ignored() {
        let p = ids(3);
        let mut pinch = TouchPinchRecognizer::new();
        pinch.contact_down(p[0], Point::new(0.0, 0.0));
        pinch.contact_down(p[1], Point::new(100.0, 0.0));
        assert_eq!(pinch.contact_ids(), vec![p[0], p[1]]);

        assert!(
            pinch.contact_down(p[2], Point::new(0.0, 500.0)).is_none(),
            "the third contact produces nothing"
        );
        assert_eq!(
            pinch.contact_ids(),
            vec![p[0], p[1]],
            "the pinch still follows the two earliest contacts"
        );
        assert!(
            pinch.contact_moved(p[2], Point::new(0.0, 900.0)).is_none(),
            "moving the ignored contact never disturbs the pinch"
        );
        // …and the pinch still tracks the two it started with.
        assert!(pinch.contact_moved(p[1], Point::new(200.0, 0.0)).is_some());
    }

    #[test]
    fn a_contact_leaving_mid_pinch_ends_the_gesture() {
        let p = ids(2);
        let mut pinch = TouchPinchRecognizer::new();
        pinch.contact_down(p[0], Point::new(0.0, 0.0));
        pinch.contact_down(p[1], Point::new(100.0, 0.0));
        assert!(pinch.is_active());

        assert!(matches!(
            pinch.contact_up(p[0]),
            Some(GestureEvent::PinchEnded)
        ));
        assert!(!pinch.is_active());
        assert_eq!(
            pinch.contact_ids(),
            vec![p[1]],
            "the remaining finger is kept so a new second finger restarts cleanly"
        );
        assert!(
            pinch.contact_up(p[1]).is_none(),
            "the second lift ends nothing — the gesture was already over"
        );
    }

    #[test]
    fn a_cancel_revokes_the_whole_gesture() {
        let p = ids(2);
        let mut pinch = TouchPinchRecognizer::new();
        pinch.contact_down(p[0], Point::new(0.0, 0.0));
        pinch.contact_down(p[1], Point::new(100.0, 0.0));

        match pinch.cancel(CancelReason::Platform) {
            Some(GestureEvent::PinchCancelled { reason }) => {
                assert_eq!(reason, CancelReason::Platform);
            }
            other => panic!("expected PinchCancelled, got {other:?}"),
        }
        assert!(pinch.contact_ids().is_empty(), "both slots are cleared");
        assert!(pinch.cancel(CancelReason::Platform).is_none());
    }

    #[test]
    fn coincident_contacts_do_not_start_a_pinch() {
        let p = ids(2);
        let mut pinch = TouchPinchRecognizer::new();
        pinch.contact_down(p[0], Point::new(10.0, 10.0));
        assert!(
            pinch.contact_down(p[1], Point::new(10.2, 10.0)).is_none(),
            "a sub-pixel span would divide the scale by near-zero"
        );
        assert!(!pinch.is_active());
    }

    #[test]
    fn the_recognizer_declares_that_it_wants_every_contact() {
        let pinch = TouchPinchRecognizer::new();
        assert!(pinch.wants_all_pointers());
        assert!(
            !pinch.competes_for_sequence(),
            "a pinch is not a press claimant — it is arbitrated by contact count"
        );
    }

    #[test]
    fn shortest_arc_folds_into_the_half_open_turn() {
        assert!((shortest_arc(0.0)).abs() < 1e-6);
        assert!((shortest_arc(std::f32::consts::TAU)).abs() < 1e-5);
        assert!(
            (shortest_arc(std::f32::consts::PI + 0.1) + std::f32::consts::PI - 0.1).abs() < 1e-5
        );
    }
}
