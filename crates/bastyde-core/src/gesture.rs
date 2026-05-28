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
//! [`LongPressRecognizer`] all default to `ButtonMask::PRIMARY` —
//! left-click only — and emit [`TapEvent`]s carrying position, the
//! finalising button, and modifier state. Multi-tap recognizers
//! require button-match across the whole sequence. Widen the accepted
//! set with `.accept_buttons(...)` / `.accept_any_button()`.

use std::time::Instant;

use bastyde_canvas::{Point, Vec2};

use crate::event::{Modifiers, PointerButton};

mod arena;
mod drag;
mod long_press;
mod multi_tap;
mod swipe;
mod tap;

pub use arena::GestureArena;
pub use drag::DragRecognizer;
pub use long_press::LongPressRecognizer;
pub use multi_tap::{DoubleTapRecognizer, TripleTapRecognizer};
pub use swipe::SwipeRecognizer;
pub use tap::TapRecognizer;

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

pub(crate) fn distance(a: Point, b: Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
pub(crate) mod test_helpers {
    use super::{Modifiers, Point, PointerButton, RawPointerEvent};

    pub fn down(pos: Point) -> RawPointerEvent {
        down_btn(pos, PointerButton::Primary)
    }

    pub fn down_btn(pos: Point, button: PointerButton) -> RawPointerEvent {
        down_full(pos, button, Modifiers::NONE)
    }

    pub fn down_full(pos: Point, button: PointerButton, modifiers: Modifiers) -> RawPointerEvent {
        RawPointerEvent::Down {
            position: pos,
            button,
            modifiers,
        }
    }

    pub fn up(pos: Point) -> RawPointerEvent {
        up_btn(pos, PointerButton::Primary)
    }

    pub fn up_btn(pos: Point, button: PointerButton) -> RawPointerEvent {
        up_full(pos, button, Modifiers::NONE)
    }

    pub fn up_full(pos: Point, button: PointerButton, modifiers: Modifiers) -> RawPointerEvent {
        RawPointerEvent::Up {
            position: pos,
            button,
            modifiers,
        }
    }

    pub fn move_to(pos: Point) -> RawPointerEvent {
        RawPointerEvent::Move { position: pos }
    }
}
