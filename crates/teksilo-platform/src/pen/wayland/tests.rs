// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Recorded `zwp_tablet_tool_v2` sequences, folded with no compositor.
//!
//! Each vector below is the event order the protocol specifies, written out as
//! [`ToolEvent`]s: `type` and the identity burst first, then
//! `proximity_in → motion → down → motion → up → proximity_out`, each group
//! terminated by a `frame`. The protocol's own words: *"For tablets without
//! real proximity detection, the sequence is: proximity_in, motion, down,
//! frame"*, and *"If any button is still down, a button release event is sent
//! before this proximity event"* — both of which the sequences here honour.

use super::*;
use std::time::Duration;

/// The surface this window owns. Everything else is a sibling window's.
const OURS: u32 = 12;
/// A sibling window's surface, for the routing test.
const THEIRS: u32 = 13;

/// A `frame` at `ms` on the compositor's millisecond clock.
///
/// Most vectors below pass `0`: they are about the fold, not the clock, and a
/// uniform stamp says "the time is not what this test is pinning". The ones
/// that *are* about the clock number their frames.
const fn frame(ms: u32) -> ToolEvent {
    ToolEvent::Frame { time_ms: ms }
}

/// Drive a recorded sequence through a fresh accumulator.
fn fold(events: &[ToolEvent]) -> Vec<PenPacket> {
    let mut state = ToolState::default();
    let mut out = Vec::new();
    for event in events {
        state.apply(*event, OURS, &mut out);
    }
    out
}

/// The canonical stroke: hover in, move, touch down, draw, lift, hover out.
fn a_full_stroke() -> Vec<ToolEvent> {
    vec![
        ToolEvent::Type(0x140),
        ToolEvent::ProximityIn { surface: OURS },
        ToolEvent::Motion { x: 10.0, y: 20.0 },
        frame(1_000),
        ToolEvent::Motion { x: 12.0, y: 22.0 },
        frame(1_003),
        ToolEvent::Down,
        ToolEvent::Pressure(32768),
        frame(1_006),
        ToolEvent::Motion { x: 40.0, y: 50.0 },
        ToolEvent::Pressure(65535),
        ToolEvent::Tilt { x: -12.0, y: 34.0 },
        ToolEvent::Rotation(90.0),
        frame(1_009),
        ToolEvent::Up,
        frame(1_012),
        ToolEvent::ProximityOut,
        frame(1_015),
    ]
}

#[test]
fn a_stroke_produces_the_right_phases_in_the_right_order() {
    let packets = fold(&a_full_stroke());
    assert_eq!(packets.len(), 6, "one packet per frame that changed state");

    // 1. proximity_in + motion: hovering, tip up.
    assert!(packets[0].in_proximity && !packets[0].down);
    assert_eq!(packets[0].position, Point::new(10.0, 20.0));
    assert_eq!(packets[0].pressure, 0.0);

    // 2. a hover move is still a hover.
    assert!(packets[1].in_proximity && !packets[1].down);
    assert_eq!(packets[1].position, Point::new(12.0, 22.0));

    // 3. the tip touches down, at the position it was already at.
    assert!(packets[2].down && packets[2].in_proximity);
    assert_eq!(packets[2].position, Point::new(12.0, 22.0));
    assert!((packets[2].pressure - 0.5).abs() < 0.001);

    // 4. drawing: every axis committed by one frame.
    assert!(packets[3].down);
    assert_eq!(packets[3].position, Point::new(40.0, 50.0));
    assert_eq!(packets[3].pressure, 1.0);
    assert_eq!(packets[3].tilt, Some((-12.0, 34.0)));
    assert_eq!(packets[3].twist, Some(90.0));

    // 5. the tip lifts but the pen is still hovering.
    assert!(!packets[4].down && packets[4].in_proximity);
    assert_eq!(packets[4].pressure, 0.0, "a lifted tip presses on nothing");

    // 6. and the hover ends.
    assert!(!packets[5].in_proximity && !packets[5].down);
}

#[test]
fn every_packet_of_a_hover_says_down_false() {
    let hover = [
        ToolEvent::Type(0x140),
        ToolEvent::ProximityIn { surface: OURS },
        ToolEvent::Motion { x: 1.0, y: 1.0 },
        frame(0),
        ToolEvent::Motion { x: 2.0, y: 2.0 },
        frame(0),
        ToolEvent::Motion { x: 3.0, y: 3.0 },
        frame(0),
        ToolEvent::ProximityOut,
        frame(0),
    ];
    let packets = fold(&hover);
    assert_eq!(packets.len(), 4);
    assert!(
        packets.iter().all(|p| !p.down),
        "a pen that never touched the surface is never down"
    );
    assert!(
        packets[..3].iter().all(|p| p.in_proximity),
        "proximity holds until proximity_out"
    );
    assert!(
        !packets[3].in_proximity,
        "proximity_out produces the leave packet"
    );
}

#[test]
fn a_leave_is_emitted_exactly_once() {
    let mut state = ToolState::default();
    let mut out = Vec::new();
    for event in [
        ToolEvent::Type(0x140),
        ToolEvent::ProximityIn { surface: OURS },
        frame(0),
        ToolEvent::ProximityOut,
        frame(0),
        // A stray frame after the session ended must not re-announce it.
        frame(0),
        ToolEvent::ProximityOut,
        frame(0),
    ] {
        state.apply(event, OURS, &mut out);
    }
    assert_eq!(out.len(), 2, "enter and leave, and nothing after");
    assert!(out[0].in_proximity);
    assert!(!out[1].in_proximity);
}

#[test]
fn axes_only_reach_the_stream_on_a_frame() {
    let mut state = ToolState::default();
    let mut out = Vec::new();
    for event in [
        ToolEvent::Type(0x140),
        ToolEvent::ProximityIn { surface: OURS },
        ToolEvent::Motion { x: 5.0, y: 5.0 },
        ToolEvent::Pressure(65535),
        ToolEvent::Down,
    ] {
        state.apply(event, OURS, &mut out);
    }
    assert!(out.is_empty(), "nothing is committed before the frame");
    state.apply(frame(0), OURS, &mut out);
    assert_eq!(out.len(), 1, "and then all of it arrives at once");
    assert_eq!(out[0].position, Point::new(5.0, 5.0));
    assert_eq!(out[0].pressure, 1.0);
    assert!(out[0].down);
}

#[test]
fn a_frame_with_nothing_in_it_commits_nothing() {
    let packets = fold(&[
        ToolEvent::Type(0x140),
        ToolEvent::ProximityIn { surface: OURS },
        frame(0),
        frame(0),
        frame(0),
    ]);
    assert_eq!(packets.len(), 1);
}

#[test]
fn buttons_are_the_barrel_and_nothing_else() {
    let packets = fold(&[
        ToolEvent::Type(0x140),
        ToolEvent::ProximityIn { surface: OURS },
        ToolEvent::Button {
            button: BTN_STYLUS,
            pressed: true,
        },
        frame(0),
        ToolEvent::Button {
            button: BTN_STYLUS2,
            pressed: true,
        },
        frame(0),
        ToolEvent::Button {
            button: BTN_STYLUS,
            pressed: false,
        },
        frame(0),
        // BTN_STYLUS3: no mapping, so no change and no packet.
        ToolEvent::Button {
            button: 0x14d,
            pressed: true,
        },
        frame(0),
    ]);
    assert_eq!(packets.len(), 3, "the unmapped button commits nothing");
    assert!(packets[0].buttons.contains(PenButtons::BARREL));
    assert!(packets[1].buttons.contains(PenButtons::SECONDARY_BARREL));
    assert!(packets[1].buttons.contains(PenButtons::BARREL));
    assert!(!packets[2].buttons.contains(PenButtons::BARREL));
    assert!(packets[2].buttons.contains(PenButtons::SECONDARY_BARREL));
}

#[test]
fn a_proximity_out_releases_the_buttons() {
    let packets = fold(&[
        ToolEvent::Type(0x140),
        ToolEvent::ProximityIn { surface: OURS },
        ToolEvent::Button {
            button: BTN_STYLUS,
            pressed: true,
        },
        ToolEvent::Down,
        ToolEvent::Pressure(65535),
        frame(0),
        ToolEvent::ProximityOut,
        frame(0),
    ]);
    let leave = packets.last().unwrap();
    assert!(
        leave.buttons.is_empty(),
        "a tool out of range holds nothing"
    );
    assert!(!leave.down);
    assert_eq!(
        leave.pressure, 0.0,
        "and presses on nothing: a leave still claiming full pressure reads \
         downstream as a pen pushed hard against the glass on its way out"
    );
}

#[test]
fn the_eraser_end_is_a_tool() {
    let packets = fold(&[
        ToolEvent::Type(0x141),
        ToolEvent::ProximityIn { surface: OURS },
        ToolEvent::Down,
        frame(0),
    ]);
    assert_eq!(packets[0].tool, PenKind::Eraser);
    assert!(
        packets[0].buttons.is_empty(),
        "the eraser is the tool, never a button"
    );
}

#[test]
fn every_stylus_tool_type_maps_and_the_two_non_pens_do_not() {
    assert_eq!(tool_kind(0x140), Some(PenKind::Pen));
    assert_eq!(tool_kind(0x141), Some(PenKind::Eraser));
    assert_eq!(tool_kind(0x142), Some(PenKind::Brush));
    assert_eq!(tool_kind(0x143), Some(PenKind::Pencil));
    assert_eq!(tool_kind(0x144), Some(PenKind::Airbrush));
    assert_eq!(tool_kind(0x145), None, "Finger is a touch contact");
    assert_eq!(tool_kind(0x146), None, "Mouse is an indirect pointer");
    assert_eq!(tool_kind(0x147), Some(PenKind::Lens));
    assert_eq!(tool_kind(0xDEAD), None);
}

#[test]
fn a_finger_on_the_tablet_produces_no_pen_stream() {
    let packets = fold(&[
        ToolEvent::Type(0x145),
        ToolEvent::ProximityIn { surface: OURS },
        ToolEvent::Motion { x: 1.0, y: 1.0 },
        ToolEvent::Down,
        frame(0),
        ToolEvent::Up,
        ToolEvent::ProximityOut,
        frame(0),
    ]);
    assert!(
        packets.is_empty(),
        "a tablet finger is delivered by wl_touch, not as a stylus"
    );
}

#[test]
fn a_tool_over_a_sibling_window_is_not_ours() {
    let packets = fold(&[
        ToolEvent::Type(0x140),
        ToolEvent::ProximityIn { surface: THEIRS },
        ToolEvent::Motion { x: 1.0, y: 1.0 },
        ToolEvent::Down,
        frame(0),
        ToolEvent::ProximityOut,
        frame(0),
    ]);
    assert!(
        packets.is_empty(),
        "a tablet seat is per-seat: another window's hover must not leak here"
    );
}

/// **The test this replaces asserted the one sequence in which the bug cannot
/// occur.** `removed` arrives with **no trailing `frame`** — the tool is gone,
/// there is nothing left to frame — which is exactly why the accumulator has
/// to commit the leave on the spot. The first version of this test fed
/// `Removed` *followed by `frame(0)`*, and that trailing frame committed the
/// leave through the ordinary `Frame` arm: both assertions passed whether or
/// not the `Removed` arm committed anything at all. Deleting the commit left
/// the suite green.
///
/// So: no trailing frame. Delete the `commit` call in `ToolState::apply`'s
/// `Removed` arm and this reddens.
///
/// What a missing leave costs, since it is silent: the translator's proximity
/// machine still believes a pen is hovering, no `PointerLeave` is synthesised,
/// and the widget under the last hover keeps its hover state, its cursor and
/// its tooltip for a tool that has been physically unplugged. There is no
/// second route out.
///
/// The leave's *stamp*, its *state* and its *geometry* are pinned here too. A
/// yanked tablet must not leave a latched barrel button, a pressed tip or a
/// live pressure reading behind — the leave is the last word this tool gets —
/// and it must say *where* the tool was when it went, because that is the
/// position the translator ends the hover at.
///
/// The geometry half is the one that has to be asserted rather than reasoned
/// about: the `Removed` arm clears the four fields a leave must zero and
/// deliberately leaves `position`, `tilt` and `twist` alone. Nothing in the
/// code says so, so a later "clear everything while we are here" is one line
/// away — and with the fields left at their defaults by the fixture, both
/// `self.position = Point::new(0.0, 0.0);` and `self.tilt = None;
/// self.twist = None;` were green additions to that arm. Hence the motion,
/// tilt and rotation below: the leave is checked against what the last frame
/// actually carried.
#[test]
fn a_removed_tool_still_ends_its_hover() {
    let mut state = ToolState::default();
    let mut out = Vec::new();
    for event in [
        ToolEvent::Type(0x140),
        ToolEvent::ProximityIn { surface: OURS },
        ToolEvent::Motion { x: 40.0, y: 50.0 },
        ToolEvent::Down,
        ToolEvent::Pressure(65535),
        ToolEvent::Tilt { x: -12.0, y: 34.0 },
        ToolEvent::Rotation(90.0),
        ToolEvent::Button {
            button: BTN_STYLUS,
            pressed: true,
        },
        frame(1_000),
        // The tablet is unplugged here. Nothing follows.
        ToolEvent::Removed,
    ] {
        state.apply(event, OURS, &mut out);
    }

    assert_eq!(
        out.len(),
        2,
        "the stroke, and the leave `removed` committed for itself"
    );
    assert!(out[0].in_proximity && out[0].down);
    assert_eq!(out[0].position, Point::new(40.0, 50.0));
    assert_eq!(out[0].tilt, Some((-12.0, 34.0)));
    assert_eq!(out[0].twist, Some(90.0));

    let leave = &out[1];
    assert!(
        !leave.in_proximity,
        "the hover ends with no frame there to end it"
    );
    assert!(!leave.down, "a tool that is gone presses on nothing");
    assert_eq!(leave.pressure, 0.0);
    assert!(
        leave.buttons.is_empty(),
        "and does not hold a barrel button down for ever"
    );
    assert_eq!(
        leave.device_time_ms,
        Some(1_000),
        "dated at the last frame the compositor gave us for this tool — \
         `last_frame_ms` is read for exactly this and nothing else"
    );
    assert_eq!(
        leave.position,
        Point::new(40.0, 50.0),
        "the leave is placed where the tool last was, not at the origin: the \
         translator ends the hover at this point, and (0, 0) is a real corner \
         of the window that some other widget owns"
    );
    assert_eq!(
        leave.tilt,
        Some((-12.0, 34.0)),
        "and carries the last known tilt rather than dropping to `None`, which \
         a consumer reads as `a tool that reports no tilt`"
    );
    assert_eq!(leave.twist, Some(90.0), "same for twist");

    // White-box, deliberately. `commit`'s end-of-session surface release is
    // belt-and-braces with its `!in_proximity && !reported_proximity` guard,
    // and a mutation run says so in as many words: deleting *either* alone
    // changes no packet this suite can observe, deleting *both* re-announces a
    // leave that was already sent (`a_leave_is_emitted_exactly_once` catches
    // that). Asserting the invariant directly is the only thing that tells the
    // two apart.
    assert_eq!(
        state.surface, None,
        "the session is over, so the surface it was over is released"
    );
}

/// The other half of the `removed` contract — and a correction to what this
/// module used to claim.
///
/// A tool unplugged **before it has ever framed** does not emit a leave with no
/// stamp. It emits *nothing at all*: no frame ever committed an enter, so no
/// hover was ever announced, and there is nothing to retract. `commit`'s
/// `!in_proximity && !reported_proximity` guard is what makes that true, and
/// deleting it synthesises a `PointerLeave` for a pointer that never arrived.
///
/// The same guard covers the in-and-out-inside-one-frame case, which is the
/// only other way to reach it.
#[test]
fn a_tool_that_never_announced_a_hover_never_retracts_one() {
    assert!(
        fold(&[
            ToolEvent::Type(0x140),
            ToolEvent::ProximityIn { surface: OURS },
            // Unplugged inside the very burst that announced it.
            ToolEvent::Removed,
        ])
        .is_empty(),
        "no frame committed an enter, so there is no hover to end"
    );

    assert!(
        fold(&[
            ToolEvent::Type(0x140),
            ToolEvent::ProximityIn { surface: OURS },
            ToolEvent::ProximityOut,
            frame(0),
        ])
        .is_empty(),
        "in and out inside one frame is not a hover either"
    );
}

/// Every `proximity_in` opens a session that owes nothing to the last one.
///
/// The defence: a tool arrives here **mid-stroke**. It was drawing on a sibling
/// window — tip down, full pressure, barrel held — and the next thing this
/// accumulator sees is a `proximity_in` over our surface. All three of those
/// belong to the session that just ended, and the first packet this window sees
/// must not inherit them.
///
/// Written deliberately with **no intervening `proximity_out`**. A well-behaved
/// compositor sends one, and the `ProximityOut` arm clears the same three
/// fields — which is precisely why the reset in the `ProximityIn` arm sat
/// unpinned: with a leave in the vector the two arms cover for each other and
/// deleting either one is invisible.
#[test]
fn a_new_proximity_session_starts_clean() {
    let packets = fold(&[
        ToolEvent::Type(0x140),
        // Drawing, hard, on somebody else's window.
        ToolEvent::ProximityIn { surface: THEIRS },
        ToolEvent::Motion { x: 500.0, y: 500.0 },
        ToolEvent::Down,
        ToolEvent::Pressure(65535),
        ToolEvent::Button {
            button: BTN_STYLUS,
            pressed: true,
        },
        frame(0),
        // ...and now it is over ours.
        ToolEvent::ProximityIn { surface: OURS },
        ToolEvent::Motion { x: 1.0, y: 1.0 },
        frame(0),
    ]);

    assert_eq!(packets.len(), 1, "only our own session reaches this stream");
    let enter = &packets[0];
    assert!(enter.in_proximity);
    assert!(!enter.down, "the tip state belonged to the other window");
    assert_eq!(enter.pressure, 0.0, "and so did the pressure");
    assert!(enter.buttons.is_empty(), "and so did the barrel button");
    assert_eq!(enter.position, Point::new(1.0, 1.0));
}

#[test]
fn pressure_and_rotation_are_normalised() {
    let packets = fold(&[
        ToolEvent::Type(0x140),
        ToolEvent::ProximityIn { surface: OURS },
        ToolEvent::Pressure(0),
        ToolEvent::Rotation(-90.0),
        frame(0),
        ToolEvent::Pressure(65535),
        ToolEvent::Rotation(361.0),
        frame(0),
    ]);
    assert_eq!(packets[0].pressure, 0.0);
    assert_eq!(
        packets[0].twist,
        Some(270.0),
        "a negative rotation wraps into 0..360 rather than escaping the range"
    );
    assert_eq!(packets[1].pressure, 1.0);
    assert_eq!(packets[1].twist, Some(1.0));
}

/// `tablet-v2.xml` normalises pressure over `0..=65535`, so anything above that
/// is a device or compositor out of spec — and the clamp is what stops it
/// reaching a widget. Unclamped, `u32::MAX` arrives as a pressure of ~65 536,
/// which every downstream `pressure * width` reads as a stroke the width of the
/// window.
///
/// Only the upper bound is reachable: `raw as f32 / 65535.0` cannot be negative
/// for a `u32`, so the lower half of the clamp is there for symmetry and is not
/// claimed to be tested.
#[test]
fn a_pressure_above_the_protocols_range_is_clamped_not_scaled() {
    let packets = fold(&[
        ToolEvent::Type(0x140),
        ToolEvent::ProximityIn { surface: OURS },
        ToolEvent::Pressure(u32::MAX),
        frame(0),
    ]);
    assert_eq!(packets[0].pressure, 1.0);
}

#[test]
fn tilt_is_clamped_to_the_documented_range() {
    let packets = fold(&[
        ToolEvent::Type(0x140),
        ToolEvent::ProximityIn { surface: OURS },
        ToolEvent::Tilt {
            x: -180.0,
            y: 180.0,
        },
        frame(0),
    ]);
    assert_eq!(packets[0].tilt, Some((-90.0, 90.0)));
}

/// The idle tier is the whole answer to "a compositor advertises the tablet
/// manager whether or not a digitizer exists". Collapsing it back onto one
/// interval puts a 250 Hz timer on every window of every Wayland session with
/// no tablet in it.
#[test]
fn a_session_with_no_tool_is_not_polled_at_stylus_rate() {
    use super::{IDLE_POLL_INTERVAL, POLL_INTERVAL, WaylandPenSource};

    assert_eq!(
        WaylandPenSource::poll_interval(true),
        POLL_INTERVAL,
        "a live tool is a continuous stream and must be read at stylus rate"
    );
    assert_eq!(
        WaylandPenSource::poll_interval(false),
        IDLE_POLL_INTERVAL,
        "no tool announced means no stroke is possible, so the thread must \
         stand down"
    );
    assert!(
        IDLE_POLL_INTERVAL >= POLL_INTERVAL * 8,
        "an idle tier worth having is at least an order of magnitude cheaper: \
         {IDLE_POLL_INTERVAL:?} vs {POLL_INTERVAL:?}"
    );
    assert!(
        IDLE_POLL_INTERVAL <= std::time::Duration::from_millis(500),
        "and short enough that a tablet plugged in mid-session is on the fast \
         tier before a hand reaches the pen: {IDLE_POLL_INTERVAL:?}"
    );
}

// ---------------------------------------------------------------------------
// The frame's own clock
// ---------------------------------------------------------------------------

/// `frame` carries the time and the protocol says so ("the time of the event
/// with millisecond granularity"). Dropping it — which this shim did — is what
/// left a whole drained batch claiming one instant.
#[test]
fn every_packet_carries_the_frame_that_committed_it() {
    let packets = fold(&a_full_stroke());
    let stamps: Vec<_> = packets.iter().map(|p| p.device_time_ms).collect();
    assert_eq!(
        stamps,
        vec![
            Some(1_000),
            Some(1_003),
            Some(1_006),
            Some(1_009),
            Some(1_012),
            Some(1_015),
        ],
        "one stamp per committed frame, in the compositor's own order"
    );
}

/// The stamp belongs to the `frame` that committed the state, not to the
/// motion inside it: a `frame` folds everything accumulated since the last
/// one, and that is the instant the compositor dates the packet at.
#[test]
fn the_stamp_is_the_committing_frames_not_the_axis_events() {
    let packets = fold(&[
        ToolEvent::Type(0x140),
        ToolEvent::ProximityIn { surface: OURS },
        ToolEvent::Motion { x: 1.0, y: 1.0 },
        ToolEvent::Pressure(100),
        ToolEvent::Tilt { x: 5.0, y: 5.0 },
        frame(4_242),
    ]);
    assert_eq!(packets.len(), 1);
    assert_eq!(packets[0].device_time_ms, Some(4_242));
}

/// A frame that commits nothing emits nothing, so it cannot contribute a
/// phantom stamp to the batch either.
#[test]
fn an_empty_frame_contributes_no_stamp() {
    let packets = fold(&[
        ToolEvent::Type(0x140),
        ToolEvent::ProximityIn { surface: OURS },
        frame(10),
        frame(20),
        frame(30),
    ]);
    assert_eq!(packets.len(), 1, "only the proximity_in was dirty");
    assert_eq!(packets[0].device_time_ms, Some(10));
}

/// End to end: a Wayland batch reaches the translator with the compositor's
/// spacing intact, not with the poll's single clock smeared across it.
#[test]
fn a_folded_batch_back_dates_to_the_compositors_spacing() {
    use crate::pen::back_date;
    use teksilo_core::pointer::EventTime;

    let packets = fold(&a_full_stroke());
    let stamps: Vec<_> = packets.iter().map(|p| p.device_time_ms).collect();
    let now = EventTime::from_millis(50_000);
    let times = back_date(now, &stamps);

    assert_eq!(*times.last().unwrap(), now, "the newest packet is `now`");
    for pair in times.windows(2) {
        assert!(pair[1] > pair[0], "{times:?} must strictly increase");
    }
    // 1_015 − 1_000 = 15 ms of stroke, preserved exactly.
    assert_eq!(now.saturating_since(times[0]), Duration::from_millis(15));
}

// ---------------------------------------------------------------------------
// The protocol decode
// ---------------------------------------------------------------------------
//
// Everything above this line drives `ToolState` with hand-written
// `ToolEvent`s, which is the right level for the fold — but it is one layer
// *above* `translate_tool_event`, and that layer is no longer a pure variant
// rename. `frame` carries a millisecond stamp, `motion` carries a pair of
// coordinates, `button` carries a code and a state: the translator moves
// **data**, and a decode that drops a field compiles, runs, and is wrong.
//
// It is also testable without a compositor. A `zwp_tablet_tool_v2::Event` is a
// plain enum; only the variants carrying live protocol objects need a
// connection, and there is exactly one of those. So every arm that reads a
// number is pinned here against a genuine protocol event.
//
// **The one arm with no test, stated plainly.** `proximity_in` carries two live
// protocol objects (`zwp_tablet_v2` and `wl_surface`), and a `Proxy` cannot
// exist without a connection to a compositor — so the single line that reads
// `surface.id().protocol_id()` has no off-device test and will not get one
// short of a fake Wayland server. Deleting the arm outright leaves this suite
// green, and a mutation run says so rather than this comment asserting it.
// What keeps it honest instead is that the field is **not droppable**:
// `ToolEvent::ProximityIn` has no default for `surface`, so a decode that
// stopped reading it would not compile, and a decode that read the *wrong*
// surface would put every stroke in the wrong window — loud, not silent. The
// routing that consumes the id *is* covered, by
// `a_tool_over_a_sibling_window_is_not_ours` and
// `a_new_proximity_session_starts_clean`.

/// Shorthand for the decode under test.
fn translate(event: zwp_tablet_tool_v2::Event) -> Option<ToolEvent> {
    translate_tool_event(&event)
}

/// **The regression.** `frame`'s `time` argument is the compositor's own
/// millisecond stamp, and this shim used to drop it on the floor: the decode
/// read `E::Frame { .. }` and produced a `ToolEvent::Frame` with nothing in
/// it, so every packet of a drained batch claimed the same instant.
///
/// Several distinct values, so a decode that returns any *constant* — `0`, or
/// a stale field from another event — fails rather than passing on the one
/// vector that happens to agree.
#[test]
fn a_frames_time_reaches_the_tool_event() {
    for ms in [0_u32, 1, 4, 1_000, 1_003, 4_242, 0xFFFF_FFFE, u32::MAX] {
        assert_eq!(
            translate(zwp_tablet_tool_v2::Event::Frame { time: ms }),
            Some(ToolEvent::Frame { time_ms: ms }),
            "frame({ms}) must arrive with its own stamp"
        );
    }
}

/// The axis events carry numbers too, and each pair is asymmetric so a decode
/// that swaps x for y fails rather than agreeing by coincidence.
#[test]
fn every_axis_event_carries_its_own_numbers() {
    assert_eq!(
        translate(zwp_tablet_tool_v2::Event::Motion { x: 10.5, y: -20.25 }),
        Some(ToolEvent::Motion { x: 10.5, y: -20.25 })
    );
    assert_eq!(
        translate(zwp_tablet_tool_v2::Event::Pressure { pressure: 32_768 }),
        Some(ToolEvent::Pressure(32_768))
    );
    assert_eq!(
        translate(zwp_tablet_tool_v2::Event::Tilt {
            tilt_x: -12.0,
            tilt_y: 34.0,
        }),
        Some(ToolEvent::Tilt { x: -12.0, y: 34.0 })
    );
    assert_eq!(
        translate(zwp_tablet_tool_v2::Event::Rotation { degrees: -90.0 }),
        Some(ToolEvent::Rotation(-90.0)),
        "the decode passes the raw degrees through; wrapping is ToolState's job"
    );
}

/// A `button` event carries two independent facts — *which* button and
/// *whether* it went down — and losing either one sticks a barrel button on or
/// off for the rest of the stroke.
#[test]
fn a_button_events_code_and_state_both_survive() {
    assert_eq!(
        translate(zwp_tablet_tool_v2::Event::Button {
            serial: 7,
            button: BTN_STYLUS,
            state: WEnum::Value(zwp_tablet_tool_v2::ButtonState::Pressed),
        }),
        Some(ToolEvent::Button {
            button: BTN_STYLUS,
            pressed: true,
        })
    );
    assert_eq!(
        translate(zwp_tablet_tool_v2::Event::Button {
            serial: 8,
            button: BTN_STYLUS2,
            state: WEnum::Value(zwp_tablet_tool_v2::ButtonState::Released),
        }),
        Some(ToolEvent::Button {
            button: BTN_STYLUS2,
            pressed: false,
        })
    );
    assert_eq!(
        translate(zwp_tablet_tool_v2::Event::Button {
            serial: 9,
            button: BTN_STYLUS,
            state: WEnum::Unknown(0xDEAD),
        }),
        Some(ToolEvent::Button {
            button: BTN_STYLUS,
            pressed: false,
        }),
        "a state this protocol version does not name is not a press"
    );
}

/// `type` arrives as a `WEnum`, and both halves of it matter: a known tool
/// keeps its protocol value (which [`tool_kind`] then maps), and an unknown one
/// keeps the raw number rather than collapsing to a default that would make a
/// future tool read as a pen.
#[test]
fn the_tool_type_survives_known_and_unknown() {
    assert_eq!(
        translate(zwp_tablet_tool_v2::Event::Type {
            tool_type: WEnum::Value(zwp_tablet_tool_v2::Type::Pen),
        }),
        Some(ToolEvent::Type(0x140))
    );
    assert_eq!(
        translate(zwp_tablet_tool_v2::Event::Type {
            tool_type: WEnum::Value(zwp_tablet_tool_v2::Type::Eraser),
        }),
        Some(ToolEvent::Type(0x141))
    );
    assert_eq!(
        translate(zwp_tablet_tool_v2::Event::Type {
            tool_type: WEnum::Value(zwp_tablet_tool_v2::Type::Lens),
        }),
        Some(ToolEvent::Type(0x147))
    );
    assert_eq!(
        translate(zwp_tablet_tool_v2::Event::Type {
            tool_type: WEnum::Unknown(0xBEEF),
        }),
        Some(ToolEvent::Type(0xBEEF)),
        "an unrecognised tool stays unrecognised — tool_kind answers None for it"
    );
}

/// The transitions carry no axis data, so all the decode owes them is the
/// right variant. `down`'s serial is deliberately discarded: Teksilo never
/// sends a request that has to quote it.
#[test]
fn the_transitions_map_one_to_one() {
    assert_eq!(
        translate(zwp_tablet_tool_v2::Event::Down { serial: 42 }),
        Some(ToolEvent::Down)
    );
    assert_eq!(
        translate(zwp_tablet_tool_v2::Event::Up),
        Some(ToolEvent::Up)
    );
    assert_eq!(
        translate(zwp_tablet_tool_v2::Event::ProximityOut),
        Some(ToolEvent::ProximityOut)
    );
    assert_eq!(
        translate(zwp_tablet_tool_v2::Event::Removed),
        Some(ToolEvent::Removed)
    );
}

/// The drops are a decision, not an oversight, so they are pinned as such: the
/// three axes Teksilo has no home for, and the identity burst that says what
/// the tool *is* rather than what it is doing. If one of these ever grows a
/// home, this test is where the decision is recorded.
#[test]
fn the_events_teksilo_has_no_home_for_are_dropped_on_purpose() {
    assert_eq!(
        translate(zwp_tablet_tool_v2::Event::Distance { distance: 500 }),
        None,
        "hover distance has no PenPacket field"
    );
    assert_eq!(
        translate(zwp_tablet_tool_v2::Event::Slider { position: -100 }),
        None,
        "the airbrush slider has no PenPacket field"
    );
    assert_eq!(
        translate(zwp_tablet_tool_v2::Event::Wheel {
            degrees: 15.0,
            clicks: 1,
        }),
        None,
        "a tool wheel is not a scroll wheel and is not routed as one"
    );
    assert_eq!(
        translate(zwp_tablet_tool_v2::Event::HardwareSerial {
            hardware_serial_hi: 1,
            hardware_serial_lo: 2,
        }),
        None
    );
    assert_eq!(
        translate(zwp_tablet_tool_v2::Event::HardwareIdWacom {
            hardware_id_hi: 3,
            hardware_id_lo: 4,
        }),
        None
    );
    assert_eq!(
        translate(zwp_tablet_tool_v2::Event::Capability {
            capability: WEnum::Value(zwp_tablet_tool_v2::Capability::Pressure),
        }),
        None,
        "capabilities are advertised, not measured: PenCaps is derived from \
         what actually arrives"
    );
    assert_eq!(translate(zwp_tablet_tool_v2::Event::Done), None);
}

/// Decode and fold, chained: the protocol events go through
/// `translate_tool_event` and straight into `ToolState`, so the stamps on the
/// packets are the ones the decode read off the wire rather than ones a test
/// wrote by hand.
///
/// `proximity_in` is spliced in as a `ToolEvent` because it is the one arm
/// that cannot be built off a compositor — see the test below.
#[test]
fn a_decoded_run_folds_with_the_compositors_own_stamps() {
    use zwp_tablet_tool_v2::Event as E;

    let mut state = ToolState::default();
    let mut out = Vec::new();

    for event in [
        E::Type {
            tool_type: WEnum::Value(zwp_tablet_tool_v2::Type::Pen),
        },
        // The identity burst a real tool sends before it is usable; the decode
        // drops all of it, and the fold must not notice.
        E::HardwareSerial {
            hardware_serial_hi: 0xDEAD,
            hardware_serial_lo: 0xBEEF,
        },
        E::Capability {
            capability: WEnum::Value(zwp_tablet_tool_v2::Capability::Pressure),
        },
        E::Done,
        E::Motion { x: 10.0, y: 20.0 },
        E::Frame { time: 1_000 },
        E::Down { serial: 1 },
        E::Pressure { pressure: 65_535 },
        E::Frame { time: 1_004 },
        E::Motion { x: 11.0, y: 21.0 },
        E::Distance { distance: 0 },
        E::Frame { time: 1_008 },
        E::Up,
        E::ProximityOut,
        E::Frame { time: 1_012 },
    ] {
        let Some(translated) = translate_tool_event(&event) else {
            continue;
        };
        state.apply(translated, OURS, &mut out);
        if matches!(translated, ToolEvent::Type(_)) {
            // `proximity_in` carries live proxies, so the session is opened by
            // hand — everything after it is decoded.
            state.apply(ToolEvent::ProximityIn { surface: OURS }, OURS, &mut out);
        }
    }

    let stamps: Vec<_> = out.iter().map(|p| p.device_time_ms).collect();
    assert_eq!(
        stamps,
        vec![Some(1_000), Some(1_004), Some(1_008), Some(1_012)],
        "the decode's stamps reach the packets in the compositor's own order"
    );
    assert_eq!(out[0].position, Point::new(10.0, 20.0));
    assert!(out[1].down);
    assert_eq!(out[1].pressure, 1.0);
    assert_eq!(out[2].position, Point::new(11.0, 21.0));
    assert!(!out[3].in_proximity);
}

// ---------------------------------------------------------------------------
// The source's own contract
// ---------------------------------------------------------------------------

/// The three answers `WaylandPenSource` gives the pen pump — none of which
/// needs a compositor, because the struct is a queue handle and nothing else.
///
/// All three were live mutation survivors: a `poll` that drops the batch on the
/// floor, a `capabilities` that stops advertising a pen, and a
/// `polls_off_thread` that claims to be synchronous each left the whole suite
/// green. The last is the quiet one — the tablet thread fills this queue
/// between loop turns, so a source answering `false` there is a stylus whose
/// ink stops until some *other* event happens to wake the loop.
#[test]
fn the_source_hands_the_pump_its_queue_and_its_terms() {
    let queue = Arc::new(PenQueue::default());
    let mut source = WaylandPenSource {
        queue: queue.clone(),
    };

    assert_eq!(
        source.capabilities(),
        PenCaps::FULL_PEN,
        "a tablet tool reports its kind, pressure, tilt and rotation"
    );
    assert!(
        source.polls_off_thread(),
        "the queue is filled by the dispatch thread, so the loop has to look \
         again after a wake"
    );

    // The dispatch thread's side of the handoff.
    queue.packets.lock().unwrap().extend(fold(&a_full_stroke()));

    let mut out = Vec::new();
    source.poll(&mut out);
    assert_eq!(out.len(), 6, "poll hands the whole batch to the caller");
    assert_eq!(out[0].position, Point::new(10.0, 20.0));
    assert_eq!(out[5].device_time_ms, Some(1_015));

    let mut again = Vec::new();
    source.poll(&mut again);
    assert!(
        again.is_empty(),
        "and drains it, so no packet is delivered twice"
    );
}
