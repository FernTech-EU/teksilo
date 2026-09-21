// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The one clock.
//!
//! # The rule
//!
//! **`Instant` never enters a recognizer.** Every deadline the input layer
//! owns — a long press, a double-tap window, a fling's decay, a press-feedback
//! delay — is an [`EventTime`] read from the tree's [`InputClock`]. A
//! recognizer that calls `Instant::now()` cannot be driven by a test, and a
//! recognizer that cannot be driven by a test is one whose timing is only ever
//! exercised by sleeping.
//!
//! # Why the epoch is shared
//!
//! A [`WidgetTree`](crate::WidgetTree) already has a simulated clock: the
//! `sim_clock` that `advance_time` moves and that the animation scheduler is
//! ticked against. Its epoch is the `Instant` captured when the tree was
//! built. [`MonotonicClock`] is seeded from **that same instant**, so
//! `EventTime::ZERO` and `simulated_now()` name the same moment and the two
//! timelines are one axis rather than two.
//!
//! Without that, a test would have to advance two clocks in step to move a
//! long press and the animation it kicks off, and the two would drift by
//! however long the test itself took — the exact failure mode the animation
//! clock already had to be rescued from (see `WidgetTree::animation_clock`).
//!
//! # Choosing an implementation
//!
//! [`MonotonicClock`] reads the wall clock and is what a real window uses.
//! [`ManualClock`] is set by the caller and never moves on its own, which is
//! what a headless test wants when it drives time explicitly. Both are held as
//! `Rc<dyn InputClock>`, so a tree's clock can be swapped at any point with
//! [`WidgetTree::set_input_clock`](crate::WidgetTree::set_input_clock).

use std::cell::Cell;
use std::time::{Duration, Instant};

use super::EventTime;

/// The source of [`EventTime`]s for one tree.
///
/// `&self` rather than `&mut self` so a clock can be shared through an `Rc` and
/// read from anywhere in a dispatch without threading a mutable borrow.
pub trait InputClock {
    /// The current time on this tree's input timeline.
    fn now(&self) -> EventTime;

    /// The real instant this clock's [`EventTime::ZERO`] corresponds to, for a
    /// clock that has one.
    ///
    /// `None` for a clock with no wall-clock anchor ([`ManualClock`]). The
    /// framework reads it wherever the input timeline has to be converted to
    /// or from a wall-clock `Instant` — `event_time_for`, `instant_for` and
    /// `rearm_sim_input_origin` each branch on whether there is an anchor at
    /// all — a test asserts through it that the input timeline and the tree's
    /// simulated clock share an origin, and a backend may read it to convert
    /// an OS timestamp into an [`EventTime`].
    fn epoch(&self) -> Option<Instant> {
        None
    }

    /// Move this clock forward by `d`, for a clock that has to be moved.
    ///
    /// A no-op by default, which is right for [`MonotonicClock`]: it already
    /// advances on its own, and shifting its epoch would make every pending
    /// deadline fire the moment a test nudged the *simulated* clock for an
    /// unrelated reason. [`ManualClock`] overrides it, so
    /// [`WidgetTree::advance_time`](crate::WidgetTree::advance_time) moves the
    /// input timeline and the simulated one together and a fling advances by
    /// exactly the duration the caller named.
    fn advance(&self, d: Duration) {
        let _ = d;
    }
}

/// A clock that reads the wall clock, measured from a fixed epoch.
///
/// The epoch is supplied rather than captured so it can be the *same* instant
/// the owning tree's simulated clock was seeded from — see the module docs.
#[derive(Debug, Clone)]
pub struct MonotonicClock {
    epoch: Instant,
}

impl MonotonicClock {
    /// A clock whose zero is `epoch`.
    pub fn new(epoch: Instant) -> Self {
        Self { epoch }
    }

    /// The instant this clock's zero corresponds to.
    pub fn epoch_instant(&self) -> Instant {
        self.epoch
    }
}

impl InputClock for MonotonicClock {
    fn now(&self) -> EventTime {
        // `saturating_duration_since` rather than `-`: a caller may hand us an
        // epoch fractionally in the future (the tree's epoch is captured a few
        // instructions before the clock is built on some platforms' coarse
        // timers), and a panic there would be absurd.
        EventTime::from_duration(Instant::now().saturating_duration_since(self.epoch))
    }

    fn epoch(&self) -> Option<Instant> {
        Some(self.epoch)
    }
}

/// A clock the caller moves by hand. Never advances on its own.
///
/// What a headless test installs when it wants gesture deadlines to fire
/// exactly when it says they do. `Cell` rather than a `&mut` API so it can be
/// held behind the same `Rc<dyn InputClock>` as [`MonotonicClock`].
#[derive(Debug, Clone)]
pub struct ManualClock(Cell<EventTime>);

impl ManualClock {
    /// A clock reading `start`.
    pub fn new(start: EventTime) -> Self {
        Self(Cell::new(start))
    }

    /// Jump to `time`.
    pub fn set(&self, time: EventTime) {
        self.0.set(time);
    }

    /// Move forward by `d`. Saturates rather than overflowing.
    pub fn advance(&self, d: Duration) {
        let next = self
            .0
            .get()
            .checked_add(d)
            .unwrap_or(EventTime::from_duration(Duration::MAX));
        self.0.set(next);
    }
}

impl Default for ManualClock {
    fn default() -> Self {
        Self::new(EventTime::ZERO)
    }
}

impl InputClock for ManualClock {
    fn now(&self) -> EventTime {
        self.0.get()
    }

    /// The owner said to move, so it moves — this is exactly the inherent
    /// [`advance`](Self::advance), reached through the trait so
    /// `WidgetTree::advance_time` can move any clock it happens to hold.
    fn advance(&self, d: Duration) {
        ManualClock::advance(self, d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manual_clock_only_moves_when_told_to() {
        let clock = ManualClock::new(EventTime::ZERO);
        assert_eq!(clock.now(), EventTime::ZERO);
        // Reading twice must not move it — that is the whole point.
        assert_eq!(clock.now(), EventTime::ZERO);

        clock.advance(Duration::from_millis(500));
        assert_eq!(clock.now(), EventTime::from_millis(500));
        clock.advance(Duration::from_millis(250));
        assert_eq!(clock.now(), EventTime::from_millis(750));

        clock.set(EventTime::from_millis(10));
        assert_eq!(clock.now(), EventTime::from_millis(10));
    }

    #[test]
    fn a_manual_clock_saturates_rather_than_overflowing() {
        let clock = ManualClock::new(EventTime::from_millis(1));
        clock.advance(Duration::MAX);
        clock.advance(Duration::MAX);
        assert_eq!(clock.now().as_duration(), Duration::MAX);
    }

    #[test]
    fn a_manual_clock_has_no_wall_clock_epoch() {
        assert_eq!(ManualClock::default().epoch(), None);
    }

    #[test]
    fn a_monotonic_clock_measures_from_its_epoch() {
        let epoch = Instant::now();
        let clock = MonotonicClock::new(epoch);
        assert_eq!(clock.epoch(), Some(epoch));
        assert_eq!(clock.epoch_instant(), epoch);
        // Time only moves forward.
        let a = clock.now();
        let b = clock.now();
        assert!(b >= a);
    }

    /// An epoch fractionally in the future must read as zero, not panic.
    #[test]
    fn a_monotonic_clock_clamps_a_future_epoch() {
        let clock = MonotonicClock::new(Instant::now() + Duration::from_secs(3600));
        assert_eq!(clock.now(), EventTime::ZERO);
    }

    #[test]
    fn clocks_are_usable_through_the_trait_object() {
        let clocks: Vec<std::rc::Rc<dyn InputClock>> = vec![
            std::rc::Rc::new(ManualClock::new(EventTime::from_millis(4))),
            std::rc::Rc::new(MonotonicClock::new(Instant::now())),
        ];
        assert_eq!(clocks[0].now(), EventTime::from_millis(4));
        assert!(clocks[1].epoch().is_some());
    }
}
