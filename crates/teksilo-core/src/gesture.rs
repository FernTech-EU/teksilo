// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! UIKit-style gesture recognizer model.
//!
//! Gesture recognizers are composable state machines attached to widgets.
//! Each recognizer monitors the raw pointer event stream and emits recognized
//! gestures when patterns complete. They are pure state machines with no
//! platform dependencies and **no wall clock** — every threshold and every
//! instant reaches them through a [`RecognizerContext`], which is what makes
//! them tunable per pointer kind and testable against a
//! [`ManualClock`](crate::pointer::clock::ManualClock).
//!
//! The [`GestureArena`] arbitrates when multiple recognizers compete on the
//! same event stream: all are fed in parallel, and when one recognizes, the
//! rest are reset (except cooperative peers — see
//! [`GestureRecognizer::resets_on_peer_recognition`]). One arena serves one
//! *contact*; the [`GestureArenaSet`] a node carries owns one arena per live
//! [`PointerId`](crate::pointer::PointerId) plus the node's [`TapStreak`],
//! which outlives every contact so a touch double tap — two presses, two
//! different pointer ids — can be recognized at all.
//!
//! **Click-style recognizers carry button + modifiers.** [`TapRecognizer`],
//! [`DoubleTapRecognizer`], [`TripleTapRecognizer`], and
//! [`LongPressRecognizer`] all default to `ButtonMask::PRIMARY` —
//! left-click only — and emit [`TapEvent`]s carrying position, the
//! finalising button, and modifier state. Multi-tap recognizers
//! require button-match across the whole sequence. Widen the accepted
//! set with `.accept_buttons(...)` / `.accept_any_button()`.

use teksilo_canvas::{Point, Vec2};

use crate::event::{Modifiers, PointerButton};
use crate::pointer::{CancelReason, EventTime, PointerInfo};

mod arena;
mod arena_set;
mod config;
mod drag;
mod long_press;
mod multi_tap;
mod palm;
mod pan;
mod pinch;
mod sequence;
mod swipe;
mod tap;

pub use arena::GestureArena;
pub use arena_set::{GestureArenaSet, GestureProto};
pub use config::{MultiContact, RecognizerContext, TapStreak, default_profile};
pub use drag::DragRecognizer;
pub use long_press::LongPressRecognizer;
pub use multi_tap::{DoubleTapRecognizer, TripleTapRecognizer};
pub use palm::{PALM_CONTACT_THRESHOLD, PalmWatch};
pub use pan::PanRecognizer;
pub use pinch::TouchPinchRecognizer;
pub use sequence::{MemberRole, MemberState, PointerSequence, SequenceMember, TapBoundary};
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

    /// Which pointer produced the gesture — the mouse, a numbered finger, a
    /// stylus. A handler reads `pointer.kind` to tell a finger tap from a
    /// click without consulting the tree.
    pub pointer: PointerInfo,
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
            pointer: PointerInfo::mouse(EventTime::ZERO),
        }
    }

    /// The same event attributed to `pointer`. The recognizers build their
    /// `TapEvent`s this way, from the pointer on the sample that finalised the
    /// gesture.
    pub fn with_pointer(mut self, pointer: PointerInfo) -> Self {
        self.pointer = pointer;
        self
    }
}

/// Raw pointer events fed into gesture recognizers.
///
/// Every variant carries the [`PointerInfo`] that produced it and the
/// [`EventTime`] the backend stamped it with, so a recognizer never has to ask
/// *which* contact this is or *when* it happened — the two questions a
/// per-contact, clock-free recognizer cannot answer for itself.
#[derive(Debug, Clone, Copy)]
pub enum RawPointerEvent {
    Down {
        position: Point,
        button: PointerButton,
        modifiers: Modifiers,
        pointer: PointerInfo,
        time: EventTime,
    },
    Move {
        position: Point,
        pointer: PointerInfo,
        time: EventTime,
    },
    Up {
        position: Point,
        button: PointerButton,
        modifiers: Modifiers,
        pointer: PointerInfo,
        time: EventTime,
    },
    /// The interaction was revoked rather than completed — the window lost
    /// focus, a modal opened over it, the OS took the pointer. Distinct from
    /// `Up` on purpose: an `Up` means the user finished, a `Cancel` means the
    /// system interrupted, and conflating them is how a drag ends up "dropped"
    /// wherever the pointer happened to be.
    Cancel {
        position: Point,
        pointer: PointerInfo,
        reason: CancelReason,
        time: EventTime,
    },
}

impl RawPointerEvent {
    /// Which pointer produced this event.
    pub fn pointer(&self) -> PointerInfo {
        match self {
            Self::Down { pointer, .. }
            | Self::Move { pointer, .. }
            | Self::Up { pointer, .. }
            | Self::Cancel { pointer, .. } => *pointer,
        }
    }

    /// When it happened, on the tree's input timeline.
    pub fn time(&self) -> EventTime {
        match self {
            Self::Down { time, .. }
            | Self::Move { time, .. }
            | Self::Up { time, .. }
            | Self::Cancel { time, .. } => *time,
        }
    }

    /// Where it happened, in the receiving widget's local coordinates.
    pub fn position(&self) -> Point {
        match self {
            Self::Down { position, .. }
            | Self::Move { position, .. }
            | Self::Up { position, .. }
            | Self::Cancel { position, .. } => *position,
        }
    }
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
        pointer: PointerInfo,
    },
    DragMoved {
        position: Point,
        delta: Vec2,
        pointer: PointerInfo,
    },
    DragEnded {
        position: Point,
        pointer: PointerInfo,
    },
    /// The drag was revoked rather than released. A handler that has been
    /// mutating state since `DragStarted` must undo it here, not commit it.
    DragCancelled {
        position: Point,
        pointer: PointerInfo,
        reason: CancelReason,
    },
    PinchStarted {
        center: Point,
    },
    /// A running pinch's geometry changed.
    ///
    /// # The producer contract
    ///
    /// Both `scale` and `rotation` are **per-sample deltas**, measured against
    /// the previous sample of this same gesture — against the geometry at
    /// [`PinchStarted`](Self::PinchStarted) for the first one. A consumer folds
    /// each sample into what it already holds (multiplying for `scale`, adding
    /// for `rotation`) and never reads a sample as an absolute.
    ///
    /// Deltas rather than values cumulative since the start, because a pinch has
    /// two producers that must agree and only one of them *can* report a
    /// cumulative value: winit's trackpad `PinchGesture` / `RotationGesture`
    /// report a change per event and hand over no gesture-start baseline to
    /// divide by. [`TouchPinchRecognizer`], which does have one, keeps it
    /// internally and exposes it under a name that cannot be mistaken for these
    /// fields — see
    /// [`cumulative_scale`](TouchPinchRecognizer::cumulative_scale).
    PinchChanged {
        /// Midpoint of the two contacts (or of the trackpad gesture), in the
        /// receiving widget's local coordinates.
        center: Point,
        /// The span **now** divided by the span at the previous sample. `1.0` is
        /// no change, above `1.0` a spread, below `1.0` a squeeze. A producer
        /// never emits `0.0`, a negative or a non-finite value; a consumer
        /// handed one anyway should drop the sample rather than apply it.
        /// Multiply by it — do not assign it.
        scale: f32,
        /// The twist since the previous sample, in **radians** (never degrees:
        /// the platform translator converts at the seam, where winit's unit is
        /// known). Signed, and unwrapped — a gesture turned past ±π keeps
        /// producing same-signed steps instead of jumping by 2π. `0.0` from a
        /// producer that reports magnification without rotation. Add it — do not
        /// assign it.
        rotation: f32,
    },
    PinchEnded,
    /// The pinch was revoked rather than released. Emitted by
    /// [`TouchPinchRecognizer`] when the cancel funnel takes one of its two
    /// contacts away.
    PinchCancelled {
        reason: CancelReason,
    },
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
///
/// `#[non_exhaustive]`: [`Cancelled`](Self::Cancelled) joined the enum with the
/// cancel funnel, and a phase carrying velocity is anticipated for the fling
/// work, so a `match` on it needs a `_` arm.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum DragPhase {
    Started {
        position: Point,
        button: PointerButton,
        pointer: PointerInfo,
    },
    Moved {
        position: Point,
        delta: Vec2,
        pointer: PointerInfo,
    },
    Ended {
        position: Point,
        pointer: PointerInfo,
    },
    /// The drag was revoked. Exactly one of `Ended` or `Cancelled` follows a
    /// `Started`; a handler that committed nothing until `Ended` has nothing
    /// to undo, and one that mutated as it went must roll back here.
    Cancelled {
        position: Point,
        pointer: PointerInfo,
        reason: CancelReason,
    },
}

/// Phase of a pinch (or rotation) gesture, as delivered to an `on_pinch`
/// handler. On desktop these are produced by OS trackpad gestures
/// (`TouchpadMagnify` / `RotationGesture`); on touch they come from a
/// dedicated recognizer ([`TouchPinchRecognizer`]). Both producers satisfy one
/// contract, stated on [`Changed`](Self::Changed).
///
/// `#[non_exhaustive]` for the same reason as [`DragPhase`].
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum PinchPhase {
    Started {
        center: Point,
        pointer: PointerInfo,
    },
    /// The pinch's geometry changed.
    ///
    /// `scale` and `rotation` are **per-sample deltas** against the previous
    /// sample of this gesture, exactly as
    /// [`GestureEvent::PinchChanged`] defines them — fold each sample in
    /// (multiply for `scale`, add for `rotation`) rather than assigning it.
    Changed {
        /// Midpoint of the gesture, in the receiving widget's local
        /// coordinates.
        center: Point,
        /// The span now over the span at the previous sample. Multiply by it.
        scale: f32,
        /// The twist since the previous sample, in **radians**. Add it.
        rotation: f32,
        pointer: PointerInfo,
    },
    Ended {
        pointer: PointerInfo,
    },
    /// The pinch was revoked rather than released.
    Cancelled {
        pointer: PointerInfo,
        reason: CancelReason,
    },
}

/// Trait for gesture recognizers. Each is a composable state machine.
///
/// Every method that could depend on the outside world takes a
/// [`RecognizerContext`]: the current time, the [`GestureProfile`] for the
/// pointer in play, the owning node's local bounds, the pointer itself, and
/// the node's [`TapStreak`]. A recognizer therefore holds only the state of the
/// *one contact* it is following — no clock, no thresholds of its own beyond
/// explicit per-instance overrides, and no cross-contact tap counting.
///
/// [`GestureProfile`]: teksilo_tokens::GestureProfile
pub trait GestureRecognizer {
    /// Feed a raw pointer event and return the recognition result.
    fn process(&mut self, event: &RawPointerEvent, cx: &RecognizerContext) -> GestureResult;

    /// Advance any time-driven state (e.g. the long-press elapsed timer).
    /// Default is a no-op — only recognizers that depend on time (like
    /// [`LongPressRecognizer`]) override this.
    fn tick(&mut self, _cx: &RecognizerContext) -> GestureResult {
        GestureResult::Pending
    }

    /// Earliest future [`EventTime`] at which calling
    /// [`tick`](GestureRecognizer::tick) could transition the recognizer into
    /// `Recognized` or `Failed`. Returns `None` when the recognizer is idle or
    /// not time-driven. Used by the event loop to schedule a wake-up before a
    /// long press fires.
    fn next_deadline(&self) -> Option<EventTime> {
        None
    }

    /// Abandon the attempt in progress without emitting anything.
    ///
    /// Distinct from [`reset`](GestureRecognizer::reset) in intent rather than
    /// in default behaviour: `reset` is arbitration bookkeeping ("you lost,
    /// start over"), `cancel` is the user or the system taking the interaction
    /// away. Defaults to `reset`; a recognizer whose mid-gesture state needs a
    /// different unwind overrides it.
    fn cancel(&mut self) {
        self.reset();
    }

    /// Reset the recognizer to its initial state.
    fn reset(&mut self);

    /// Priority for arbitration when multiple recognizers compete.
    /// Higher priority wins.
    fn priority(&self) -> u32;

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

    /// Whether this recognizer belongs to the *tap family* — tap, double tap,
    /// triple tap, long press.
    ///
    /// Read by [`GestureArenaSet::cancel_taps`], which revokes exactly this
    /// family and leaves a live drag alone. That asymmetry is what WCAG 2.2
    /// SC 2.5.2 ("Pointer Cancellation") needs: sliding off a control must
    /// abort its activation, without aborting a drag the same press started.
    fn tap_family(&self) -> bool {
        false
    }

    /// Whether this recognizer takes part in cross-node sequence arbitration —
    /// the "who owns this press" negotiation between a scrollable and the row
    /// inside it. [`PanRecognizer`] is the one that says `true`.
    ///
    /// It is a **declaration, not a hook**: the router arbitrates on the
    /// sequence's own [`MemberRole`], which it holds directly, so nothing on
    /// the dispatch path has to interrogate a boxed recognizer to find out
    /// what kind of competitor it is. The flag is what a reader — and a
    /// third-party recognizer author — reads to know which side of that
    /// negotiation a type belongs on.
    fn competes_for_sequence(&self) -> bool {
        false
    }

    /// Whether this recognizer wants every live contact rather than just the
    /// one its arena was created for. [`TouchPinchRecognizer`] says `true`;
    /// every single-contact recognizer says `false`.
    ///
    /// Also a declaration rather than a hook, and for a structural reason: a
    /// [`GestureArena`] serves exactly one contact, so a recognizer that needs
    /// two cannot live in one at all. The tree owns its pinch directly and
    /// feeds it every contact (`widget_tree::pan_arbiter::feed_pinch`); the
    /// flag is how such a type declares that it must be owned that way.
    fn wants_all_pointers(&self) -> bool {
        false
    }
}

pub(crate) fn distance(a: Point, b: Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
pub(crate) mod test_helpers {
    use super::{EventTime, Modifiers, Point, PointerButton, PointerInfo, RawPointerEvent};
    use crate::pointer::CancelReason;

    /// The pointer every helper below attributes its event to unless told
    /// otherwise: the mouse, at the epoch.
    pub fn mouse_pointer() -> PointerInfo {
        PointerInfo::mouse(EventTime::ZERO)
    }

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
            pointer: mouse_pointer(),
            time: EventTime::ZERO,
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
            pointer: mouse_pointer(),
            time: EventTime::ZERO,
        }
    }

    pub fn move_to(pos: Point) -> RawPointerEvent {
        RawPointerEvent::Move {
            position: pos,
            pointer: mouse_pointer(),
            time: EventTime::ZERO,
        }
    }

    pub fn cancel_at(pos: Point) -> RawPointerEvent {
        RawPointerEvent::Cancel {
            position: pos,
            pointer: mouse_pointer(),
            reason: CancelReason::Platform,
            time: EventTime::ZERO,
        }
    }

    /// The same event attributed to `pointer` and stamped at `time`.
    pub fn retimed(
        event: RawPointerEvent,
        pointer: PointerInfo,
        time: EventTime,
    ) -> RawPointerEvent {
        match event {
            RawPointerEvent::Down {
                position,
                button,
                modifiers,
                ..
            } => RawPointerEvent::Down {
                position,
                button,
                modifiers,
                pointer,
                time,
            },
            RawPointerEvent::Move { position, .. } => RawPointerEvent::Move {
                position,
                pointer,
                time,
            },
            RawPointerEvent::Up {
                position,
                button,
                modifiers,
                ..
            } => RawPointerEvent::Up {
                position,
                button,
                modifiers,
                pointer,
                time,
            },
            RawPointerEvent::Cancel {
                position, reason, ..
            } => RawPointerEvent::Cancel {
                position,
                pointer,
                reason,
                time,
            },
        }
    }
}

#[cfg(test)]
mod source_scan_tests {
    /// Every `.rs` file the gesture layer is made of.
    fn gesture_sources() -> Vec<(String, String)> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/gesture");
        let mut out = vec![(
            "gesture.rs".to_string(),
            std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/gesture.rs"),
            )
            .expect("gesture.rs is readable"),
        )];
        for entry in std::fs::read_dir(&root).expect("src/gesture is readable") {
            let path = entry.expect("readable dir entry").path();
            if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?")
                    .to_string();
                out.push((name, std::fs::read_to_string(&path).expect("readable")));
            }
        }
        out
    }

    /// Everything outside a `#[cfg(test)] mod` / `#[cfg(test)] impl` block:
    /// the code that actually ships.
    ///
    /// Byte-indexed throughout — these files are full of em-dashes, and the
    /// only positions it ever cuts at (`#[cfg(test)]`, `{`, `}`) are ASCII, so
    /// every slice lands on a character boundary.
    fn production_only(source: &str) -> String {
        const MARKER: &[u8] = b"#[cfg(test)]";
        let bytes = source.as_bytes();
        let mut out = String::with_capacity(source.len());
        let mut kept_from = 0usize;
        let mut i = 0usize;
        while i < bytes.len() {
            if !bytes[i..].starts_with(MARKER) {
                i += 1;
                continue;
            }
            // Only `mod` and `impl` introduce a whole block of test-only code.
            // A `#[cfg(test)]` on a field or a single fn is left in place — and
            // must therefore still be free of wall-clock reads to matter.
            let after = i + MARKER.len();
            let head_end = bytes[after..]
                .iter()
                .position(|c| *c == b'{')
                .map(|off| after + off);
            let Some(head_end) = head_end else { break };
            let head = source[after..head_end].trim_start();
            if !(head.starts_with("mod ") || head.starts_with("impl ")) {
                i = after;
                continue;
            }
            // Brace-match from the block's opening brace.
            let mut depth = 0usize;
            let mut j = head_end;
            while j < bytes.len() {
                match bytes[j] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            j += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            out.push_str(&source[kept_from..i]);
            kept_from = j;
            i = j;
        }
        out.push_str(&source[kept_from..]);
        out
    }

    /// A recognizer that reads the wall clock cannot be driven by a simulated
    /// one, and a test that cannot advance the clock cannot test a long press
    /// or a double-tap window without sleeping. `Instant::now()` is therefore
    /// banned from the gesture layer outside its own test blocks — time
    /// arrives through `RecognizerContext::now`.
    /// Everything before the first `//` on each line: code, not prose. Keeps
    /// the scan from tripping over its own documentation, which names
    /// `Instant::now()` to explain why it is banned.
    fn code_only(source: &str) -> String {
        source
            .lines()
            .map(|line| match line.find("//") {
                Some(idx) => &line[..idx],
                None => line,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn no_wall_clock_in_gestures() {
        for (name, source) in gesture_sources() {
            let production = code_only(&production_only(&source));
            assert!(
                !production.contains("Instant::now()"),
                "{name} reads the wall clock outside its test blocks; gesture \
                 recognizers must take their time from `RecognizerContext::now`"
            );
        }
    }

    /// The stripper must not eat the whole file — otherwise the scan above
    /// passes vacuously.
    #[test]
    fn the_scan_keeps_the_production_half_of_each_file() {
        for (name, source) in gesture_sources() {
            let production = production_only(&source);
            assert!(
                production.contains("// SPDX-License-Identifier"),
                "{name}: the test-block stripper ate the file header"
            );
            assert!(
                code_only(&production).contains("use "),
                "{name}: the comment stripper ate the code"
            );
            if source.contains("pub struct") {
                assert!(
                    production.contains("pub struct"),
                    "{name}: the test-block stripper ate the production types"
                );
            }
        }
    }

    /// The scan is only meaningful if it actually found the files.
    #[test]
    fn the_source_scan_sees_every_recognizer() {
        let names: Vec<String> = gesture_sources().into_iter().map(|(n, _)| n).collect();
        for expected in [
            "gesture.rs",
            "arena.rs",
            "arena_set.rs",
            "config.rs",
            "drag.rs",
            "long_press.rs",
            "multi_tap.rs",
            "palm.rs",
            "pan.rs",
            "pinch.rs",
            "swipe.rs",
            "tap.rs",
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "the wall-clock scan missed {expected}; it saw {names:?}"
            );
        }
    }
}
