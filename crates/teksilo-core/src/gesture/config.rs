// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a recognizer is told, and the per-node tap streak it reads.
//!
//! Before this module every recognizer carried its own copy of every
//! threshold (`5.0` here, `300 ms` there) and several of them read
//! `Instant::now()` in the middle of a state machine. That made two things
//! impossible: tuning a gesture per pointer kind (a finger needs 18 dp of slop
//! where a mouse needs 5), and testing a time-driven gesture without sleeping.
//!
//! Both are fixed by handing every `process` / `tick` call a
//! [`RecognizerContext`]: the time comes from the tree's one
//! [`InputClock`](crate::pointer::clock::InputClock), the thresholds come from
//! the [`GestureProfile`] selected for *this* pointer's kind, and the
//! recognizer keeps only the state that is genuinely its own.
//!
//! # The streak
//!
//! [`TapStreak`] is the one piece of tap state that is deliberately *not*
//! recognizer-owned. A double tap spans two presses, and on a touchscreen the
//! second press is a different [`PointerId`](crate::pointer::PointerId) — a
//! new contact, a new arena. State held inside `DoubleTapRecognizer` would be
//! destroyed between the two taps and touch double-tap would be structurally
//! impossible. So the streak lives on the node, outlives every contact, and
//! the recognizers only read it.

use std::time::Duration;

use teksilo_canvas::{Point, Rect};
use teksilo_tokens::{GestureProfile, InputTokens, PointerKind, TargetDensity};

use crate::event::PointerButton;
use crate::pointer::{EventTime, PointerInfo};

use super::{RawPointerEvent, distance};

/// The token set a context-free caller falls back on — the shipped Compact
/// ladder, whose mouse column is byte-for-byte the constants Teksilo shipped
/// before the touch programme.
const FALLBACK_INPUT: InputTokens = InputTokens::for_density(TargetDensity::Compact);

/// The shipped [`GestureProfile`] for `kind`, for a caller with no theme in
/// hand (a hand-rolled [`GestureArena`](super::GestureArena), a unit test).
///
/// The dispatch path does **not** use this: it reads the live theme's
/// [`InputTokens`], so an app that retunes a profile retunes the recognizers.
pub fn default_profile(kind: PointerKind) -> GestureProfile {
    *FALLBACK_INPUT.profile(kind)
}

/// Everything a [`GestureRecognizer`](super::GestureRecognizer) is allowed to
/// know beyond the event in front of it.
///
/// Rebuilt per dispatch rather than stored, so a theme change, a density
/// change or a different pointer kind is picked up without touching a single
/// recognizer.
#[derive(Debug, Clone, Copy)]
pub struct RecognizerContext<'a> {
    /// Now, on the tree's input timeline. Never `Instant::now()` — see
    /// [`EventTime`].
    pub now: EventTime,
    /// The thresholds for this pointer's kind. A recognizer reads its slop and
    /// its timings from here unless the call site set an explicit override.
    pub profile: GestureProfile,
    /// The owning node's bounds in its own coordinate space (origin at zero),
    /// for a recognizer that needs to know whether the pointer is still inside
    /// the target it pressed.
    pub local_bounds: Rect,
    /// Which pointer produced the event being processed.
    pub pointer: PointerInfo,
    /// The owning node's tap streak. Read-only here: it is advanced by the
    /// [`GestureArenaSet`](super::GestureArenaSet) that owns it, once per
    /// qualifying release, *before* the recognizers see the event.
    pub streak: &'a TapStreak,
}

/// The streak a context with no node behind it points at.
static NO_STREAK: TapStreak = TapStreak::EMPTY;

impl<'a> RecognizerContext<'a> {
    /// A context with no streak behind it — for a recognizer driven directly
    /// rather than through an arena set.
    pub fn new(
        now: EventTime,
        profile: GestureProfile,
        local_bounds: Rect,
        pointer: PointerInfo,
    ) -> RecognizerContext<'static> {
        RecognizerContext {
            now,
            profile,
            local_bounds,
            pointer,
            streak: &NO_STREAK,
        }
    }

    /// The same context reading `streak` instead. Used by
    /// [`GestureArenaSet`](super::GestureArenaSet) to splice its own streak in
    /// without the caller having to own one.
    pub fn with_streak<'b>(&self, streak: &'b TapStreak) -> RecognizerContext<'b> {
        RecognizerContext {
            now: self.now,
            profile: self.profile,
            local_bounds: self.local_bounds,
            pointer: self.pointer,
            streak,
        }
    }

    /// The context a single mouse event implies, with the shipped mouse
    /// profile and no bounds. What [`GestureArena::process`](super::GestureArena::process)
    /// builds for a caller that supplies no node.
    pub fn for_event(event: &RawPointerEvent) -> RecognizerContext<'static> {
        let pointer = event.pointer();
        Self::new(
            event.time(),
            default_profile(pointer.kind),
            Rect::ZERO,
            pointer,
        )
    }
}

/// The longest streak that means anything. A fourth tap inside the window
/// restarts at one rather than growing without bound — the Qt convention, and
/// the only one that keeps a long burst producing alternating double and
/// triple taps.
const MAX_STREAK: u8 = 3;

/// How many taps in a row have landed on one node, and what the last of them
/// looked like.
///
/// Owned by the node (through its [`GestureArenaSet`](super::GestureArenaSet)),
/// **not** by a recognizer, because a tap streak outlives the contact that
/// produced each tap. On a touchscreen each tap is a fresh
/// [`PointerId`](crate::pointer::PointerId) and therefore a fresh arena; state
/// kept inside `DoubleTapRecognizer` would be gone before the second tap
/// arrived.
///
/// # Continuation rule
///
/// A tap continues the streak when **all** of these hold, and starts a new one
/// (count 1) otherwise:
///
/// 1. it landed on the same node — true by construction, the streak *is* the
///    node's;
/// 2. it used the same button as the previous tap;
/// 3. `now - last_up <= profile.multi_tap_interval`;
/// 4. `distance(press point, last_position) <= profile.multi_tap_slop`;
/// 5. no other gesture completed on the node in between — the arena set calls
///    [`reset`](Self::reset) when a non-tap gesture wins.
///
/// Condition 4 measures **press to press**. The pre-P06 recognizers measured
/// release to release; the two differ only by the within-tap travel, which is
/// itself bounded by the tap slop, and pressing is the point the user aimed at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TapStreak {
    count: u8,
    last_up: Option<EventTime>,
    last_position: Point,
    button: Option<PointerButton>,
    /// The gap measured by the most recent [`advance`](Self::advance), or zero
    /// when that advance started a fresh streak.
    gap: Duration,
    /// The press-to-press travel measured by the most recent
    /// [`advance`](Self::advance), or zero when it started a fresh streak.
    travel: f32,
}

impl TapStreak {
    /// A streak with no taps in it.
    pub const EMPTY: Self = Self {
        count: 0,
        last_up: None,
        last_position: Point::ZERO,
        button: None,
        gap: Duration::ZERO,
        travel: 0.0,
    };

    /// How many taps the current streak holds. `0` before the first tap,
    /// `2` on the release that should fire a double tap, `3` on a triple.
    pub fn count(&self) -> u8 {
        self.count
    }

    /// When the streak's most recent tap was released.
    pub fn last_up(&self) -> Option<EventTime> {
        self.last_up
    }

    /// Where the streak's most recent tap was pressed.
    pub fn last_position(&self) -> Point {
        self.last_position
    }

    /// Which button the streak is running on.
    pub fn button(&self) -> Option<PointerButton> {
        self.button
    }

    /// The interval between the last two taps of the streak, or
    /// [`Duration::ZERO`] when the last advance started a fresh one.
    ///
    /// A recognizer carrying an explicit — and therefore *tighter* — interval
    /// override re-checks it against this. A **looser** override cannot widen
    /// the window: the streak is the node's, and it uses the profile.
    pub fn since_previous(&self) -> Duration {
        self.gap
    }

    /// The press-to-press distance between the last two taps of the streak, or
    /// `0.0` when the last advance started a fresh one. Same override rule as
    /// [`since_previous`](Self::since_previous).
    pub fn travel_from_previous(&self) -> f32 {
        self.travel
    }

    /// Record a tap that has just been released, and return the new count.
    ///
    /// `position` is where the tap was **pressed** (see the continuation rule);
    /// `now` is the release time.
    pub fn advance(
        &mut self,
        now: EventTime,
        profile: &GestureProfile,
        position: Point,
        button: PointerButton,
    ) -> u8 {
        let gap = self.last_up.map(|last| now.saturating_since(last));
        let travel = self.last_up.map(|_| distance(position, self.last_position));
        let continues = match (gap, travel, self.button) {
            (Some(gap), Some(travel), Some(previous)) => {
                previous == button
                    && gap <= profile.multi_tap_interval
                    && travel <= profile.multi_tap_slop
            }
            _ => false,
        };

        if continues && self.count < MAX_STREAK {
            self.count += 1;
            self.gap = gap.unwrap_or(Duration::ZERO);
            self.travel = travel.unwrap_or(0.0);
        } else {
            self.count = 1;
            self.gap = Duration::ZERO;
            self.travel = 0.0;
        }
        self.last_up = Some(now);
        self.last_position = position;
        self.button = Some(button);
        self.count
    }

    /// Break the streak. Called when a non-tap gesture completes on the node
    /// (continuation rule 5), when a contact is cancelled, and when the tap
    /// family is cancelled out from under the user.
    pub fn reset(&mut self) {
        *self = Self::EMPTY;
    }
}

impl Default for TapStreak {
    fn default() -> Self {
        Self::EMPTY
    }
}

/// Press bookkeeping for one contact, kept beside the recognizers so the
/// "was that a tap?" question is answered once per contact instead of once per
/// recognizer.
///
/// Mirrors what the multi-tap recognizers used to do inline: remember the press
/// point and button, fail the moment the pointer strays past
/// `multi_tap_slop`, and accept a release that lands within it on the button it
/// started on.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TapContact {
    press: Option<(Point, PointerButton)>,
    strayed: bool,
}

impl TapContact {
    /// Feed the contact an event; return the press point and button when this
    /// event is a release that qualifies as a tap for streak purposes.
    pub(crate) fn observe(
        &mut self,
        event: &RawPointerEvent,
        profile: &GestureProfile,
    ) -> Option<(Point, PointerButton)> {
        match event {
            RawPointerEvent::Down {
                position, button, ..
            } => {
                self.press = Some((*position, *button));
                self.strayed = false;
                None
            }
            RawPointerEvent::Move { position, .. } => {
                if let Some((press, _)) = self.press
                    && distance(*position, press) > profile.multi_tap_slop
                {
                    self.strayed = true;
                }
                None
            }
            RawPointerEvent::Up {
                position, button, ..
            } => {
                let (press, pressed_button) = self.press.take()?;
                if self.strayed
                    || pressed_button != *button
                    || distance(*position, press) > profile.multi_tap_slop
                {
                    return None;
                }
                Some((press, *button))
            }
            RawPointerEvent::Cancel { .. } => {
                self.press = None;
                self.strayed = false;
                None
            }
        }
    }
}

/// How many simultaneous contacts a node handles.
///
/// The default, [`First`](Self::First), is what every widget written before the
/// touch programme assumes: one press at a time. Under it a *second* contact
/// arriving on the node is terminated there — neither delivered to the node nor
/// bubbled to an ancestor — which is what stops two fingers landing on a button
/// inside a scroll area from starting a pan with the second finger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MultiContact {
    /// Serve the first contact; refuse any other while it is live.
    #[default]
    First,
    /// Serve every contact, each with its own live arena. What a multi-touch
    /// surface (a pinch-zoom canvas, a piano keyboard) declares.
    All,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gesture::test_helpers::*;

    fn mouse() -> GestureProfile {
        GestureProfile::MOUSE
    }

    #[test]
    fn a_first_tap_starts_the_streak_at_one() {
        let mut streak = TapStreak::EMPTY;
        assert_eq!(streak.count(), 0);
        let count = streak.advance(
            EventTime::from_millis(10),
            &mouse(),
            Point::new(4.0, 4.0),
            PointerButton::Primary,
        );
        assert_eq!(count, 1);
        assert_eq!(streak.since_previous(), Duration::ZERO);
    }

    #[test]
    fn a_second_tap_in_the_window_continues() {
        let mut streak = TapStreak::EMPTY;
        let p = Point::new(4.0, 4.0);
        streak.advance(
            EventTime::from_millis(10),
            &mouse(),
            p,
            PointerButton::Primary,
        );
        let count = streak.advance(
            EventTime::from_millis(200),
            &mouse(),
            p,
            PointerButton::Primary,
        );
        assert_eq!(count, 2);
        assert_eq!(streak.since_previous(), Duration::from_millis(190));
    }

    #[test]
    fn a_different_button_restarts_the_streak() {
        let mut streak = TapStreak::EMPTY;
        let p = Point::new(4.0, 4.0);
        streak.advance(
            EventTime::from_millis(10),
            &mouse(),
            p,
            PointerButton::Primary,
        );
        let count = streak.advance(
            EventTime::from_millis(100),
            &mouse(),
            p,
            PointerButton::Secondary,
        );
        assert_eq!(count, 1, "a mixed-button pair is never a double tap");
    }

    #[test]
    fn exceeding_the_interval_restarts_the_streak() {
        let mut streak = TapStreak::EMPTY;
        let p = Point::new(4.0, 4.0);
        streak.advance(
            EventTime::from_millis(10),
            &mouse(),
            p,
            PointerButton::Primary,
        );
        let count = streak.advance(
            EventTime::from_millis(10 + 301),
            &mouse(),
            p,
            PointerButton::Primary,
        );
        assert_eq!(count, 1);
    }

    #[test]
    fn exceeding_the_multi_tap_slop_restarts_the_streak() {
        let mut streak = TapStreak::EMPTY;
        streak.advance(
            EventTime::from_millis(10),
            &mouse(),
            Point::new(0.0, 0.0),
            PointerButton::Primary,
        );
        let count = streak.advance(
            EventTime::from_millis(100),
            &mouse(),
            Point::new(11.0, 0.0),
            PointerButton::Primary,
        );
        assert_eq!(
            count, 1,
            "11 dp apart exceeds the mouse 10 dp multi-tap slop"
        );
    }

    #[test]
    fn a_reset_between_taps_restarts_the_streak() {
        let mut streak = TapStreak::EMPTY;
        let p = Point::new(4.0, 4.0);
        streak.advance(
            EventTime::from_millis(10),
            &mouse(),
            p,
            PointerButton::Primary,
        );
        // Continuation rule 5: another gesture completed on the node.
        streak.reset();
        let count = streak.advance(
            EventTime::from_millis(100),
            &mouse(),
            p,
            PointerButton::Primary,
        );
        assert_eq!(count, 1);
    }

    #[test]
    fn the_streak_restarts_after_three() {
        let mut streak = TapStreak::EMPTY;
        let p = Point::new(4.0, 4.0);
        let mut counts = Vec::new();
        for i in 0..4 {
            counts.push(streak.advance(
                EventTime::from_millis(10 + 100 * i),
                &mouse(),
                p,
                PointerButton::Primary,
            ));
        }
        assert_eq!(counts, vec![1, 2, 3, 1]);
    }

    #[test]
    fn a_touch_profile_widens_the_window_the_streak_accepts() {
        let mut streak = TapStreak::EMPTY;
        streak.advance(
            EventTime::from_millis(0),
            &GestureProfile::TOUCH,
            Point::new(0.0, 0.0),
            PointerButton::Primary,
        );
        // 30 dp apart: over the mouse's 10 dp slop, inside touch's 40 dp.
        let count = streak.advance(
            EventTime::from_millis(100),
            &GestureProfile::TOUCH,
            Point::new(30.0, 0.0),
            PointerButton::Primary,
        );
        assert_eq!(count, 2);
    }

    #[test]
    fn a_release_that_strayed_does_not_qualify() {
        let mut contact = TapContact::default();
        let profile = mouse();
        contact.observe(&down(Point::new(0.0, 0.0)), &profile);
        contact.observe(&move_to(Point::new(40.0, 0.0)), &profile);
        // Back where it started, but the excursion already disqualified it —
        // exactly what the pre-P06 recognizers did by failing on the move.
        assert!(
            contact
                .observe(&up(Point::new(0.0, 0.0)), &profile)
                .is_none()
        );
    }

    #[test]
    fn a_release_within_slop_qualifies_and_reports_the_press_point() {
        let mut contact = TapContact::default();
        let profile = mouse();
        contact.observe(&down(Point::new(1.0, 2.0)), &profile);
        assert_eq!(
            contact.observe(&up(Point::new(4.0, 2.0)), &profile),
            Some((Point::new(1.0, 2.0), PointerButton::Primary))
        );
    }

    #[test]
    fn the_default_profile_follows_the_pointer_kind() {
        assert_eq!(default_profile(PointerKind::Mouse), GestureProfile::MOUSE);
        assert_eq!(default_profile(PointerKind::Touch), GestureProfile::TOUCH);
    }
}
