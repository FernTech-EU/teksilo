// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The backend conformance suite.
//!
//! Six invariants that any [`PointerBackend`] must satisfy, and a harness
//! ([`Conformance`]) that checks all six against a *recorded* event vector. A
//! future backend — winit 0.31's unified pointer API, a replay backend reading
//! a captured trace, a platform Teksilo has not met — earns its trust by being
//! run through the same six.
//!
//! Nothing here touches an OS. The vectors are `winit::event::WindowEvent`
//! values written out by hand from a reading of the winit 0.30 backend that
//! would produce them, so the suite runs on a machine with no display, no
//! touchscreen and no GPU.
//!
//! # The six
//!
//! 1. **Identity is unique across OS id reuse.** winit reuses `Touch::id`; two
//!    successive contacts on the same raw id must be two `PointerId`s.
//! 2. **Cancel completeness.** Every `Down` is terminated by exactly one `Up`
//!    or one `Cancel` — never both, never neither.
//! 3. **Time is monotone.** `EventTime` never runs backwards within a stream.
//! 4. **Primacy and hover.** At most one live pointer *of a kind* is primary
//!    (W3C `isPrimary`), a mouse sample is always primary, and no direct
//!    pointer ever hovers.
//! 5. **One stream per contact.** A single physical touch produces one pointer
//!    stream, not a touch stream plus an emulated mouse one.
//! 6. **Well-formed scroll phases.** `Began → Changed* → Ended`, and
//!    `Momentum*` only after an `Ended`.

use std::collections::{HashMap, HashSet};

use teksilo_canvas::Point;
use teksilo_core::PointerId;
use teksilo_core::event::ButtonMask;
use teksilo_core::pointer::{EventTime, PointerPhase, ScrollPhase};
use teksilo_platform::event_translation::TranslationState;
use teksilo_platform::pointer_backend::{BackendEvent, InputSample, PointerBackend};
use teksilo_platform::window_system::WindowSystem;
use teksilo_tokens::{InputTokens, PointerKind};

// ---------------------------------------------------------------------------
// Recorded vectors
// ---------------------------------------------------------------------------

/// One recorded OS packet: the winit event and the millisecond it arrived at.
struct Packet {
    at_ms: u64,
    event: winit::event::WindowEvent,
}

fn dummy_device() -> winit::event::DeviceId {
    winit::event::DeviceId::dummy()
}

fn touch(phase: winit::event::TouchPhase, id: u64, x: f64, y: f64) -> winit::event::WindowEvent {
    winit::event::WindowEvent::Touch(winit::event::Touch {
        device_id: dummy_device(),
        phase,
        location: winit::dpi::PhysicalPosition::new(x, y),
        force: None,
        id,
    })
}

fn cursor_moved(x: f64, y: f64) -> winit::event::WindowEvent {
    winit::event::WindowEvent::CursorMoved {
        device_id: dummy_device(),
        position: winit::dpi::PhysicalPosition::new(x, y),
    }
}

fn wheel(
    delta: winit::event::MouseScrollDelta,
    phase: winit::event::TouchPhase,
) -> winit::event::WindowEvent {
    winit::event::WindowEvent::MouseWheel {
        device_id: dummy_device(),
        delta,
        phase,
    }
}

fn pixels(y: f64) -> winit::event::MouseScrollDelta {
    winit::event::MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(0.0, y))
}

/// **Windows, two fingers, `WM_TOUCH`.**
///
/// winit's Windows backend calls `RegisterTouchWindow(hwnd, TWF_WANTPALM)` and
/// answers `WM_TOUCH` without calling `DefWindowProc`, so there is no promoted
/// mouse stream to interleave: the whole physical interaction is these eight
/// packets and nothing else. `force` is `None` — the comment in winit's
/// `event_loop.rs` reads "WM_TOUCH doesn't support pressure information".
///
/// The two contacts interleave (finger 1 down, finger 2 down, both move, both
/// lift) because that is what makes invariant 4 bite: only the first is
/// primary.
fn windows_two_finger_wm_touch() -> Vec<Packet> {
    use winit::event::TouchPhase::*;
    vec![
        Packet {
            at_ms: 0,
            event: touch(Started, 10, 120.0, 200.0),
        },
        Packet {
            at_ms: 12,
            event: touch(Started, 11, 300.0, 210.0),
        },
        Packet {
            at_ms: 28,
            event: touch(Moved, 10, 128.0, 204.0),
        },
        Packet {
            at_ms: 28,
            event: touch(Moved, 11, 292.0, 214.0),
        },
        Packet {
            at_ms: 44,
            event: touch(Moved, 10, 140.0, 208.0),
        },
        Packet {
            at_ms: 44,
            event: touch(Moved, 11, 280.0, 218.0),
        },
        Packet {
            at_ms: 61,
            event: touch(Ended, 11, 280.0, 218.0),
        },
        Packet {
            at_ms: 74,
            event: touch(Ended, 10, 140.0, 208.0),
        },
    ]
}

/// **X11, one finger, with the phantom `CursorMoved`.**
///
/// `event_processor.rs`'s `xinput2_touch` emits a `CursorMoved` of its own —
/// "Mouse cursor position changes when touch events are received. Only the
/// first concurrently active touch ID moves the mouse cursor." — immediately
/// *before* each `Touch` packet of the first contact, at the same location and
/// through `util::VIRTUAL_CORE_POINTER`, the very device a real mouse uses.
/// Emulated *button* events are filtered by winit (`XIPointerEmulated`);
/// emulated motion is not, because winit is the one making it.
///
/// Recorded with the real-mouse move that precedes the touch, so the vector
/// also exercises "a genuine move is kept".
fn x11_first_touch_with_phantom_motion() -> Vec<Packet> {
    use winit::event::TouchPhase::*;
    vec![
        // A real mouse, well before the finger. Kept.
        Packet {
            at_ms: 0,
            event: cursor_moved(600.0, 40.0),
        },
        // The finger. Each Touch is preceded by winit's synthetic move.
        Packet {
            at_ms: 500,
            event: cursor_moved(120.0, 200.0),
        },
        Packet {
            at_ms: 500,
            event: touch(Started, 4, 120.0, 200.0),
        },
        Packet {
            at_ms: 516,
            event: cursor_moved(126.0, 206.0),
        },
        Packet {
            at_ms: 516,
            event: touch(Moved, 4, 126.0, 206.0),
        },
        Packet {
            at_ms: 532,
            event: cursor_moved(134.0, 214.0),
        },
        Packet {
            at_ms: 532,
            event: touch(Moved, 4, 134.0, 214.0),
        },
        Packet {
            at_ms: 548,
            event: cursor_moved(134.0, 214.0),
        },
        Packet {
            at_ms: 548,
            event: touch(Ended, 4, 134.0, 214.0),
        },
        // The core pointer stays parked at the lift point for a moment.
        Packet {
            at_ms: 560,
            event: cursor_moved(134.0, 214.0),
        },
    ]
}

/// **macOS, two-finger scroll with OS momentum.**
///
/// winit's `scrollWheel:` collapses `NSEvent`'s `phase` and `momentumPhase`
/// into one `TouchPhase` — momentum takes priority, and `Began`/`MayBegin` from
/// either maps to `Started`, `Ended`/`Cancelled` from either maps to `Ended`.
/// So AppKit's `phase: Began..Ended` then `momentumPhase: Began..Ended` arrives
/// as **two** `Started → Moved → Ended` runs with nothing in the event to tell
/// them apart. Reading the second as a new gesture is what would stack a
/// Teksilo fling on top of the OS's.
fn macos_wheel_with_momentum() -> Vec<Packet> {
    use winit::event::TouchPhase::*;
    vec![
        Packet {
            at_ms: 0,
            event: cursor_moved(400.0, 300.0),
        },
        // NSEventPhase::Began / Changed / Ended.
        Packet {
            at_ms: 8,
            event: wheel(pixels(6.0), Started),
        },
        Packet {
            at_ms: 24,
            event: wheel(pixels(14.0), Moved),
        },
        Packet {
            at_ms: 40,
            event: wheel(pixels(22.0), Moved),
        },
        Packet {
            at_ms: 56,
            event: wheel(pixels(18.0), Ended),
        },
        // momentumPhase::Began / Changed / Ended, same run-loop turn.
        Packet {
            at_ms: 57,
            event: wheel(pixels(16.0), Started),
        },
        Packet {
            at_ms: 73,
            event: wheel(pixels(9.0), Moved),
        },
        Packet {
            at_ms: 89,
            event: wheel(pixels(4.0), Moved),
        },
        Packet {
            at_ms: 340,
            event: wheel(pixels(0.5), Ended),
        },
    ]
}

/// **An OS contact id reused across two successive taps.**
///
/// winit's own documentation on `Touch`: "The finger id may be reused by the
/// system after an `Ended` event. The user should assume that a new `Started`
/// event received with the same id has nothing to do with the old finger."
/// This is the reason `PointerId` is minted per press.
fn os_id_reuse_across_two_contacts() -> Vec<Packet> {
    use winit::event::TouchPhase::*;
    vec![
        Packet {
            at_ms: 0,
            event: touch(Started, 0, 50.0, 50.0),
        },
        Packet {
            at_ms: 40,
            event: touch(Ended, 0, 50.0, 50.0),
        },
        Packet {
            at_ms: 900,
            event: touch(Started, 0, 51.0, 52.0),
        },
        Packet {
            at_ms: 940,
            event: touch(Ended, 0, 51.0, 52.0),
        },
    ]
}

/// **Wayland, a contact the compositor revokes.**
///
/// `wl_touch.cancel` is the only source of a real `TouchPhase::Cancelled` on
/// the desktop in winit 0.30 (`linux/wayland/seat/touch/mod.rs`). It exists in
/// the suite so invariant 2 is exercised on its `Cancel` arm and not only on
/// `Up`.
fn wayland_cancelled_contact() -> Vec<Packet> {
    use winit::event::TouchPhase::*;
    vec![
        Packet {
            at_ms: 0,
            event: touch(Started, 2, 30.0, 30.0),
        },
        Packet {
            at_ms: 16,
            event: touch(Moved, 2, 36.0, 34.0),
        },
        Packet {
            at_ms: 32,
            event: touch(Cancelled, 2, 36.0, 34.0),
        },
    ]
}

// ---------------------------------------------------------------------------
// The harness
// ---------------------------------------------------------------------------

/// State a live pointer contributes to the invariants.
#[derive(Clone, Debug)]
struct LivePointer {
    kind: PointerKind,
    primary: bool,
}

/// Runs a recorded vector through a [`PointerBackend`] and checks the six
/// invariants after every sample.
struct Conformance {
    /// The name used in assertion messages.
    name: &'static str,
    /// Pointers currently down, and how they were introduced.
    live: HashMap<PointerId, LivePointer>,
    /// Every id ever seen down, so a reused OS id cannot resolve to an old one.
    ever_down: HashSet<PointerId>,
    /// Ids already terminated, so a double termination is caught.
    terminated: HashSet<PointerId>,
    /// The last `EventTime` observed, for monotonicity.
    last_time: Option<EventTime>,
    /// The scroll phase machine's last state.
    last_scroll: Option<ScrollPhase>,
    /// Positions a direct contact occupied, per sample index, so an emulated
    /// mouse stream duplicating them can be detected.
    contact_positions: Vec<Point>,
    /// Positions a mouse sample reported while a contact was live.
    shadowed_mouse_positions: Vec<Point>,
    /// Everything observed, for per-vector assertions beyond the six.
    samples: Vec<InputSample>,
}

impl Conformance {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            live: HashMap::new(),
            ever_down: HashSet::new(),
            terminated: HashSet::new(),
            last_time: None,
            last_scroll: None,
            contact_positions: Vec::new(),
            shadowed_mouse_positions: Vec::new(),
            samples: Vec::new(),
        }
    }

    /// Drive a whole vector, then check the end-state invariants.
    fn run(mut self, backend: &mut dyn PointerBackend, packets: &[Packet]) -> Self {
        for packet in packets {
            let now = EventTime::from_millis(packet.at_ms);
            for sample in backend.translate(&BackendEvent::Winit(&packet.event), now) {
                self.observe(&sample);
                self.samples.push(sample);
            }
        }
        self
    }

    /// Terminate the stream the way a closing window would, and check that
    /// nothing is left unterminated.
    fn finish(mut self, backend: &mut dyn PointerBackend, at_ms: u64) -> Self {
        for sample in backend.cancel_all(EventTime::from_millis(at_ms)) {
            self.observe(&sample);
            self.samples.push(sample);
        }
        assert!(
            self.live.is_empty(),
            "{}: invariant 2 — {} pointer(s) still down after cancel_all: {:?}",
            self.name,
            self.live.len(),
            self.live.keys().collect::<Vec<_>>()
        );
        self
    }

    fn observe(&mut self, sample: &InputSample) {
        match sample {
            InputSample::Pointer(p) => {
                // --- 3. Time is monotone ---------------------------------
                if let Some(last) = self.last_time {
                    assert!(
                        p.pointer.time >= last,
                        "{}: invariant 3 — {:?} precedes {last:?}",
                        self.name,
                        p.pointer.time
                    );
                }
                self.last_time = Some(p.pointer.time);

                match p.phase {
                    PointerPhase::Down => {
                        // --- 1. Identity is unique -----------------------
                        assert!(
                            self.ever_down.insert(p.pointer.id),
                            "{}: invariant 1 — {:?} was minted twice; a reused OS \
                             contact id must resolve to a fresh PointerId",
                            self.name,
                            p.pointer.id
                        );
                        // --- 2. Down opens exactly one stream ------------
                        assert!(
                            self.live
                                .insert(
                                    p.pointer.id,
                                    LivePointer {
                                        kind: p.pointer.kind,
                                        primary: p.pointer.primary,
                                    },
                                )
                                .is_none(),
                            "{}: invariant 2 — {:?} went down twice",
                            self.name,
                            p.pointer.id
                        );
                    }
                    PointerPhase::Up | PointerPhase::Cancel => {
                        assert!(
                            self.live.remove(&p.pointer.id).is_some(),
                            "{}: invariant 2 — {:?} terminated without a Down",
                            self.name,
                            p.pointer.id
                        );
                        assert!(
                            self.terminated.insert(p.pointer.id),
                            "{}: invariant 2 — {:?} terminated twice",
                            self.name,
                            p.pointer.id
                        );
                    }
                    PointerPhase::Move => {
                        // A move for a pointer with no buttons is a hover, and
                        // only an indirect pointer may hover. A move for a
                        // *down* pointer must belong to a live stream.
                        if !p.pointer.buttons.is_empty() {
                            assert!(
                                self.live.contains_key(&p.pointer.id),
                                "{}: invariant 2 — {:?} moved with buttons held \
                                 but is not live",
                                self.name,
                                p.pointer.id
                            );
                        }
                    }
                }

                // --- 4. Primacy and hover -----------------------------------
                if p.pointer.kind == PointerKind::Mouse {
                    assert!(
                        p.pointer.primary,
                        "{}: invariant 4 — a mouse sample is always primary",
                        self.name
                    );
                }
                if p.pointer.is_direct() {
                    assert!(
                        p.phase != PointerPhase::Move || !p.pointer.buttons.is_empty(),
                        "{}: invariant 4 — a direct pointer never hovers, so a \
                         buttonless move is impossible",
                        self.name
                    );
                    assert!(
                        p.phase != PointerPhase::Down
                            || p.pointer
                                .buttons
                                .contains(teksilo_core::event::PointerButton::Primary),
                        "{}: invariant 4 — a contact must report Primary while \
                         down or every accept_buttons() recognizer is dead on it",
                        self.name
                    );
                }
                self.assert_one_primary_per_kind();

                // --- 5. No duplicate streams ------------------------------
                match p.pointer.kind {
                    PointerKind::Mouse if !self.live_has_direct() => {}
                    PointerKind::Mouse => self.shadowed_mouse_positions.push(p.position),
                    _ => self.contact_positions.push(p.position),
                }
            }

            InputSample::Scroll(s) => {
                if let Some(last) = self.last_time {
                    assert!(
                        s.pointer.time >= last,
                        "{}: invariant 3 — scroll {:?} precedes {last:?}",
                        self.name,
                        s.pointer.time
                    );
                }
                self.last_time = Some(s.pointer.time);
                self.assert_scroll_phase_well_formed(s.phase);
                self.last_scroll = Some(s.phase);
            }

            InputSample::Gesture(_) => {}
        }
    }

    fn live_has_direct(&self) -> bool {
        self.live.values().any(|p| p.kind.is_direct())
    }

    /// Invariant 4, the counting half. W3C `isPrimary` semantics: the mouse is
    /// always primary in its own stream and the first contact of a touch
    /// sequence is primary in its; the two coexist on a hybrid machine, and it
    /// is the tree's pointer table — not the platform layer — that arbitrates
    /// between streams. What must never happen is two primaries *of one kind*.
    fn assert_one_primary_per_kind(&self) {
        let mut per_kind: HashMap<PointerKind, usize> = HashMap::new();
        for pointer in self.live.values() {
            if pointer.primary {
                *per_kind.entry(pointer.kind).or_default() += 1;
            }
        }
        for (kind, count) in per_kind {
            assert!(
                count <= 1,
                "{}: invariant 4 — {count} live {kind:?} pointers claim primacy",
                self.name
            );
        }
    }

    /// Invariant 6.
    fn assert_scroll_phase_well_formed(&self, phase: ScrollPhase) {
        let previous = self.last_scroll;
        let ok = match phase {
            // A wheel notch has no phase structure and may appear anywhere.
            ScrollPhase::Discrete => true,
            ScrollPhase::Began => !matches!(
                previous,
                Some(ScrollPhase::Began | ScrollPhase::Changed | ScrollPhase::Momentum)
            ),
            ScrollPhase::Changed => {
                matches!(previous, Some(ScrollPhase::Began | ScrollPhase::Changed))
            }
            ScrollPhase::Ended => {
                matches!(previous, Some(ScrollPhase::Began | ScrollPhase::Changed))
            }
            // Momentum only ever follows an Ended or more Momentum. This is
            // the arm that stops an OS momentum stream from being read as a
            // fresh gesture.
            ScrollPhase::Momentum => {
                matches!(previous, Some(ScrollPhase::Ended | ScrollPhase::Momentum))
            }
            ScrollPhase::MomentumEnded => matches!(previous, Some(ScrollPhase::Momentum)),
            ScrollPhase::Cancelled | ScrollPhase::Fling => true,
            _ => true,
        };
        assert!(
            ok,
            "{}: invariant 6 — {phase:?} cannot follow {previous:?}",
            self.name
        );
    }

    /// Invariant 5, checked at the end: no mouse sample duplicated a contact's
    /// position while that contact was live.
    fn assert_no_duplicate_streams(&self) {
        for mouse in &self.shadowed_mouse_positions {
            for contact in &self.contact_positions {
                let duplicate =
                    (mouse.x - contact.x).abs() <= 1.0 && (mouse.y - contact.y).abs() <= 1.0;
                assert!(
                    !duplicate,
                    "{}: invariant 5 — a mouse sample at {mouse:?} shadows the \
                     contact at {contact:?}; one physical touch produced two streams",
                    self.name
                );
            }
        }
    }

    fn pointer_samples(&self) -> Vec<&teksilo_core::pointer::PointerSample> {
        self.samples
            .iter()
            .filter_map(InputSample::as_pointer)
            .collect()
    }

    fn scroll_phases(&self) -> Vec<ScrollPhase> {
        self.samples
            .iter()
            .filter_map(InputSample::as_scroll)
            .map(|s| s.phase)
            .collect()
    }
}

fn backend_for(window_system: WindowSystem) -> TranslationState {
    let mut state = TranslationState::new();
    state.set_window_system(window_system);
    state
}

// ---------------------------------------------------------------------------
// The vectors, through the harness
// ---------------------------------------------------------------------------

#[test]
fn windows_two_finger_touch_is_conformant() {
    let mut backend = backend_for(WindowSystem::Unknown);
    let run = Conformance::new("windows/wm_touch")
        .run(&mut backend, &windows_two_finger_wm_touch())
        .finish(&mut backend, 100);
    run.assert_no_duplicate_streams();

    let samples = run.pointer_samples();
    assert_eq!(samples.len(), 8, "eight packets, eight samples");
    assert!(
        samples.iter().all(|s| s.pointer.kind == PointerKind::Touch),
        "no mouse stream is synthesised on Windows"
    );

    // Two distinct identities, and the first one down is the primary.
    let ids: HashSet<PointerId> = samples.iter().map(|s| s.pointer.id).collect();
    assert_eq!(ids.len(), 2);
    assert!(samples[0].pointer.primary);
    assert!(!samples[1].pointer.primary);

    // Every contact holds Primary for its whole life.
    for sample in &samples {
        match sample.phase {
            PointerPhase::Down | PointerPhase::Move => {
                assert_eq!(sample.pointer.buttons, ButtonMask::PRIMARY)
            }
            _ => assert_eq!(sample.pointer.buttons, ButtonMask::NONE),
        }
    }
}

#[test]
fn the_x11_phantom_motion_does_not_double_the_stream() {
    let mut backend = backend_for(WindowSystem::X11);
    let run = Conformance::new("x11/phantom")
        .run(&mut backend, &x11_first_touch_with_phantom_motion())
        .finish(&mut backend, 900);
    run.assert_no_duplicate_streams();

    let samples = run.pointer_samples();
    let mouse: Vec<_> = samples
        .iter()
        .filter(|s| s.pointer.kind == PointerKind::Mouse)
        .collect();
    let contacts: Vec<_> = samples
        .iter()
        .filter(|s| s.pointer.kind == PointerKind::Touch)
        .collect();

    assert_eq!(contacts.len(), 4, "Started, Moved, Moved, Ended");
    assert_eq!(
        mouse.len(),
        2,
        "the real mouse move before the touch, plus the one leaked ahead of the \
         first Touch packet — winit emits its synthetic CursorMoved *before* the \
         Touch that establishes the contact, and closing that would cost every \
         real X11 mouse move a frame of lookahead"
    );
    assert_eq!(mouse[0].position, Point::new(600.0, 40.0), "the real mouse");
    assert_eq!(
        mouse[1].position,
        Point::new(120.0, 200.0),
        "the documented residual: one leaked sample at the touch-down point"
    );
}

/// The same vector on a platform that does *not* promote: nothing is
/// suppressed, and the phantom is simply a mouse move. This is what makes the
/// suppressor's scoping visible rather than assumed.
#[test]
fn the_same_vector_off_x11_suppresses_nothing() {
    let mut backend = backend_for(WindowSystem::Wayland);
    let run = Conformance::new("wayland/no-suppression")
        .run(&mut backend, &x11_first_touch_with_phantom_motion())
        .finish(&mut backend, 900);

    let mouse = run
        .pointer_samples()
        .iter()
        .filter(|s| s.pointer.kind == PointerKind::Mouse)
        .count();
    assert_eq!(mouse, 6, "every CursorMoved in the vector reaches the tree");
}

#[test]
fn macos_momentum_is_never_a_second_gesture() {
    let mut backend = backend_for(WindowSystem::Unknown);
    let run = Conformance::new("macos/momentum")
        .run(&mut backend, &macos_wheel_with_momentum())
        .finish(&mut backend, 400);
    run.assert_no_duplicate_streams();

    assert_eq!(
        run.scroll_phases(),
        vec![
            ScrollPhase::Began,
            ScrollPhase::Changed,
            ScrollPhase::Changed,
            ScrollPhase::Ended,
            ScrollPhase::Momentum,
            ScrollPhase::Momentum,
            ScrollPhase::Momentum,
            ScrollPhase::MomentumEnded,
        ]
    );
    assert_eq!(
        run.scroll_phases()
            .iter()
            .filter(|p| **p == ScrollPhase::Began)
            .count(),
        1,
        "one flick is one gesture; a second Began would let a Teksilo fling \
         stack on top of the OS's momentum"
    );
}

#[test]
fn a_reused_os_contact_id_is_two_identities() {
    let mut backend = backend_for(WindowSystem::Unknown);
    let run = Conformance::new("os-id-reuse")
        .run(&mut backend, &os_id_reuse_across_two_contacts())
        .finish(&mut backend, 1000);
    run.assert_no_duplicate_streams();

    let samples = run.pointer_samples();
    assert_eq!(samples.len(), 4);
    assert_ne!(
        samples[0].pointer.id, samples[2].pointer.id,
        "invariant 1 — winit reuses Touch::id after a lift"
    );
    assert!(
        samples[2].pointer.id > samples[0].pointer.id,
        "ids are monotonic"
    );
    assert!(
        samples[0].pointer.primary && samples[2].pointer.primary,
        "each sequence elects its own primary"
    );
}

#[test]
fn a_wayland_cancel_terminates_the_contact() {
    let mut backend = backend_for(WindowSystem::Wayland);
    let run = Conformance::new("wayland/cancel")
        .run(&mut backend, &wayland_cancelled_contact())
        .finish(&mut backend, 100);
    run.assert_no_duplicate_streams();

    let samples = run.pointer_samples();
    assert_eq!(samples.len(), 3);
    assert_eq!(samples[2].phase, PointerPhase::Cancel);
    assert_eq!(
        samples[2].button, None,
        "a cancel has no meaningful end state, so it names no button"
    );
}

/// An interrupted stream — the window goes away mid-gesture — still satisfies
/// cancel completeness, because `cancel_all` terminates what is left.
#[test]
fn an_interrupted_stream_is_completed_by_cancel_all() {
    let mut backend = backend_for(WindowSystem::Unknown);
    let truncated: Vec<Packet> = windows_two_finger_wm_touch().into_iter().take(4).collect();
    let run = Conformance::new("interrupted")
        .run(&mut backend, &truncated)
        .finish(&mut backend, 200);

    let cancels = run
        .pointer_samples()
        .iter()
        .filter(|s| s.phase == PointerPhase::Cancel)
        .count();
    assert_eq!(cancels, 2, "both live contacts are cancelled");
}

/// The kill switch, checked through the harness: with touch off, the touch
/// vectors produce no pointer samples at all, and the six invariants are
/// trivially satisfied because there is nothing to violate them.
#[test]
fn the_kill_switch_empties_every_touch_vector() {
    for (name, packets) in [
        ("windows", windows_two_finger_wm_touch()),
        ("x11", x11_first_touch_with_phantom_motion()),
        ("reuse", os_id_reuse_across_two_contacts()),
        ("wayland", wayland_cancelled_contact()),
    ] {
        let mut backend = backend_for(WindowSystem::Unknown);
        backend.set_input_tokens(InputTokens {
            touch_enabled: false,
            ..InputTokens::default()
        });

        let run = Conformance::new("kill-switch")
            .run(&mut backend, &packets)
            .finish(&mut backend, 2000);

        let contacts = run
            .pointer_samples()
            .iter()
            .filter(|s| s.pointer.kind == PointerKind::Touch)
            .count();
        assert_eq!(
            contacts, 0,
            "{name}: touch_enabled = false must yield no contact"
        );
    }
}
