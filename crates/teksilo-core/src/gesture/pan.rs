// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`PanRecognizer`] — the competitor a scroll container fields when a finger
//! lands on it.
//!
//! # A pan is not a `GestureEvent`
//!
//! Every other recognizer in this module answers with a
//! [`GestureEvent`](super::GestureEvent), which the arena routes to a matching
//! `on_*` handler. A pan does not, and deliberately: its product is a
//! synthesised [`WidgetEvent::Scroll`](crate::event::WidgetEvent::Scroll),
//! delivered by the router along the claimant chain
//! (`WidgetTree::deliver_pan`). That is what lets a finger drive the fourteen
//! surfaces that already implement `on_scroll` without any of them gaining a
//! second delta path — and it is why there is no `GestureEvent::Pan` and no
//! `on_pan` handler to add one.
//!
//! So [`GestureRecognizer::process`] here
//! never returns `Recognized`: it feeds the tracker, reports `Pending` while
//! the press could still become a pan, and `Failed` once it cannot. A caller
//! learns that the pan armed from [`PanRecognizer::past_slop`] and drives the
//! delivery itself. The trait is implemented all the same, because
//! [`competes_for_sequence`](super::GestureRecognizer::competes_for_sequence)
//! is a trait method and "this recognizer takes part in the cross-node
//! negotiation" is exactly what a pan claimant declares.
//!
//! # Coalesced samples
//!
//! [`feed_coalesced`](PanRecognizer::feed_coalesced) exists so a 240 Hz
//! digitiser's release velocity is fitted over every position the OS batched
//! into the packet, not over the one that happened to survive to frame rate.
//! A tracker fed at frame rate under-reads a flick by roughly the ratio of the
//! two rates, which is the difference between a list that coasts and one that
//! stops dead.

use teksilo_canvas::{Point, Vec2};
use teksilo_tokens::GestureProfile;

use crate::kinetic::VelocityTracker;
use crate::pointer::EventTime;
use crate::pointer::touch_action::{Axis, PanClaim};

use super::{GestureRecognizer, GestureResult, RawPointerEvent, RecognizerContext};

/// The competitor a node with a [`PanClaim`] fields for one contact.
///
/// Installed by the router on the nodes whose claim the frozen
/// [`TouchAction`](crate::pointer::touch_action::TouchAction) permits, **direct
/// pointers only** — a mouse has no `pan_slop` at all
/// ([`GestureProfile::pan_slop`] is `None` for it), so a mouse press builds
/// none of these and every mouse sequence arbitrates exactly as it did before
/// the touch programme.
#[derive(Debug)]
pub struct PanRecognizer {
    /// What this node declared: axes, devices, and whether a release hands off
    /// to a fling.
    claim: PanClaim,
    /// Per-instance override of [`GestureProfile::pan_slop`]. `None` — the
    /// normal case — reads the profile, so retuning the theme retunes the
    /// recognizer.
    slop: Option<f32>,
    /// The velocity behind the release, fitted over every sample including the
    /// coalesced ones.
    tracker: VelocityTracker,
    /// Where the press landed. `None` before it.
    origin: Option<Point>,
    /// Where the contact was at the previous sample, so each sample yields a
    /// delta rather than an absolute the receiver would have to difference.
    last: Option<Point>,
    /// Whether the press has already left the slop radius on a claimed axis.
    armed: bool,
}

impl PanRecognizer {
    /// A recognizer for `claim`, reading its slop from the profile.
    pub fn new(claim: PanClaim) -> Self {
        Self {
            claim,
            slop: None,
            tracker: VelocityTracker::new(),
            origin: None,
            last: None,
            armed: false,
        }
    }

    /// The same, with an explicit slop that overrides the profile's.
    pub fn with_slop(claim: PanClaim, slop: f32) -> Self {
        Self {
            slop: Some(slop),
            ..Self::new(claim)
        }
    }

    /// What this recognizer's node declared.
    pub fn claim(&self) -> PanClaim {
        self.claim
    }

    /// The slop this recognizer arms at, or `None` when the pointer kind never
    /// pans (the mouse).
    pub fn slop(&self, profile: &GestureProfile) -> Option<f32> {
        self.slop.or(profile.pan_slop)
    }

    /// Record the press and start a fresh velocity fit.
    pub fn press(&mut self, position: Point, time: EventTime) {
        self.tracker.clear();
        self.tracker.add(time, position);
        self.origin = Some(position);
        self.last = Some(position);
        self.armed = false;
    }

    /// Feed one position, and report how far the contact moved since the
    /// previous sample **on the claimed axes only**.
    ///
    /// The unclaimed axis is zeroed rather than passed through: a vertical-only
    /// list that let a diagonal drag through horizontally would scroll
    /// sideways at a boundary its own claim said it never pans on.
    pub fn feed(&mut self, position: Point, time: EventTime) -> Vec2 {
        self.tracker.add(time, position);
        let previous = self.last.replace(position).unwrap_or(position);
        self.on_axes(Vec2::new(position.x - previous.x, position.y - previous.y))
    }

    /// [`feed`](Self::feed), with the positions the OS batched into this
    /// packet folded into the velocity fit first.
    ///
    /// `history` is oldest-first and excludes `position`, matching
    /// [`PointerSample::coalesced`](crate::pointer::PointerSample::coalesced).
    /// The reported delta is still measured from the previous sample to
    /// `position`, so a consumer that ignores coalescing sees exactly the same
    /// movement — only the velocity estimate is better informed.
    pub fn feed_coalesced(
        &mut self,
        history: &[(EventTime, Point)],
        position: Point,
        time: EventTime,
    ) -> Vec2 {
        self.tracker.add_coalesced(history);
        self.feed(position, time)
    }

    /// The axis, if any, on which the press has travelled past the pan slop.
    ///
    /// `None` for a pointer kind with no `pan_slop` (the mouse), for a claim
    /// that names no axis, and for a press that has not moved far enough yet.
    /// Once it answers `Some` it keeps answering `Some` for the rest of the
    /// press: the claim is taken at the crossing and is not given back if the
    /// finger wanders back toward the origin.
    pub fn past_slop(&mut self, profile: &GestureProfile) -> Option<Axis> {
        let (Some(origin), Some(last), Some(slop)) = (self.origin, self.last, self.slop(profile))
        else {
            return None;
        };
        let dx = (last.x - origin.x).abs();
        let dy = (last.y - origin.y).abs();
        // Larger travel first, so a diagonal drag reports the axis it is
        // mostly on rather than whichever the enum happens to list first.
        let axis = if dx >= dy {
            [(Axis::X, dx), (Axis::Y, dy)]
        } else {
            [(Axis::Y, dy), (Axis::X, dx)]
        }
        .into_iter()
        .find(|&(axis, travel)| self.claim.axes.contains(axis) && travel >= slop)
        .map(|(axis, _)| axis);
        if axis.is_some() {
            self.armed = true;
        }
        axis
    }

    /// Whether [`past_slop`](Self::past_slop) has ever answered `Some` for this
    /// press.
    pub fn is_armed(&self) -> bool {
        self.armed
    }

    /// The release velocity, in logical pixels per second, clamped to the
    /// profile's `max_fling_velocity` and zeroed on the unclaimed axis.
    ///
    /// `Vec2::ZERO` when the fit has too few samples to answer, which is what
    /// a slow drag-and-hold release produces and exactly what should start no
    /// fling.
    pub fn velocity(&self, profile: &GestureProfile) -> Vec2 {
        let Some(estimate) = self.tracker.estimate() else {
            return Vec2::ZERO;
        };
        let max = profile.max_fling_velocity;
        let v = self.on_axes(estimate.pixels_per_second);
        Vec2::new(v.x.clamp(-max, max), v.y.clamp(-max, max))
    }

    /// Whether `velocity` is fast enough to be worth a fling on any claimed
    /// axis.
    pub fn should_fling(&self, velocity: Vec2, profile: &GestureProfile) -> bool {
        self.claim.kinetic
            && (velocity.x.abs() >= profile.min_fling_velocity
                || velocity.y.abs() >= profile.min_fling_velocity)
    }

    /// Zero whichever axes the claim does not name.
    fn on_axes(&self, v: Vec2) -> Vec2 {
        Vec2::new(
            if self.claim.axes.contains(Axis::X) {
                v.x
            } else {
                0.0
            },
            if self.claim.axes.contains(Axis::Y) {
                v.y
            } else {
                0.0
            },
        )
    }
}

impl GestureRecognizer for PanRecognizer {
    /// Feed the tracker and report whether a pan is still possible.
    ///
    /// Never `Recognized` — see the module docs: a pan's product is a
    /// synthesised `Scroll`, not a `GestureEvent`, so there is nothing for the
    /// arena to route. `Failed` on a release or a cancel, because the press is
    /// over either way.
    fn process(&mut self, event: &RawPointerEvent, cx: &RecognizerContext) -> GestureResult {
        match event {
            RawPointerEvent::Down { position, time, .. } => {
                self.press(*position, *time);
                GestureResult::Pending
            }
            RawPointerEvent::Move { position, time, .. } => {
                self.feed(*position, *time);
                self.past_slop(&cx.profile);
                GestureResult::Pending
            }
            RawPointerEvent::Up { .. } | RawPointerEvent::Cancel { .. } => GestureResult::Failed,
        }
    }

    fn reset(&mut self) {
        self.tracker.clear();
        self.origin = None;
        self.last = None;
        self.armed = false;
    }

    /// Below the tap family and below a drag: a pan never *wins* inside an
    /// arena — it wins the cross-node sequence, which is arbitrated by the
    /// router rather than here — so its priority only ever decides ordering
    /// among peers that all answered `Pending`.
    fn priority(&self) -> u32 {
        0
    }

    /// The whole reason this type implements the trait.
    fn competes_for_sequence(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gesture::default_profile;
    use teksilo_tokens::PointerKind;

    fn touch() -> GestureProfile {
        default_profile(PointerKind::Touch)
    }

    fn at(ms: u64) -> EventTime {
        EventTime::from_millis(ms)
    }

    #[test]
    fn a_mouse_never_arms_a_pan_because_it_has_no_pan_slop() {
        let mouse = default_profile(PointerKind::Mouse);
        assert!(
            mouse.pan_slop.is_none(),
            "the mouse profile has no pan slop"
        );

        let mut pan = PanRecognizer::new(PanClaim::both());
        pan.press(Point::new(0.0, 0.0), at(0));
        pan.feed(Point::new(500.0, 500.0), at(16));
        assert_eq!(pan.past_slop(&mouse), None);
    }

    #[test]
    fn the_claim_zeroes_the_axis_it_does_not_name() {
        let mut pan = PanRecognizer::new(PanClaim::vertical());
        pan.press(Point::new(0.0, 0.0), at(0));
        let delta = pan.feed(Point::new(30.0, 40.0), at(16));
        assert_eq!(delta.x, 0.0, "a vertical claim never reports x movement");
        assert_eq!(delta.y, 40.0);
    }

    #[test]
    fn slop_is_crossed_on_the_dominant_claimed_axis() {
        let profile = touch();
        let slop = profile.pan_slop.expect("touch pans");
        let mut pan = PanRecognizer::new(PanClaim::both());
        pan.press(Point::new(0.0, 0.0), at(0));

        pan.feed(Point::new(slop - 1.0, 0.0), at(8));
        assert_eq!(
            pan.past_slop(&profile),
            None,
            "still inside the slop radius"
        );
        assert!(!pan.is_armed());

        pan.feed(Point::new(slop + 1.0, 2.0), at(16));
        assert_eq!(pan.past_slop(&profile), Some(Axis::X));
        assert!(pan.is_armed());
    }

    #[test]
    fn a_claim_that_forbids_the_travelled_axis_never_arms() {
        let profile = touch();
        let slop = profile.pan_slop.expect("touch pans");
        let mut pan = PanRecognizer::new(PanClaim::vertical());
        pan.press(Point::new(0.0, 0.0), at(0));
        pan.feed(Point::new(slop * 4.0, 0.0), at(16));
        assert_eq!(
            pan.past_slop(&profile),
            None,
            "a vertical claim is not armed by horizontal travel"
        );
    }

    #[test]
    fn once_armed_it_stays_armed_even_if_the_finger_comes_back() {
        let profile = touch();
        let slop = profile.pan_slop.expect("touch pans");
        let mut pan = PanRecognizer::new(PanClaim::vertical());
        pan.press(Point::new(0.0, 0.0), at(0));
        pan.feed(Point::new(0.0, slop + 5.0), at(16));
        assert!(pan.past_slop(&profile).is_some());
        pan.feed(Point::new(0.0, 0.0), at(32));
        assert!(
            pan.is_armed(),
            "the claim is taken at the crossing and not given back"
        );
    }

    /// The coalesced positions must reach the velocity fit — the whole point of
    /// the second feed method, and the difference between a list that coasts
    /// and one that stops dead.
    ///
    /// A flick lasting a single 60 Hz frame is nine positions on a 500 Hz
    /// digitiser and **two** at packet rate — one under
    /// [`MIN_SAMPLE_SIZE`](crate::kinetic::MIN_SAMPLE_SIZE), so the fit
    /// declines to answer at all and the release starts no fling. Fed
    /// coalesced, the same flick reads its real 500 dp/s and flings.
    #[test]
    fn coalesced_samples_recover_a_flick_the_packet_rate_would_lose() {
        let profile = touch();
        let claim = PanClaim {
            kinetic: true,
            ..PanClaim::vertical()
        };

        // Every position the digitiser produced: 1 dp every 2 ms = 500 dp/s,
        // all inside one 16 ms frame.
        let all: Vec<(EventTime, Point)> = (0..=16)
            .step_by(2)
            .map(|ms| (at(ms), Point::new(0.0, ms as f32 / 2.0)))
            .collect();
        let (time, position) = *all.last().expect("non-empty");
        let history = &all[1..all.len() - 1];

        let mut coalesced = PanRecognizer::new(claim);
        coalesced.press(all[0].1, all[0].0);
        coalesced.feed_coalesced(history, position, time);

        let mut decimated = PanRecognizer::new(claim);
        decimated.press(all[0].1, all[0].0);
        decimated.feed(position, time);

        let full = coalesced.velocity(&profile);
        assert!(
            (full.y - 500.0).abs() < 25.0,
            "the coalesced fit recovers the real 500 dp/s, got {}",
            full.y
        );
        assert!(coalesced.should_fling(full, &profile));

        let thin = decimated.velocity(&profile);
        assert_eq!(
            thin,
            Vec2::ZERO,
            "two samples are under the minimum fit, so the flick reads as nothing"
        );
        assert!(!decimated.should_fling(thin, &profile));
    }

    #[test]
    fn velocity_is_clamped_and_gated_by_the_profile() {
        let profile = touch();
        let claim = PanClaim {
            kinetic: true,
            ..PanClaim::vertical()
        };
        let mut pan = PanRecognizer::new(claim);
        // 1000 dp in 1 ms is far past `max_fling_velocity`.
        pan.press(Point::new(0.0, 0.0), at(0));
        pan.feed(Point::new(0.0, 500.0), at(1));
        pan.feed(Point::new(0.0, 1000.0), at(2));
        pan.feed(Point::new(0.0, 1500.0), at(3));
        let v = pan.velocity(&profile);
        assert!(
            v.y.abs() <= profile.max_fling_velocity,
            "clamped to the profile ceiling"
        );
        assert!(pan.should_fling(v, &profile));
        assert!(
            !pan.should_fling(Vec2::new(0.0, 1.0), &profile),
            "a crawl is under `min_fling_velocity`"
        );
    }

    #[test]
    fn a_non_kinetic_claim_never_flings() {
        let profile = touch();
        let pan = PanRecognizer::new(PanClaim::vertical());
        assert!(!pan.claim().kinetic);
        assert!(!pan.should_fling(Vec2::new(0.0, 5000.0), &profile));
    }

    #[test]
    fn the_recognizer_declares_itself_a_sequence_competitor() {
        let pan = PanRecognizer::new(PanClaim::both());
        assert!(pan.competes_for_sequence());
        assert!(
            !pan.wants_all_pointers(),
            "a pan follows one contact; the pinch is the multi-contact one"
        );
    }

    /// `process` is a real trait impl, not a stub: it presses, feeds and fails.
    #[test]
    fn process_feeds_the_tracker_and_fails_on_release() {
        let profile = touch();
        let mut pan = PanRecognizer::new(PanClaim::vertical());
        let pointer = crate::pointer::PointerInfo::touch(crate::pointer::PointerId::MOUSE, at(0));
        let cx = RecognizerContext::new(at(0), profile, teksilo_canvas::Rect::ZERO, pointer);

        assert!(matches!(
            pan.process(
                &RawPointerEvent::Down {
                    position: Point::ZERO,
                    button: crate::event::PointerButton::Primary,
                    modifiers: crate::event::Modifiers::NONE,
                    pointer,
                    time: at(0),
                },
                &cx
            ),
            GestureResult::Pending
        ));
        let slop = profile.pan_slop.expect("touch pans");
        assert!(matches!(
            pan.process(
                &RawPointerEvent::Move {
                    position: Point::new(0.0, slop + 5.0),
                    pointer,
                    time: at(16),
                },
                &cx
            ),
            GestureResult::Pending
        ));
        assert!(pan.is_armed(), "the move armed it through the trait path");
        assert!(matches!(
            pan.process(
                &RawPointerEvent::Up {
                    position: Point::new(0.0, slop + 5.0),
                    button: crate::event::PointerButton::Primary,
                    modifiers: crate::event::Modifiers::NONE,
                    pointer,
                    time: at(32),
                },
                &cx
            ),
            GestureResult::Failed
        ));
    }

    #[test]
    fn reset_forgets_the_press() {
        let profile = touch();
        let mut pan = PanRecognizer::new(PanClaim::vertical());
        pan.press(Point::ZERO, at(0));
        pan.feed(Point::new(0.0, 200.0), at(16));
        assert!(pan.past_slop(&profile).is_some());
        pan.reset();
        assert!(!pan.is_armed());
        assert_eq!(pan.past_slop(&profile), None);
        assert_eq!(pan.velocity(&profile), Vec2::ZERO);
    }
}
