// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Recorded `POINTER_PEN_INFO` / `POINTER_TOUCH_INFO` layouts, decoded with no
//! Windows in sight.
//!
//! Every vector here is a byte image of the 64-bit struct as documented in
//! `super::layout` — Microsoft's field order laid out by the Windows x64 C ABI.
//! They are **synthesised from that layout, not captured from a digitizer**:
//! this machine has no Windows and no tablet. What keeps them honest is the
//! other half of the pincer — compiled *for* Windows, `super::abi_assertions`
//! checks every offset in the table against `windows-rs`'s own struct with
//! `offset_of!`, so a vector that decodes correctly here decodes correctly
//! there or the build fails.
//!
//! [`GOLDEN_PEN_DOWN`] is written out byte by byte so the offsets themselves
//! are pinned by something a reader can check against a hex dump; the rest are
//! built through [`pen_bytes`], which writes the same offsets by name.

use super::*;

// ---------------------------------------------------------------------------
// Vectors
// ---------------------------------------------------------------------------

/// A pen in contact at half pressure with the barrel button held.
///
/// `pointerType = PT_PEN`, `pointerId = 0x2A`,
/// `pointerFlags = INRANGE | INCONTACT | FIRSTBUTTON | PRIMARY` (`0x2016`),
/// `ptPixelLocation = (1920, 540)`, `dwTime = 0x0012D687`,
/// `penFlags = PEN_FLAG_BARREL`, `penMask = PRESSURE | ROTATION | TILT_X |
/// TILT_Y`, `pressure = 512`, `rotation = 271`, `tiltX = -37`, `tiltY = 60`.
#[rustfmt::skip]
const GOLDEN_PEN_DOWN: [u8; 120] = [
    0x03, 0x00, 0x00, 0x00, 0x2a, 0x00, 0x00, 0x00,
    0x07, 0x00, 0x00, 0x00, 0x16, 0x20, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x80, 0x07, 0x00, 0x00, 0x1c, 0x02, 0x00, 0x00,
    0x70, 0xc6, 0x00, 0x00, 0xcf, 0x37, 0x00, 0x00,
    0x80, 0x07, 0x00, 0x00, 0x1c, 0x02, 0x00, 0x00,
    0x70, 0xc6, 0x00, 0x00, 0xcf, 0x37, 0x00, 0x00,
    0x87, 0xd6, 0x12, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x33, 0x1c, 0x5d, 0x04, 0x00, 0x00, 0x00,
    0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x0f, 0x00, 0x00, 0x00,
    0x00, 0x02, 0x00, 0x00, 0x0f, 0x01, 0x00, 0x00,
    0xdb, 0xff, 0xff, 0xff, 0x3c, 0x00, 0x00, 0x00,
];

/// Which axes a synthesised packet marks valid.
const ALL_AXES: u32 = PEN_MASK_PRESSURE | PEN_MASK_ROTATION | PEN_MASK_TILT_X | PEN_MASK_TILT_Y;

/// Build a `POINTER_PEN_INFO` image by writing the documented offsets.
#[allow(clippy::too_many_arguments)]
fn pen_bytes(
    flags: u32,
    pen_flags: u32,
    pen_mask: u32,
    pressure: u32,
    rotation: u32,
    tilt_x: i32,
    tilt_y: i32,
    screen: (i32, i32),
) -> [u8; layout::PEN_INFO_SIZE] {
    let mut bytes = [0u8; layout::PEN_INFO_SIZE];
    let mut put = |offset: usize, value: u32| {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    };
    put(layout::POINTER_TYPE, PT_PEN);
    put(layout::POINTER_ID, 0x2A);
    put(layout::POINTER_FLAGS, flags);
    put(layout::PIXEL_X, screen.0 as u32);
    put(layout::PIXEL_Y, screen.1 as u32);
    put(layout::TIME, 0x0012_D687);
    put(layout::PEN_FLAGS, pen_flags);
    put(layout::PEN_MASK, pen_mask);
    put(layout::PEN_PRESSURE, pressure);
    put(layout::PEN_ROTATION, rotation);
    put(layout::PEN_TILT_X, tilt_x as u32);
    put(layout::PEN_TILT_Y, tilt_y as u32);
    bytes
}

/// Build a `POINTER_TOUCH_INFO` image the same way.
fn touch_bytes(
    pointer_id: u32,
    flags: u32,
    touch_mask: u32,
    contact: (i32, i32, i32, i32),
    pressure: u32,
) -> [u8; layout::TOUCH_INFO_SIZE] {
    let mut bytes = [0u8; layout::TOUCH_INFO_SIZE];
    let mut put = |offset: usize, value: u32| {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    };
    put(layout::POINTER_TYPE, PT_TOUCH);
    put(layout::POINTER_ID, pointer_id);
    put(layout::POINTER_FLAGS, flags);
    put(layout::PIXEL_X, contact.0 as u32);
    put(layout::PIXEL_Y, contact.1 as u32);
    put(layout::TOUCH_MASK, touch_mask);
    put(layout::TOUCH_CONTACT_LEFT, contact.0 as u32);
    put(layout::TOUCH_CONTACT_TOP, contact.1 as u32);
    put(layout::TOUCH_CONTACT_RIGHT, contact.2 as u32);
    put(layout::TOUCH_CONTACT_BOTTOM, contact.3 as u32);
    put(layout::TOUCH_ORIENTATION, 90);
    put(layout::TOUCH_PRESSURE, pressure);
    bytes
}

// ---------------------------------------------------------------------------
// The layout itself
// ---------------------------------------------------------------------------

#[test]
fn the_golden_vector_decodes_field_for_field() {
    let info = decode_pen_info(&GOLDEN_PEN_DOWN).expect("120 bytes is a whole POINTER_PEN_INFO");
    assert_eq!(info.pointer_type, PT_PEN);
    assert_eq!(info.pointer_id, 0x2A);
    assert_eq!(info.flags, 0x2016);
    assert_eq!(info.screen, (1920, 540));
    assert_eq!(info.time_ms, 0x0012_D687);
    assert_eq!(info.pen_flags, PEN_FLAG_BARREL);
    assert_eq!(info.pen_mask, ALL_AXES);
    assert_eq!(info.pressure, 512);
    assert_eq!(info.rotation, 271);
    assert_eq!(info.tilt_x, -37);
    assert_eq!(info.tilt_y, 60);
    assert!(info.in_proximity() && info.down() && !info.cancelled());
}

#[test]
fn a_short_slice_decodes_to_nothing() {
    assert!(decode_pen_info(&GOLDEN_PEN_DOWN[..119]).is_none());
    assert!(decode_pen_info(&[]).is_none());
    assert!(decode_touch_info(&[0u8; layout::TOUCH_INFO_SIZE - 1]).is_none());
}

// ---------------------------------------------------------------------------
// Pressure
// ---------------------------------------------------------------------------

#[test]
fn pressure_spans_zero_to_one_over_the_documented_range() {
    for (raw, expected) in [(0u32, 0.0f32), (512, 0.5), (1024, 1.0)] {
        let bytes = pen_bytes(
            POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT,
            0,
            ALL_AXES,
            raw,
            0,
            0,
            0,
            (0, 0),
        );
        let info = decode_pen_info(&bytes).unwrap();
        assert_eq!(
            info.pressure_normalised(),
            Some(expected),
            "raw pressure {raw} must normalise to {expected}"
        );
    }
}

#[test]
fn an_unmeasured_axis_is_none_not_zero() {
    // A device with no pressure sensor leaves PEN_MASK_PRESSURE clear and
    // writes 0. Reading that as "no pressure at all" while the tip is pressed
    // to the glass is the bug this test exists to prevent.
    let bytes = pen_bytes(
        POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT,
        0,
        0,
        0,
        0,
        0,
        0,
        (0, 0),
    );
    let info = decode_pen_info(&bytes).unwrap();
    assert_eq!(info.pressure_normalised(), None);
    assert_eq!(info.tilt_degrees(), None);
    assert_eq!(info.twist_degrees(), None);

    // …and the packet a source builds from it says "full pressure while down",
    // not "zero pressure while down".
    let packet = info.to_packet((0, 0), 1.0);
    assert_eq!(packet.pressure, 1.0);
    assert!(packet.down);
}

#[test]
fn half_a_tilt_pair_is_no_tilt() {
    let bytes = pen_bytes(
        POINTER_FLAG_INRANGE,
        0,
        PEN_MASK_TILT_X,
        0,
        0,
        45,
        0,
        (0, 0),
    );
    assert_eq!(decode_pen_info(&bytes).unwrap().tilt_degrees(), None);
}

// ---------------------------------------------------------------------------
// Tilt, twist
// ---------------------------------------------------------------------------

#[test]
fn tilt_survives_the_extremes_of_its_range() {
    for (x, y) in [(-90i32, 90i32), (90, -90), (0, 0), (-37, 60)] {
        let bytes = pen_bytes(POINTER_FLAG_INRANGE, 0, ALL_AXES, 0, 0, x, y, (0, 0));
        let info = decode_pen_info(&bytes).unwrap();
        assert_eq!(
            info.tilt_degrees(),
            Some((x as f32, y as f32)),
            "tilt ({x}, {y}) must survive the round trip, sign and all"
        );
    }
}

#[test]
fn a_tilt_outside_the_documented_range_is_clamped() {
    let bytes = pen_bytes(POINTER_FLAG_INRANGE, 0, ALL_AXES, 0, 0, -1000, 1000, (0, 0));
    assert_eq!(
        decode_pen_info(&bytes).unwrap().tilt_degrees(),
        Some((-90.0, 90.0))
    );
}

#[test]
fn rotation_becomes_twist() {
    for raw in [0u32, 90, 271, 359] {
        let bytes = pen_bytes(POINTER_FLAG_INRANGE, 0, ALL_AXES, 0, raw, 0, 0, (0, 0));
        let info = decode_pen_info(&bytes).unwrap();
        assert_eq!(info.twist_degrees(), Some(raw as f32));
        assert_eq!(info.to_packet((0, 0), 1.0).twist, Some(raw as f32));
    }
}

// ---------------------------------------------------------------------------
// Tool and buttons
// ---------------------------------------------------------------------------

#[test]
fn the_eraser_is_a_tool_not_a_button() {
    // Flipped stylus.
    let inverted = pen_bytes(
        POINTER_FLAG_INRANGE,
        PEN_FLAG_INVERTED,
        ALL_AXES,
        0,
        0,
        0,
        0,
        (0, 0),
    );
    assert_eq!(decode_pen_info(&inverted).unwrap().tool(), PenKind::Eraser);

    // Eraser *button* on a stylus that has one: same meaning, same tool.
    let eraser_button = pen_bytes(
        POINTER_FLAG_INRANGE,
        PEN_FLAG_ERASER,
        ALL_AXES,
        0,
        0,
        0,
        0,
        (0, 0),
    );
    let info = decode_pen_info(&eraser_button).unwrap();
    assert_eq!(info.tool(), PenKind::Eraser);
    // The raw flag is carried, but it is not the barrel and it never becomes
    // a dispatched button.
    assert!(info.buttons().contains(PenButtons::ERASER));
    assert!(!info.buttons().contains(PenButtons::BARREL));

    // A plain tip is a plain pen.
    let plain = pen_bytes(POINTER_FLAG_INRANGE, 0, ALL_AXES, 0, 0, 0, 0, (0, 0));
    assert_eq!(decode_pen_info(&plain).unwrap().tool(), PenKind::Pen);
}

#[test]
fn the_barrel_is_read_from_either_place_the_os_reports_it() {
    let via_pen_flag = pen_bytes(
        POINTER_FLAG_INRANGE,
        PEN_FLAG_BARREL,
        ALL_AXES,
        0,
        0,
        0,
        0,
        (0, 0),
    );
    assert!(
        decode_pen_info(&via_pen_flag)
            .unwrap()
            .buttons()
            .contains(PenButtons::BARREL)
    );

    let via_pointer_flag = pen_bytes(
        POINTER_FLAG_INRANGE | POINTER_FLAG_SECONDBUTTON,
        0,
        ALL_AXES,
        0,
        0,
        0,
        0,
        (0, 0),
    );
    assert!(
        decode_pen_info(&via_pointer_flag)
            .unwrap()
            .buttons()
            .contains(PenButtons::BARREL)
    );

    let third = pen_bytes(
        POINTER_FLAG_INRANGE | POINTER_FLAG_THIRDBUTTON,
        0,
        ALL_AXES,
        0,
        0,
        0,
        0,
        (0, 0),
    );
    let buttons = decode_pen_info(&third).unwrap().buttons();
    assert!(buttons.contains(PenButtons::SECONDARY_BARREL));
    assert!(!buttons.contains(PenButtons::BARREL));
}

// ---------------------------------------------------------------------------
// Proximity and coordinates
// ---------------------------------------------------------------------------

#[test]
fn proximity_and_contact_are_independent_flags() {
    let hovering = decode_pen_info(&pen_bytes(
        POINTER_FLAG_INRANGE,
        0,
        ALL_AXES,
        0,
        0,
        0,
        0,
        (0, 0),
    ))
    .unwrap();
    assert!(hovering.in_proximity() && !hovering.down());
    let packet = hovering.to_packet((0, 0), 1.0);
    assert!(packet.in_proximity && !packet.down);
    assert_eq!(packet.pressure, 0.0, "a hovering pen presses on nothing");

    let gone = decode_pen_info(&pen_bytes(0, 0, ALL_AXES, 0, 0, 0, 0, (0, 0))).unwrap();
    assert!(!gone.in_proximity() && !gone.down());
}

#[test]
fn screen_pixels_become_window_logical_points() {
    let info = decode_pen_info(&GOLDEN_PEN_DOWN).unwrap();
    // Client area starts at screen (100, 80) on a 200 % display.
    let packet = info.to_packet((100, 80), 2.0);
    assert_eq!(
        packet.position,
        Point::new((1920 - 100) as f32 / 2.0, 230.0)
    );
    assert_eq!(packet.pressure, 0.5);
    assert_eq!(packet.tilt, Some((-37.0, 60.0)));
    assert_eq!(packet.twist, Some(271.0));
    assert!(packet.buttons.contains(PenButtons::BARREL));
    assert!(packet.down && packet.in_proximity);
    assert_eq!(packet.tool, PenKind::Pen);
}

#[test]
fn a_nonsense_scale_does_not_produce_an_infinity() {
    let info = decode_pen_info(&GOLDEN_PEN_DOWN).unwrap();
    let packet = info.to_packet((0, 0), 0.0);
    assert!(packet.position.x.is_finite() && packet.position.y.is_finite());
}

// ---------------------------------------------------------------------------
// Touch: the contact geometry WM_TOUCH cannot carry
// ---------------------------------------------------------------------------

#[test]
fn a_touch_packet_yields_contact_geometry() {
    let bytes = touch_bytes(
        7,
        POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT,
        TOUCH_MASK_CONTACTAREA | TOUCH_MASK_PRESSURE,
        (400, 300, 448, 336),
        512,
    );
    let info = decode_touch_info(&bytes).expect("144 bytes is a whole POINTER_TOUCH_INFO");
    assert_eq!(info.pointer_id, 7);
    assert_eq!(info.contact, (400, 300, 448, 336));
    // 48 × 36 physical at 200 % is a 24 × 18 logical fingertip.
    assert_eq!(info.contact_size(2.0), Some(Size::new(24.0, 18.0)));
    assert_eq!(info.contact_size(1.0), Some(Size::new(48.0, 36.0)));
    assert_eq!(info.pressure_normalised(), Some(0.5));
}

#[test]
fn an_unreported_or_degenerate_contact_area_is_none() {
    let unmasked = touch_bytes(7, 0, TOUCH_MASK_PRESSURE, (400, 300, 448, 336), 0);
    assert_eq!(
        decode_touch_info(&unmasked).unwrap().contact_size(1.0),
        None
    );
    assert_eq!(
        decode_touch_info(&unmasked).unwrap().pressure_normalised(),
        Some(0.0)
    );

    let degenerate = touch_bytes(7, 0, TOUCH_MASK_CONTACTAREA, (400, 300, 400, 300), 0);
    assert_eq!(
        decode_touch_info(&degenerate).unwrap().contact_size(1.0),
        None,
        "a zero-area rectangle is not a measurement"
    );
}
