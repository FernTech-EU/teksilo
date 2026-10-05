// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The one door every redraw teksilo-app asks of a window goes through, and
//! what a window that draws nothing still runs.
//!
//! # Withheld redraws
//!
//! A window presents with `pre_present_notify`, so on Wayland winit holds
//! every later `RedrawRequested` back until the compositor's frame callback.
//! A compositor sends none to a surface it does not show: a minimised window,
//! one fully covered, one on another workspace. winit tells teksilo none of
//! these on Wayland. Meanwhile the window's timer deadlines (a tween, a
//! tooltip dwell, a frame tick) only move forward inside a frame, so a
//! deadline already due stays due: counted as is, it would wake the loop at
//! once, find the window due, ask for a redraw winit is still holding, and
//! wake again, at 100 % of a core.
//!
//! So a window remembers that it asked ([`RedrawGate::request_with`]), and
//! the redraw handler clears the stamp with [`RedrawGate::delivered`] before
//! anything else. While a request is outstanding, a deadline cannot wake the
//! loop sooner than [`WITHHELD_AFTER`] after it: a redraw on its way arrives
//! well before. One still outstanding then is *withheld*, and the window is
//! ticked like a hidden one until it arrives.
//!
//! A draw wake a producer thread makes asks winit for its redraw itself (see
//! `teksilo_platform`'s wake target), and so is never stamped here. It only
//! changes pixels: a window that gets nothing else while its compositor
//! withholds its frames is not counted as withheld, which costs nothing, and
//! the repaint waits for the frame winit delivers. What must stay current on
//! a window that draws nothing reaches it as a posted wake instead, and the
//! event loop asks for that one here.
//!
//! # Windows that draw nothing
//!
//! A hidden window (minimised, or fully occluded, where the platform says so)
//! and a window whose redraw is withheld draw nothing, but their app state
//! must stay current: idle callbacks run, layout runs (so clocks and
//! off-thread state advance) and the accessibility tree is delivered, because
//! a macOS window can be occluded and still focused, and a screen reader can
//! act on a window nobody sees. So a request to such a window marks a
//! *non-visual tick* as pending, which the event loop runs from
//! `about_to_wait` without rendering. A hidden window does not ask winit at
//! all; a withheld one's request is already with winit.
//!
//! Ticks are rate-limited per window: a tick runs no sooner than
//! [`TICK_INTERVAL`] after the window's previous frame, rendered or
//! not, whatever asks for it (a timer, an event, the async executor under
//! `ControlFlow::Poll`). A pending tick held back by the limit is not lost:
//! the window's deadline is the moment the limit lifts. That bounds what a
//! window nobody sees can cost at ten non-visual frames a second.
//!
//! # State ticks for a window that draws
//!
//! A wake that keeps state current (content off the UI thread that a widget
//! must take in, an accessibility client attaching) may change nothing on
//! screen: a terminal in a background tab printing, say. Answering it with a
//! redraw would draw and present a frame identical to the last one, per
//! burst. [`RedrawGate::request_tick`] asks for a non-visual tick instead,
//! rate-limited like a hidden window's; the tick ends like an event, so if it
//! did change something visible, the window is then asked for a redraw. A
//! tick is not run while a redraw is on its way: that redraw's frame does the
//! same work.
//!
//! Going hidden asks for one last rendered frame (the inactive look a
//! compositor's thumbnail shows), and coming back asks for one redraw.

use std::cell::Cell;
use std::time::{Duration, Instant};

/// How long a requested redraw may stay undelivered on a focused window
/// before the debug watchdog reports it.
pub(crate) const STALL_REPORT_AFTER: Duration = Duration::from_secs(1);

/// The shortest interval between a window's frame and a non-visual tick
/// after it: what a window that draws nothing, and a state tick, can cost.
pub(crate) const TICK_INTERVAL: Duration = Duration::from_millis(100);

/// How long a request may stay undelivered before it counts as withheld.
pub(crate) const WITHHELD_AFTER: Duration = TICK_INTERVAL;

/// What [`RedrawGate::set_hidden`] changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HiddenTransition {
    Unchanged,
    /// The window just went hidden: render one more frame, then nothing.
    BecameHidden,
    /// The window is shown again: it needs a redraw.
    BecameVisible,
}

/// Per-window redraw bookkeeping. Interior-mutable, so the window manager's
/// `&self` fan-outs can request through it.
#[derive(Debug, Default)]
pub(crate) struct RedrawGate {
    /// When the oldest undelivered request was made; `None` when nothing is
    /// outstanding.
    outstanding_since: Cell<Option<Instant>>,
    /// The watchdog already reported the current stall.
    stall_reported: Cell<bool>,
    /// The window is minimised or fully occluded.
    hidden: Cell<bool>,
    /// Something asked for a frame the window has not run yet. A rendered
    /// frame serves it; while the window draws nothing, a tick does.
    tick_pending: Cell<bool>,
    /// When the window last ran a frame, rendered or not.
    last_frame_at: Cell<Option<Instant>>,
    /// The one frame a window renders on its way to hidden is still owed.
    transition_frame: Cell<bool>,
}

impl RedrawGate {
    /// Record a request at `now` and, unless the window is hidden, run
    /// `request`, which asks winit.
    ///
    /// A request made while one is outstanding keeps the first stamp, so the
    /// stamp measures how long the window has been waiting, not how long
    /// since it was last asked. `request` still runs: winit coalesces
    /// repeated requests itself, and a request it never saw cannot be
    /// delivered.
    pub(crate) fn request_with(&self, now: Instant, request: impl FnOnce()) {
        self.tick_pending.set(true);
        if self.hidden.get() {
            return;
        }
        if self.outstanding_since.get().is_none() {
            self.outstanding_since.set(Some(now));
            self.stall_reported.set(false);
        }
        request();
    }

    /// Ask for a non-visual tick, not a redraw: state must be brought up to
    /// date, and nothing says the window's pixels changed. See the module
    /// docs.
    pub(crate) fn request_tick(&self) {
        self.tick_pending.set(true);
    }

    /// The redraw arrived: nothing is outstanding, and the frame it runs
    /// serves whatever was pending.
    pub(crate) fn delivered(&self) {
        self.outstanding_since.set(None);
        self.stall_reported.set(false);
        self.tick_pending.set(false);
    }

    /// The window ran a frame at `now`, rendered or not. Ticks are counted
    /// from it.
    pub(crate) fn ran_frame(&self, now: Instant) {
        self.last_frame_at.set(Some(now));
    }

    /// A requested redraw has not arrived yet.
    #[cfg(test)]
    pub(crate) fn awaits_redraw(&self) -> bool {
        self.outstanding_since.get().is_some()
    }

    /// A requested redraw has been outstanding for at least
    /// [`WITHHELD_AFTER`] at `now`.
    pub(crate) fn is_withheld(&self, now: Instant) -> bool {
        self.outstanding_since
            .get()
            .is_some_and(|since| now.saturating_duration_since(since) >= WITHHELD_AFTER)
    }

    /// The window draws nothing at `now`: it is hidden, or its redraw is
    /// withheld. Its frames are non-visual ticks.
    pub(crate) fn draws_nothing(&self, now: Instant) -> bool {
        self.hidden.get() || self.is_withheld(now)
    }

    /// Record whether the window is hidden, and what changed.
    ///
    /// Going hidden stops waiting for an outstanding redraw (it may never be
    /// delivered now, and the window no longer needs one to keep its state
    /// current) and owes one transition frame. Coming back clears any pending
    /// tick: the redraw the caller asks for does that work and more.
    pub(crate) fn set_hidden(&self, hidden: bool) -> HiddenTransition {
        if self.hidden.replace(hidden) == hidden {
            return HiddenTransition::Unchanged;
        }
        if hidden {
            self.outstanding_since.set(None);
            self.stall_reported.set(false);
            self.transition_frame.set(true);
            HiddenTransition::BecameHidden
        } else {
            self.tick_pending.set(false);
            self.transition_frame.set(false);
            HiddenTransition::BecameVisible
        }
    }

    pub(crate) fn is_hidden(&self) -> bool {
        self.hidden.get()
    }

    /// Whether a non-visual tick should run at `now`, clearing it if so:
    /// something asked for a frame, no redraw that would serve it is on its
    /// way (the window draws nothing, or nothing is outstanding), and its
    /// previous frame is at least [`TICK_INTERVAL`] old.
    pub(crate) fn take_tick(&self, now: Instant) -> bool {
        let no_redraw_coming = self.draws_nothing(now) || self.outstanding_since.get().is_none();
        if !self.tick_pending.get() || !no_redraw_coming || !self.tick_allowed(now) {
            return false;
        }
        self.tick_pending.set(false);
        true
    }

    /// Whether the frame rendered on the way to hidden is still owed,
    /// clearing it.
    pub(crate) fn take_transition_frame(&self) -> bool {
        self.transition_frame.replace(false)
    }

    /// When the loop should next wake for this window, given its tree's next
    /// timer deadline `timer`; `None` when nothing is due.
    ///
    /// A window that is shown and waits for nothing wakes at `timer`, or for
    /// a pending state tick once the interval allows it, whichever is first:
    /// the tick never delays the timer. One awaiting its redraw wakes no
    /// sooner than the moment that redraw counts as withheld, and one that
    /// draws nothing no sooner than its next tick is allowed; at that moment
    /// a pending tick or a due `timer` runs one.
    pub(crate) fn deadline(&self, now: Instant, timer: Option<Instant>) -> Option<Instant> {
        if !self.hidden.get() && self.outstanding_since.get().is_none() {
            let tick = self.tick_pending.get().then(|| self.next_tick_at(now));
            return match (timer, tick) {
                (Some(timer), Some(tick)) => Some(timer.min(tick)),
                (timer, tick) => timer.or(tick),
            };
        }
        let wanted = match (timer, self.tick_pending.get()) {
            (Some(timer), true) => Some(timer.min(now)),
            (None, true) => Some(now),
            (timer, false) => timer,
        }?;
        let mut earliest = wanted;
        if !self.hidden.get()
            && let Some(since) = self.outstanding_since.get()
        {
            earliest = earliest.max(since + WITHHELD_AFTER);
        }
        if let Some(last) = self.last_frame_at.get() {
            earliest = earliest.max(last + TICK_INTERVAL);
        }
        Some(earliest)
    }

    /// How long the outstanding request has waited, the first time that
    /// exceeds [`STALL_REPORT_AFTER`]; `None` otherwise, and on every later
    /// call for the same stall.
    pub(crate) fn take_stall(&self, now: Instant) -> Option<Duration> {
        let since = self.outstanding_since.get()?;
        let waited = now.saturating_duration_since(since);
        if waited < STALL_REPORT_AFTER || self.stall_reported.get() {
            return None;
        }
        self.stall_reported.set(true);
        Some(waited)
    }

    /// The first instant at or after `now` a tick is allowed.
    fn next_tick_at(&self, now: Instant) -> Instant {
        self.last_frame_at
            .get()
            .map_or(now, |last| (last + TICK_INTERVAL).max(now))
    }

    fn tick_allowed(&self, now: Instant) -> bool {
        self.last_frame_at
            .get()
            .is_none_or(|last| now >= last + TICK_INTERVAL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    /// The earliest of several windows' deadlines, as `update_control_flow`
    /// folds them.
    fn earliest_deadline<'a>(
        now: Instant,
        windows: impl IntoIterator<Item = (&'a RedrawGate, Option<Instant>)>,
    ) -> Option<Instant> {
        windows
            .into_iter()
            .filter_map(|(gate, timer)| gate.deadline(now, timer))
            .min()
    }

    #[test]
    fn a_request_is_outstanding_until_delivered() {
        let gate = RedrawGate::default();
        let now = Instant::now();
        let mut asked = 0;
        assert!(!gate.awaits_redraw());
        gate.request_with(now, || asked += 1);
        assert!(gate.awaits_redraw());
        gate.request_with(now + 5 * MS, || asked += 1);
        assert_eq!(asked, 2, "every request still reaches winit");
        gate.delivered();
        assert!(!gate.awaits_redraw());
    }

    #[test]
    fn a_window_awaiting_its_redraw_does_not_wake_the_loop_for_a_past_deadline() {
        let now = Instant::now();
        let waiting = RedrawGate::default();
        waiting.request_with(now, || {});
        let idle = RedrawGate::default();
        let past = now - 50 * MS;
        let later = now + 16 * MS;

        // Counting the waiting window's past deadline as is would make the
        // loop wake at once, again and again.
        assert_eq!(
            earliest_deadline(now, [(&waiting, Some(past)), (&idle, Some(later))]),
            Some(later)
        );
        assert_eq!(
            earliest_deadline(now, [(&waiting, Some(past))]),
            Some(now + WITHHELD_AFTER),
            "it wakes when its redraw would count as withheld"
        );
        waiting.delivered();
        assert_eq!(
            earliest_deadline(now, [(&waiting, Some(past)), (&idle, Some(later))]),
            Some(past),
            "once the redraw is delivered, the deadline counts again"
        );
    }

    #[test]
    fn a_shown_window_that_waits_for_nothing_wakes_at_its_timer() {
        let now = Instant::now();
        let gate = RedrawGate::default();
        gate.ran_frame(now);
        let soon = now + 3 * MS;
        assert_eq!(gate.deadline(now, Some(soon)), Some(soon));
        assert_eq!(gate.deadline(now, None), None);
    }

    #[test]
    fn a_redraw_outstanding_past_the_threshold_is_withheld_and_ticks() {
        let t0 = Instant::now();
        let gate = RedrawGate::default();
        gate.ran_frame(t0);
        gate.request_with(t0, || {});
        assert!(!gate.is_withheld(t0 + 99 * MS));
        assert!(
            !gate.take_tick(t0 + 99 * MS),
            "a redraw on its way is waited for"
        );
        assert!(gate.is_withheld(t0 + WITHHELD_AFTER));
        assert!(gate.draws_nothing(t0 + WITHHELD_AFTER));
        assert!(
            gate.take_tick(t0 + WITHHELD_AFTER),
            "a withheld window runs its pending tick"
        );
        assert!(!gate.take_tick(t0 + WITHHELD_AFTER), "taken once");
        // The tick ran a frame; winit still holds the redraw.
        gate.ran_frame(t0 + WITHHELD_AFTER);
        gate.request_with(t0 + 150 * MS, || {});
        assert!(
            !gate.take_tick(t0 + 150 * MS),
            "the next tick waits for the interval"
        );
        assert!(gate.take_tick(t0 + 200 * MS));
        gate.delivered();
        assert!(!gate.draws_nothing(t0 + 300 * MS), "delivery ends it");
    }

    #[test]
    fn a_withheld_window_wakes_the_loop_for_its_due_timer_at_the_interval() {
        let t0 = Instant::now();
        let gate = RedrawGate::default();
        gate.ran_frame(t0);
        gate.request_with(t0, || {});
        gate.delivered();
        // Asked again, never delivered: a frame callback that never comes.
        gate.request_with(t0 + 10 * MS, || {});
        let past = t0;
        assert_eq!(
            gate.deadline(t0 + 20 * MS, Some(past)),
            Some(t0 + 10 * MS + WITHHELD_AFTER)
        );
        gate.ran_frame(t0 + 120 * MS);
        assert_eq!(
            gate.deadline(t0 + 130 * MS, Some(past)),
            Some(t0 + 120 * MS + TICK_INTERVAL),
            "after a tick, the next waits for the interval"
        );
    }

    #[test]
    fn a_stall_is_reported_once_and_from_the_first_request() {
        let gate = RedrawGate::default();
        let t0 = Instant::now();
        gate.request_with(t0, || {});
        gate.request_with(t0 + 900 * MS, || {});
        assert_eq!(gate.take_stall(t0 + 999 * MS), None);
        assert_eq!(
            gate.take_stall(t0 + 1200 * MS),
            Some(1200 * MS),
            "measured from the first request, not the latest"
        );
        assert_eq!(gate.take_stall(t0 + Duration::from_secs(5)), None);

        gate.delivered();
        gate.request_with(t0 + Duration::from_secs(6), || {});
        assert_eq!(
            gate.take_stall(t0 + Duration::from_secs(8)),
            Some(Duration::from_secs(2)),
            "a new stall is reported again"
        );
    }

    #[test]
    fn a_hidden_window_marks_a_tick_instead_of_asking_winit() {
        let gate = RedrawGate::default();
        let now = Instant::now();
        assert_eq!(gate.set_hidden(true), HiddenTransition::BecameHidden);
        let mut asked = 0;
        gate.request_with(now, || asked += 1);
        assert_eq!(asked, 0, "a hidden window does not ask winit");
        assert!(!gate.awaits_redraw(), "and so waits for nothing");
        assert!(gate.take_tick(now), "the request became a non-visual tick");
        assert!(!gate.take_tick(now), "taken once");
    }

    #[test]
    fn going_hidden_stops_waiting_and_owes_one_frame() {
        let gate = RedrawGate::default();
        gate.request_with(Instant::now(), || {});
        assert!(gate.awaits_redraw());
        assert_eq!(gate.set_hidden(true), HiddenTransition::BecameHidden);
        assert!(
            !gate.awaits_redraw(),
            "a redraw that may never come is not waited for"
        );
        assert_eq!(gate.set_hidden(true), HiddenTransition::Unchanged);
        assert!(gate.take_transition_frame());
        assert!(!gate.take_transition_frame());
    }

    #[test]
    fn coming_back_drops_the_pending_tick_and_asks_again() {
        let gate = RedrawGate::default();
        let now = Instant::now();
        gate.set_hidden(true);
        gate.request_with(now, || {});
        assert_eq!(gate.set_hidden(false), HiddenTransition::BecameVisible);
        assert!(
            !gate.take_tick(now),
            "the reveal redraw does the tick's work"
        );
        assert!(!gate.take_transition_frame());
        let mut asked = 0;
        gate.request_with(now, || asked += 1);
        assert_eq!(asked, 1, "visible again, requests reach winit");
    }

    #[test]
    fn a_hidden_window_ticks_at_most_every_interval_whatever_asks() {
        let t0 = Instant::now();
        let gate = RedrawGate::default();
        gate.set_hidden(true);
        gate.ran_frame(t0);
        // Requests from events, not timers: each marks the tick pending, and
        // none of them runs one before the interval is up.
        for at in [1, 30, 60, 99] {
            gate.request_with(t0 + at * MS, || {});
            assert!(!gate.take_tick(t0 + at * MS), "{at} ms after a frame");
        }
        assert_eq!(
            gate.deadline(t0 + 99 * MS, None),
            Some(t0 + TICK_INTERVAL),
            "a pending tick held back by the interval wakes the loop when it lifts"
        );
        assert!(gate.take_tick(t0 + TICK_INTERVAL));
    }

    #[test]
    fn a_shown_window_runs_a_state_tick_without_asking_winit() {
        let t0 = Instant::now();
        let gate = RedrawGate::default();
        gate.ran_frame(t0);
        gate.request_tick();
        assert!(!gate.awaits_redraw(), "a state tick asks winit nothing");
        assert!(!gate.take_tick(t0 + 50 * MS), "rate-limited like any tick");
        assert_eq!(
            gate.deadline(t0 + 50 * MS, None),
            Some(t0 + TICK_INTERVAL),
            "and wakes the loop when the interval lifts"
        );
        let soon = t0 + 60 * MS;
        assert_eq!(
            gate.deadline(t0 + 50 * MS, Some(soon)),
            Some(soon),
            "it never delays a timer"
        );
        assert!(gate.take_tick(t0 + TICK_INTERVAL));
        assert!(!gate.take_tick(t0 + TICK_INTERVAL), "taken once");
    }

    #[test]
    fn a_state_tick_waits_for_a_redraw_already_on_its_way() {
        let t0 = Instant::now();
        let gate = RedrawGate::default();
        gate.ran_frame(t0 - TICK_INTERVAL);
        gate.request_with(t0, || {});
        gate.request_tick();
        assert!(
            !gate.take_tick(t0 + 10 * MS),
            "the redraw coming does the tick's work"
        );
        gate.delivered();
        assert!(!gate.take_tick(t0 + 20 * MS), "and it served it");
    }

    #[test]
    fn a_hidden_windows_due_timer_wakes_the_loop_after_the_interval() {
        let t0 = Instant::now();
        let gate = RedrawGate::default();
        gate.set_hidden(true);
        gate.ran_frame(t0);
        let past = t0 - 50 * MS;
        assert_eq!(
            gate.deadline(t0 + 10 * MS, Some(past)),
            Some(t0 + TICK_INTERVAL),
            "held from the last frame, not from whenever the loop asks"
        );
        // The hold is not pushed forward by asking again later.
        assert_eq!(
            gate.deadline(t0 + 90 * MS, Some(past)),
            Some(t0 + TICK_INTERVAL)
        );
        let far = t0 + Duration::from_secs(5);
        assert_eq!(gate.deadline(t0, Some(far)), Some(far));
        assert_eq!(gate.deadline(t0, None), None);
    }
}
