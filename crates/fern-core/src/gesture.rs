//! UIKit-style gesture recognizer model.
//!
//! Gesture recognizers are composable state machines attached to widgets.
//! Each recognizer monitors the raw pointer event stream and emits recognized
//! gestures when patterns complete. They are pure state machines with no
//! platform dependencies, making them trivially unit-testable.
//!
//! The [`GestureArena`] arbitrates when multiple recognizers compete on the
//! same event stream: all are fed in parallel, and when one recognizes, the
//! rest are reset (except cooperative peers — see
//! [`GestureRecognizer::resets_on_peer_recognition`]).
//!
//! **Click-style recognizers carry button + modifiers.** [`TapRecognizer`],
//! [`DoubleTapRecognizer`], [`TripleTapRecognizer`], and
//! [`LongPressRecognizer`] all default to [`ButtonMask::PRIMARY`] —
//! left-click only — and emit [`TapEvent`]s carrying position, the
//! finalising button, and modifier state. Multi-tap recognizers
//! require button-match across the whole sequence. Widen the accepted
//! set with `.accept_buttons(...)` / `.accept_any_button()`.

use std::time::{Duration, Instant};

use fern_canvas::{Point, Vec2};

use crate::event::{ButtonMask, Modifiers, PointerButton};

/// Information about a recognized click-style gesture, passed to the
/// four tap-family handlers (`on_tap`, `on_double_tap`, `on_triple_tap`,
/// `on_long_press`).
///
/// The struct is `#[non_exhaustive]` so future fields (timestamp, click
/// count for a hypothetical `on_n_tap`, pressure for stylus events) can
/// land without breaking existing match patterns.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct TapEvent {
    /// Pointer position in widget-local coords, captured at the
    /// finalising event (the `Up` of the last tap for tap / double-tap /
    /// triple-tap; the held `Down` for long-press, since long-press
    /// recognises on a `tick` before any `Up`).
    pub position: Point,

    /// Which button finalised the gesture. Multi-tap recognizers
    /// require every tap in the sequence to use the same button —
    /// mixed-button sequences fail rather than spuriously firing.
    pub button: PointerButton,

    /// Modifier keys held at the finalising event. Sourced from
    /// `WidgetEvent::PointerUp { modifiers, .. }` (or `PointerDown` for
    /// long-press).
    pub modifiers: Modifiers,
}

impl TapEvent {
    /// Construct a `TapEvent` directly. Useful for tests; widgets receive
    /// `&TapEvent` from the recognizer pipeline and rarely need to build
    /// one by hand.
    pub fn new(position: Point, button: PointerButton, modifiers: Modifiers) -> Self {
        Self {
            position,
            button,
            modifiers,
        }
    }
}

/// Raw pointer events fed into gesture recognizers.
#[derive(Debug, Clone, Copy)]
pub enum RawPointerEvent {
    Down {
        position: Point,
        button: PointerButton,
        modifiers: Modifiers,
    },
    Move {
        position: Point,
    },
    Up {
        position: Point,
        button: PointerButton,
        modifiers: Modifiers,
    },
}

/// Result of processing a raw event through a gesture recognizer.
#[derive(Debug, Clone)]
pub enum GestureResult {
    /// Not enough data yet — keep feeding events.
    Pending,
    /// A gesture has been recognized.
    Recognized(GestureEvent),
    /// This event sequence cannot match the gesture — recognizer should be reset.
    Failed,
}

/// A recognized gesture event.
///
/// The four click-style variants (`Tap` / `DoubleTap` / `TripleTap` /
/// `LongPress`) carry a [`TapEvent`] payload — pointer position, the
/// finalising mouse button, and the modifier state at that moment.
#[derive(Debug, Clone, Copy)]
pub enum GestureEvent {
    Tap(TapEvent),
    DoubleTap(TapEvent),
    TripleTap(TapEvent),
    LongPress(TapEvent),
    DragStarted {
        position: Point,
        button: PointerButton,
    },
    DragMoved {
        position: Point,
        delta: Vec2,
    },
    DragEnded {
        position: Point,
    },
    PinchStarted {
        center: Point,
    },
    PinchChanged {
        center: Point,
        scale: f32,
        rotation: f32,
    },
    PinchEnded,
    Swipe {
        direction: SwipeDirection,
        velocity: f32,
    },
}

/// Direction of a swipe gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeDirection {
    Left,
    Right,
    Up,
    Down,
}

/// Phase of a drag gesture, as delivered to an `on_drag` handler.
///
/// This is the public API for drag handlers — the raw `GestureEvent::Drag*`
/// variants are an implementation detail of the recognizer pipeline. A
/// handler only ever receives `Started` once, followed by zero or more
/// `Moved`, then exactly one `Ended`.
#[derive(Debug, Clone, Copy)]
pub enum DragPhase {
    Started {
        position: Point,
        button: PointerButton,
    },
    Moved {
        position: Point,
        delta: Vec2,
    },
    Ended {
        position: Point,
    },
}

/// Phase of a pinch (or rotation) gesture, as delivered to an `on_pinch`
/// handler. On desktop these are produced by OS trackpad gestures
/// (`TouchpadMagnify` / `RotationGesture`); on touch they come from a
/// dedicated recognizer.
#[derive(Debug, Clone, Copy)]
pub enum PinchPhase {
    Started {
        center: Point,
    },
    Changed {
        center: Point,
        scale: f32,
        rotation: f32,
    },
    Ended,
}

/// Trait for gesture recognizers. Each is a composable state machine.
pub trait GestureRecognizer {
    /// Feed a raw pointer event and return the recognition result.
    fn process(&mut self, event: &RawPointerEvent) -> GestureResult;

    /// Reset the recognizer to its initial state.
    fn reset(&mut self);

    /// Priority for arbitration when multiple recognizers compete.
    /// Higher priority wins.
    fn priority(&self) -> u32;

    /// Advance any time-driven state (e.g. long-press elapsed timer).
    /// Default is a no-op — only recognizers that depend on wall-clock
    /// time (like [`LongPressRecognizer`]) override this.
    fn tick(&mut self, _now: Instant) -> GestureResult {
        GestureResult::Pending
    }

    /// Earliest future `Instant` at which calling [`GestureRecognizer::tick`]
    /// could transition the recognizer into `Recognized` or `Failed`.
    /// Returns `None` when the recognizer is idle or not time-driven. Used
    /// by the event loop to schedule a wake-up before a long-press fires.
    fn next_deadline(&self) -> Option<Instant> {
        None
    }

    /// Whether this recognizer should be reset when a peer wins arbitration
    /// in the same `GestureArena::process` call. The default is `true` —
    /// winner-take-all, the usual behaviour for mutually exclusive gestures
    /// (tap vs drag, long-press vs tap). Multi-tap recognizers
    /// (`DoubleTapRecognizer`, `TripleTapRecognizer`) override this to
    /// `false` so a `DoubleTap` firing at click 2 does not wipe the
    /// `TripleTapRecognizer`'s accumulated state before click 3 arrives.
    fn resets_on_peer_recognition(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// TapRecognizer
// ---------------------------------------------------------------------------

/// Recognizes a single tap (pointer down + up without significant movement).
///
/// The recognizer is button-aware: it only fires when the press button
/// matches the release button, and only for buttons present in
/// [`accept`](TapRecognizer::accept_buttons). Default `accept` is
/// [`ButtonMask::PRIMARY`] — left-click only — which is what users
/// expect from a "tap" and keeps right-click free for context menus.
#[derive(Debug)]
pub struct TapRecognizer {
    max_distance: f32,
    accept: ButtonMask,
    down_position: Option<Point>,
    down_button: Option<PointerButton>,
}

impl TapRecognizer {
    pub fn new() -> Self {
        Self {
            max_distance: 5.0,
            accept: ButtonMask::PRIMARY,
            down_position: None,
            down_button: None,
        }
    }

    pub fn max_distance(mut self, d: f32) -> Self {
        self.max_distance = d;
        self
    }

    /// Restrict (or extend) the set of buttons that can fire this
    /// recognizer. Default is [`ButtonMask::PRIMARY`].
    pub fn accept_buttons(mut self, mask: impl Into<ButtonMask>) -> Self {
        self.accept = mask.into();
        self
    }

    /// Convenience: accept any pointer button.
    pub fn accept_any_button(self) -> Self {
        self.accept_buttons(ButtonMask::ALL)
    }
}

impl Default for TapRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureRecognizer for TapRecognizer {
    fn process(&mut self, event: &RawPointerEvent) -> GestureResult {
        match event {
            RawPointerEvent::Down {
                position, button, ..
            } => {
                if !self.accept.contains(*button) {
                    return GestureResult::Pending;
                }
                self.down_position = Some(*position);
                self.down_button = Some(*button);
                GestureResult::Pending
            }
            RawPointerEvent::Move { position } => {
                if let Some(down) = self.down_position {
                    let dx = position.x - down.x;
                    let dy = position.y - down.y;
                    if (dx * dx + dy * dy).sqrt() > self.max_distance {
                        self.down_position = None;
                        self.down_button = None;
                        return GestureResult::Failed;
                    }
                }
                GestureResult::Pending
            }
            RawPointerEvent::Up {
                position,
                button,
                modifiers,
            } => {
                let Some(down) = self.down_position.take() else {
                    return GestureResult::Failed;
                };
                let Some(down_button) = self.down_button.take() else {
                    return GestureResult::Failed;
                };
                if *button != down_button {
                    return GestureResult::Failed;
                }
                let dx = position.x - down.x;
                let dy = position.y - down.y;
                if (dx * dx + dy * dy).sqrt() <= self.max_distance {
                    return GestureResult::Recognized(GestureEvent::Tap(TapEvent {
                        position: *position,
                        button: *button,
                        modifiers: *modifiers,
                    }));
                }
                GestureResult::Failed
            }
        }
    }

    fn reset(&mut self) {
        self.down_position = None;
        self.down_button = None;
    }

    fn priority(&self) -> u32 {
        10
    }
}

// ---------------------------------------------------------------------------
// DoubleTapRecognizer
// ---------------------------------------------------------------------------

/// Recognizes a double-tap (two taps within a time window and distance,
/// using the same button).
///
/// Default `accept` is [`ButtonMask::PRIMARY`]; presses on other buttons
/// are ignored. Within the recognized sequence, both taps must match
/// the press button — a `Primary` then `Secondary` sequence resets to
/// the new tap as a fresh "first" rather than firing `DoubleTap`.
#[derive(Debug)]
pub struct DoubleTapRecognizer {
    max_distance: f32,
    max_interval: Duration,
    accept: ButtonMask,
    first_tap_position: Option<Point>,
    first_tap_time: Option<Instant>,
    first_tap_button: Option<PointerButton>,
    down_position: Option<Point>,
    down_button: Option<PointerButton>,
}

impl DoubleTapRecognizer {
    pub fn new() -> Self {
        Self {
            max_distance: 10.0,
            max_interval: Duration::from_millis(300),
            accept: ButtonMask::PRIMARY,
            first_tap_position: None,
            first_tap_time: None,
            first_tap_button: None,
            down_position: None,
            down_button: None,
        }
    }

    pub fn max_distance(mut self, d: f32) -> Self {
        self.max_distance = d;
        self
    }

    pub fn max_interval(mut self, interval: Duration) -> Self {
        self.max_interval = interval;
        self
    }

    /// Restrict (or extend) the set of buttons that can fire this
    /// recognizer. Default is [`ButtonMask::PRIMARY`].
    pub fn accept_buttons(mut self, mask: impl Into<ButtonMask>) -> Self {
        self.accept = mask.into();
        self
    }

    /// Convenience: accept any pointer button.
    pub fn accept_any_button(self) -> Self {
        self.accept_buttons(ButtonMask::ALL)
    }

    /// Feed an event with an explicit timestamp (for testability without real clocks).
    pub fn process_at(&mut self, event: &RawPointerEvent, now: Instant) -> GestureResult {
        match event {
            RawPointerEvent::Down {
                position, button, ..
            } => {
                if !self.accept.contains(*button) {
                    return GestureResult::Pending;
                }
                // Cross-tap button-match: if we have a first tap from a
                // different button, the new press starts fresh — reset
                // the accumulated state to avoid spuriously firing a
                // mixed-button DoubleTap.
                if let Some(first_button) = self.first_tap_button
                    && first_button != *button
                {
                    self.first_tap_position = None;
                    self.first_tap_time = None;
                    self.first_tap_button = None;
                }
                self.down_position = Some(*position);
                self.down_button = Some(*button);
                GestureResult::Pending
            }
            RawPointerEvent::Move { position } => {
                if let Some(down) = self.down_position
                    && distance(*position, down) > self.max_distance
                {
                    return GestureResult::Failed;
                }
                GestureResult::Pending
            }
            RawPointerEvent::Up {
                position,
                button,
                modifiers,
            } => {
                let Some(down) = self.down_position else {
                    return GestureResult::Failed;
                };
                let Some(down_button) = self.down_button else {
                    return GestureResult::Failed;
                };
                self.down_position = None;
                self.down_button = None;
                if *button != down_button {
                    return GestureResult::Failed;
                }
                if distance(*position, down) > self.max_distance {
                    return GestureResult::Failed;
                }

                if let (Some(first_pos), Some(first_time), Some(first_button)) = (
                    self.first_tap_position,
                    self.first_tap_time,
                    self.first_tap_button,
                ) {
                    // Second tap — check distance, time interval, AND
                    // button match against the first tap.
                    if first_button == *button
                        && distance(*position, first_pos) <= self.max_distance
                        && now.duration_since(first_time) <= self.max_interval
                    {
                        self.reset();
                        return GestureResult::Recognized(GestureEvent::DoubleTap(
                            TapEvent {
                                position: *position,
                                button: *button,
                                modifiers: *modifiers,
                            },
                        ));
                    }
                    // Out of window or button mismatch — treat as new
                    // first tap.
                    self.first_tap_position = Some(*position);
                    self.first_tap_time = Some(now);
                    self.first_tap_button = Some(*button);
                    GestureResult::Pending
                } else {
                    // First tap — record and wait for second.
                    self.first_tap_position = Some(*position);
                    self.first_tap_time = Some(now);
                    self.first_tap_button = Some(*button);
                    GestureResult::Pending
                }
            }
        }
    }
}

impl Default for DoubleTapRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureRecognizer for DoubleTapRecognizer {
    fn process(&mut self, event: &RawPointerEvent) -> GestureResult {
        self.process_at(event, Instant::now())
    }

    fn reset(&mut self) {
        self.first_tap_position = None;
        self.first_tap_time = None;
        self.first_tap_button = None;
        self.down_position = None;
        self.down_button = None;
    }

    fn priority(&self) -> u32 {
        15 // Higher than tap — double-tap should win over single tap
    }

    fn resets_on_peer_recognition(&self) -> bool {
        // Cooperative with `TripleTapRecognizer`: when we fire a DoubleTap
        // at click 2, the triple-tap recognizer may still be mid-sequence
        // waiting for click 3. The arena must not wipe triple-tap state
        // because of our win, and symmetrically we don't want our state
        // wiped by a triple-tap's win either (though we've already reset
        // ourselves internally by then).
        false
    }
}

// ---------------------------------------------------------------------------
// TripleTapRecognizer
// ---------------------------------------------------------------------------

/// Recognizes a triple tap (three taps within a time window and
/// distance, all using the same button).
///
/// State machine mirrors `DoubleTapRecognizer` with one extra step:
/// Idle → FirstTapLanded → SecondTapLanded → Recognized(TripleTap).
/// Defaults match `DoubleTapRecognizer` (300 ms / 10 px / Primary only)
/// so the two fire as a natural escalating pair. Mixed-button sequences
/// reset to a fresh first tap.
#[derive(Debug)]
pub struct TripleTapRecognizer {
    max_distance: f32,
    max_interval: Duration,
    accept: ButtonMask,
    first_tap_position: Option<Point>,
    first_tap_time: Option<Instant>,
    first_tap_button: Option<PointerButton>,
    second_tap_position: Option<Point>,
    second_tap_time: Option<Instant>,
    second_tap_button: Option<PointerButton>,
    down_position: Option<Point>,
    down_button: Option<PointerButton>,
}

impl TripleTapRecognizer {
    pub fn new() -> Self {
        Self {
            max_distance: 10.0,
            max_interval: Duration::from_millis(300),
            accept: ButtonMask::PRIMARY,
            first_tap_position: None,
            first_tap_time: None,
            first_tap_button: None,
            second_tap_position: None,
            second_tap_time: None,
            second_tap_button: None,
            down_position: None,
            down_button: None,
        }
    }

    pub fn max_distance(mut self, d: f32) -> Self {
        self.max_distance = d;
        self
    }

    pub fn max_interval(mut self, interval: Duration) -> Self {
        self.max_interval = interval;
        self
    }

    /// Restrict (or extend) the set of buttons that can fire this
    /// recognizer. Default is [`ButtonMask::PRIMARY`].
    pub fn accept_buttons(mut self, mask: impl Into<ButtonMask>) -> Self {
        self.accept = mask.into();
        self
    }

    /// Convenience: accept any pointer button.
    pub fn accept_any_button(self) -> Self {
        self.accept_buttons(ButtonMask::ALL)
    }

    /// Feed an event with an explicit timestamp (for testability without real clocks).
    pub fn process_at(&mut self, event: &RawPointerEvent, now: Instant) -> GestureResult {
        match event {
            RawPointerEvent::Down {
                position, button, ..
            } => {
                if !self.accept.contains(*button) {
                    return GestureResult::Pending;
                }
                // Cross-tap button-match: if any accumulated tap used a
                // different button, drop everything and start fresh.
                let mismatch = self
                    .first_tap_button
                    .map(|b| b != *button)
                    .unwrap_or(false)
                    || self.second_tap_button.map(|b| b != *button).unwrap_or(false);
                if mismatch {
                    self.first_tap_position = None;
                    self.first_tap_time = None;
                    self.first_tap_button = None;
                    self.second_tap_position = None;
                    self.second_tap_time = None;
                    self.second_tap_button = None;
                }
                self.down_position = Some(*position);
                self.down_button = Some(*button);
                GestureResult::Pending
            }
            RawPointerEvent::Move { position } => {
                if let Some(down) = self.down_position
                    && distance(*position, down) > self.max_distance
                {
                    return GestureResult::Failed;
                }
                GestureResult::Pending
            }
            RawPointerEvent::Up {
                position,
                button,
                modifiers,
            } => {
                let Some(down) = self.down_position else {
                    return GestureResult::Failed;
                };
                let Some(down_button) = self.down_button else {
                    return GestureResult::Failed;
                };
                self.down_position = None;
                self.down_button = None;
                if *button != down_button {
                    return GestureResult::Failed;
                }
                if distance(*position, down) > self.max_distance {
                    return GestureResult::Failed;
                }

                // Third tap landed — this is the third if both prior
                // timings AND buttons are in window/match.
                if let (
                    Some(first_pos),
                    Some(first_time),
                    Some(first_button),
                    Some(second_pos),
                    Some(second_time),
                    Some(second_button),
                ) = (
                    self.first_tap_position,
                    self.first_tap_time,
                    self.first_tap_button,
                    self.second_tap_position,
                    self.second_tap_time,
                    self.second_tap_button,
                ) {
                    if first_button == *button
                        && second_button == *button
                        && distance(*position, second_pos) <= self.max_distance
                        && now.duration_since(second_time) <= self.max_interval
                        && distance(second_pos, first_pos) <= self.max_distance
                        && second_time.duration_since(first_time) <= self.max_interval
                    {
                        self.reset();
                        return GestureResult::Recognized(GestureEvent::TripleTap(
                            TapEvent {
                                position: *position,
                                button: *button,
                                modifiers: *modifiers,
                            },
                        ));
                    }
                    // Out of window or button mismatch: fold this tap
                    // forward as a fresh first.
                    self.first_tap_position = Some(*position);
                    self.first_tap_time = Some(now);
                    self.first_tap_button = Some(*button);
                    self.second_tap_position = None;
                    self.second_tap_time = None;
                    self.second_tap_button = None;
                    return GestureResult::Pending;
                }

                // First or second tap.
                if let (Some(first_pos), Some(first_time), Some(first_button)) = (
                    self.first_tap_position,
                    self.first_tap_time,
                    self.first_tap_button,
                ) {
                    // Second tap — if in window AND button matches, promote.
                    if first_button == *button
                        && distance(*position, first_pos) <= self.max_distance
                        && now.duration_since(first_time) <= self.max_interval
                    {
                        self.second_tap_position = Some(*position);
                        self.second_tap_time = Some(now);
                        self.second_tap_button = Some(*button);
                        return GestureResult::Pending;
                    }
                    // Out of window or mismatch — treat as fresh first.
                    self.first_tap_position = Some(*position);
                    self.first_tap_time = Some(now);
                    self.first_tap_button = Some(*button);
                    self.second_tap_position = None;
                    self.second_tap_time = None;
                    self.second_tap_button = None;
                    return GestureResult::Pending;
                }

                // No prior tap — record as first.
                self.first_tap_position = Some(*position);
                self.first_tap_time = Some(now);
                self.first_tap_button = Some(*button);
                self.second_tap_position = None;
                self.second_tap_time = None;
                self.second_tap_button = None;
                GestureResult::Pending
            }
        }
    }
}

impl Default for TripleTapRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureRecognizer for TripleTapRecognizer {
    fn process(&mut self, event: &RawPointerEvent) -> GestureResult {
        self.process_at(event, Instant::now())
    }

    fn reset(&mut self) {
        self.first_tap_position = None;
        self.first_tap_time = None;
        self.first_tap_button = None;
        self.second_tap_position = None;
        self.second_tap_time = None;
        self.second_tap_button = None;
        self.down_position = None;
        self.down_button = None;
    }

    fn priority(&self) -> u32 {
        // Higher than DoubleTap so that when both would fire on the same
        // up event (shouldn't happen in practice — TripleTap only fires
        // after three taps and DoubleTap only at tap 2) TripleTap wins.
        20
    }

    fn resets_on_peer_recognition(&self) -> bool {
        // Cooperative with `DoubleTapRecognizer` — see the matching
        // override on DoubleTapRecognizer. The arena must not wipe our
        // accumulated first/second tap state when DoubleTap fires at
        // click 2, or click 3 would never promote us to TripleTap.
        false
    }
}

// ---------------------------------------------------------------------------
// LongPressRecognizer
// ---------------------------------------------------------------------------

/// Recognizes a long press (pointer held down beyond a duration without movement).
///
/// Because recognizers are pure state machines, the caller must drive time
/// by calling [`GestureRecognizer::tick`] when the timer fires (e.g. from
/// an event-loop deadline). The recognizer itself does not spawn timers.
/// The [`GestureRecognizer::next_deadline`] method exposes when the next
/// tick is needed so the event loop can wake up in time.
///
/// Default `accept` is [`ButtonMask::PRIMARY`]; presses on other buttons
/// are ignored. Modifiers are captured at the `Down` (since the
/// recognition timer fires before any `Up`) and surfaced through the
/// emitted [`TapEvent`].
#[derive(Debug)]
pub struct LongPressRecognizer {
    max_distance: f32,
    min_duration: Duration,
    accept: ButtonMask,
    down_position: Option<Point>,
    down_time: Option<Instant>,
    down_button: Option<PointerButton>,
    down_modifiers: Modifiers,
    recognized: bool,
}

impl LongPressRecognizer {
    pub fn new() -> Self {
        Self {
            max_distance: 5.0,
            min_duration: Duration::from_millis(500),
            accept: ButtonMask::PRIMARY,
            down_position: None,
            down_time: None,
            down_button: None,
            down_modifiers: Modifiers::NONE,
            recognized: false,
        }
    }

    pub fn max_distance(mut self, d: f32) -> Self {
        self.max_distance = d;
        self
    }

    pub fn min_duration(mut self, dur: Duration) -> Self {
        self.min_duration = dur;
        self
    }

    /// Restrict (or extend) the set of buttons that can fire this
    /// recognizer. Default is [`ButtonMask::PRIMARY`].
    pub fn accept_buttons(mut self, mask: impl Into<ButtonMask>) -> Self {
        self.accept = mask.into();
        self
    }

    /// Convenience: accept any pointer button.
    pub fn accept_any_button(self) -> Self {
        self.accept_buttons(ButtonMask::ALL)
    }

    #[cfg(test)]
    fn check_timeout(&mut self, now: Instant) -> GestureResult {
        self.tick(now)
    }
}

impl Default for LongPressRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureRecognizer for LongPressRecognizer {
    fn process(&mut self, event: &RawPointerEvent) -> GestureResult {
        match event {
            RawPointerEvent::Down {
                position,
                button,
                modifiers,
            } => {
                if !self.accept.contains(*button) {
                    return GestureResult::Pending;
                }
                self.down_position = Some(*position);
                self.down_time = Some(Instant::now());
                self.down_button = Some(*button);
                self.down_modifiers = *modifiers;
                self.recognized = false;
                GestureResult::Pending
            }
            RawPointerEvent::Move { position } => {
                if let Some(down) = self.down_position
                    && distance(*position, down) > self.max_distance
                {
                    return GestureResult::Failed;
                }
                GestureResult::Pending
            }
            RawPointerEvent::Up { .. } => {
                if self.recognized {
                    // Already fired — the up is just cleanup
                    self.reset();
                    GestureResult::Pending
                } else {
                    GestureResult::Failed
                }
            }
        }
    }

    fn reset(&mut self) {
        self.down_position = None;
        self.down_time = None;
        self.down_button = None;
        self.down_modifiers = Modifiers::NONE;
        self.recognized = false;
    }

    fn priority(&self) -> u32 {
        25 // Higher than drag — long press wins over drag
    }

    fn tick(&mut self, now: Instant) -> GestureResult {
        if self.recognized {
            return GestureResult::Pending;
        }
        if let (Some(pos), Some(time), Some(button)) =
            (self.down_position, self.down_time, self.down_button)
            && now.duration_since(time) >= self.min_duration
        {
            self.recognized = true;
            return GestureResult::Recognized(GestureEvent::LongPress(TapEvent {
                position: pos,
                button,
                modifiers: self.down_modifiers,
            }));
        }
        GestureResult::Pending
    }

    fn next_deadline(&self) -> Option<Instant> {
        if self.recognized {
            return None;
        }
        self.down_time.map(|t| t + self.min_duration)
    }
}

// ---------------------------------------------------------------------------
// DragRecognizer
// ---------------------------------------------------------------------------

/// Recognizes a drag gesture (pointer down + move beyond threshold).
#[derive(Debug)]
pub struct DragRecognizer {
    threshold: f32,
    down_position: Option<Point>,
    down_button: Option<PointerButton>,
    dragging: bool,
    last_position: Option<Point>,
}

impl DragRecognizer {
    pub fn new() -> Self {
        Self {
            threshold: 5.0,
            down_position: None,
            down_button: None,
            dragging: false,
            last_position: None,
        }
    }

    pub fn threshold(mut self, t: f32) -> Self {
        self.threshold = t;
        self
    }
}

impl Default for DragRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureRecognizer for DragRecognizer {
    fn process(&mut self, event: &RawPointerEvent) -> GestureResult {
        match event {
            RawPointerEvent::Down {
                position, button, ..
            } => {
                self.down_position = Some(*position);
                self.down_button = Some(*button);
                self.last_position = Some(*position);
                self.dragging = false;
                GestureResult::Pending
            }
            RawPointerEvent::Move { position } => {
                let Some(down) = self.down_position else {
                    return GestureResult::Pending;
                };

                if self.dragging {
                    let last = self.last_position.unwrap_or(down);
                    let delta = Vec2::new(position.x - last.x, position.y - last.y);
                    self.last_position = Some(*position);
                    return GestureResult::Recognized(GestureEvent::DragMoved {
                        position: *position,
                        delta,
                    });
                }

                let dx = position.x - down.x;
                let dy = position.y - down.y;
                if (dx * dx + dy * dy).sqrt() >= self.threshold {
                    self.dragging = true;
                    self.last_position = Some(*position);
                    // `DragStarted.position` reports the *initial press*
                    // (where `down_position` was stored on `Down`), not the
                    // threshold-crossing position. Widgets that need to
                    // classify where the grab originated (e.g. ScrollBar's
                    // thumb-vs-track hit test) can rely on this; widgets
                    // that want the current pointer will get it in the
                    // immediately-following `DragMoved`.
                    return GestureResult::Recognized(GestureEvent::DragStarted {
                        position: down,
                        button: self.down_button.unwrap_or(PointerButton::Primary),
                    });
                }

                GestureResult::Pending
            }
            RawPointerEvent::Up { position, .. } => {
                if self.dragging {
                    self.dragging = false;
                    self.down_position = None;
                    return GestureResult::Recognized(GestureEvent::DragEnded {
                        position: *position,
                    });
                }
                self.down_position = None;
                GestureResult::Failed
            }
        }
    }

    fn reset(&mut self) {
        self.down_position = None;
        self.down_button = None;
        self.dragging = false;
        self.last_position = None;
    }

    fn priority(&self) -> u32 {
        20
    }
}

// ---------------------------------------------------------------------------
// SwipeRecognizer
// ---------------------------------------------------------------------------

/// Recognizes a swipe gesture (quick directional movement above velocity threshold).
#[derive(Debug)]
pub struct SwipeRecognizer {
    min_velocity: f32,
    min_distance: f32,
    max_cross_ratio: f32,
    down_position: Option<Point>,
    down_time: Option<Instant>,
}

impl SwipeRecognizer {
    pub fn new() -> Self {
        Self {
            min_velocity: 200.0,  // pixels per second
            min_distance: 30.0,   // minimum swipe distance
            max_cross_ratio: 0.5, // max perpendicular/parallel ratio
            down_position: None,
            down_time: None,
        }
    }

    pub fn min_velocity(mut self, v: f32) -> Self {
        self.min_velocity = v;
        self
    }

    pub fn min_distance(mut self, d: f32) -> Self {
        self.min_distance = d;
        self
    }

    /// Process with an explicit timestamp for testability.
    pub fn process_at(&mut self, event: &RawPointerEvent, now: Instant) -> GestureResult {
        match event {
            RawPointerEvent::Down { position, .. } => {
                self.down_position = Some(*position);
                self.down_time = Some(now);
                GestureResult::Pending
            }
            RawPointerEvent::Move { .. } => GestureResult::Pending,
            RawPointerEvent::Up { position, .. } => {
                let (Some(down), Some(time)) = (self.down_position, self.down_time) else {
                    return GestureResult::Failed;
                };

                let dx = position.x - down.x;
                let dy = position.y - down.y;
                let dist = (dx * dx + dy * dy).sqrt();
                let elapsed = now.duration_since(time).as_secs_f32();

                if dist < self.min_distance || elapsed <= 0.0 {
                    self.reset();
                    return GestureResult::Failed;
                }

                let velocity = dist / elapsed;
                if velocity < self.min_velocity {
                    self.reset();
                    return GestureResult::Failed;
                }

                let abs_dx = dx.abs();
                let abs_dy = dy.abs();

                // Determine primary axis and check cross-axis ratio
                let (direction, cross_ratio) = if abs_dx >= abs_dy {
                    let dir = if dx > 0.0 {
                        SwipeDirection::Right
                    } else {
                        SwipeDirection::Left
                    };
                    (dir, abs_dy / abs_dx.max(0.001))
                } else {
                    let dir = if dy > 0.0 {
                        SwipeDirection::Down
                    } else {
                        SwipeDirection::Up
                    };
                    (dir, abs_dx / abs_dy.max(0.001))
                };

                if cross_ratio > self.max_cross_ratio {
                    self.reset();
                    return GestureResult::Failed;
                }

                self.reset();
                GestureResult::Recognized(GestureEvent::Swipe {
                    direction,
                    velocity,
                })
            }
        }
    }
}

impl Default for SwipeRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureRecognizer for SwipeRecognizer {
    fn process(&mut self, event: &RawPointerEvent) -> GestureResult {
        self.process_at(event, Instant::now())
    }

    fn reset(&mut self) {
        self.down_position = None;
        self.down_time = None;
    }

    fn priority(&self) -> u32 {
        30 // High priority — swipe is decisive
    }
}

// ---------------------------------------------------------------------------
// GestureArena — multi-recognizer arbitration
// ---------------------------------------------------------------------------

/// Arbitrates among multiple gesture recognizers competing on the same event
/// stream. All recognizers are fed each event in parallel. When one recognizes,
/// the others are reset. Failed recognizers are excluded from future events
/// until the next sequence (pointer up resets all).
pub struct GestureArena {
    entries: Vec<ArenaEntry>,
}

struct ArenaEntry {
    recognizer: Box<dyn GestureRecognizer>,
    failed: bool,
}

impl GestureArena {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Add a recognizer to the arena.
    pub fn add(&mut self, recognizer: impl GestureRecognizer + 'static) {
        self.entries.push(ArenaEntry {
            recognizer: Box::new(recognizer),
            failed: false,
        });
    }

    /// Feed a raw pointer event to all active recognizers.
    ///
    /// Returns the recognized gesture from the highest-priority recognizer,
    /// or `None` if no gesture was recognized yet.
    pub fn process(&mut self, event: &RawPointerEvent) -> Option<GestureEvent> {
        // On pointer down, give every recognizer a fresh chance — clear
        // the per-arena `failed` flag so a recognizer that failed on the
        // previous sequence can compete again. We deliberately do NOT call
        // `recognizer.reset()` here: the recognizers all overwrite their
        // per-sequence state on `RawPointerEvent::Down` themselves, and
        // calling `reset()` would wipe legitimate cross-sequence state
        // (notably `DoubleTapRecognizer::first_tap_time`, which must
        // survive the second `Down` for the double-tap to be recognized).
        if matches!(event, RawPointerEvent::Down { .. }) {
            for entry in &mut self.entries {
                entry.failed = false;
            }
        }

        let mut best: Option<(usize, u32, GestureEvent)> = None;

        for (i, entry) in self.entries.iter_mut().enumerate() {
            if entry.failed {
                continue;
            }
            match entry.recognizer.process(event) {
                GestureResult::Recognized(gesture) => {
                    let prio = entry.recognizer.priority();
                    if best.as_ref().is_none_or(|(_, bp, _)| prio > *bp) {
                        best = Some((i, prio, gesture));
                    }
                }
                GestureResult::Failed => {
                    entry.failed = true;
                }
                GestureResult::Pending => {}
            }
        }

        if let Some((winner_idx, _, _)) = &best {
            // Winner recognized — reset all non-winning recognizers.
            // The winner keeps its state (important for multi-event gestures
            // like drag, which fire DragStart then DragUpdate then DragEnd).
            // Peers that explicitly opt out via `resets_on_peer_recognition`
            // (multi-tap family) keep their state so an escalating sequence
            // (tap → double tap → triple tap) can progress across winners.
            for (i, entry) in self.entries.iter_mut().enumerate() {
                if i == *winner_idx || entry.failed {
                    continue;
                }
                if !entry.recognizer.resets_on_peer_recognition() {
                    continue;
                }
                entry.recognizer.reset();
                entry.failed = false;
            }
        }

        best.map(|(_, _, gesture)| gesture)
    }

    /// Advance time-driven recognizers (notably `LongPressRecognizer`) and
    /// return the highest-priority gesture that just transitioned to
    /// `Recognized`, if any. Must be called by the event loop on each wake
    /// so long-press fires without requiring further pointer traffic.
    pub fn tick(&mut self, now: Instant) -> Option<GestureEvent> {
        let mut best: Option<(usize, u32, GestureEvent)> = None;

        for (i, entry) in self.entries.iter_mut().enumerate() {
            if entry.failed {
                continue;
            }
            match entry.recognizer.tick(now) {
                GestureResult::Recognized(gesture) => {
                    let prio = entry.recognizer.priority();
                    if best.as_ref().is_none_or(|(_, bp, _)| prio > *bp) {
                        best = Some((i, prio, gesture));
                    }
                }
                GestureResult::Failed => {
                    entry.failed = true;
                }
                GestureResult::Pending => {}
            }
        }

        if let Some((winner_idx, _, _)) = &best {
            for (i, entry) in self.entries.iter_mut().enumerate() {
                if i == *winner_idx || entry.failed {
                    continue;
                }
                if !entry.recognizer.resets_on_peer_recognition() {
                    continue;
                }
                entry.recognizer.reset();
                entry.failed = false;
            }
        }

        best.map(|(_, _, gesture)| gesture)
    }

    /// Earliest wall-clock instant at which any recognizer in this arena
    /// would like `tick()` to be called. Returns `None` if no recognizer
    /// has a pending time-driven transition.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.entries
            .iter()
            .filter(|entry| !entry.failed)
            .filter_map(|entry| entry.recognizer.next_deadline())
            .min()
    }

    /// Reset all recognizers in the arena.
    pub fn reset(&mut self) {
        for entry in &mut self.entries {
            entry.recognizer.reset();
            entry.failed = false;
        }
    }

    /// Returns true if the arena has any recognizers.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of recognizers in the arena.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for GestureArena {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for GestureArena {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureArena")
            .field("num_recognizers", &self.entries.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn distance(a: Point, b: Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ─── helpers ──────────────────────────────────────────────────────────

    fn down(pos: Point) -> RawPointerEvent {
        down_btn(pos, PointerButton::Primary)
    }

    fn down_btn(pos: Point, button: PointerButton) -> RawPointerEvent {
        down_full(pos, button, Modifiers::NONE)
    }

    fn down_full(pos: Point, button: PointerButton, modifiers: Modifiers) -> RawPointerEvent {
        RawPointerEvent::Down {
            position: pos,
            button,
            modifiers,
        }
    }

    fn up(pos: Point) -> RawPointerEvent {
        up_btn(pos, PointerButton::Primary)
    }

    fn up_btn(pos: Point, button: PointerButton) -> RawPointerEvent {
        up_full(pos, button, Modifiers::NONE)
    }

    fn up_full(pos: Point, button: PointerButton, modifiers: Modifiers) -> RawPointerEvent {
        RawPointerEvent::Up {
            position: pos,
            button,
            modifiers,
        }
    }

    fn move_to(pos: Point) -> RawPointerEvent {
        RawPointerEvent::Move { position: pos }
    }

    // --- TapRecognizer ---

    #[test]
    fn tap_recognized_on_quick_down_up() {
        let mut rec = TapRecognizer::new();
        assert!(matches!(
            rec.process(&down(Point::new(10.0, 10.0))),
            GestureResult::Pending
        ));
        assert!(matches!(
            rec.process(&up(Point::new(10.0, 10.0))),
            GestureResult::Recognized(GestureEvent::Tap(_))
        ));
    }

    #[test]
    fn tap_fails_if_moved_too_far() {
        let mut rec = TapRecognizer::new().max_distance(5.0);
        rec.process(&down(Point::new(10.0, 10.0)));
        assert!(matches!(
            rec.process(&move_to(Point::new(20.0, 10.0))),
            GestureResult::Failed
        ));
    }

    // --- TapRecognizer: button filter + modifiers ---

    #[test]
    fn tap_default_filters_secondary_button() {
        let mut rec = TapRecognizer::new();
        // Right-click: Down is silently ignored (still Pending), Up
        // sees no down_position recorded and fails — no Tap fires.
        assert!(matches!(
            rec.process(&down_btn(Point::new(10.0, 10.0), PointerButton::Secondary)),
            GestureResult::Pending
        ));
        assert!(matches!(
            rec.process(&up_btn(Point::new(10.0, 10.0), PointerButton::Secondary)),
            GestureResult::Failed
        ));
    }

    #[test]
    fn tap_default_filters_middle_button() {
        let mut rec = TapRecognizer::new();
        assert!(matches!(
            rec.process(&down_btn(Point::new(10.0, 10.0), PointerButton::Middle)),
            GestureResult::Pending
        ));
        assert!(matches!(
            rec.process(&up_btn(Point::new(10.0, 10.0), PointerButton::Middle)),
            GestureResult::Failed
        ));
    }

    #[test]
    fn tap_accept_secondary_recognises_right_click() {
        let mut rec = TapRecognizer::new().accept_buttons(ButtonMask::SECONDARY);
        rec.process(&down_btn(Point::new(10.0, 10.0), PointerButton::Secondary));
        match rec.process(&up_btn(Point::new(10.0, 10.0), PointerButton::Secondary)) {
            GestureResult::Recognized(GestureEvent::Tap(event)) => {
                assert_eq!(event.button, PointerButton::Secondary);
            }
            other => panic!("expected Tap on right-click, got {:?}", other),
        }
    }

    #[test]
    fn tap_button_mismatch_fails() {
        let mut rec = TapRecognizer::new().accept_any_button();
        rec.process(&down_btn(Point::new(10.0, 10.0), PointerButton::Primary));
        // Up with a different button → Failed, no Tap.
        assert!(matches!(
            rec.process(&up_btn(Point::new(10.0, 10.0), PointerButton::Secondary)),
            GestureResult::Failed
        ));
    }

    #[test]
    fn tap_carries_modifiers_from_up() {
        let mut rec = TapRecognizer::new();
        rec.process(&down_full(
            Point::new(10.0, 10.0),
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        match rec.process(&up_full(
            Point::new(10.0, 10.0),
            PointerButton::Primary,
            Modifiers::CTRL | Modifiers::SHIFT,
        )) {
            GestureResult::Recognized(GestureEvent::Tap(event)) => {
                assert!(event.modifiers.ctrl());
                assert!(event.modifiers.shift());
                assert!(!event.modifiers.alt());
            }
            other => panic!("expected Tap with modifiers, got {:?}", other),
        }
    }

    #[test]
    fn tap_recognised_for_forward_or_back_when_accepted() {
        let mut rec = TapRecognizer::new()
            .accept_buttons(ButtonMask::FORWARD | ButtonMask::BACK);
        rec.process(&down_btn(Point::new(0.0, 0.0), PointerButton::Forward));
        assert!(matches!(
            rec.process(&up_btn(Point::new(0.0, 0.0), PointerButton::Forward)),
            GestureResult::Recognized(GestureEvent::Tap(_))
        ));
        rec.process(&down_btn(Point::new(0.0, 0.0), PointerButton::Back));
        assert!(matches!(
            rec.process(&up_btn(Point::new(0.0, 0.0), PointerButton::Back)),
            GestureResult::Recognized(GestureEvent::Tap(_))
        ));
    }

    // --- DoubleTapRecognizer ---

    #[test]
    fn double_tap_recognized_within_interval() {
        let mut rec = DoubleTapRecognizer::new().max_interval(Duration::from_millis(500));
        let t0 = Instant::now();
        let p = Point::new(10.0, 10.0);

        // First tap
        rec.process_at(&down(p), t0);
        rec.process_at(&up(p), t0 + Duration::from_millis(50));

        // Second tap within interval
        rec.process_at(&down(Point::new(11.0, 10.0)), t0 + Duration::from_millis(200));
        let result = rec.process_at(
            &up(Point::new(11.0, 10.0)),
            t0 + Duration::from_millis(250),
        );
        assert!(matches!(
            result,
            GestureResult::Recognized(GestureEvent::DoubleTap(_))
        ));
    }

    #[test]
    fn double_tap_fails_if_too_slow() {
        let mut rec = DoubleTapRecognizer::new().max_interval(Duration::from_millis(300));
        let t0 = Instant::now();
        let p = Point::new(10.0, 10.0);

        rec.process_at(&down(p), t0);
        rec.process_at(&up(p), t0 + Duration::from_millis(50));

        rec.process_at(&down(p), t0 + Duration::from_millis(400));
        let result = rec.process_at(&up(p), t0 + Duration::from_millis(450));
        // Should be Pending (treated as new first tap), not Recognized
        assert!(matches!(result, GestureResult::Pending));
    }

    #[test]
    fn double_tap_fails_if_too_far() {
        let mut rec = DoubleTapRecognizer::new().max_distance(5.0);
        let t0 = Instant::now();
        let p = Point::new(10.0, 10.0);

        rec.process_at(&down(p), t0);
        rec.process_at(&up(p), t0 + Duration::from_millis(50));

        // Second tap too far from first
        rec.process_at(&down(Point::new(50.0, 50.0)), t0 + Duration::from_millis(100));
        let result = rec.process_at(
            &up(Point::new(50.0, 50.0)),
            t0 + Duration::from_millis(150),
        );
        // Treated as new first tap
        assert!(matches!(result, GestureResult::Pending));
    }

    #[test]
    fn double_tap_button_mismatch_fails_at_second_down() {
        // First tap Primary, second tap Secondary → no DoubleTap. The
        // second tap is recorded as a fresh first instead.
        let mut rec = DoubleTapRecognizer::new().accept_any_button();
        let t0 = Instant::now();
        let p = Point::new(10.0, 10.0);

        rec.process_at(&down_btn(p, PointerButton::Primary), t0);
        rec.process_at(&up_btn(p, PointerButton::Primary), t0 + Duration::from_millis(50));

        rec.process_at(
            &down_btn(p, PointerButton::Secondary),
            t0 + Duration::from_millis(150),
        );
        let result = rec.process_at(
            &up_btn(p, PointerButton::Secondary),
            t0 + Duration::from_millis(200),
        );
        assert!(matches!(result, GestureResult::Pending));
    }

    #[test]
    fn double_tap_carries_modifiers_from_second_up() {
        let mut rec = DoubleTapRecognizer::new();
        let t0 = Instant::now();
        let p = Point::new(10.0, 10.0);

        rec.process_at(&down(p), t0);
        rec.process_at(&up(p), t0 + Duration::from_millis(50));

        rec.process_at(&down(p), t0 + Duration::from_millis(150));
        let result = rec.process_at(
            &up_full(p, PointerButton::Primary, Modifiers::SHIFT),
            t0 + Duration::from_millis(200),
        );
        match result {
            GestureResult::Recognized(GestureEvent::DoubleTap(event)) => {
                assert!(event.modifiers.shift());
            }
            other => panic!("expected DoubleTap with shift, got {:?}", other),
        }
    }

    // --- LongPressRecognizer ---

    #[test]
    fn long_press_recognized_after_timeout() {
        let mut rec = LongPressRecognizer::new().min_duration(Duration::from_millis(500));
        rec.process(&down(Point::new(10.0, 10.0)));

        let down_time = rec.down_time.unwrap();

        // Not yet
        assert!(matches!(
            rec.check_timeout(down_time + Duration::from_millis(200)),
            GestureResult::Pending
        ));

        // Now!
        assert!(matches!(
            rec.check_timeout(down_time + Duration::from_millis(600)),
            GestureResult::Recognized(GestureEvent::LongPress(_))
        ));
    }

    #[test]
    fn long_press_fails_on_movement() {
        let mut rec = LongPressRecognizer::new().max_distance(5.0);
        rec.process(&down(Point::new(10.0, 10.0)));
        assert!(matches!(
            rec.process(&move_to(Point::new(30.0, 30.0))),
            GestureResult::Failed
        ));
    }

    #[test]
    fn long_press_fails_on_early_up() {
        let mut rec = LongPressRecognizer::new();
        rec.process(&down(Point::new(10.0, 10.0)));
        assert!(matches!(
            rec.process(&up(Point::new(10.0, 10.0))),
            GestureResult::Failed
        ));
    }

    #[test]
    fn long_press_default_filters_secondary() {
        let mut rec = LongPressRecognizer::new().min_duration(Duration::from_millis(50));
        // Right-click Down is silently ignored.
        rec.process(&down_btn(Point::new(0.0, 0.0), PointerButton::Secondary));
        // Even after the timeout, no LongPress fires because no down
        // state was captured.
        let later = Instant::now() + Duration::from_millis(500);
        assert!(matches!(rec.check_timeout(later), GestureResult::Pending));
    }

    #[test]
    fn long_press_carries_modifiers_from_down() {
        let mut rec = LongPressRecognizer::new().min_duration(Duration::from_millis(50));
        rec.process(&down_full(
            Point::new(0.0, 0.0),
            PointerButton::Primary,
            Modifiers::SHIFT,
        ));
        let down_time = rec.down_time.unwrap();
        match rec.check_timeout(down_time + Duration::from_millis(100)) {
            GestureResult::Recognized(GestureEvent::LongPress(event)) => {
                assert!(event.modifiers.shift());
                assert_eq!(event.button, PointerButton::Primary);
            }
            other => panic!("expected LongPress, got {:?}", other),
        }
    }

    // --- DragRecognizer ---

    #[test]
    fn drag_recognizer_fires_after_threshold() {
        let mut rec = DragRecognizer::new().threshold(5.0);

        assert!(matches!(rec.process(&down(Point::new(10.0, 10.0))), GestureResult::Pending));

        // Small move — still pending
        assert!(matches!(
            rec.process(&move_to(Point::new(12.0, 10.0))),
            GestureResult::Pending
        ));

        // Large move — drag started
        assert!(matches!(
            rec.process(&move_to(Point::new(20.0, 10.0))),
            GestureResult::Recognized(GestureEvent::DragStarted { .. })
        ));
    }

    #[test]
    fn drag_emits_moved_and_ended() {
        let mut rec = DragRecognizer::new().threshold(1.0);
        rec.process(&down(Point::new(0.0, 0.0)));
        // Cross threshold
        rec.process(&move_to(Point::new(10.0, 0.0)));

        // Subsequent move
        let result = rec.process(&move_to(Point::new(15.0, 0.0)));
        assert!(matches!(
            result,
            GestureResult::Recognized(GestureEvent::DragMoved { .. })
        ));

        // Release
        let result = rec.process(&up(Point::new(15.0, 0.0)));
        assert!(matches!(
            result,
            GestureResult::Recognized(GestureEvent::DragEnded { .. })
        ));
    }

    #[test]
    fn drag_fails_on_up_without_movement() {
        let mut rec = DragRecognizer::new().threshold(5.0);
        rec.process(&down(Point::new(10.0, 10.0)));
        assert!(matches!(rec.process(&up(Point::new(10.0, 10.0))), GestureResult::Failed));
    }

    #[test]
    fn reset_clears_state() {
        let mut rec = DragRecognizer::new().threshold(1.0);
        rec.process(&down(Point::new(0.0, 0.0)));
        rec.process(&move_to(Point::new(10.0, 0.0)));
        rec.reset();

        // After reset, move without down should be pending (not drag)
        assert!(matches!(
            rec.process(&move_to(Point::new(20.0, 0.0))),
            GestureResult::Pending
        ));
    }

    // --- SwipeRecognizer ---

    #[test]
    fn swipe_right_recognized() {
        let mut rec = SwipeRecognizer::new()
            .min_velocity(100.0)
            .min_distance(20.0);
        let t0 = Instant::now();

        rec.process_at(&down(Point::new(10.0, 50.0)), t0);
        let result = rec.process_at(
            &up(Point::new(200.0, 55.0)),
            t0 + Duration::from_millis(100),
        );
        match result {
            GestureResult::Recognized(GestureEvent::Swipe {
                direction,
                velocity,
            }) => {
                assert_eq!(direction, SwipeDirection::Right);
                assert!(velocity > 100.0);
            }
            other => panic!("Expected Swipe, got {:?}", other),
        }
    }

    #[test]
    fn swipe_left_recognized() {
        let mut rec = SwipeRecognizer::new()
            .min_velocity(100.0)
            .min_distance(20.0);
        let t0 = Instant::now();

        rec.process_at(&down(Point::new(200.0, 50.0)), t0);
        let result = rec.process_at(
            &up(Point::new(10.0, 55.0)),
            t0 + Duration::from_millis(100),
        );
        assert!(matches!(
            result,
            GestureResult::Recognized(GestureEvent::Swipe {
                direction: SwipeDirection::Left,
                ..
            })
        ));
    }

    #[test]
    fn swipe_fails_if_too_slow() {
        let mut rec = SwipeRecognizer::new().min_velocity(500.0);
        let t0 = Instant::now();

        rec.process_at(&down(Point::new(10.0, 10.0)), t0);
        let result = rec.process_at(
            &up(Point::new(50.0, 10.0)),
            t0 + Duration::from_secs(5), // Very slow
        );
        assert!(matches!(result, GestureResult::Failed));
    }

    #[test]
    fn swipe_fails_if_diagonal() {
        let mut rec = SwipeRecognizer::new()
            .min_velocity(100.0)
            .min_distance(20.0);
        let t0 = Instant::now();

        rec.process_at(&down(Point::new(10.0, 10.0)), t0);
        // Equal dx and dy — diagonal, cross_ratio = 1.0 > 0.5
        let result = rec.process_at(
            &up(Point::new(100.0, 100.0)),
            t0 + Duration::from_millis(100),
        );
        assert!(matches!(result, GestureResult::Failed));
    }

    // --- GestureArena ---

    #[test]
    fn arena_tap_wins_over_nothing() {
        let mut arena = GestureArena::new();
        arena.add(TapRecognizer::new());

        arena.process(&down(Point::new(10.0, 10.0)));
        let result = arena.process(&up(Point::new(10.0, 10.0)));
        assert!(matches!(result, Some(GestureEvent::Tap(_))));
    }

    #[test]
    fn arena_drag_wins_over_tap_on_movement() {
        let mut arena = GestureArena::new();
        arena.add(TapRecognizer::new().max_distance(5.0));
        arena.add(DragRecognizer::new().threshold(5.0));

        arena.process(&down(Point::new(10.0, 10.0)));

        // Move beyond both thresholds — drag recognized (higher priority)
        let result = arena.process(&move_to(Point::new(30.0, 10.0)));
        assert!(matches!(result, Some(GestureEvent::DragStarted { .. })));
    }

    #[test]
    fn arena_tap_recognized_when_drag_fails() {
        let mut arena = GestureArena::new();
        arena.add(TapRecognizer::new());
        arena.add(DragRecognizer::new().threshold(5.0));

        arena.process(&down(Point::new(10.0, 10.0)));

        // Up without significant movement — drag fails, tap wins
        let result = arena.process(&up(Point::new(10.0, 10.0)));
        assert!(matches!(result, Some(GestureEvent::Tap(_))));
    }

    // --- TripleTapRecognizer ---

    #[test]
    fn triple_tap_recognized_within_intervals() {
        let mut rec = TripleTapRecognizer::new();
        let t0 = Instant::now();
        let p = Point::new(10.0, 10.0);

        // Three taps all within window, all at (10, 10).
        for i in 0..3 {
            let offset = Duration::from_millis(200 * i as u64);
            rec.process_at(&down(p), t0 + offset);
            let result = rec.process_at(&up(p), t0 + offset + Duration::from_millis(50));
            if i < 2 {
                assert!(matches!(result, GestureResult::Pending));
            } else {
                assert!(matches!(
                    result,
                    GestureResult::Recognized(GestureEvent::TripleTap(_))
                ));
            }
        }
    }

    #[test]
    fn triple_tap_fails_if_third_is_too_slow() {
        let mut rec = TripleTapRecognizer::new().max_interval(Duration::from_millis(300));
        let t0 = Instant::now();
        let stamp = |ms| t0 + Duration::from_millis(ms);
        let p = Point::new(10.0, 10.0);

        rec.process_at(&down(p), stamp(0));
        rec.process_at(&up(p), stamp(50));
        rec.process_at(&down(p), stamp(200));
        rec.process_at(&up(p), stamp(250));

        // Third tap > 300 ms after the second — does not recognize.
        rec.process_at(&down(p), stamp(700));
        let result = rec.process_at(&up(p), stamp(750));
        assert!(matches!(result, GestureResult::Pending));
    }

    #[test]
    fn triple_tap_fails_if_third_is_too_far() {
        let mut rec = TripleTapRecognizer::new().max_distance(5.0);
        let t0 = Instant::now();
        let stamp = |ms| t0 + Duration::from_millis(ms);
        let p = Point::new(10.0, 10.0);

        rec.process_at(&down(p), stamp(0));
        rec.process_at(&up(p), stamp(50));
        rec.process_at(&down(p), stamp(100));
        rec.process_at(&up(p), stamp(150));

        // Third tap > 5 px from the second.
        rec.process_at(&down(Point::new(30.0, 10.0)), stamp(200));
        let result = rec.process_at(&up(Point::new(30.0, 10.0)), stamp(250));
        assert!(matches!(result, GestureResult::Pending));
    }

    #[test]
    fn triple_tap_button_mismatch_fails_at_third_down() {
        // Third tap with a different button → no TripleTap. The
        // second-tap state collapses and the new tap becomes a fresh
        // first.
        let mut rec = TripleTapRecognizer::new().accept_any_button();
        let t0 = Instant::now();
        let stamp = |ms| t0 + Duration::from_millis(ms);
        let p = Point::new(10.0, 10.0);

        rec.process_at(&down_btn(p, PointerButton::Primary), stamp(0));
        rec.process_at(&up_btn(p, PointerButton::Primary), stamp(50));
        rec.process_at(&down_btn(p, PointerButton::Primary), stamp(150));
        rec.process_at(&up_btn(p, PointerButton::Primary), stamp(200));

        rec.process_at(&down_btn(p, PointerButton::Secondary), stamp(300));
        let result = rec.process_at(&up_btn(p, PointerButton::Secondary), stamp(350));
        assert!(matches!(result, GestureResult::Pending));
    }

    #[test]
    fn arena_double_and_triple_tap_cooperate() {
        // Regression for the cooperative-recognizer contract: both
        // `DoubleTapRecognizer` and `TripleTapRecognizer` must observe
        // the full click sequence. Without `resets_on_peer_recognition =
        // false` on both, DoubleTap's win at click 2 would reset the
        // TripleTapRecognizer and click 3 would never fire TripleTap.
        let mut arena = GestureArena::new();
        arena.add(DoubleTapRecognizer::new());
        arena.add(TripleTapRecognizer::new());

        let pos = Point::new(10.0, 10.0);

        // Click 1 — both recognizers pending.
        assert!(arena.process(&down(pos)).is_none());
        assert!(arena.process(&up(pos)).is_none());

        // Click 2 — DoubleTap fires.
        assert!(arena.process(&down(pos)).is_none());
        let second = arena.process(&up(pos));
        assert!(
            matches!(second, Some(GestureEvent::DoubleTap(_))),
            "click 2 must produce DoubleTap, got {:?}",
            second
        );

        // Click 3 — TripleTap fires. If the arena reset TripleTapRecognizer
        // after DoubleTap won, this would be None.
        assert!(arena.process(&down(pos)).is_none());
        let third = arena.process(&up(pos));
        assert!(
            matches!(third, Some(GestureEvent::TripleTap(_))),
            "click 3 must produce TripleTap, got {:?}",
            third
        );
    }

    #[test]
    fn arena_resets_on_new_down() {
        let mut arena = GestureArena::new();
        arena.add(TapRecognizer::new().max_distance(5.0));

        // First sequence: fail the tap by moving
        arena.process(&down(Point::new(10.0, 10.0)));
        arena.process(&move_to(Point::new(50.0, 50.0)));

        // New sequence: should work fresh
        arena.process(&down(Point::new(10.0, 10.0)));
        let result = arena.process(&up(Point::new(10.0, 10.0)));
        assert!(matches!(result, Some(GestureEvent::Tap(_))));
    }

    #[test]
    fn arena_empty_returns_none() {
        let mut arena = GestureArena::new();
        assert!(arena.is_empty());
        let result = arena.process(&down(Point::new(10.0, 10.0)));
        assert!(result.is_none());
    }
}
