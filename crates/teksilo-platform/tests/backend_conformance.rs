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
//!    successive contacts on the same raw id must be two `PointerId`s. This
//!    binds a **coarse** pointer only: a mouse and a pen have one identity that
//!    outlives any number of presses, which is the whole difference between a
//!    pointer that hovers and one that is minted when it lands.
//! 2. **Cancel completeness.** Every `Down` is followed by exactly one
//!    **completion** — one `Up` or one `Cancel`, never both, never neither —
//!    before that pointer may go down again. A hovering-capable pointer may
//!    *additionally* end its proximity session with one `Cancel` while nothing
//!    is down; that is a session end, not a completion, and a pen emits one
//!    when it leaves the digitizer's range, however many strokes it made while
//!    it was in it.
//! 3. **Time is monotone.** `EventTime` never runs backwards within a stream.
//! 4. **Primacy and hover.** At most one live pointer *of a kind* is primary
//!    (W3C `isPrimary`), a mouse sample is always primary, and no **coarse**
//!    pointer ever hovers. Coarse, not direct: a pen is direct and hovers, and
//!    its whole first act is a buttonless move made while the nib is above the
//!    glass. A finger has nothing to hover with.
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
use teksilo_platform::pen::{PenPacket, PenSource};
use teksilo_platform::pointer_backend::{BackendEvent, InputSample, PointerBackend};
use teksilo_platform::window_system::WindowSystem;
use teksilo_tokens::{InputTokens, PenKind, PointerKind};

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

/// **A pen stroke, from proximity to withdrawal.**
///
/// The shape a digitizer actually produces, and the reason invariants 2 and 4
/// had to be restated. A stylus announces itself by hovering — buttonless
/// moves, tip up — then touches down, draws, lifts, hovers again and finally
/// leaves range. One `PointerId` spans the whole of it, the tip's `Up` is the
/// completion, and the withdrawal is a `Cancel` that completes nothing.
///
/// The stroke deliberately contains **two** tip contacts, because that is the
/// case a per-contact identity rule would reject: two taps inside one
/// proximity session are two presses of one pointer, not two pointers.
fn pen_stroke_with_two_taps() -> Vec<PenPacket> {
    let tool = PenKind::Pen;
    let at = |x: f32, y: f32| Point::new(x, y);
    vec![
        // In range, nib above the glass.
        PenPacket::hovering(tool, at(100.0, 100.0)),
        PenPacket::hovering(tool, at(104.0, 103.0)),
        // First contact.
        PenPacket::hovering(tool, at(104.0, 103.0)).down_at(0.4),
        PenPacket::hovering(tool, at(110.0, 108.0)).down_at(0.7),
        // Lift, still in range.
        PenPacket::hovering(tool, at(110.0, 108.0)),
        // Second contact, same session.
        PenPacket::hovering(tool, at(112.0, 110.0)).down_at(0.5),
        PenPacket::hovering(tool, at(112.0, 110.0)),
        // Out of range.
        PenPacket::out_of_proximity(tool, at(112.0, 110.0)),
    ]
}

/// **A pen that hovers and is taken away without ever touching down.**
///
/// The reader glancing at a tablet. No `Down` is ever emitted, so the session's
/// closing `Cancel` has nothing to complete — which is precisely the shape the
/// old "every terminator closes a Down" reading rejected.
fn pen_hover_only() -> Vec<PenPacket> {
    let tool = PenKind::Pen;
    vec![
        PenPacket::hovering(tool, Point::new(50.0, 60.0)),
        PenPacket::hovering(tool, Point::new(58.0, 66.0)),
        PenPacket::out_of_proximity(tool, Point::new(58.0, 66.0)),
    ]
}

/// **One drain carrying a whole coalesced batch.**
///
/// The shape a Windows `WM_POINTERUPDATE` with a populated history produces,
/// and the shape a Wayland dispatch thread produces whenever the event loop was
/// busy: several digitizer packets, each with its own device stamp, arriving in
/// a single `poll_pen`. Every other pen vector here is drained one packet at a
/// time, so the multi-packet path — where the back-dating in `poll_pen` lives —
/// is otherwise unexercised by the six invariants, and invariant 3 in
/// particular has nothing to say about it.
///
/// The stamps are 4 ms apart: a 250 Hz stylus through one 32 ms turn.
fn pen_batch_with_device_stamps() -> Vec<PenPacket> {
    let tool = PenKind::Pen;
    let at = |x: f32, y: f32| Point::new(x, y);
    let ms = |i: u32| 60_000 + i * 4;
    vec![
        PenPacket::hovering(tool, at(200.0, 100.0)).at_device_ms(ms(0)),
        PenPacket::hovering(tool, at(202.0, 101.0)).at_device_ms(ms(1)),
        PenPacket::hovering(tool, at(204.0, 103.0))
            .down_at(0.3)
            .at_device_ms(ms(2)),
        PenPacket::hovering(tool, at(208.0, 108.0))
            .down_at(0.6)
            .at_device_ms(ms(3)),
        PenPacket::hovering(tool, at(214.0, 115.0))
            .down_at(0.9)
            .at_device_ms(ms(4)),
        PenPacket::hovering(tool, at(214.0, 115.0)).at_device_ms(ms(5)),
        PenPacket::out_of_proximity(tool, at(214.0, 115.0)).at_device_ms(ms(6)),
    ]
}

/// A [`PenSource`] that hands back a recorded list, so a pen vector goes
/// through the same `poll_pen` the event loop calls rather than through a
/// private entry point.
#[derive(Debug)]
struct RecordedPenSource(Vec<PenPacket>);

impl PenSource for RecordedPenSource {
    fn poll(&mut self, out: &mut Vec<PenPacket>) {
        out.append(&mut self.0);
    }

    fn capabilities(&self) -> teksilo_platform::PenCaps {
        teksilo_platform::PenCaps::FULL_PEN
    }
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
    /// How many completions each id has had, so a coarse pointer completing
    /// twice is caught while a mouse or a pen pressing again is not.
    completions: HashMap<PointerId, usize>,
    /// Ids whose proximity session has ended, so a second session end is
    /// caught.
    session_ended: HashSet<PointerId>,
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
            completions: HashMap::new(),
            session_ended: HashSet::new(),
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

    /// Drive a recorded pen session through `backend`'s shim, one packet per
    /// poll so each is timed like the event-loop turn it would have arrived on.
    ///
    /// Concrete `TranslationState` rather than `dyn PointerBackend` because the
    /// digitizer is not a winit event: `PointerBackend::translate` never sees a
    /// pen packet, and `poll_pen` — the pump the event loop calls once a turn —
    /// is the shim's whole surface. The invariants checked are the same six.
    fn run_pen(mut self, backend: &mut TranslationState, packets: &[PenPacket]) -> Self {
        for (index, packet) in packets.iter().enumerate() {
            backend.set_pen_source(Box::new(RecordedPenSource(vec![*packet])));
            let now = EventTime::from_millis(index as u64 * 8);
            for sample in backend.poll_pen(now) {
                self.observe(&sample);
                self.samples.push(sample);
            }
        }
        backend.take_pen_source();
        self
    }

    /// Drive a whole recorded pen session through **one** drain, the way a
    /// coalescing platform delivers it.
    ///
    /// The counterpart of [`run_pen`](Self::run_pen), which gives each packet
    /// its own poll. Here the shim hands over the whole batch at once and
    /// `poll_pen` has to place it on the tree's timeline itself — so the six
    /// invariants, invariant 3 above all, are checked against the back-dated
    /// times rather than against one `now` per packet.
    fn run_pen_batch(mut self, backend: &mut TranslationState, packets: &[PenPacket]) -> Self {
        backend.set_pen_source(Box::new(RecordedPenSource(packets.to_vec())));
        for sample in backend.poll_pen(EventTime::from_millis(100)) {
            self.observe(&sample);
            self.samples.push(sample);
        }
        backend.take_pen_source();
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
                        // Coarse only. A pen's tip may touch down, lift and
                        // touch down again inside one proximity session, and a
                        // mouse clicks all day on the same id; neither is a
                        // reused OS contact id resolving to a stale identity,
                        // which is what this invariant is about.
                        let fresh = self.ever_down.insert(p.pointer.id);
                        assert!(
                            fresh || !p.pointer.kind.is_coarse(),
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
                        if self.live.remove(&p.pointer.id).is_some() {
                            // A completion: it closes the press that was open.
                            let seen = self.completions.entry(p.pointer.id).or_default();
                            *seen += 1;
                            assert!(
                                *seen == 1 || !p.pointer.kind.is_coarse(),
                                "{}: invariant 2 — {:?} completed {seen} times; a \
                                 contact's identity does not outlive its press",
                                self.name,
                                p.pointer.id
                            );
                        } else {
                            // Not live. The only well-formed shape is a
                            // hovering-capable pointer's proximity session
                            // ending: the completion, if there was one, was the
                            // tip's `Up`, which has already been delivered.
                            assert!(
                                p.phase == PointerPhase::Cancel,
                                "{}: invariant 2 — {:?} lifted with no press open",
                                self.name,
                                p.pointer.id
                            );
                            assert!(
                                p.pointer.kind.hovers(),
                                "{}: invariant 2 — {:?} terminated without a Down, \
                                 and it cannot hover, so there was no session to end",
                                self.name,
                                p.pointer.id
                            );
                            assert!(
                                self.session_ended.insert(p.pointer.id),
                                "{}: invariant 2 — {:?} ended its session twice",
                                self.name,
                                p.pointer.id
                            );
                        }
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
                if p.pointer.kind.is_coarse() {
                    assert!(
                        p.phase != PointerPhase::Move || !p.pointer.buttons.is_empty(),
                        "{}: invariant 4 — a coarse pointer never hovers, so a \
                         buttonless move is impossible",
                        self.name
                    );
                }
                if p.pointer.is_direct() {
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

/// A pen stroke satisfies all six, including the two clauses it forced into
/// their present shape.
#[test]
fn a_pen_stroke_is_conformant() {
    let mut backend = backend_for(WindowSystem::Wayland);
    let run = Conformance::new("pen/stroke")
        .run_pen(&mut backend, &pen_stroke_with_two_taps())
        .finish(&mut backend, 500);
    run.assert_no_duplicate_streams();

    let samples = run.pointer_samples();
    assert!(
        samples
            .iter()
            .all(|s| matches!(s.pointer.kind, PointerKind::Pen(_))),
        "a digitizer produces pen samples and nothing else"
    );

    // One identity for the whole proximity session, two presses inside it.
    let ids: HashSet<PointerId> = samples.iter().map(|s| s.pointer.id).collect();
    assert_eq!(ids.len(), 1, "one proximity session is one pointer");
    let downs = samples
        .iter()
        .filter(|s| s.phase == PointerPhase::Down)
        .count();
    let ups = samples
        .iter()
        .filter(|s| s.phase == PointerPhase::Up)
        .count();
    assert_eq!((downs, ups), (2, 2), "two taps, two completions");

    // The clause invariant 4 had to be restated for: the session opens with a
    // buttonless move made by a *direct* pointer.
    let first = samples[0];
    assert_eq!(first.phase, PointerPhase::Move);
    assert!(
        first.pointer.buttons.is_empty(),
        "the nib is above the glass"
    );
    assert!(first.pointer.is_direct(), "a pen is a direct pointer");
    assert!(first.pointer.kind.hovers(), "and it hovers");

    // The clause invariant 2 had to be restated for: the last sample is a
    // `Cancel` that completes nothing — every press was already closed.
    let last = samples[samples.len() - 1];
    assert_eq!(last.phase, PointerPhase::Cancel);
    assert_eq!(
        samples[samples.len() - 2].phase,
        PointerPhase::Up,
        "the second tap had already completed when the tool withdrew, so the \
         Cancel closes the session and not a press"
    );
}

/// A hover-only session — in range, never touched down, taken away — is
/// conformant too. Its closing `Cancel` has no `Down` anywhere behind it.
#[test]
fn a_pen_that_never_touches_down_is_conformant() {
    let mut backend = backend_for(WindowSystem::Wayland);
    let run = Conformance::new("pen/hover-only")
        .run_pen(&mut backend, &pen_hover_only())
        .finish(&mut backend, 500);

    let samples = run.pointer_samples();
    assert!(
        samples.iter().all(|s| s.pointer.buttons.is_empty()),
        "nothing was ever held"
    );
    assert_eq!(
        samples
            .iter()
            .filter(|s| s.phase == PointerPhase::Down)
            .count(),
        0,
        "the tip never touched"
    );
    assert_eq!(
        samples[samples.len() - 1].phase,
        PointerPhase::Cancel,
        "leaving range ends the session"
    );
}

/// `cancel_all` on a pen still in proximity ends its session — and does it
/// once, so a window closing mid-hover leaves nothing behind.
#[test]
fn cancel_all_ends_a_pen_session_exactly_once() {
    let mut backend = backend_for(WindowSystem::Wayland);
    // Everything but the withdrawal, so the tool is still in range.
    let held: Vec<PenPacket> = pen_stroke_with_two_taps()
        .into_iter()
        .filter(|p| p.in_proximity)
        .collect();
    let run = Conformance::new("pen/interrupted")
        .run_pen(&mut backend, &held)
        .finish(&mut backend, 500);

    let cancels = run
        .pointer_samples()
        .iter()
        .filter(|s| s.phase == PointerPhase::Cancel)
        .count();
    assert_eq!(cancels, 1, "one session, one end");
}

/// A coalesced batch is conformant, and its samples carry the digitizer's own
/// spacing rather than the drain's single clock.
///
/// Invariant 3 is checked by `observe` on every sample here, which is the point
/// of routing this through `Conformance` at all: back-dating a batch is exactly
/// the operation that could hand a consumer time running backwards, and the
/// suite is where that promise is kept.
#[test]
fn a_coalesced_pen_batch_is_conformant_and_keeps_its_own_spacing() {
    let mut backend = backend_for(WindowSystem::Wayland);
    let run = Conformance::new("pen/batch")
        .run_pen_batch(&mut backend, &pen_batch_with_device_stamps())
        .finish(&mut backend, 500);
    run.assert_no_duplicate_streams();

    let samples = run.pointer_samples();
    let times: Vec<EventTime> = samples.iter().map(|s| s.pointer.time).collect();

    // The batch is not one instant. Before `poll_pen` back-dated, every one of
    // these was the drain's `now`.
    let distinct: HashSet<_> = times.iter().map(|t| t.as_duration()).collect();
    assert!(
        distinct.len() > 1,
        "a seven-packet drain must not collapse onto one timestamp: {times:?}"
    );

    // Spacing is the device's: 24 ms from the first packet to the last.
    let span = times
        .last()
        .unwrap()
        .saturating_since(*times.first().unwrap());
    assert_eq!(
        span,
        std::time::Duration::from_millis(24),
        "the six 4 ms device gaps must survive the drain: {times:?}"
    );

    // And the newest packet is the one the poll's clock actually describes.
    assert_eq!(*times.last().unwrap(), EventTime::from_millis(100));
}
