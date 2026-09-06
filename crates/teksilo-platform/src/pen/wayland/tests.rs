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

/// The surface this window owns. Everything else is a sibling window's.
const OURS: u32 = 12;
/// A sibling window's surface, for the routing test.
const THEIRS: u32 = 13;

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
        ToolEvent::Frame,
        ToolEvent::Motion { x: 12.0, y: 22.0 },
        ToolEvent::Frame,
        ToolEvent::Down,
        ToolEvent::Pressure(32768),
        ToolEvent::Frame,
        ToolEvent::Motion { x: 40.0, y: 50.0 },
        ToolEvent::Pressure(65535),
        ToolEvent::Tilt { x: -12.0, y: 34.0 },
        ToolEvent::Rotation(90.0),
        ToolEvent::Frame,
        ToolEvent::Up,
        ToolEvent::Frame,
        ToolEvent::ProximityOut,
        ToolEvent::Frame,
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
        ToolEvent::Frame,
        ToolEvent::Motion { x: 2.0, y: 2.0 },
        ToolEvent::Frame,
        ToolEvent::Motion { x: 3.0, y: 3.0 },
        ToolEvent::Frame,
        ToolEvent::ProximityOut,
        ToolEvent::Frame,
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
        ToolEvent::Frame,
        ToolEvent::ProximityOut,
        ToolEvent::Frame,
        // A stray frame after the session ended must not re-announce it.
        ToolEvent::Frame,
        ToolEvent::ProximityOut,
        ToolEvent::Frame,
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
    state.apply(ToolEvent::Frame, OURS, &mut out);
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
        ToolEvent::Frame,
        ToolEvent::Frame,
        ToolEvent::Frame,
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
        ToolEvent::Frame,
        ToolEvent::Button {
            button: BTN_STYLUS2,
            pressed: true,
        },
        ToolEvent::Frame,
        ToolEvent::Button {
            button: BTN_STYLUS,
            pressed: false,
        },
        ToolEvent::Frame,
        // BTN_STYLUS3: no mapping, so no change and no packet.
        ToolEvent::Button {
            button: 0x14d,
            pressed: true,
        },
        ToolEvent::Frame,
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
        ToolEvent::Frame,
        ToolEvent::ProximityOut,
        ToolEvent::Frame,
    ]);
    let leave = packets.last().unwrap();
    assert!(
        leave.buttons.is_empty(),
        "a tool out of range holds nothing"
    );
    assert!(!leave.down);
}

#[test]
fn the_eraser_end_is_a_tool() {
    let packets = fold(&[
        ToolEvent::Type(0x141),
        ToolEvent::ProximityIn { surface: OURS },
        ToolEvent::Down,
        ToolEvent::Frame,
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
        ToolEvent::Frame,
        ToolEvent::Up,
        ToolEvent::ProximityOut,
        ToolEvent::Frame,
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
        ToolEvent::Frame,
        ToolEvent::ProximityOut,
        ToolEvent::Frame,
    ]);
    assert!(
        packets.is_empty(),
        "a tablet seat is per-seat: another window's hover must not leak here"
    );
}

#[test]
fn a_removed_tool_still_ends_its_hover() {
    // `removed` arrives with no trailing frame, so the accumulator has to
    // commit the leave itself — the same thing the Dispatch impl does.
    let mut state = ToolState::default();
    let mut out = Vec::new();
    for event in [
        ToolEvent::Type(0x140),
        ToolEvent::ProximityIn { surface: OURS },
        ToolEvent::Frame,
        ToolEvent::Removed,
        ToolEvent::Frame,
    ] {
        state.apply(event, OURS, &mut out);
    }
    assert_eq!(out.len(), 2);
    assert!(!out[1].in_proximity);
}

#[test]
fn pressure_and_rotation_are_normalised() {
    let packets = fold(&[
        ToolEvent::Type(0x140),
        ToolEvent::ProximityIn { surface: OURS },
        ToolEvent::Pressure(0),
        ToolEvent::Rotation(-90.0),
        ToolEvent::Frame,
        ToolEvent::Pressure(65535),
        ToolEvent::Rotation(361.0),
        ToolEvent::Frame,
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

#[test]
fn tilt_is_clamped_to_the_documented_range() {
    let packets = fold(&[
        ToolEvent::Type(0x140),
        ToolEvent::ProximityIn { surface: OURS },
        ToolEvent::Tilt {
            x: -180.0,
            y: 180.0,
        },
        ToolEvent::Frame,
    ]);
    assert_eq!(packets[0].tilt, Some((-90.0, 90.0)));
}
